//! Static word resolution: brace expansion then quote removal, as bash does before
//! running a command. Words needing runtime state stay raw and are flagged dynamic.

use brush_parser::ParserOptions;
use brush_parser::word::{
    self, BraceExpressionMember, BraceExpressionOrText, WordPiece, WordPieceWithSource,
};

use crate::types::{RawArgv, ResolvedArgv};

const MAX_BRACE_WORDS: usize = 256;
const MAX_BRACE_OPENS: usize = 10;
const MAX_SEQUENCE_DIGITS: usize = 18;

struct Word {
    text: String,
    dynamic: bool,
}

impl Word {
    fn resolved(text: String) -> Self {
        Self {
            text,
            dynamic: false,
        }
    }

    fn dynamic(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            dynamic: true,
        }
    }
}

pub(crate) enum SourceWord {
    Plain(String),
    ProcessSubstitution(String),
}

impl SourceWord {
    fn resolve(&self) -> Vec<Word> {
        match self {
            Self::Plain(text) => resolve_word(text),
            Self::ProcessSubstitution(text) => vec![Word::dynamic(text)],
        }
    }

    fn into_text(self) -> String {
        match self {
            Self::Plain(text) | Self::ProcessSubstitution(text) => text,
        }
    }
}

impl ResolvedArgv {
    pub(crate) fn words(&self) -> impl Iterator<Item = (&str, bool)> {
        let name = self.name.as_deref().map(|name| (name, self.dynamic_name));
        let args = self
            .args
            .iter()
            .enumerate()
            .map(|(i, arg)| (arg.as_str(), self.dynamic_args.contains(&i)));
        name.into_iter().chain(args)
    }
}

pub(crate) fn resolve_argv(raw_name: Option<String>, source_args: Vec<SourceWord>) -> ResolvedArgv {
    let mut words = raw_name
        .iter()
        .flat_map(|w| resolve_word(w))
        .chain(source_args.iter().flat_map(SourceWord::resolve));

    let (name, dynamic_name) = match words.next() {
        Some(w) => (Some(w.text), w.dynamic),
        None => (None, false),
    };
    let mut args = Vec::new();
    let mut dynamic_args = Vec::new();
    for (i, w) in words.enumerate() {
        if w.dynamic {
            dynamic_args.push(i);
        }
        args.push(w.text);
    }

    let raw_args: Vec<String> = source_args.into_iter().map(SourceWord::into_text).collect();
    let raw = (name != raw_name || args != raw_args).then_some(RawArgv {
        name: raw_name,
        args: raw_args,
    });

    ResolvedArgv {
        name,
        args,
        raw,
        dynamic_name,
        dynamic_args,
    }
}

fn resolve_word(raw: &str) -> Vec<Word> {
    match brace_expand(raw) {
        Some(expanded) => expanded
            .iter()
            .filter(|w| !w.is_empty())
            .map(|w| remove_quotes(w).map_or_else(|| Word::dynamic(w), Word::resolved))
            .collect(),
        None => vec![Word::dynamic(raw)],
    }
}

pub(crate) fn brace_expand(raw: &str) -> Option<Vec<String>> {
    let brace_opens = raw.matches('{').count();
    if brace_opens == 0 {
        return Some(vec![raw.to_owned()]);
    }
    if brace_opens > MAX_BRACE_OPENS || longest_digit_run(raw) > MAX_SEQUENCE_DIGITS {
        return None;
    }
    match word::parse_brace_expansions(raw, &ParserOptions::default()).ok()? {
        Some(pieces) => expand_brace_pieces(&pieces),
        None => Some(vec![raw.to_owned()]),
    }
}

fn longest_digit_run(raw: &str) -> usize {
    raw.split(|c: char| !c.is_ascii_digit())
        .map(str::len)
        .max()
        .unwrap_or(0)
}

fn expand_brace_pieces(pieces: &[BraceExpressionOrText]) -> Option<Vec<String>> {
    let mut words = vec![String::new()];
    for piece in pieces {
        let alternatives = match piece {
            BraceExpressionOrText::Text(text) => vec![text.clone()],
            BraceExpressionOrText::Expr(members) => expand_brace_members(members)?,
        };
        if words.len() * alternatives.len() > MAX_BRACE_WORDS {
            return None;
        }
        words = words
            .iter()
            .flat_map(|prefix| alternatives.iter().map(move |alt| format!("{prefix}{alt}")))
            .collect();
    }
    Some(words)
}

