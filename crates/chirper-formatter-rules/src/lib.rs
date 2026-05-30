use chirper_core::{ChirperResult, DictationMode, Formatter, Transcript};

#[derive(Debug, Clone, Copy, Default)]
pub struct RuleFormatter;

impl Formatter for RuleFormatter {
    fn format(&self, transcript: &Transcript, mode: DictationMode) -> ChirperResult<String> {
        Ok(format_spoken_rules(&transcript.text, mode))
    }
}

pub fn format_spoken_rules(text: &str, mode: DictationMode) -> String {
    let tokens = text.split_whitespace().map(Token::new).collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        if let Some((piece, consumed)) = match_command(&tokens[index..], mode) {
            pieces.push(piece);
            index += consumed;
        } else {
            pieces.push(RenderPiece::Word(tokens[index].raw.clone()));
            index += 1;
        }
    }

    render(&pieces, mode)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    raw: String,
    normalized: String,
}

impl Token {
    fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
            normalized: normalize(raw),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RenderPiece {
    Word(String),
    Punct(&'static str),
    Tight(&'static str),
    Spaced(&'static str),
    Open(&'static str),
    Close(&'static str),
    Newline(usize),
    Space,
    NoSpace,
    Tab,
    Raw(&'static str),
}

fn match_command(tokens: &[Token], mode: DictationMode) -> Option<(RenderPiece, usize)> {
    let first = tokens.first()?.normalized.as_str();
    let second = tokens.get(1).map(|token| token.normalized.as_str());
    let third = tokens.get(2).map(|token| token.normalized.as_str());
    let aggressive = aggressive_symbol_mode(mode);

    if first == "literal" {
        if let Some(token) = tokens.get(1) {
            return Some((RenderPiece::Word(token.raw.clone()), 2));
        }
    }

    match (first, second, third) {
        ("new", Some("paragraph"), _) => Some((RenderPiece::Newline(2), 2)),
        ("new", Some("line"), _) => Some((RenderPiece::Newline(1), 2)),
        ("dot", Some("dot"), Some("dot")) => Some((RenderPiece::Punct("..."), 3)),
        ("open", Some("code"), Some("block")) => Some((RenderPiece::Raw("```"), 3)),
        ("close", Some("code"), Some("block")) => Some((RenderPiece::Raw("```"), 3)),
        ("bullet", Some("point"), _) => Some((RenderPiece::Raw("-"), 2)),
        ("checked", Some("box"), _) => Some((RenderPiece::Raw("- [x]"), 2)),
        ("double", Some("colon"), _) => Some((RenderPiece::Tight("::"), 2)),
        ("double", Some("slash"), _) => Some((RenderPiece::Tight("//"), 2)),
        ("question", Some("mark"), _) => Some((RenderPiece::Punct("?"), 2)),
        ("exclamation", Some("mark" | "point"), _) => Some((RenderPiece::Punct("!"), 2)),
        ("full", Some("stop"), _) => Some((RenderPiece::Punct("."), 2)),
        ("em", Some("dash"), _) => Some((RenderPiece::Spaced("—"), 2)),
        ("en", Some("dash"), _) => Some((RenderPiece::Spaced("–"), 2)),
        ("at", Some("sign"), _) => Some((RenderPiece::Tight("@"), 2)),
        ("pound", Some("sign"), _) => Some((RenderPiece::Tight("#"), 2)),
        ("dollar", Some("sign"), _) => Some((RenderPiece::Tight("$"), 2)),
        ("percent", Some("sign"), _) => Some((RenderPiece::Tight("%"), 2)),
        ("and", Some("sign"), _) => Some((RenderPiece::Tight("&"), 2)),
        ("plus", Some("sign"), _) => Some((RenderPiece::Tight("+"), 2)),
        ("minus", Some("sign"), _) => Some((RenderPiece::Tight("-"), 2)),
        ("equal", Some("sign"), _) => Some((RenderPiece::Tight("="), 2)),
        ("forward", Some("slash"), _) => Some((RenderPiece::Tight("/"), 2)),
        ("vertical", Some("bar"), _) => Some((RenderPiece::Tight("|"), 2)),
        ("fat", Some("arrow"), _) => Some((RenderPiece::Tight("=>"), 2)),
        ("triple", Some("dot"), _) => Some((RenderPiece::Punct("..."), 2)),
        ("checkbox", _, _) => Some((RenderPiece::Raw("- [ ]"), 1)),
        ("arrow", _, _) if aggressive => Some((RenderPiece::Tight("->"), 1)),
        ("arrow", _, _) => Some((RenderPiece::Spaced("→"), 1)),
        ("open", Some("square"), Some("bracket")) => Some((RenderPiece::Open("["), 3)),
        ("close", Some("square"), Some("bracket")) => Some((RenderPiece::Close("]"), 3)),
        ("left", Some("square"), Some("bracket")) => Some((RenderPiece::Open("["), 3)),
        ("right", Some("square"), Some("bracket")) => Some((RenderPiece::Close("]"), 3)),
        ("open", Some("curly"), Some("brace" | "bracket")) => Some((RenderPiece::Open("{"), 3)),
        ("close", Some("curly"), Some("brace" | "bracket")) => Some((RenderPiece::Close("}"), 3)),
        ("left", Some("curly"), Some("brace" | "bracket")) => Some((RenderPiece::Open("{"), 3)),
        ("right", Some("curly"), Some("brace" | "bracket")) => Some((RenderPiece::Close("}"), 3)),
        ("open", Some("angle"), Some("bracket")) => Some((RenderPiece::Open("<"), 3)),
        ("close", Some("angle"), Some("bracket")) => Some((RenderPiece::Close(">"), 3)),
        ("left", Some("angle"), Some("bracket")) => Some((RenderPiece::Open("<"), 3)),
        ("right", Some("angle"), Some("bracket")) => Some((RenderPiece::Close(">"), 3)),
        ("angle", Some("bracket"), _) | ("less", Some("than"), _) => {
            Some((RenderPiece::Open("<"), 2))
        }
        ("greater", Some("than"), _) => Some((RenderPiece::Close(">"), 2)),
        ("open", Some("bracket"), _)
        | ("bracket", Some("open"), _)
        | ("left", Some("bracket"), _) => Some((RenderPiece::Open("["), 2)),
        ("close", Some("bracket"), _)
        | ("bracket", Some("close"), _)
        | ("right", Some("bracket"), _) => Some((RenderPiece::Close("]"), 2)),
        ("open", Some("paren" | "parenthesis"), _)
        | ("paren" | "parenthesis", Some("open"), _)
        | ("left", Some("paren" | "parenthesis"), _) => Some((RenderPiece::Open("("), 2)),
        ("close", Some("paren" | "parenthesis"), _)
        | ("paren" | "parenthesis", Some("close"), _)
        | ("right", Some("paren" | "parenthesis"), _) => Some((RenderPiece::Close(")"), 2)),
        ("open", Some("brace"), _) | ("brace", Some("open"), _) | ("left", Some("brace"), _) => {
            Some((RenderPiece::Open("{"), 2))
        }
        ("close", Some("brace"), _) | ("brace", Some("close"), _) | ("right", Some("brace"), _) => {
            Some((RenderPiece::Close("}"), 2))
        }
        ("curly", Some("brace"), _) => Some((RenderPiece::Open("{"), 2)),
        ("double", Some("quote"), _) => Some((RenderPiece::Tight("\""), 2)),
        ("single", Some("quote"), _) => Some((RenderPiece::Tight("'"), 2)),
        ("open" | "start", Some("quote"), _) => Some((RenderPiece::Open("\""), 2)),
        ("close" | "end", Some("quote"), _) => Some((RenderPiece::Close("\""), 2)),
        ("dot", Some("com" | "org" | "net" | "io" | "dev" | "local"), _) => {
            Some((RenderPiece::Tight("."), 1))
        }
        ("no", Some("space"), _) if aggressive => Some((RenderPiece::NoSpace, 2)),
        _ => match first {
            "comma" => Some((RenderPiece::Punct(","), 1)),
            "period" => Some((RenderPiece::Punct("."), 1)),
            "dot" if aggressive => Some((RenderPiece::Tight("."), 1)),
            "colon" if aggressive => Some((RenderPiece::Tight(":"), 1)),
            "colon" => Some((RenderPiece::Punct(":"), 1)),
            "semicolon" => Some((RenderPiece::Punct(";"), 1)),
            "ellipsis" => Some((RenderPiece::Punct("..."), 1)),
            "apostrophe" => Some((RenderPiece::Tight("'"), 1)),
            "unquote" => Some((RenderPiece::Close("\""), 1)),
            "quote" => Some((RenderPiece::Tight("\""), 1)),
            "dash" | "hyphen" => Some((RenderPiece::Tight("-"), 1)),
            "hashtag" => Some((RenderPiece::Tight("#"), 1)),
            "ampersand" => Some((RenderPiece::Tight("&"), 1)),
            "asterisk" => Some((RenderPiece::Tight("*"), 1)),
            "slash" => Some((RenderPiece::Tight("/"), 1)),
            "backslash" => Some((RenderPiece::Tight("\\"), 1)),
            "underscore" => Some((RenderPiece::Tight("_"), 1)),
            "tilde" => Some((RenderPiece::Tight("~"), 1)),
            "caret" => Some((RenderPiece::Tight("^"), 1)),
            "backtick" => Some((RenderPiece::Tight("`"), 1)),
            "at" if aggressive => Some((RenderPiece::Tight("@"), 1)),
            "hash" if aggressive => Some((RenderPiece::Tight("#"), 1)),
            "plus" if aggressive => Some((RenderPiece::Tight("+"), 1)),
            "minus" if aggressive => Some((RenderPiece::Tight("-"), 1)),
            "equals" if aggressive => Some((RenderPiece::Tight("="), 1)),
            "percent" if aggressive => Some((RenderPiece::Tight("%"), 1)),
            "star" if aggressive => Some((RenderPiece::Tight("*"), 1)),
            "pipe" if aggressive => Some((RenderPiece::Tight("|"), 1)),
            "space" if aggressive => Some((RenderPiece::Space, 1)),
            "newline" => Some((RenderPiece::Newline(1), 1)),
            "tab" if aggressive => Some((RenderPiece::Tab, 1)),
            _ => None,
        },
    }
}

fn render(pieces: &[RenderPiece], mode: DictationMode) -> String {
    let compact = matches!(mode, DictationMode::Command | DictationMode::Code);
    let mut output = String::new();
    let mut suppress_next_space = false;

    for piece in pieces {
        match piece {
            RenderPiece::Word(word) => push_word(&mut output, word, &mut suppress_next_space),
            RenderPiece::Punct(value) => {
                push_punct(&mut output, value);
                suppress_next_space = false;
            }
            RenderPiece::Tight(value) => {
                push_tight(&mut output, value);
                suppress_next_space = true;
            }
            RenderPiece::Spaced(value) => {
                push_spaced(&mut output, value);
                suppress_next_space = false;
            }
            RenderPiece::Open(value) => {
                push_open(&mut output, value, compact);
                suppress_next_space = true;
            }
            RenderPiece::Close(value) => {
                push_close(&mut output, value);
                suppress_next_space = false;
            }
            RenderPiece::Newline(count) => {
                push_newline(&mut output, *count);
                suppress_next_space = false;
            }
            RenderPiece::Space => {
                push_space(&mut output);
                suppress_next_space = false;
            }
            RenderPiece::NoSpace => {
                trim_trailing_spaces(&mut output);
                suppress_next_space = true;
            }
            RenderPiece::Tab => {
                output.push('\t');
                suppress_next_space = false;
            }
            RenderPiece::Raw(value) => {
                push_word(&mut output, value, &mut suppress_next_space);
            }
        }
    }

    output.trim().to_string()
}

fn push_word(output: &mut String, word: &str, suppress_next_space: &mut bool) {
    if *suppress_next_space {
        *suppress_next_space = false;
    } else if needs_space_before_word(output) {
        output.push(' ');
    }

    output.push_str(word);
}

fn push_punct(output: &mut String, value: &str) {
    trim_trailing_spaces(output);
    output.push_str(value);
}

fn push_tight(output: &mut String, value: &str) {
    trim_trailing_spaces(output);
    output.push_str(value);
}

fn push_spaced(output: &mut String, value: &str) {
    trim_trailing_spaces(output);
    if needs_space_before_word(output) {
        output.push(' ');
    }
    output.push_str(value);
    output.push(' ');
}

fn push_open(output: &mut String, value: &str, compact: bool) {
    if !compact && needs_space_before_word(output) {
        output.push(' ');
    }

    output.push_str(value);
}

fn push_close(output: &mut String, value: &str) {
    trim_trailing_spaces(output);
    output.push_str(value);
}

fn push_newline(output: &mut String, count: usize) {
    trim_trailing_spaces(output);
    for _ in 0..count {
        output.push('\n');
    }
}

fn push_space(output: &mut String) {
    trim_trailing_spaces(output);
    if !output.is_empty() && !output.ends_with('\n') {
        output.push(' ');
    }
}

fn needs_space_before_word(output: &str) -> bool {
    output
        .chars()
        .last()
        .is_some_and(|last| !last.is_whitespace() && !matches!(last, '(' | '[' | '{' | '<' | '\t'))
}

fn trim_trailing_spaces(output: &mut String) {
    while output.ends_with(' ') || output.ends_with('\t') {
        output.pop();
    }
}

fn normalize(value: &str) -> String {
    value
        .trim_matches(|character: char| !character.is_alphanumeric())
        .to_ascii_lowercase()
}

fn aggressive_symbol_mode(mode: DictationMode) -> bool {
    matches!(mode, DictationMode::Command | DictationMode::Code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_common_punctuation() {
        assert_eq!(
            format_spoken_rules("hello comma world period", DictationMode::Auto),
            "hello, world."
        );
    }

    #[test]
    fn handles_expanded_safe_punctuation() {
        assert_eq!(
            format_spoken_rules(
                "wait em dash really question mark yes exclamation point",
                DictationMode::Auto
            ),
            "wait — really? yes!"
        );
        assert_eq!(
            format_spoken_rules("pause dot dot dot continue", DictationMode::Auto),
            "pause... continue"
        );
    }

    #[test]
    fn literal_escapes_next_command_word() {
        assert_eq!(
            format_spoken_rules("literal comma comma literal period", DictationMode::Auto),
            "comma, period"
        );
    }

    #[test]
    fn replaces_bracket_open_orderings() {
        assert_eq!(
            format_spoken_rules(
                "items bracket open one comma two bracket close",
                DictationMode::Auto
            ),
            "items [one, two]"
        );
    }

    #[test]
    fn handles_newlines_and_paragraphs() {
        assert_eq!(
            format_spoken_rules(
                "hello new line world new paragraph done",
                DictationMode::Auto
            ),
            "hello\nworld\n\ndone"
        );
    }

    #[test]
    fn code_mode_keeps_open_paren_tight() {
        assert_eq!(
            format_spoken_rules("print open paren hello close paren", DictationMode::Code),
            "print(hello)"
        );
    }

    #[test]
    fn ambiguous_symbols_are_conservative_in_auto_mode() {
        assert_eq!(
            format_spoken_rules("email at example dot com", DictationMode::Auto),
            "email at example.com"
        );
        assert_eq!(
            format_spoken_rules("look at star", DictationMode::Auto),
            "look at star"
        );
    }

    #[test]
    fn explicit_symbols_work_in_auto_mode() {
        assert_eq!(
            format_spoken_rules(
                "me at sign example dot com slash docs hashtag intro",
                DictationMode::Auto
            ),
            "me@example.com/docs#intro"
        );
    }

    #[test]
    fn code_mode_enables_ambiguous_symbols() {
        assert_eq!(
            format_spoken_rules(
                "user at host colon path slash file dot rs",
                DictationMode::Code
            ),
            "user@host:path/file.rs"
        );
    }

    #[test]
    fn markdown_commands_render_common_markers() {
        assert_eq!(
            format_spoken_rules(
                "checkbox task new line checked box done",
                DictationMode::Auto
            ),
            "- [ ] task\n- [x] done"
        );
    }

    #[test]
    fn arrows_are_mode_aware() {
        assert_eq!(
            format_spoken_rules("go arrow home", DictationMode::Auto),
            "go → home"
        );
        assert_eq!(
            format_spoken_rules("value arrow result", DictationMode::Code),
            "value->result"
        );
        assert_eq!(
            format_spoken_rules("value fat arrow result", DictationMode::Code),
            "value=>result"
        );
    }

    #[test]
    fn quote_commands_are_tight() {
        assert_eq!(
            format_spoken_rules("open quote hello close quote", DictationMode::Auto),
            "\"hello\""
        );
        assert_eq!(
            format_spoken_rules("don apostrophe t", DictationMode::Auto),
            "don't"
        );
    }

    #[test]
    fn whitespace_commands_are_conservative_in_auto_mode() {
        assert_eq!(
            format_spoken_rules("there is no space left in this tab", DictationMode::Auto),
            "there is no space left in this tab"
        );
    }

    #[test]
    fn code_mode_enables_whitespace_commands() {
        assert_eq!(
            format_spoken_rules("foo no space bar space baz tab qux", DictationMode::Code),
            "foobar baz\tqux"
        );
    }

    #[test]
    fn standard_mode_spaces_before_open_paren() {
        assert_eq!(
            format_spoken_rules("see open paren note close paren", DictationMode::Standard),
            "see (note)"
        );
    }

    #[test]
    fn formatter_trait_returns_formatted_text() {
        let transcript = Transcript {
            text: "hello colon world".to_string(),
            language: None,
        };

        assert_eq!(
            RuleFormatter
                .format(&transcript, DictationMode::Auto)
                .unwrap(),
            "hello: world"
        );
    }
}
