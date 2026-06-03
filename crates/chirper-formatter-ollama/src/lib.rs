use std::{
    io::Write,
    process::{Command, Stdio},
};

use chirper_core::{
    ChirperConfig, ChirperError, ChirperResult, DictationMode, Formatter, Transcript,
    VocabularyEntry,
};

pub const AI_FORMATTING_PROMPT_TEMPLATE: &str = "\
Your job is to fix transcription errors and human made mistakes. The user may misspeak and try to correct themselves or specify specific spellings of words and names. Apply spoken edit commands, punctuation, casing, spelling, URLs, emails, basic markdown and identifiers. Remove any spoken edits you have applied from the transcript. Do not explain your actions. Return only the cleaned-up final text. This is the original transcript: {transcript}";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaFormatRun {
    pub prompt: String,
    pub output: String,
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
    }

    pub fn format_with_prompt_input_and_note(
        &self,
        raw_transcript: &Transcript,
        preprocessed_text: &str,
        mode: DictationMode,
        input: OllamaPromptInput,
        prompt_note: Option<&str>,
    ) -> ChirperResult<String> {
        self.ensure_model_installed()?;

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
        let input_text = match input {
            OllamaPromptInput::RawOnly => &raw_transcript.text,
            OllamaPromptInput::RawAndPreprocessed => preprocessed_text,
        };

        self.format_custom_prompt(&prompt, input_text)
    }

    pub fn format_custom_prompt(
        &self,
        prompt: &str,
        non_empty_input: &str,
    ) -> ChirperResult<String> {
        self.ensure_model_installed()?;

        let mut child = Command::new(&self.options.command)
            .arg("run")
            .arg("--nowordwrap")
            .arg("--hidethinking")
            .arg("--keepalive")
            .arg("0")
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

        let stdout = String::from_utf8_lossy(&output.stdout);
        let formatted = clean_model_output(&stdout);
        if formatted.is_empty() && !non_empty_input.trim().is_empty() {
            return Err(ChirperError::Formatting(
                "ollama returned an empty formatter response".to_string(),
            ));
        }

        Ok(formatted)
    }

    pub fn format_ai_prompt(&self, transcript: &Transcript) -> ChirperResult<OllamaFormatRun> {
        let prompt = build_ai_formatting_prompt(&transcript.text);
        let output = self.format_custom_prompt(&prompt, &transcript.text)?;

        Ok(OllamaFormatRun { prompt, output })
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
    AI_FORMATTING_PROMPT_TEMPLATE.replace("{transcript}", transcript)
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
You format dictated speech into final text.

Return only the final text. Do not explain, summarize, add facts, or wrap the result in quotes.
You are a conservative proofreader for speech-to-text output, not a rewriting assistant.
Preserve the speaker's meaning, wording, order, and all ordinary content words.
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
        assert!(prompt.contains("Raw transcript:"));
        assert!(prompt.contains("hello comma world that's spelled as one word"));
        assert!(prompt.contains("Preprocessed draft:"));
        assert!(prompt.contains("HelloWorld"));
        assert!(prompt.contains("Return only the final text"));
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
        assert!(prompt.contains("Raw transcript:"));
        assert!(!prompt.contains("Preprocessed draft:"));
    }

    #[test]
    fn ai_prompt_uses_raw_transcript_placeholder() {
        let prompt = build_ai_formatting_prompt("hello comma world");

        assert!(prompt.contains("fix transcription errors"));
        assert!(prompt.contains("hello comma world"));
        assert!(!prompt.contains("{transcript}"));
    }

    #[test]
    fn cleans_common_prefix_but_preserves_markdown_fences() {
        assert_eq!(clean_model_output("Final text: Hello."), "Hello.");
        assert_eq!(clean_model_output("```\nHello.\n```"), "```\nHello.\n```");
        assert_eq!(clean_model_output("Hel\u{1b}[1D\u{1b}[Klo\r."), "Hello.");
    }
}
