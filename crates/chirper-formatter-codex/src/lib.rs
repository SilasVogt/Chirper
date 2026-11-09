use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use chirper_core::{
    ai_formatting::{self, FormatterReply, ProofreadingPrompt},
    ChirperConfig, ChirperError, ChirperResult, CodexProfileConfig, DictationMode, Formatter,
    Transcript, VocabularyEntry,
};

const CODEX_RULES: &str = "\
You run inside Codex, but this is only a text cleanup task: do not inspect files, do not run commands, and do not use tools. Do not mention Codex.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexPromptInput {
    RawOnly,
    RawAndPreprocessed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexOptions {
    pub command: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub config_overrides: Vec<String>,
    pub vocabulary: Vec<VocabularyEntry>,
}

impl CodexOptions {
    pub fn from_config(config: &ChirperConfig) -> Self {
        Self {
            command: config.codex_command.clone(),
            model: config.codex_model.clone(),
            profile: config.codex_profile.clone(),
            reasoning_effort: config.codex_reasoning_effort.clone(),
            service_tier: config.codex_service_tier.clone(),
            config_overrides: config.codex_config_overrides.clone(),
            vocabulary: config.vocabulary.clone(),
        }
    }

    pub fn from_named_profile(config: &ChirperConfig, profile: &CodexProfileConfig) -> Self {
        Self {
            command: config.codex_command.clone(),
            model: profile.model.clone(),
            profile: profile.profile.clone(),
            reasoning_effort: profile.reasoning_effort.clone(),
            service_tier: profile.service_tier.clone(),
            config_overrides: profile.config_overrides.clone(),
            vocabulary: config.vocabulary.clone(),
        }
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if let Some(model) = &self.model {
            parts.push(model.clone());
        }
        if let Some(reasoning_effort) = &self.reasoning_effort {
            parts.push(format!("effort={reasoning_effort}"));
        }
        if let Some(service_tier) = &self.service_tier {
            parts.push(format!("tier={service_tier}"));
        }
        if let Some(profile) = &self.profile {
            parts.push(format!("profile={profile}"));
        }

        if parts.is_empty() {
            "codex-default".to_string()
        } else {
            parts.join(",")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexFormatter {
    options: CodexOptions,
}

impl CodexFormatter {
    pub fn new(options: CodexOptions) -> Self {
        Self { options }
    }

    pub fn format_with_context(
        &self,
        raw_transcript: &Transcript,
        preprocessed_text: &str,
        mode: DictationMode,
    ) -> ChirperResult<String> {
        self.format_with_prompt_input(
            raw_transcript,
            preprocessed_text,
            mode,
            CodexPromptInput::RawAndPreprocessed,
        )
    }

    pub fn format_with_prompt_input(
        &self,
        raw_transcript: &Transcript,
        preprocessed_text: &str,
        mode: DictationMode,
        input: CodexPromptInput,
    ) -> ChirperResult<String> {
        self.format_with_prompt_input_and_note(raw_transcript, preprocessed_text, mode, input, None)
            .map(|reply| reply.text)
    }

    pub fn format_with_prompt_input_and_note(
        &self,
        raw_transcript: &Transcript,
        preprocessed_text: &str,
        mode: DictationMode,
        input: CodexPromptInput,
        prompt_note: Option<&str>,
    ) -> ChirperResult<FormatterReply> {
        if raw_transcript.text.trim().is_empty() {
            return Ok(FormatterReply::default());
        }
        let prompt = build_formatting_prompt(
            &raw_transcript.text,
            match input {
                CodexPromptInput::RawOnly => None,
                CodexPromptInput::RawAndPreprocessed => Some(preprocessed_text),
            },
            mode,
            &self.options.vocabulary,
            prompt_note,
        );
        let input_text = match input {
            CodexPromptInput::RawOnly => &raw_transcript.text,
            CodexPromptInput::RawAndPreprocessed => preprocessed_text,
        };

        let final_message = self.run(&prompt, Some(ai_formatting::REPLY_SCHEMA))?;

        ai_formatting::checked_reply("codex", &final_message, input_text)
    }

    /// Runs a caller-written prompt as is, without forcing the JSON reply
    /// format. A reply that follows the JSON contract is still split into text
    /// and notes; any other reply is used as the text.
    pub fn format_custom_prompt(
        &self,
        prompt: &str,
        non_empty_input: &str,
    ) -> ChirperResult<FormatterReply> {
        let final_message = self.run(prompt, None)?;

        ai_formatting::lenient_reply("codex", &final_message, non_empty_input)
    }

    /// Runs `codex exec` once and returns the cleaned final message.
    /// `output_schema` is written to a temporary file for `--output-schema`.
    fn run(&self, prompt: &str, output_schema: Option<&str>) -> ChirperResult<String> {
        let output_path = codex_temp_path("txt");
        // Create the reply file first so a planted symlink at that path makes
        // this fail instead of being followed when Codex writes the reply.
        write_new_file(&output_path, "").map_err(|source| {
            ChirperError::Formatting(format!(
                "failed to create Codex output file {}: {source}",
                output_path.display()
            ))
        })?;
        let schema_path = match output_schema.map(write_schema_file).transpose() {
            Ok(schema_path) => schema_path,
            Err(error) => {
                let _ = fs::remove_file(&output_path);
                return Err(error);
            }
        };
        let work_dir = env::temp_dir();
        let mut command = Command::new(&self.options.command);

        command
            .arg("--ask-for-approval")
            .arg("never")
            .arg("exec")
            .arg("--skip-git-repo-check")
            .arg("--ephemeral")
            .arg("--ignore-rules")
            .arg("--sandbox")
            .arg("read-only")
            .arg("--color")
            .arg("never")
            .arg("-C")
            .arg(&work_dir)
            .arg("-o")
            .arg(&output_path);

        if let Some(schema_path) = &schema_path {
            command.arg("--output-schema").arg(schema_path);
        }

        if let Some(model) = self
            .options
            .model
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            command.arg("-m").arg(model);
        }

        if let Some(profile) = self
            .options
            .profile
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            command.arg("-p").arg(profile);
        }

        if let Some(reasoning_effort) = self
            .options
            .reasoning_effort
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            command
                .arg("-c")
                .arg(format!("model_reasoning_effort=\"{reasoning_effort}\""));
        }

        if let Some(service_tier) = self
            .options
            .service_tier
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            command
                .arg("-c")
                .arg(format!("service_tier=\"{service_tier}\""));
        }

