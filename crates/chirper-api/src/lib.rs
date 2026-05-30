use std::{
    env,
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ApiRequest {
    Status,
    Toggle,
    StartRecording,
    StopRecording,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiResponse {
    pub ok: bool,
    pub state: String,
    pub message: String,
    pub recording_path: Option<String>,
    pub transcript: Option<String>,
    pub formatted: Option<String>,
    pub copied: bool,
}

impl ApiResponse {
    pub fn ok(state: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: true,
            state: state.into(),
            message: message.into(),
            recording_path: None,
            transcript: None,
            formatted: None,
            copied: false,
        }
    }

    pub fn error(state: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            state: state.into(),
            message: message.into(),
            recording_path: None,
            transcript: None,
            formatted: None,
            copied: false,
        }
    }
}

pub fn default_socket_path() -> PathBuf {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("chirper/daemon.sock");
    }

    env::temp_dir().join("chirper/daemon.sock")
}

pub fn send_request(request: &ApiRequest) -> Result<ApiResponse, String> {
    send_request_to(default_socket_path(), request)
}

pub fn send_request_to(path: PathBuf, request: &ApiRequest) -> Result<ApiResponse, String> {
    let mut stream = UnixStream::connect(&path)
        .map_err(|source| format!("failed to connect to {}: {source}", path.display()))?;
    let mut payload = serde_json::to_vec(request)
        .map_err(|source| format!("failed to encode API request: {source}"))?;
    payload.push(b'\n');

    stream
        .write_all(&payload)
        .map_err(|source| format!("failed to write API request: {source}"))?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|source| format!("failed to finish API request: {source}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|source| format!("failed to read API response: {source}"))?;

    serde_json::from_str(response.trim())
        .map_err(|source| format!("failed to decode API response: {source}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_names_are_stable_snake_case() {
        let value = serde_json::to_string(&ApiRequest::StartRecording).unwrap();

        assert_eq!(value, r#"{"command":"start_recording"}"#);
    }

    #[test]
    fn response_defaults_to_no_payload() {
        let response = ApiResponse::ok("idle", "ready");

        assert!(response.ok);
        assert_eq!(response.state, "idle");
        assert_eq!(response.message, "ready");
        assert_eq!(response.recording_path, None);
        assert_eq!(response.transcript, None);
        assert_eq!(response.formatted, None);
        assert!(!response.copied);
    }
}
