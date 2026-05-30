use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use chirper_core::{
    ChirperConfig, ChirperError, ChirperResult, CodexProfileConfig, DictationMode, Formatter,
    Transcript, VocabularyEntry,
};

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
    }

    pub fn format_with_prompt_input_and_note(
        &self,
        raw_transcript: &Transcript,
        preprocessed_text: &str,
        mode: DictationMode,
        input: CodexPromptInput,
        prompt_note: Option<&str>,
    ) -> ChirperResult<String> {
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

        self.format_custom_prompt(&prompt, input_text)
    }

    pub fn format_custom_prompt(
        &self,
        prompt: &str,
        non_empty_input: &str,
    ) -> ChirperResult<String> {
        let output_path = codex_output_path();
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
            .output()
            .map_err(|source| {
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
            .unwrap_or_else(|_| String::from_utf8_lossy(&output.stdout).to_string());
        let _ = fs::remove_file(&output_path);
        let formatted = clean_model_output(&final_message);

        if formatted.is_empty() && !non_empty_input.trim().is_empty() {
            return Err(ChirperError::Formatting(
                "codex returned an empty formatter response".to_string(),
            ));
        }

        Ok(formatted)
    }
}

impl Formatter for CodexFormatter {
    fn format(&self, transcript: &Transcript, mode: DictationMode) -> ChirperResult<String> {
        self.format_with_context(transcript, &transcript.text, mode)
    }
}

fn codex_output_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    env::temp_dir().join(format!(
        "chirper-codex-{}-{timestamp}.txt",
        std::process::id()
    ))
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
    let vocabulary_section = if vocabulary.is_empty() {
        "Preferred spellings: none configured.\n".to_string()
    } else {
        let mut section =
            "Preferred spellings. Apply these exact spellings when the spoken phrase appears:\n"
                .to_string();
        for entry in vocabulary {
            section.push_str(&format!(
                "- \"{}\" => \"{}\"\n",
                entry.spoken, entry.written
            ));
        }
        section
    };
    let input_guidance = match preprocessed_text {
        Some(_) => "\
You receive both the raw transcript and Chirper's local preprocessed draft.
The preprocessed draft is the authoritative baseline and has already applied edit commands, spoken punctuation, casing commands, and preferred spellings.
Use the raw transcript as extra evidence for intended spelling and casing clues, such as \"spelled as one word in Pascal case\", \"pronounced ...\", \"capital p capital f\", \"all caps\", or letter-by-letter spellings.
",
        None => "\
You receive only the raw transcript.
You MUST apply clear spoken edit commands, spoken punctuation, spelling instructions, casing instructions, and preferred spellings yourself.
Treat spoken instruction phrases as formatting instructions, not ordinary content.
",
    };
    let input_section = match preprocessed_text {
        Some(preprocessed_text) => format!(
            "\
Raw transcript:
<<<
{raw_text}
>>>

Preprocessed draft:
<<<
{preprocessed_text}
>>>
"
        ),
        None => format!(
            "\
Raw transcript:
<<<
{raw_text}
>>>
"
        ),
    };
    let extra_instruction_section = prompt_note
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(|note| {
            format!(
                "\
Additional compare-run instructions:
{note}
"
            )
        })
        .unwrap_or_default();

    format!(
        "\
You are a dictation proofreader inside Chirper.

Return only the final text. Do not explain, summarize, add facts, mention Codex, or wrap the result in quotes.
Do not inspect files, do not run commands, and do not use tools. This is only a text cleanup task.
Preserve the speaker's meaning, wording, order, and ordinary content words.
Fix only likely transcription errors, casing, punctuation, spacing, and paragraph breaks.
{input_guidance}
You MUST apply spoken spelling/casing instructions when they clearly modify a nearby name, handle, acronym, URL, email, version, or identifier.
After applying a spoken instruction, remove the instruction words from the final text.
Examples:
- \"called pixel ferret tv that's capital p capital f capital t capital v\" -> \"called PixelFerretTV\"
- \"j a n a pronounced yah nah\" -> \"Jana, pronounced Yah-nah\"
- \"ops and things dot io slash episodes slash zero four two\" -> \"opsandthings.io/episodes/042\"
Do not reintroduce text removed from the draft, do not output edit commands, do not duplicate corrected names, and do not undo existing corrections.
If the text looks like code, shell input, Markdown, a URL, or an email address, preserve that structure.
Preserve existing camelCase and PascalCase identifiers exactly, including product, channel, and project names.
Do not add line breaks unless the input already contains them or the user clearly dictated a paragraph break.
{vocabulary_section}
{extra_instruction_section}
If the input is empty or contains no speech, return an empty string.

Mode: {mode:?}

{input_section}

Final text:"
    )
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
    use super::*;

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
        assert!(!prompt.contains("Preprocessed draft:"));
        assert!(prompt.contains("Do not inspect files"));
    }

    #[test]
    fn cleans_terminal_output() {
        assert_eq!(clean_model_output("Final text: Hello."), "Hello.");
        assert_eq!(clean_model_output("Hel\u{1b}[1D\u{1b}[Klo\r."), "Hello.");
    }
}