        for override_value in &self.options.config_overrides {
            let override_value = override_value.trim();
            if !override_value.is_empty() {
                command.arg("-c").arg(override_value);
            }
        }

        let output = command
            .arg(prompt)
            .env("NO_COLOR", "1")
            .env("TERM", "xterm-256color")
            .stdin(Stdio::null())
            .output();
        if let Some(schema_path) = &schema_path {
            let _ = fs::remove_file(schema_path);
        }
        let output = output.map_err(|source| {
            let _ = fs::remove_file(&output_path);
            ChirperError::Formatting(format!(
                "failed to run `{}`: {source}",
                self.options.command
            ))
        })?;

        if !output.status.success() {
            let _ = fs::remove_file(&output_path);
            return Err(ChirperError::Formatting(format!(
                "codex exited with status {}: {}{}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim(),
                stdout_context(&output.stdout)
            )));
        }

        let final_message = fs::read_to_string(&output_path)
            .ok()
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| String::from_utf8_lossy(&output.stdout).to_string());
        let _ = fs::remove_file(&output_path);

        Ok(clean_model_output(&final_message))
    }
}

impl Formatter for CodexFormatter {
    fn format(&self, transcript: &Transcript, mode: DictationMode) -> ChirperResult<String> {
        self.format_with_context(transcript, &transcript.text, mode)
    }
}

fn codex_temp_path(extension: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    env::temp_dir().join(format!(
        "chirper-codex-{}-{timestamp}.{extension}",
        std::process::id()
    ))
}

fn write_schema_file(schema: &str) -> ChirperResult<PathBuf> {
    let path = codex_temp_path("schema.json");
    write_new_file(&path, schema).map_err(|source| {
        ChirperError::Formatting(format!(
            "failed to write Codex output schema {}: {source}",
            path.display()
        ))
    })?;

    Ok(path)
}

/// Refuses to follow or overwrite an existing path, such as a planted symlink.
fn write_new_file(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;

    file.write_all(contents.as_bytes())
}

fn stdout_context(stdout: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stdout = stdout.trim();

    if stdout.is_empty() {
        String::new()
    } else {
        format!(" stdout: {stdout}")
    }
}

