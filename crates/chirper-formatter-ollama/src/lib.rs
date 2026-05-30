use std::process::{Command, Stdio};

use chirper_core::{
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaFormatter {
    options: OllamaOptions,
}

impl OllamaFormatter {
    pub fn new(options: OllamaOptions) -> Self {
        Self { options }
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

impl Formatter for OllamaFormatter {
    fn format(&self, transcript: &Transcript, mode: DictationMode) -> ChirperResult<String> {
        self.ensure_model_installed()?;

        let prompt = build_formatting_prompt(&transcript.text, mode, &self.options.vocabulary);
        let output = Command::new(&self.options.command)
            .arg("run")
            .arg("--nowordwrap")
            .arg("--hidethinking")
            .arg(&self.options.model)
            .arg(prompt)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| {
                ChirperError::Formatting(format!(
                    "failed to run `{}`: {source}",
                    self.options.command
                ))
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
        if formatted.is_empty() && !transcript.text.trim().is_empty() {
            return Err(ChirperError::Formatting(
                "ollama returned an empty formatter response".to_string(),
            ));
        }

        Ok(formatted)
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
    text: &str,
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
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

    format!(
        "\
You format dictated speech into final text.

Return only the final text. Do not explain, summarize, add facts, or wrap the result in quotes.
You are a conservative proofreader for speech-to-text output, not a rewriting assistant.
Preserve the speaker's meaning, wording, order, and all ordinary content words.
Fix only likely transcription errors, casing, punctuation, spacing, and paragraph breaks.
The input has already passed through Chirper's local rules and preferred spelling preprocessor.
Do not reinterpret spelling commands, duplicate corrected names, or undo existing corrections.
If the text looks like code, shell input, Markdown, a URL, or an email address, preserve that structure.
Preserve existing camelCase and PascalCase identifiers exactly, including product, channel, and project names.
Do not add line breaks unless the input already contains them or the user clearly dictated a paragraph break.
{vocabulary_section}
If the input is empty or contains no speech, return an empty string.

Mode: {mode:?}

Input:
<<<
{text}
>>>

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
            "hello comma world",
            DictationMode::Standard,
            &[VocabularyEntry {
                spoken: "silas on linux".to_string(),
                written: "SilasOnLinux".to_string(),
            }],
        );

        assert!(prompt.contains("Mode: Standard"));
        assert!(prompt.contains("hello comma world"));
        assert!(prompt.contains("Return only the final text"));
        assert!(prompt.contains("\"silas on linux\" => \"SilasOnLinux\""));
    }

    #[test]
    fn cleans_common_prefix_but_preserves_markdown_fences() {
        assert_eq!(clean_model_output("Final text: Hello."), "Hello.");
        assert_eq!(clean_model_output("```\nHello.\n```"), "```\nHello.\n```");
        assert_eq!(clean_model_output("Hel\u{1b}[1D\u{1b}[Klo\r."), "Hello.");
    }
}
