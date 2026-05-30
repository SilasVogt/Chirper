#!/usr/bin/env bash
set -euo pipefail

backend="auto"
model="base"
prefix="${XDG_DATA_HOME:-$HOME/.local/share}/chirper"
write_config=false

usage() {
  cat <<'USAGE'
Usage: scripts/setup-whispercpp.sh [--backend auto|cpu|vulkan|rocm] [--model MODEL] [--prefix PATH] [--write-config]

Builds whisper.cpp locally and downloads a ggml model for Chirper.

Examples:
  scripts/setup-whispercpp.sh --backend vulkan --model base
  scripts/setup-whispercpp.sh --backend rocm --model small
  scripts/setup-whispercpp.sh --backend auto --model base --write-config

Common models:
  base, small, medium, large-v3, large-v3-turbo

Quantized variants such as small-q8_0 and large-v3-turbo-q5_0 are also
accepted when whisper.cpp's download script supports them.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --backend)
      backend="${2:?missing value for --backend}"
      shift 2
      ;;
    --model)
      model="${2:?missing value for --model}"
      shift 2
      ;;
    --prefix)
      prefix="${2:?missing value for --prefix}"
      shift 2
      ;;
    --write-config)
      write_config=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

need_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    print_install_hint "$1" >&2
    exit 1
  fi
}

print_install_hint() {
  case "$1" in
    cmake)
      echo "Arch: sudo pacman -S cmake"
      ;;
    glslc)
      echo "Arch: sudo pacman -S shaderc"
      ;;
    glslangValidator)
      echo "Arch: sudo pacman -S glslang"
      ;;
    SPIRV-Headers)
      echo "Arch: sudo pacman -S spirv-headers"
      ;;
  esac
}

need_cmake_package() {
  package="$1"
  tmp_dir="$(mktemp -d)"

  cat > "$tmp_dir/CMakeLists.txt" <<CMAKE
cmake_minimum_required(VERSION 3.16)
project(chirper_preflight NONE)
find_package($package CONFIG REQUIRED)
CMAKE

  if ! cmake -S "$tmp_dir" -B "$tmp_dir/build" >/dev/null 2>&1; then
    rm -rf "$tmp_dir"
    echo "missing required CMake package: $package" >&2
    print_install_hint "$package" >&2
    exit 1
  fi

  rm -rf "$tmp_dir"
}

detect_backend() {
  if command -v hipcc >/dev/null 2>&1 && command -v rocminfo >/dev/null 2>&1 && [ -e /dev/kfd ]; then
    echo "rocm"
    return
  fi

  if [ -e /usr/lib/libvulkan_radeon.so ] || [ -e /usr/lib64/libvulkan_radeon.so ] || [ -e /usr/lib/x86_64-linux-gnu/libvulkan_radeon.so ]; then
    echo "vulkan"
    return
  fi

  echo "cpu"
}

rocm_target() {
  if command -v rocminfo >/dev/null 2>&1; then
    rocminfo | awk '/Name: *gfx/ { print $NF; exit }'
  fi
}

need_command git
need_command cmake

if [ "$backend" = "auto" ]; then
  backend="$(detect_backend)"
fi

case "$backend" in
  cpu|vulkan|rocm) ;;
  *)
    echo "unsupported backend: $backend" >&2
    exit 2
    ;;
esac

case "$backend" in
  vulkan)
    need_command glslc
    need_command glslangValidator
    need_cmake_package SPIRV-Headers
    ;;
  rocm)
    need_command hipcc
    need_command rocminfo
    ;;
esac

src_dir="$prefix/src/whisper.cpp"
model_dir="$prefix/models"
build_dir="$src_dir/build-$backend"
model_path="$model_dir/ggml-$model.bin"

mkdir -p "$prefix/src" "$model_dir"

if [ -d "$src_dir/.git" ]; then
  git -C "$src_dir" pull --ff-only
else
  git clone --depth 1 https://github.com/ggml-org/whisper.cpp.git "$src_dir"
fi

cmake_args=(-B "$build_dir" -S "$src_dir" -DCMAKE_BUILD_TYPE=Release)

case "$backend" in
  vulkan)
    cmake_args+=(-DGGML_VULKAN=1)
    ;;
  rocm)
    cmake_args+=(-DGGML_HIP=1)
    target="$(rocm_target || true)"
    if [ -n "${target:-}" ]; then
      cmake_args+=("-DAMDGPU_TARGETS=$target")
    fi
    ;;
esac

cmake "${cmake_args[@]}"
cmake --build "$build_dir" -j --config Release

if [ ! -f "$model_path" ]; then
  "$src_dir/models/download-ggml-model.sh" "$model" "$model_dir"
fi

write_default_config() {
  config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/chirper"
  config_path="$config_dir/config.toml"

  mkdir -p "$config_dir"

  if [ -f "$config_path" ]; then
    echo
    echo "Config already exists at $config_path; leaving it unchanged."
    echo "Add or update the snippet below if these whisper.cpp paths changed."
    return
  fi

  cat > "$config_path" <<CONFIG
audio_backend = "pipewire"
asr_backend = "whisper-cpp"
gpu_backend = "$backend"
formatter_backend = "rules"
insertion_backend = "clipboard"
dictation_mode = "auto"

whisper_model = "$model"
whispercpp_command = "$build_dir/bin/whisper-cli"
whispercpp_model_path = "$model_path"
whisper_language = "auto"

ollama_command = "ollama"
ollama_model = "llama3.2"

[vocabulary]
CONFIG

  echo
  echo "Wrote Chirper config to $config_path"
}

if [ "$write_config" = true ]; then
  write_default_config
fi

cat <<CONFIG

whisper.cpp is ready.

Add this to ~/.config/chirper/config.toml:

gpu_backend = "$backend"
whispercpp_command = "$build_dir/bin/whisper-cli"
whispercpp_model_path = "$model_path"
whisper_language = "auto"

Smoke test:
  cargo run -p chirper-cli -- transcribe-file /path/to/audio.wav "$model_path"
CONFIG
