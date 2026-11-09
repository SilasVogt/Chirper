use std::{
    io::Write,
    process::{Command, Stdio},
};

use chirper_core::{
    ai_formatting::{self, FormatterReply, ProofreadingPrompt},
    ChirperConfig, ChirperError, ChirperResult, DictationMode, Formatter, Transcript,
    VocabularyEntry,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaOptions {
    pub command: String,
    pub model: String,
    pub vocabulary: Vec<VocabularyEntry>,
}

impl OllamaOptions {
    pub fn from_config(config: &ChirperConfig) -> Self {
        Self {
            command: config.ollama_command.clone(),
            model: config.ollama_model.clone(),
            vocabulary: config.vocabulary.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaModel {
    pub name: String,
}

/// One formatter call: the prompt that was sent, the model's raw reply, and
/// the parsed reply or why the raw reply was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaFormatRun {
    pub prompt: String,
    /// The model's reply before parsing. Empty when the model was not called.
    pub raw_output: String,
    /// The cleaned-up text and the model's notes, or why `raw_output` was
    /// rejected.
    pub reply: ChirperResult<FormatterReply>,
}

impl OllamaFormatRun {
    /// An empty transcript has nothing to clean up, so the model is skipped.
    fn skipped(prompt: String) -> Self {
        Self {
            prompt,
            raw_output: String::new(),
            reply: Ok(FormatterReply::default()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OllamaPromptInput {
    RawOnly,
    RawAndPreprocessed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaFormatter {
    options: OllamaOptions,
}

impl OllamaFormatter {
    pub fn new(options: OllamaOptions) -> Self {
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
            OllamaPromptInput::RawAndPreprocessed,
        )
    }

    pub fn format_with_prompt_input(
        &self,
        raw_transcript: &Transcript,
        preprocessed_text: &str,
        mode: DictationMode,
        input: OllamaPromptInput,
    ) -> ChirperResult<String> {
        self.format_with_prompt_input_and_note(raw_transcript, preprocessed_text, mode, input, None)
            .map(|reply| reply.text)
    }

    pub fn format_with_prompt_input_and_note(
        &self,
        raw_transcript: &Transcript,
        preprocessed_text: &str,
        mode: DictationMode,
        input: OllamaPromptInput,
        prompt_note: Option<&str>,
    ) -> ChirperResult<FormatterReply> {
        let prompt = build_formatting_prompt(
            &raw_transcript.text,
            match input {
                OllamaPromptInput::RawOnly => None,
                OllamaPromptInput::RawAndPreprocessed => Some(preprocessed_text),
            },
            mode,
            &self.options.vocabulary,
            prompt_note,
        );
        if raw_transcript.text.trim().is_empty() {
            return Ok(FormatterReply::default());
        }
        let input_text = match input {
            OllamaPromptInput::RawOnly => &raw_transcript.text,
            OllamaPromptInput::RawAndPreprocessed => preprocessed_text,
        };

        self.run_structured(prompt, input_text)?.reply
    }

    /// Runs the regular app prompt on the raw transcript. This is what the
    /// daemon does when Ollama formatting is enabled.
    ///
    /// Fails only when Ollama could not be run. A reply that breaks the JSON
    /// contract comes back in [`OllamaFormatRun::reply`], so callers can still
    /// log what the model said.
    pub fn format_ai_prompt(&self, transcript: &Transcript) -> ChirperResult<OllamaFormatRun> {
        let prompt = build_ai_formatting_prompt(&transcript.text);
        if transcript.text.trim().is_empty() {
            return Ok(OllamaFormatRun::skipped(prompt));
        }

        self.run_structured(prompt, &transcript.text)
    }

    /// Runs a caller-written prompt as is, without forcing the JSON reply
    /// format. A reply that follows the JSON contract is still split into text
    /// and notes; any other reply is used as the text.
    pub fn format_custom_prompt(
        &self,
        prompt: &str,
        non_empty_input: &str,
    ) -> ChirperResult<FormatterReply> {
        let raw_output = self.run(prompt, None)?;

        ai_formatting::lenient_reply("ollama", &raw_output, non_empty_input)
    }

    fn run_structured(&self, prompt: String, input: &str) -> ChirperResult<OllamaFormatRun> {
        let raw_output = self.run(&prompt, Some(ai_formatting::REPLY_SCHEMA))?;
        let reply = ai_formatting::checked_reply("ollama", &raw_output, input);

        Ok(OllamaFormatRun {
            prompt,
            raw_output,
            reply,
        })
    }

    /// Sends one prompt to `ollama run` and returns the cleaned stdout.
    /// `format` is passed to `--format`: a JSON schema or `json`.
    fn run(&self, prompt: &str, format: Option<&str>) -> ChirperResult<String> {
        self.ensure_model_installed()?;

        let mut command = Command::new(&self.options.command);
        command
            .arg("run")
            .arg("--nowordwrap")
            .arg("--hidethinking")
            // The notes field is the model's scratch space. Thinking only adds
            // latency to a cleanup task, and models that cannot switch it off
            // ignore this flag.
            .arg("--think=false")
            .arg("--keepalive")
            .arg("0");
        if let Some(format) = format {
            command.arg("--format").arg(format);
        }

        let mut child = command
            .arg(&self.options.model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| {
                ChirperError::Formatting(format!(
                    "failed to run `{}`: {source}",
                    self.options.command
                ))
            })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ChirperError::Formatting("failed to open Ollama stdin".to_string()))?;
        stdin.write_all(prompt.as_bytes()).map_err(|source| {
            ChirperError::Formatting(format!("failed to write Ollama prompt: {source}"))
        })?;
        drop(stdin);

        let output = child.wait_with_output().map_err(|source| {
            ChirperError::Formatting(format!("failed to wait for Ollama: {source}"))
        })?;

        if !output.status.success() {
            return Err(ChirperError::Formatting(format!(
                "ollama exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        Ok(clean_model_output(&String::from_utf8_lossy(&output.stdout)))
    }

    fn ensure_model_installed(&self) -> ChirperResult<()> {
        let models = list_ollama_models(&self.options.command)?;
        if models.iter().any(|model| model.name == self.options.model) {
            return Ok(());
        }

        Err(ChirperError::Formatting(format!(
            "Ollama model `{}` is not installed; run `ollama pull {}` first",
            self.options.model, self.options.model
        )))
    }
}

pub fn build_ai_formatting_prompt(transcript: &str) -> String {
    ai_formatting::app_prompt(transcript)
}

impl Formatter for OllamaFormatter {
    fn format(&self, transcript: &Transcript, mode: DictationMode) -> ChirperResult<String> {
        self.format_with_context(transcript, &transcript.text, mode)
    }
}

pub fn list_ollama_models(command: &str) -> ChirperResult<Vec<OllamaModel>> {
    let output = Command::new(command)
        .arg("list")
        .stdin(Stdio::null())
        .output()
        .map_err(|source| {
            ChirperError::Formatting(format!("failed to run `{command} list`: {source}"))
        })?;

    if !output.status.success() {
        return Err(ChirperError::Formatting(format!(
            "`{command} list` exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(parse_ollama_list(&String::from_utf8_lossy(&output.stdout)))
}

pub fn parse_ollama_list(stdout: &str) -> Vec<OllamaModel> {
    stdout
        .lines()
        .filter_map(|line| {
            let name = line.split_whitespace().next()?;
            if name.eq_ignore_ascii_case("name") {
                return None;
            }

            Some(OllamaModel {
                name: name.to_string(),
            })
        })
        .collect()
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
        backend_rules: None,
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
    use super::*;

    fn formatter_with_command(command: &str) -> OllamaFormatter {
        OllamaFormatter::new(OllamaOptions {
            command: command.to_string(),
            model: "test-model".to_string(),
            vocabulary: Vec::new(),
        })
    }

    fn transcript(text: &str) -> Transcript {
        Transcript {
            text: text.to_string(),
            language: None,
        }
    }

    #[test]
    fn parses_ollama_list_names() {
        let output = "\
NAME              ID              SIZE      MODIFIED
llama3.2:latest   a80c4f17acd5    2.0 GB    2 weeks ago
qwen2.5:7b        845dbda0ea48    4.7 GB    yesterday
";

        let models = parse_ollama_list(output);

        assert_eq!(
            models,
            vec![
                OllamaModel {
                    name: "llama3.2:latest".to_string()
                },
                OllamaModel {
                    name: "qwen2.5:7b".to_string()
                }
            ]
        );
    }

    #[test]
    fn prompt_contains_mode_and_input() {
        let prompt = build_formatting_prompt(
            "hello comma world that's spelled as one word in pascal case",
            Some("HelloWorld"),
            DictationMode::Standard,
            &[VocabularyEntry {
                spoken: "silas on linux".to_string(),
                written: "SilasOnLinux".to_string(),
            }],
            Some("Prefer concise output."),
        );

        assert!(prompt.contains("Mode: Standard"));
        assert!(prompt.contains("<raw_transcript>"));
        assert!(prompt.contains("hello comma world that's spelled as one word"));
        assert!(prompt.contains("<preprocessed_draft>\nHelloWorld\n</preprocessed_draft>"));
        assert!(prompt.contains("Reply with one JSON object"));
        assert!(prompt.contains("authoritative baseline"));
        assert!(prompt.contains("capital p capital f"));
        assert!(prompt.contains("PixelFerretTV"));
        assert!(prompt.contains("Jana, pronounced Yah-nah"));
        assert!(prompt.contains("\"silas on linux\" => \"SilasOnLinux\""));
        assert!(prompt.contains("Prefer concise output."));
    }

    #[test]
    fn raw_only_prompt_omits_preprocessed_draft() {
        let prompt = build_formatting_prompt(
            "hello comma world",
            None,
            DictationMode::Standard,
            &[],
            None,
        );

        assert!(prompt.contains("You receive only the raw transcript"));
        assert!(prompt.contains("<raw_transcript>"));
        assert!(!prompt.contains("<preprocessed_draft>"));
    }

    #[test]
    fn ai_prompt_wraps_raw_transcript() {
        let prompt = build_ai_formatting_prompt("hello comma world");

        assert!(prompt.contains("Fix transcription errors"));
        assert!(prompt.contains("<transcript>\nhello comma world\n</transcript>"));
        assert!(prompt.contains("\"notes\""));
        assert!(!prompt.contains("{transcript}"));
    }

    #[test]
    fn empty_transcript_skips_the_model() {
        // The command does not exist, so any attempt to call Ollama would fail.
        let formatter = formatter_with_command("/nonexistent/chirper-test-ollama");

        let run = formatter.format_ai_prompt(&transcript("  \n")).unwrap();
        assert_eq!(run.reply, Ok(FormatterReply::default()));
        assert_eq!(run.raw_output, "");

        let reply = formatter
            .format_with_prompt_input_and_note(
                &transcript(""),
                "",
                DictationMode::Auto,
                OllamaPromptInput::RawAndPreprocessed,
                None,
            )
            .unwrap();
        assert_eq!(reply, FormatterReply::default());
    }

    #[test]
    fn non_empty_transcript_still_calls_the_model() {
        let formatter = formatter_with_command("/nonexistent/chirper-test-ollama");

        assert!(formatter.format_ai_prompt(&transcript("hello")).is_err());
    }

    #[test]
    fn cleans_common_prefix_but_preserves_markdown_fences() {
        assert_eq!(clean_model_output("Final text: Hello."), "Hello.");
        assert_eq!(clean_model_output("```\nHello.\n```"), "```\nHello.\n```");
        assert_eq!(clean_model_output("Hel\u{1b}[1D\u{1b}[Klo\r."), "Hello.");
        assert_eq!(
            clean_model_output(
                "\u{1b}[?25l\u{1b}[?2026h{\"notes\":\"\",\"text\":\"Hi.\"}\u{1b}[?25h"
            ),
            "{\"notes\":\"\",\"text\":\"Hi.\"}"
        );
    }
}