fn build_formatting_prompt(
    raw_text: &str,
    preprocessed_text: Option<&str>,
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
    prompt_note: Option<&str>,
) -> String {
    ai_formatting::proofreading_prompt(ProofreadingPrompt {
        raw_transcript: raw_text,
        preprocessed_draft: preprocessed_text,
        mode,
        vocabulary,
        prompt_note,
        backend_rules: Some(CODEX_RULES),
    })
}

fn clean_model_output(stdout: &str) -> String {
    let mut output = strip_terminal_controls(stdout).trim().to_string();

    if let Some(stripped) = output.strip_prefix("Final text:") {
        output = stripped.trim().to_string();
    }

    output
}

fn strip_terminal_controls(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();

    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }

        if character != '\r' {
            output.push(character);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Tests that create Codex temp files share one process id prefix, so they
    /// take turns to keep file counts stable.
    static TEMP_FILES: Mutex<()> = Mutex::new(());

    #[test]
    fn options_from_named_profile_prefer_profile_values() {
        let config = ChirperConfig {
            codex_command: "codex".to_string(),
            codex_model: Some("gpt-5.5".to_string()),
            codex_reasoning_effort: Some("low".to_string()),
            vocabulary: vec![VocabularyEntry {
                spoken: "silas on linux".to_string(),
                written: "SilasOnLinux".to_string(),
            }],
            ..ChirperConfig::default()
        };
        let profile = CodexProfileConfig {
            name: "fast".to_string(),
            model: Some("gpt-5.4-mini".to_string()),
            profile: None,
            reasoning_effort: Some("medium".to_string()),
            service_tier: Some("fast".to_string()),
            config_overrides: vec!["model_verbosity=\"low\"".to_string()],
        };
        let options = CodexOptions::from_named_profile(&config, &profile);

        assert_eq!(options.command, "codex");
        assert_eq!(options.model.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(options.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(options.service_tier.as_deref(), Some("fast"));
        assert_eq!(options.config_overrides, vec!["model_verbosity=\"low\""]);
        assert_eq!(options.vocabulary.len(), 1);
    }

    #[test]
    fn prompt_supports_raw_only_mode() {
        let prompt =
            build_formatting_prompt("hello comma world", None, DictationMode::Auto, &[], None);

        assert!(prompt.contains("You receive only the raw transcript"));
        assert!(!prompt.contains("<preprocessed_draft>"));
        assert!(prompt.contains("do not inspect files"));
        assert!(prompt.contains("Reply with one JSON object"));
    }

    #[test]
    fn empty_transcript_skips_codex() {
        let formatter = CodexFormatter::new(CodexOptions {
            command: "/nonexistent/chirper-test-codex".to_string(),
            model: None,
            profile: None,
            reasoning_effort: None,
            service_tier: None,
            config_overrides: Vec::new(),
            vocabulary: Vec::new(),
        });
        let transcript = Transcript {
            text: " ".to_string(),
            language: None,
        };

        let reply = formatter
            .format_with_prompt_input_and_note(
                &transcript,
                "",
                DictationMode::Auto,
                CodexPromptInput::RawAndPreprocessed,
                None,
            )
            .unwrap();
        assert_eq!(reply, FormatterReply::default());
    }

    #[test]
    fn missing_codex_cleans_up_its_temp_files() {
        let _guard = TEMP_FILES.lock().unwrap_or_else(|error| error.into_inner());
        let formatter = CodexFormatter::new(CodexOptions {
            command: "/nonexistent/chirper-test-codex".to_string(),
            model: None,
            profile: None,
            reasoning_effort: None,
            service_tier: None,
            config_overrides: Vec::new(),
            vocabulary: Vec::new(),
        });
        let prefix = format!("chirper-codex-{}-", std::process::id());
        let leftovers = || {
            fs::read_dir(env::temp_dir())
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
                .count()
        };
        let before = leftovers();

        assert!(formatter
            .run("hello", Some(ai_formatting::REPLY_SCHEMA))
            .is_err());
        assert_eq!(leftovers(), before);
    }

    #[test]
    fn schema_file_refuses_existing_paths() {
        let _guard = TEMP_FILES.lock().unwrap_or_else(|error| error.into_inner());
        let path = write_schema_file(ai_formatting::REPLY_SCHEMA).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            ai_formatting::REPLY_SCHEMA
        );
        assert!(write_new_file(&path, "{}").is_err());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn cleans_terminal_output() {
        assert_eq!(clean_model_output("Final text: Hello."), "Hello.");
        assert_eq!(clean_model_output("Hel\u{1b}[1D\u{1b}[Klo\r."), "Hello.");
    }
}
