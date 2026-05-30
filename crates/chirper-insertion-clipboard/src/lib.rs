use std::{
    io::Write,
    process::{Command, Stdio},
};

use chirper_core::{ChirperError, ChirperResult, InsertionTarget, TextInserter};
use chirper_platform::find_executable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardInserter {
    command: ClipboardCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardCommand {
    WlCopy,
    XClip,
    Custom { program: String, args: Vec<String> },
}

impl ClipboardInserter {
    pub fn detect() -> ChirperResult<Self> {
        if find_executable("wl-copy").is_some() {
            return Ok(Self {
                command: ClipboardCommand::WlCopy,
            });
        }

        if find_executable("xclip").is_some() {
            return Ok(Self {
                command: ClipboardCommand::XClip,
            });
        }

        Err(ChirperError::Insertion(
            "no clipboard command found; install wl-clipboard or xclip".to_string(),
        ))
    }

    pub fn new(command: ClipboardCommand) -> Self {
        Self { command }
    }

    pub fn command(&self) -> &ClipboardCommand {
        &self.command
    }
}

impl TextInserter for ClipboardInserter {
    fn insert(&self, text: &str, _target: Option<&InsertionTarget>) -> ChirperResult<()> {
        let (program, args) = command_parts(&self.command);
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| {
                ChirperError::Insertion(format!("failed to start clipboard command: {source}"))
            })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            ChirperError::Insertion("failed to open clipboard command stdin".to_string())
        })?;
        stdin.write_all(text.as_bytes()).map_err(|source| {
            ChirperError::Insertion(format!("failed to write clipboard text: {source}"))
        })?;
        drop(stdin);

        let output = child.wait_with_output().map_err(|source| {
            ChirperError::Insertion(format!("failed to wait for clipboard command: {source}"))
        })?;

        if output.status.success() {
            Ok(())
        } else {
            Err(ChirperError::Insertion(format!(
                "clipboard command exited with status {}",
                output.status
            )))
        }
    }
}

fn command_parts(command: &ClipboardCommand) -> (&str, Vec<&str>) {
    match command {
        ClipboardCommand::WlCopy => ("wl-copy", Vec::new()),
        ClipboardCommand::XClip => ("xclip", vec!["-selection", "clipboard"]),
        ClipboardCommand::Custom { program, args } => {
            (program.as_str(), args.iter().map(String::as_str).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wl_copy_has_no_extra_arguments() {
        let (program, args) = command_parts(&ClipboardCommand::WlCopy);

        assert_eq!(program, "wl-copy");
        assert!(args.is_empty());
    }

    #[test]
    fn xclip_targets_clipboard_selection() {
        let (program, args) = command_parts(&ClipboardCommand::XClip);

        assert_eq!(program, "xclip");
        assert_eq!(args, vec!["-selection", "clipboard"]);
    }
}
