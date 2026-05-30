use std::{
    env, fs,
    path::{Path, PathBuf},
};

use chirper_core::GpuBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStatus {
    pub name: String,
    pub path: Option<PathBuf>,
}

impl ToolStatus {
    pub fn detect(name: impl Into<String>) -> Self {
        let name = name.into();
        let path = find_executable(&name);

        Self { name, path }
    }

    pub fn is_available(&self) -> bool {
        self.path.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStatus {
    pub label: String,
    pub path: Option<PathBuf>,
    pub exists: bool,
}

impl PathStatus {
    pub fn new(label: impl Into<String>, path: Option<PathBuf>) -> Self {
        let exists = path.as_ref().is_some_and(|path| path.exists());

        Self {
            label: label.into(),
            path,
            exists,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDiagnostics {
    pub amd_gpu_detected: bool,
    pub render_node_detected: bool,
    pub kfd_detected: bool,
    pub vulkan_loader_detected: bool,
    pub vulkan_radeon_detected: bool,
    pub rocm_path_detected: bool,
    pub rocm_tool_detected: bool,
    pub suggested_gpu_backend: GpuBackend,
}

impl GpuDiagnostics {
    pub fn detect() -> Self {
        let amd_gpu_detected = amd_gpu_detected();
        let render_node_detected = render_node_detected();
        let kfd_detected = Path::new("/dev/kfd").exists();
        let vulkan_loader_detected = any_path_exists(&[
            "/usr/lib/libvulkan.so",
            "/usr/lib64/libvulkan.so",
            "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        ]);
        let vulkan_radeon_detected = any_path_exists(&[
            "/usr/lib/libvulkan_radeon.so",
            "/usr/lib64/libvulkan_radeon.so",
            "/usr/lib/x86_64-linux-gnu/libvulkan_radeon.so",
        ]);
        let rocm_path_detected = Path::new("/opt/rocm").exists();
        let rocm_tool_detected =
            find_executable("hipcc").is_some() || find_executable("rocminfo").is_some();

        let suggested_gpu_backend = if amd_gpu_detected && kfd_detected && rocm_tool_detected {
            GpuBackend::Rocm
        } else if amd_gpu_detected && render_node_detected && vulkan_loader_detected {
            GpuBackend::Vulkan
        } else {
            GpuBackend::Cpu
        };

        Self {
            amd_gpu_detected,
            render_node_detected,
            kfd_detected,
            vulkan_loader_detected,
            vulkan_radeon_detected,
            rocm_path_detected,
            rocm_tool_detected,
            suggested_gpu_backend,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDiagnostics {
    pub tools: Vec<ToolStatus>,
    pub gpu: GpuDiagnostics,
}

impl PlatformDiagnostics {
    pub fn detect() -> Self {
        Self {
            tools: [
                "pw-record",
                "ffprobe",
                "whisper-cli",
                "cmake",
                "ninja",
                "make",
                "git",
                "wl-copy",
                "wl-paste",
                "xclip",
                "vulkaninfo",
                "hipcc",
                "rocminfo",
            ]
            .into_iter()
            .map(ToolStatus::detect)
            .collect(),
            gpu: GpuDiagnostics::detect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDiagnostics {
    pub whispercpp_command: PathStatus,
    pub whispercpp_model_path: PathStatus,
}

impl RuntimeDiagnostics {
    pub fn detect(command: &str, model_path: Option<PathBuf>) -> Self {
        let command_path = if command.contains('/') {
            Some(PathBuf::from(command))
        } else {
            find_executable(command)
        };

        Self {
            whispercpp_command: PathStatus::new("whispercpp_command", command_path),
            whispercpp_model_path: PathStatus::new("whispercpp_model_path", model_path),
        }
    }
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return path.is_file().then_some(path);
    }

    let path = env::var_os("PATH")?;

    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn amd_gpu_detected() -> bool {
    drm_device_paths().iter().any(|path| {
        fs::read_to_string(path.join("vendor")).is_ok_and(|vendor| vendor.trim() == "0x1002")
    })
}

fn render_node_detected() -> bool {
    fs::read_dir("/dev/dri")
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("renderD"))
            })
        })
        .unwrap_or(false)
}

fn drm_device_paths() -> Vec<PathBuf> {
    fs::read_dir("/sys/class/drm")
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path().join("device"))
                .filter(|path| path.exists())
                .collect()
        })
        .unwrap_or_default()
}

fn any_path_exists(paths: &[&str]) -> bool {
    paths.iter().any(|path| Path::new(path).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_missing_executable_is_not_available() {
        assert_eq!(find_executable("/definitely/not/chirper"), None);
    }

    #[test]
    fn explicit_tool_status_reports_availability_from_path() {
        let status = ToolStatus::detect("/definitely/not/chirper");

        assert!(!status.is_available());
    }
}
