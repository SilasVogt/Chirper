use chirper_core::{ChirperResult, DictationMode, Formatter, Transcript, VocabularyEntry};

#[derive(Debug, Clone, Copy, Default)]
pub struct RuleFormatter;

impl Formatter for RuleFormatter {
    fn format(&self, transcript: &Transcript, mode: DictationMode) -> ChirperResult<String> {
        Ok(format_spoken_rules(&transcript.text, mode))
    }
}

pub fn format_spoken_rules(text: &str, mode: DictationMode) -> String {
    format_spoken_rules_with_vocabulary(text, mode, &[])
}

pub fn format_spoken_rules_with_vocabulary(
    text: &str,
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
) -> String {
    let tokens = text.split_whitespace().map(Token::new).collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        if let Some((list_pieces, consumed)) = match_spoken_list(&tokens[index..], mode, vocabulary)
        {
            if !pieces.is_empty() && !matches!(pieces.last(), Some(RenderPiece::Newline(_))) {
                pieces.push(RenderPiece::Newline(1));
            }
            pieces.extend(list_pieces);
            index += consumed;
        } else if let Some(consumed) = match_scratch_command(&tokens[index..]) {
            pieces.clear();
            index += consumed;
        } else if let Some(consumed) = match_list_end(&tokens[index..]) {
            index += consumed;
        } else if let Some((correction, consumed, terminator)) =
            match_spelling_command(&tokens[index..])
        {
            replace_previous_identifier_phrase(&mut pieces, correction);
            if let Some(terminator) = terminator {
                pieces.push(RenderPiece::Punct(terminator));
            }
            index += consumed;
        } else if let Some(consumed) = match_pascal_case_command(&tokens[index..]) {
            let terminator = terminal_punctuation(&tokens[index + consumed - 1].raw);
            apply_pascal_case(&mut pieces);
            if let Some(terminator) = terminator {
                pieces.push(RenderPiece::Punct(terminator));
            }
            index += consumed;
        } else if let Some((piece, consumed)) = match_vocabulary(&tokens[index..], vocabulary) {
            pieces.push(piece);
            if let Some(punctuation) = trailing_punctuation(&tokens[index + consumed - 1].raw) {
                pieces.push(RenderPiece::Punct(punctuation));
            }
            index += consumed;
        } else if let Some((piece, consumed)) = match_command(&tokens[index..], mode) {
            pieces.push(piece);
            index += consumed;
        } else {
            pieces.push(RenderPiece::Word(tokens[index].raw.clone()));
            index += 1;
        }
    }

    rewrite_contextual_phrases(render(&pieces, mode))
}

pub fn learn_spelling_vocabulary(text: &str) -> Vec<VocabularyEntry> {
    let tokens = text.split_whitespace().map(Token::new).collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut entries = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        if let Some(consumed) = match_scratch_command(&tokens[index..]) {
            pieces.clear();
            index += consumed;
        } else if let Some((correction, consumed, _terminator)) =
            match_spelling_command(&tokens[index..])
        {
            let written = correction.written.clone();
            if let Some(spoken) = replace_previous_identifier_phrase(&mut pieces, correction) {
                entries.push(VocabularyEntry { spoken, written });
            }
            index += consumed;
        } else {
            pieces.push(RenderPiece::Word(tokens[index].raw.clone()));
            index += 1;
        }
    }

    dedupe_entries(entries)
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

