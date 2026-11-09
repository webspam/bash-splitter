// TODO: Remove after https://github.com/reubeno/brush/pull/1421 (59360b5d) is released.

use brush_parser::{Token, ast, tokenize_str};

use crate::Error;

pub(crate) const LINE_CONTINUATION: &str = "\\\n";

pub(crate) fn reject_ansi_c_line_continuation(input: &str) -> Result<(), Error> {
    let Ok(tokens) = tokenize_str(input) else {
        return Ok(());
    };
    let byte_offsets: Vec<usize> = input
        .char_indices()
        .map(|(offset, _)| offset)
        .chain([input.len()])
        .collect();
    let source_of =
        |start: usize, end: usize| Some(&input[*byte_offsets.get(start)?..*byte_offsets.get(end)?]);
    let affected = tokens.iter().any(|token| match token {
        Token::Word(text, span) => {
            text.contains("$'")
                && source_of(span.start.index, span.end.index)
                    .is_some_and(|source| source.contains(LINE_CONTINUATION))
        }
        Token::Operator(..) => false,
    });
    if affected {
        Err(Error::Unsupported(
            "line continuation in a word with $'...' quoting",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn splits_heredoc_end_tag(redirect: &ast::IoRedirect) -> bool {
    let ast::IoRedirect::HereDocument(_, doc) = redirect else {
        return false;
    };
    if !doc.requires_expansion {
        return false;
    }
    let is_end_tag = |line: &str| {
        let line = if doc.remove_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        line == doc.here_end.value
    };
    let mut line = String::new();
    let mut continued = false;
    let mut chars = doc.doc.value.chars();
    while let Some(c) = chars.next() {
        continued = false;
        match c {
            '\\' => match chars.next() {
                Some('\n') => continued = true,
                Some(escaped) => line.extend([c, escaped]),
                None => line.push(c),
            },
            '\n' => {
                if is_end_tag(&line) {
                    return true;
                }
                line.clear();
            }
            _ => line.push(c),
        }
    }
    continued
}