fn expand_brace_members(members: &[BraceExpressionMember]) -> Option<Vec<String>> {
    let mut alternatives = Vec::new();
    for member in members {
        let BraceExpressionMember::Child(pieces) = member else {
            return None;
        };
        alternatives.extend(expand_brace_pieces(pieces)?);
        if alternatives.len() > MAX_BRACE_WORDS {
            return None;
        }
    }
    Some(alternatives)
}

fn remove_quotes(word: &str) -> Option<String> {
    let pieces = word::parse(word, &ParserOptions::default()).ok()?;
    let mut out = String::new();
    for piece in &pieces {
        match &piece.piece {
            WordPiece::Text(text) => {
                if expands_unquoted(&out, text) {
                    return None;
                }
                out.push_str(text);
            }
            WordPiece::SingleQuotedText(text) => out.push_str(text),
            WordPiece::AnsiCQuotedText(text) => out.push_str(&decode_ansi_c(text)?),
            WordPiece::DoubleQuotedSequence(inner) => out.push_str(&double_quoted_text(inner)?),
            WordPiece::EscapeSequence(escape) => out.push_str(unescape(escape)),
            _ => return None,
        }
    }
    Some(out)
}

pub(crate) fn expands_unquoted(preceding: &str, text: &str) -> bool {
    if text.contains(['*', '?', '[', '(']) {
        return true;
    }
    let mut prev = preceding.chars().next_back();
    for c in text.chars() {
        if c == '~' && matches!(prev, None | Some('=' | ':')) {
            return true;
        }
        prev = Some(c);
    }
    false
}

fn double_quoted_text(pieces: &[WordPieceWithSource]) -> Option<String> {
    let mut out = String::new();
    for piece in pieces {
        match &piece.piece {
            WordPiece::Text(text) => out.push_str(&text.replace("\\\n", "")),
            WordPiece::EscapeSequence(escape) => out.push_str(unescape(escape)),
            _ => return None,
        }
    }
    Some(out)
}

fn unescape(escape: &str) -> &str {
    match &escape[1..] {
        "\n" => "",
        escaped => escaped,
    }
}

fn decode_ansi_c(text: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            push_char(&mut out, c);
            continue;
        }
        let Some(escaped) = chars.next() else {
            out.push(b'\\');
            break;
        };
        match escaped {
            'a' => out.push(0x07),
            'b' => out.push(0x08),
            'e' | 'E' => out.push(0x1b),
            'f' => out.push(0x0c),
            'n' => out.push(b'\n'),
            'r' => out.push(b'\r'),
            't' => out.push(b'\t'),
            'v' => out.push(0x0b),
            '\\' | '\'' | '"' | '?' => push_char(&mut out, escaped),
            '0'..='7' => {
                let first = escaped.to_digit(8)?;
                let value = take_digits(&mut chars, 8, 2)
                    .map_or(first, |(rest, len)| first * 8u32.pow(len) + rest);
                out.push(u8::try_from(value & 0xff).ok()?);
            }
            'x' => match take_digits(&mut chars, 16, 2) {
                Some((value, _)) => out.push(u8::try_from(value).ok()?),
                None => out.extend_from_slice(b"\\x"),
            },
            'u' | 'U' => {
                let max = if escaped == 'u' { 4 } else { 8 };
                if let Some((value, _)) = take_digits(&mut chars, 16, max) {
                    push_char(&mut out, char::from_u32(value)?);
                } else {
                    out.push(b'\\');
                    push_char(&mut out, escaped);
                }
            }
            'c' => match chars.next() {
                Some('?') => out.push(0x7f),
                Some(control) if control.is_ascii() => {
                    if control == '\\' && chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    out.push(control.to_ascii_uppercase() as u8 & 0x1f);
                }
                Some(_) => return None,
                None => out.extend_from_slice(b"\\c"),
            },
            other => {
                out.push(b'\\');
                push_char(&mut out, other);
            }
        }
    }
    if out.contains(&0) {
        return None;
    }
    String::from_utf8(out).ok()
}

fn take_digits(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    radix: u32,
    max: u32,
) -> Option<(u32, u32)> {
    let mut value = 0;
    let mut len = 0;
    while len < max
        && let Some(digit) = chars.peek().and_then(|c| c.to_digit(radix))
    {
        value = value * radix + digit;
        len += 1;
        chars.next();
    }
    (len > 0).then_some((value, len))
}

fn push_char(out: &mut Vec<u8>, c: char) {
    out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
}