fn match_scratch_command(tokens: &[Token]) -> Option<usize> {
    let first = tokens.first()?.normalized.as_str();
    let second = tokens.get(1).map(|token| token.normalized.as_str());
    let third = tokens.get(2).map(|token| token.normalized.as_str());
    let fourth = tokens.get(3).map(|token| token.normalized.as_str());

    match (first, second, third, fourth) {
        (
            "scratch" | "delete",
            Some("that" | "this" | "it"),
            Some("last"),
            Some("sentence" | "phrase" | "part"),
        ) => Some(4),
        ("scratch" | "delete", Some("that" | "this" | "it"), _, _) => Some(2),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpokenListKind {
    Bullet,
    Numbered,
}

fn match_spoken_list(
    tokens: &[Token],
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
) -> Option<(Vec<RenderPiece>, usize)> {
    let (kind, mut index) = match_list_start(tokens)?;
    let title = match_list_title(tokens, &mut index, mode, vocabulary);
    let (items, consumed) = collect_list_items(&tokens[index..], mode, vocabulary)?;

    if items.is_empty() {
        return None;
    }

    let mut pieces = Vec::new();
    if let Some(title) = title {
        pieces.push(RenderPiece::Word(title));
        pieces.push(RenderPiece::Punct(":"));
        pieces.push(RenderPiece::Newline(1));
    }

    for (item_index, item) in items.into_iter().enumerate() {
        let marker = match kind {
            SpokenListKind::Bullet => "-".to_string(),
            SpokenListKind::Numbered => format!("{}.", item_index + 1),
        };
        pieces.push(RenderPiece::Word(marker));
        pieces.push(RenderPiece::Word(item));
        pieces.push(RenderPiece::Newline(1));
    }

    Some((pieces, index + consumed))
}

fn match_list_start(tokens: &[Token]) -> Option<(SpokenListKind, usize)> {
    for prefix in [0, 2, 3] {
        if prefix == 2
            && !(tokens.first()?.normalized == "this" && tokens.get(1)?.normalized == "is")
        {
            continue;
        }
        if prefix == 3
            && !(tokens.first()?.normalized == "this"
                && tokens.get(1)?.normalized == "is"
                && matches!(tokens.get(2)?.normalized.as_str(), "a" | "the"))
        {
            continue;
        }

        if let Some((kind, consumed)) = match_list_kind(&tokens[prefix..]) {
            return Some((kind, prefix + consumed));
        }
    }

    None
}

fn match_list_kind(tokens: &[Token]) -> Option<(SpokenListKind, usize)> {
    let first = tokens.first()?.normalized.as_str();
    let second = tokens.get(1).map(|token| token.normalized.as_str());
    let third = tokens.get(2).map(|token| token.normalized.as_str());

    match (first, second, third) {
        ("bullet", Some("point"), Some("list")) => Some((SpokenListKind::Bullet, 3)),
        ("bullet", Some("list"), _) => Some((SpokenListKind::Bullet, 2)),
        ("numbered" | "ordered", Some("list"), _) => Some((SpokenListKind::Numbered, 2)),
        ("number", Some("list"), _) => Some((SpokenListKind::Numbered, 2)),
        _ => None,
    }
}

fn match_list_title(
    tokens: &[Token],
    index: &mut usize,
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
) -> Option<String> {
    let title_start = match tokens.get(*index).map(|token| token.normalized.as_str()) {
        Some("titled") | Some("called") => *index + 1,
        Some("with")
            if tokens
                .get(*index + 1)
                .is_some_and(|token| token.normalized == "title") =>
        {
            *index + 2
        }
        _ => return None,
    };

    let mut title_end = title_start;
    while title_end < tokens.len() {
        if is_list_boundary_token(&tokens[title_end]) {
            title_end += 1;
            break;
        }
        title_end += 1;
    }

    if title_end == title_start || title_end >= tokens.len() {
        return None;
    }

    let title = render_list_item(&tokens[title_start..title_end], mode, vocabulary)?;
    *index = title_end;
    Some(title)
}

fn collect_list_items(
    tokens: &[Token],
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
) -> Option<(Vec<String>, usize)> {
    let mut items = Vec::new();
    let mut current_start = 0;
    let mut index = 0;

    while index < tokens.len() {
        if let Some(end_consumed) = match_list_end(&tokens[index..]) {
            push_list_item(&mut items, &tokens[current_start..index], mode, vocabulary);
            return Some((items, index + end_consumed));
        }

        if match_scratch_command(&tokens[index..]).is_some() {
            return None;
        }

        if let Some(separator_consumed) = match_spoken_item_separator(&tokens[index..]) {
            push_list_item(&mut items, &tokens[current_start..index], mode, vocabulary);
            index += separator_consumed;
            current_start = index;
            continue;
        }

        if is_list_boundary_token(&tokens[index]) {
            push_list_item(&mut items, &tokens[current_start..=index], mode, vocabulary);
            index += 1;
            if terminal_punctuation(&tokens[index - 1].raw).is_some() && index < tokens.len() {
                if let Some(separator_consumed) = match_spoken_item_separator(&tokens[index..]) {
                    index += separator_consumed;
                    current_start = index;
                    continue;
                }

                return Some((items, index));
            }
            current_start = index;
            continue;
        }

        index += 1;
    }

    if current_start < tokens.len() {
        push_list_item(&mut items, &tokens[current_start..], mode, vocabulary);
    }

    (!items.is_empty()).then_some((items, index))
}

fn push_list_item(
    items: &mut Vec<String>,
    tokens: &[Token],
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
) {
    if let Some(item) = render_list_item(tokens, mode, vocabulary) {
        items.push(item);
    }
}

fn render_list_item(
    tokens: &[Token],
    mode: DictationMode,
    vocabulary: &[VocabularyEntry],
) -> Option<String> {
    let mut raw = tokens
        .iter()
        .map(|token| token.raw.as_str())
        .collect::<Vec<_>>();

    while raw
        .first()
        .is_some_and(|token| matches!(normalize(token).as_str(), "and" | "or"))
    {
        raw.remove(0);
    }

    while raw
        .last()
        .is_some_and(|token| normalize(token).is_empty() || is_spoken_separator_word(token))
    {
        raw.pop();
    }

    let last = raw.last_mut()?;
    *last = last.trim_end_matches([',', '.', '?', '!', ':', ';']);

    let text = raw
        .iter()
        .copied()
        .filter(|token| !token.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if text.trim().is_empty() {
        return None;
    }

    let rendered = format_spoken_rules_with_vocabulary(&text, mode, vocabulary);
    (!rendered.trim().is_empty()).then(|| rendered.trim().to_string())
}

fn match_list_end(tokens: &[Token]) -> Option<usize> {
    let first = tokens.first()?.normalized.as_str();
    let second = tokens.get(1).map(|token| token.normalized.as_str());
    let third = tokens.get(2).map(|token| token.normalized.as_str());

    match (first, second, third) {
        ("end", Some("of"), Some("list")) => Some(3),
        ("end", Some("list"), _) => Some(2),
        _ => None,
    }
}

fn match_spoken_item_separator(tokens: &[Token]) -> Option<usize> {
    let first = tokens.first()?.normalized.as_str();
    let second = tokens.get(1).map(|token| token.normalized.as_str());

    match (first, second) {
        ("comma" | "period" | "semicolon", _) => Some(1),
        ("new", Some("line")) => Some(2),
        ("next", Some("item")) => Some(2),
        _ => None,
    }
}

fn is_list_boundary_token(token: &Token) -> bool {
    trailing_punctuation(&token.raw).is_some()
}

fn is_spoken_separator_word(token: &str) -> bool {
    matches!(normalize(token).as_str(), "comma" | "period" | "semicolon")
}

fn match_pascal_case_command(tokens: &[Token]) -> Option<usize> {
    let first = tokens.first()?.normalized.as_str();
    let second = tokens.get(1).map(|token| token.normalized.as_str());
    let third = tokens.get(2).map(|token| token.normalized.as_str());

    if matches!(first, "that's" | "thats")
        && second == Some("spelled")
        && third == Some("as")
        && tokens.get(3).map(|token| token.normalized.as_str()) == Some("one")
        && tokens.get(4).map(|token| token.normalized.as_str()) == Some("word")
        && tokens.get(5).map(|token| token.normalized.as_str()) == Some("in")
        && tokens.get(6).map(|token| token.normalized.as_str()) == Some("pascal")
        && tokens.get(7).map(|token| token.normalized.as_str()) == Some("case")
    {
        return Some(8);
    }

    if first == "that"
        && second == Some("is")
        && third == Some("spelled")
        && tokens.get(3).map(|token| token.normalized.as_str()) == Some("as")
        && tokens.get(4).map(|token| token.normalized.as_str()) == Some("one")
        && tokens.get(5).map(|token| token.normalized.as_str()) == Some("word")
        && tokens.get(6).map(|token| token.normalized.as_str()) == Some("in")
        && tokens.get(7).map(|token| token.normalized.as_str()) == Some("pascal")
        && tokens.get(8).map(|token| token.normalized.as_str()) == Some("case")
    {
        return Some(9);
    }

    match (first, second, third) {
        ("in", Some("pascal"), Some("case")) => Some(3),
        ("in", Some("pascalcase"), _) => Some(2),
        ("pascal", Some("case"), _) => Some(2),
        ("pascalcase", _, _) => Some(1),
        _ => None,
    }
}

fn match_spelling_command(
    tokens: &[Token],
) -> Option<(SpellingCorrection, usize, Option<&'static str>)> {
    let first = tokens.first()?.normalized.as_str();
    let second = tokens.get(1).map(|token| token.normalized.as_str());
    let third = tokens.get(2).map(|token| token.normalized.as_str());

    let value_start = match (first, second, third) {
        ("spelled" | "spelt", Some("as"), _) => 2,
        ("spelled" | "spelt", _, _) => 1,
        ("is", Some("spelled" | "spelt"), Some("as")) => 3,
        ("is", Some("spelled" | "spelt"), _) => 2,
        ("should", Some("be"), Some("spelled" | "spelt")) => 3,
        ("written", Some("as"), _) => 2,
        ("written", _, _) => 1,
        _ => return None,
    };

    let (correction, consumed, terminator) = parse_spelling_value(&tokens[value_start..])?;

    Some((correction, value_start + consumed, terminator))
}

fn parse_spelling_value(
    tokens: &[Token],
) -> Option<(SpellingCorrection, usize, Option<&'static str>)> {
    let mut values = Vec::new();
    let mut index = 0;
    let mut any_explicit_case = false;
    let mut all_caps = false;
    let mut terminator = None;

    if tokens.first().map(|token| token.normalized.as_str()) == Some("all")
        && tokens.get(1).map(|token| token.normalized.as_str()) == Some("caps")
    {
        all_caps = true;
        index = 2;
    }

    while index < tokens.len() {
        let token = &tokens[index];

        if matches!(token.normalized.as_str(), "capital" | "uppercase" | "upper") {
            let next = tokens.get(index + 1)?;
            let Some(character) = letter_token(next) else {
                break;
            };
            values.push(SpelledValue {
                character,
                uppercase: true,
            });
            any_explicit_case = true;
            terminator = trailing_punctuation(&next.raw);
            index += 2;

            if terminator.is_some() {
                break;
            }

            continue;
        }

        if matches!(token.normalized.as_str(), "lowercase" | "lower") {
            let next = tokens.get(index + 1)?;
            let Some(character) = letter_token(next) else {
                break;
            };
            values.push(SpelledValue {
                character,
                uppercase: false,
            });
            any_explicit_case = true;
            terminator = trailing_punctuation(&next.raw);
            index += 2;

            if terminator.is_some() {
                break;
            }

            continue;
        }

        let Some(character) = letter_token(token) else {
            break;
        };
        values.push(SpelledValue {
            character,
            uppercase: false,
        });
        terminator = trailing_punctuation(&token.raw);
        index += 1;

        if terminator.is_some() {
            break;
        }
    }

    if values.is_empty() {
        return None;
    }

    Some((
        SpellingCorrection {
            written: render_spelled_value(&values, all_caps, any_explicit_case),
            allow_phrase_case: !all_caps && !any_explicit_case,
        },
        index,
        terminator,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpellingCorrection {
    written: String,
    allow_phrase_case: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpelledValue {
    character: char,
    uppercase: bool,
}

fn letter_token(token: &Token) -> Option<char> {
    let mut chars = token.normalized.chars();
    let character = chars.next()?;

    if chars.next().is_none() && character.is_ascii_alphanumeric() {
        Some(character)
    } else {
        None
    }
}

fn render_spelled_value(
    values: &[SpelledValue],
    all_caps: bool,
    any_explicit_case: bool,
) -> String {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if !value.character.is_ascii_alphabetic() {
                return value.character.to_string();
            }

            if all_caps || value.uppercase || (!any_explicit_case && index == 0) {
                value.character.to_ascii_uppercase().to_string()
            } else {
                value.character.to_ascii_lowercase().to_string()
            }
        })
        .collect()
}

fn match_vocabulary(
    tokens: &[Token],
    vocabulary: &[VocabularyEntry],
) -> Option<(RenderPiece, usize)> {
    for entry in vocabulary {
        let spoken = entry
            .spoken
            .split_whitespace()
            .map(normalize)
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();

        if spoken.is_empty() || spoken.len() > tokens.len() {
            continue;
        }

        if spoken
            .iter()
            .zip(tokens)
            .all(|(spoken, token)| spoken == &token.normalized)
        {
            if match_spelling_command(&tokens[spoken.len()..]).is_some() {
                return None;
            }

            return Some((RenderPiece::Word(entry.written.clone()), spoken.len()));
        }
    }

    None
}

fn apply_pascal_case(pieces: &mut Vec<RenderPiece>) {
    trim_trailing_weak_punctuation(pieces);

    let Some((start, end)) = identifier_phrase_range(pieces) else {
        return;
    };
    let words = pieces[start..end]
        .iter()
        .filter_map(|piece| match piece {
            RenderPiece::Word(word) => identifier_word(word),
            _ => None,
        })
        .collect::<Vec<_>>();

    if words.is_empty() {
        return;
    }

    pieces.splice(
        start..end,
        [RenderPiece::Word(words_to_pascal_case(&words))],
    );
}

fn replace_previous_identifier_phrase(
    pieces: &mut Vec<RenderPiece>,
    correction: SpellingCorrection,
) -> Option<String> {
    trim_trailing_weak_punctuation(pieces);

    let (start, end) = identifier_phrase_range(pieces)?;
    let spoken = spoken_phrase_from_pieces(&pieces[start..end])?;
    let words = identifier_words_from_pieces(&pieces[start..end]);
    let written = if correction.allow_phrase_case {
        phrase_case_spelling(&correction.written, &words).unwrap_or(correction.written)
    } else {
        correction.written
    };
    pieces.splice(start..end, [RenderPiece::Word(written)]);

    Some(spoken)
}

fn spoken_phrase_from_pieces(pieces: &[RenderPiece]) -> Option<String> {
    let words = identifier_words_from_pieces(pieces)
        .into_iter()
        .map(|word| normalize(&word))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

fn identifier_words_from_pieces(pieces: &[RenderPiece]) -> Vec<String> {
    pieces
        .iter()
        .filter_map(|piece| match piece {
            RenderPiece::Word(word) => identifier_word(word),
            _ => None,
        })
        .collect()
}

fn phrase_case_spelling(written: &str, phrase_words: &[String]) -> Option<String> {
    if phrase_words.len() < 2
        || !written
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return None;
    }

    let normalized_words = phrase_words
        .iter()
        .map(|word| normalize(word))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let expected_len = normalized_words
        .iter()
        .map(|word| word.chars().count())
        .sum::<usize>();

    if expected_len != written.chars().count() {
        return None;
    }

    let mut remaining = written.chars();
    let mut output = String::new();

    for word in normalized_words {
        let segment = remaining
            .by_ref()
            .take(word.chars().count())
            .collect::<String>();
        output.push_str(&capitalize_pascal_segment(&segment));
    }

    Some(output)
}

fn trim_trailing_weak_punctuation(pieces: &mut Vec<RenderPiece>) {
    while matches!(pieces.last(), Some(RenderPiece::Punct("," | ":" | ";"))) {
        pieces.pop();
    }
}

fn identifier_phrase_range(pieces: &[RenderPiece]) -> Option<(usize, usize)> {
    let end = pieces.len();
    let mut start = end;
    let mut saw_word = false;

    for index in (0..end).rev() {
        match &pieces[index] {
            RenderPiece::Word(word) => {
                let normalized = normalize(word);
                if normalized.is_empty() {
                    break;
                }

                if saw_word && phrase_boundary_word(&normalized) {
                    break;
                }

                start = index;
                saw_word = true;

                if strong_sentence_end(word) {
                    break;
                }
            }
            RenderPiece::Punct("." | "?" | "!") | RenderPiece::Newline(_) => {
                break;
            }
            RenderPiece::Punct("," | ":" | ";") if !saw_word => {
                start = index;
            }
            _ => {
                if saw_word {
                    break;
                }
            }
        }
    }

    saw_word.then_some((start, end))
}

fn phrase_boundary_word(normalized: &str) -> bool {
    matches!(
        normalized,
        "called" | "named" | "titled" | "is" | "are" | "was" | "were" | "as" | "the" | "a" | "an"
    )
}

fn identifier_word(word: &str) -> Option<String> {
    let trimmed = word.trim_matches(|character: char| !character.is_alphanumeric());

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn words_to_pascal_case(words: &[String]) -> String {
    words
        .iter()
        .flat_map(|word| {
            word.split(|character: char| !character.is_alphanumeric())
                .filter(|segment| !segment.is_empty())
                .map(capitalize_pascal_segment)
        })
        .collect()
}

fn capitalize_pascal_segment(segment: &str) -> String {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut output = String::new();
    output.extend(first.to_uppercase());
    let rest = chars.as_str();

    if segment.chars().any(char::is_uppercase) {
        output.push_str(rest);
    } else {
        output.push_str(&rest.to_ascii_lowercase());
    }

    output
}

fn terminal_punctuation(word: &str) -> Option<&'static str> {
    match word.chars().last()? {
        '.' => Some("."),
        '?' => Some("?"),
        '!' => Some("!"),
        _ => None,
    }
}

fn trailing_punctuation(word: &str) -> Option<&'static str> {
    match word.chars().last()? {
        ',' => Some(","),
        '.' => Some("."),
        '?' => Some("?"),
        '!' => Some("!"),
        ':' => Some(":"),
        ';' => Some(";"),
        _ => None,
    }
}

fn strong_sentence_end(word: &str) -> bool {
    matches!(word.chars().last(), Some('.' | '?' | '!'))
}

fn dedupe_entries(entries: Vec<VocabularyEntry>) -> Vec<VocabularyEntry> {
    let mut deduped = Vec::new();

    for entry in entries {
        if deduped
            .iter()
            .any(|existing: &VocabularyEntry| existing.spoken == entry.spoken)
        {
            continue;
        }

        deduped.push(entry);
    }

    deduped
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
        ("dot", Some(tld), _) if known_domain_tld(tld) => Some((RenderPiece::Tight("."), 1)),
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

fn rewrite_contextual_phrases(text: String) -> String {
    rewrite_domain_prepositions(rewrite_domain_tokens(rewrite_email_phrases(text)))
}

fn rewrite_email_phrases(text: String) -> String {
    let mut segments = split_text_segments(&text);
    let mut tokens = word_segment_indexes(&segments);
    let mut index = 0;

    while index < tokens.len() {
        if !segments[tokens[index]].value.eq_ignore_ascii_case("email") {
            index += 1;
            continue;
        }

        if index + 5 < tokens.len()
            && segments[tokens[index + 1]].value.eq_ignore_ascii_case("me")
            && matches_ignore_ascii_case(&segments[tokens[index + 2]].value, &["on", "at"])
            && segments[tokens[index + 4]].value.eq_ignore_ascii_case("at")
            && domain_token(&segments[tokens[index + 5]].value).is_some()
        {
            let local = email_part_token(&segments[tokens[index + 3]].value);
            let (domain, punctuation) = domain_token(&segments[tokens[index + 5]].value).unwrap();
            segments[tokens[index + 2]].value = "at".to_string();
            segments[tokens[index + 3]].value = format!(
                "{}@{}{}",
                local.to_ascii_lowercase(),
                domain.to_ascii_lowercase(),
                punctuation
            );
            clear_word_and_preceding_whitespace(&mut segments, tokens[index + 4]);
            clear_word_and_preceding_whitespace(&mut segments, tokens[index + 5]);
            tokens = word_segment_indexes(&segments);
            index += 4;
            continue;
        }

        if index + 3 < tokens.len()
            && !segments[tokens[index + 1]].value.eq_ignore_ascii_case("me")
            && segments[tokens[index + 2]].value.eq_ignore_ascii_case("at")
            && domain_token(&segments[tokens[index + 3]].value).is_some()
        {
            let local = email_part_token(&segments[tokens[index + 1]].value);
            let (domain, punctuation) = domain_token(&segments[tokens[index + 3]].value).unwrap();
            segments[tokens[index + 1]].value = format!(
                "{}@{}{}",
                local.to_ascii_lowercase(),
                domain.to_ascii_lowercase(),
                punctuation
            );
            clear_word_and_preceding_whitespace(&mut segments, tokens[index + 2]);
            clear_word_and_preceding_whitespace(&mut segments, tokens[index + 3]);
            tokens = word_segment_indexes(&segments);
            index += 2;
            continue;
        }

        index += 1;
    }

    render_text_segments(&segments)
}

fn rewrite_domain_tokens(text: String) -> String {
    let mut segments = split_text_segments(&text);

    for segment in segments.iter_mut().filter(|segment| !segment.whitespace) {
        if let Some((domain, punctuation)) = domain_token(&segment.value) {
            segment.value = format!("{}{}", domain.to_ascii_lowercase(), punctuation);
        }
    }

    render_text_segments(&segments)
}

fn rewrite_domain_prepositions(text: String) -> String {
    let mut segments = split_text_segments(&text);
    let tokens = word_segment_indexes(&segments);

    for index in 0..tokens.len().saturating_sub(1) {
        if !segments[tokens[index]].value.eq_ignore_ascii_case("on")
            || domain_token(&segments[tokens[index + 1]].value).is_none()
            || !domain_preposition_context(&segments, &tokens, index)
        {
            continue;
        }

        segments[tokens[index]].value = "at".to_string();
    }

    render_text_segments(&segments)
}

fn domain_preposition_context(segments: &[TextSegment], tokens: &[usize], index: usize) -> bool {
    let previous = index
        .checked_sub(1)
        .map(|previous| segments[tokens[previous]].value.to_ascii_lowercase());
    let previous_two = index
        .checked_sub(2)
        .map(|previous| segments[tokens[previous]].value.to_ascii_lowercase());

    matches!(
        previous.as_deref(),
        Some("media" | "site" | "website" | "blog" | "profile")
    ) || matches!(
        (previous_two.as_deref(), previous.as_deref()),
        (Some("working"), Some("on"))
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextSegment {
    value: String,
    whitespace: bool,
}

fn split_text_segments(text: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_whitespace = None;

    for character in text.chars() {
        let whitespace = character.is_whitespace();
        if current_whitespace == Some(whitespace) {
            current.push(character);
            continue;
        }

        if let Some(previous_whitespace) = current_whitespace {
            segments.push(TextSegment {
                value: std::mem::take(&mut current),
                whitespace: previous_whitespace,
            });
        }

        current.push(character);
        current_whitespace = Some(whitespace);
    }

    if let Some(whitespace) = current_whitespace {
        segments.push(TextSegment {
            value: current,
            whitespace,
        });
    }

    segments
}

fn word_segment_indexes(segments: &[TextSegment]) -> Vec<usize> {
    segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            (!segment.whitespace && !segment.value.is_empty()).then_some(index)
        })
        .collect()
}

fn clear_word_and_preceding_whitespace(segments: &mut [TextSegment], word_index: usize) {
    segments[word_index].value.clear();

    if word_index > 0 && segments[word_index - 1].whitespace {
        segments[word_index - 1].value.clear();
    }
}

fn render_text_segments(segments: &[TextSegment]) -> String {
    segments
        .iter()
        .map(|segment| segment.value.as_str())
        .collect::<String>()
}

fn matches_ignore_ascii_case(value: &str, options: &[&str]) -> bool {
    options
        .iter()
        .any(|option| value.eq_ignore_ascii_case(option))
}

fn email_part_token(token: &str) -> String {
    token
        .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
        .to_string()
}

fn domain_token(token: &str) -> Option<(String, &'static str)> {
    let punctuation = trailing_punctuation(token).unwrap_or("");
    let domain = token.trim_end_matches([',', '.', '?', '!', ':', ';']);

    if known_domain(domain) {
        Some((domain.to_string(), punctuation))
    } else {
        None
    }
}

fn known_domain(domain: &str) -> bool {
    let Some((name, tld)) = domain.rsplit_once('.') else {
        return false;
    };

    !name.is_empty()
        && !tld.is_empty()
        && domain
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
        && known_domain_tld(&tld.to_ascii_lowercase())
}

fn known_domain_tld(tld: &str) -> bool {
    matches!(
        tld,
        "ai" | "app"
            | "cloud"
            | "co"
            | "com"
            | "dev"
            | "gg"
            | "io"
            | "local"
            | "me"
            | "net"
            | "org"
            | "site"
            | "social"
            | "systems"
            | "tech"
            | "tv"
            | "uk"
            | "xyz"
    )
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
    fn spoken_list_ranges_become_markdown_lists() {
        assert_eq!(
            format_spoken_rules(
                "I need to write down accent-friendly words. This is a bullet point list: water, tomato, schedule, data, router, aluminium, privacy. End of list.",
                DictationMode::Auto,
            ),
            "I need to write down accent-friendly words.\n- water\n- tomato\n- schedule\n- data\n- router\n- aluminium\n- privacy"
        );
        assert_eq!(
            format_spoken_rules(
                "This is a bullet point list with title Accent Friendly Words: water, tomato. End of list. Note below.",
                DictationMode::Auto,
            ),
            "Accent Friendly Words:\n- water\n- tomato\nNote below."
        );
        assert_eq!(
            format_spoken_rules(
                "This is a numbered list: push to PR, releases and nightly builds, auto update mechanism. End of list.",
                DictationMode::Auto,
            ),
            "1. push to PR\n2. releases and nightly builds\n3. auto update mechanism"
        );
        assert_eq!(
            format_spoken_rules(
                "This is a bullet point list: apples, oranges, bananas.",
                DictationMode::Auto,
            ),
            "- apples\n- oranges\n- bananas"
        );
        assert_eq!(
            format_spoken_rules(
                "This is a bullet point list: apples, oranges, bananas. Please reply to John.",
                DictationMode::Auto,
            ),
            "- apples\n- oranges\n- bananas\nPlease reply to John."
        );
        assert_eq!(
            format_spoken_rules(
                "This is a bullet point list: finish by Friday. next item include screenshots.",
                DictationMode::Auto,
            ),
            "- finish by Friday\n- include screenshots"
        );
    }

    #[test]
    fn scratch_that_clears_recording_so_far() {
        assert_eq!(
            format_spoken_rules(
                "Hello there. My name is Silas, I run a YouTube, sorry, scratch that. Hello, my name is Silas",
                DictationMode::Auto
            ),
            "Hello, my name is Silas"
        );
        assert_eq!(
            format_spoken_rules("wrong start delete that correct start", DictationMode::Auto),
            "correct start"
        );
        assert_eq!(
            format_spoken_rules(
                "wrong start scratch that last sentence correct start",
                DictationMode::Auto
            ),
            "correct start"
        );
        assert_eq!(
            format_spoken_rules(
                "This is a bullet point list: first item, scratch that, end of list. correct start",
                DictationMode::Auto
            ),
            "correct start"
        );
    }

    #[test]
    fn pascal_case_command_transforms_previous_phrase() {
        assert_eq!(
            format_spoken_rules(
                "channel called Silas on Linux Pascal Case",
                DictationMode::Auto
            ),
            "channel called SilasOnLinux"
        );
        assert_eq!(
            format_spoken_rules(
                "channel called Silas on Linux, Pascal Case. You can find it",
                DictationMode::Auto
            ),
            "channel called SilasOnLinux. You can find it"
        );
        assert_eq!(
            format_spoken_rules(
                "my name is silas on linux in pascal case",
                DictationMode::Auto
            ),
            "my name is SilasOnLinux"
        );
        assert_eq!(
            format_spoken_rules(
                "channel called Silas on Linux that's spelled as one word in Pascal case",
                DictationMode::Auto
            ),
            "channel called SilasOnLinux"
        );
        assert_eq!(
            format_spoken_rules(
                "channel called Silas on Linux that is spelled as one word in Pascal case",
                DictationMode::Auto
            ),
            "channel called SilasOnLinux"
        );
    }

    #[test]
    fn spelling_command_rewrites_and_can_be_learned() {
        assert_eq!(
            format_spoken_rules(
                "the app is called prepped spelled p r e p d.",
                DictationMode::Auto
            ),
            "the app is called Prepd."
        );
        assert_eq!(
            format_spoken_rules(
                "channel called silas on linux spelled capital s i l a s capital o n capital l i n u x",
                DictationMode::Auto
            ),
            "channel called SilasOnLinux"
        );
        assert_eq!(
            format_spoken_rules(
                "channel called silas on linux spelled s i l a s o n l i n u x",
                DictationMode::Auto
            ),
            "channel called SilasOnLinux"
        );

        let entries = learn_spelling_vocabulary(
            "the app is called prepped spelled p r e p d. then keep talking",
        );

        assert_eq!(
            entries,
            vec![VocabularyEntry {
                spoken: "prepped".to_string(),
                written: "Prepd".to_string(),
            }]
        );
    }

    #[test]
    fn configured_vocabulary_rewrites_spoken_phrases() {
        let vocabulary = vec![
            VocabularyEntry {
                spoken: "silas on linux".to_string(),
                written: "SilasOnLinux".to_string(),
            },
            VocabularyEntry {
                spoken: "prepped".to_string(),
                written: "Prepd".to_string(),
            },
        ];

        assert_eq!(
            format_spoken_rules_with_vocabulary(
                "Silas on Linux uses prepped.",
                DictationMode::Auto,
                &vocabulary,
            ),
            "SilasOnLinux uses Prepd."
        );
        assert_eq!(
            format_spoken_rules_with_vocabulary(
                "Silas on Linux spelled s i l a s o n l i n u x",
                DictationMode::Auto,
                &vocabulary,
            ),
            "SilasOnLinux"
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
    fn domain_words_render_as_lowercase_domains() {
        assert_eq!(
            format_spoken_rules(
                "visit Silas dot systems and Silas dot GG",
                DictationMode::Auto
            ),
            "visit silas.systems and silas.gg"
        );
    }

    #[test]
    fn email_context_rewrites_spoken_address() {
        assert_eq!(
            format_spoken_rules(
                "email me on Silas at Silas dot systems",
                DictationMode::Auto
            ),
            "email me at silas@silas.systems"
        );
        assert_eq!(
            format_spoken_rules(
                "or you can email Silas at Silas dot systems.",
                DictationMode::Auto
            ),
            "or you can email silas@silas.systems."
        );
        assert_eq!(
            format_spoken_rules("email me at example dot com", DictationMode::Auto),
            "email me at example.com"
        );
    }

    #[test]
    fn domain_contact_phrases_use_at_preposition() {
        assert_eq!(
            format_spoken_rules(
                "apps I am working on on Silas dot systems and social media on Silas dot GG",
                DictationMode::Auto
            ),
            "apps I am working on at silas.systems and social media at silas.gg"
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
