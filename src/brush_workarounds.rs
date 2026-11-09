// TODO: Remove after https://github.com/reubeno/brush/pull/1421 (59360b5d) is released.

use brush_parser::{Token, ast, tokenize_str};

use crate::Error;

pub(crate) fn keeps_line_continuation(heredoc_text: &str) -> bool {
    heredoc_text.contains("\\\n")
}

pub(crate) fn reject_ansi_c_line_continuation(input: &str) -> Result<(), Error> {
    let Ok(tokens) = tokenize_str(input) else {
        return Ok(());
    };
    let affected = tokens.iter().any(|token| match token {
        Token::Word(text, span) => text.contains("$'") && text.chars().count() < span.length(),
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
                if line == doc.here_end.value {
                    return true;
                }
                line.clear();
            }
            _ => line.push(c),
        }
    }
    continued
}
