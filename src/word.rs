use brush_parser::ParserOptions;
use brush_parser::word::{self, WordPiece, WordPieceWithSource};

use crate::params::{collect_param_expr_words, param_name, param_of, param_subscript};
use crate::resolve::brace_expand;
use crate::types::{StageId, Sub};

/// Appends `v` unless already present, keeping first-seen order.
pub(crate) fn push_unique(vars: &mut Vec<String>, v: String) {
    if !vars.contains(&v) {
        vars.push(v);
    }
}

/// Pulls the parameter names and command-substitution sources out of a single word.
pub(crate) fn collect_word_expansions(
    word: &str,
    parent: Option<StageId>,
    vars: &mut Vec<String>,
    subs: &mut Vec<Sub>,
) {
    let Ok(pieces) = word::parse(word, &ParserOptions::default()) else {
        return;
    };
    collect_piece_expansions(&pieces, parent, vars, subs);
}

pub(crate) fn collect_brace_expanded_expansions(
    word: &str,
    parent: Option<StageId>,
    vars: &mut Vec<String>,
    subs: &mut Vec<Sub>,
) {
    collect_word_subs(word, parent, subs);
    for expanded in brace_expand(word).unwrap_or_else(|| vec![word.to_owned()]) {
        collect_word_expansions(&expanded, parent, vars, &mut Vec::new());
    }
}

fn collect_piece_expansions(
    pieces: &[WordPieceWithSource],
    parent: Option<StageId>,
    vars: &mut Vec<String>,
    subs: &mut Vec<Sub>,
) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::CommandSubstitution(s) | WordPiece::BackquotedCommandSubstitution(s) => subs
                .push(Sub {
                    source: s.clone(),
                    parent,
                }),
            // Both expand inside double quotes (`"$x"`, `"$(...)"`) but not single quotes.
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => {
                collect_piece_expansions(inner, parent, vars, subs)
            }
            WordPiece::ParameterExpansion(expr) => {
                if let Some(parameter) = param_of(expr) {
                    push_unique(vars, param_name(parameter));
                }
                // `${arr[$i]}` and `${x:-$y}`: the subscript and value/pattern words expand
                // too. A subscript is an arithmetic context, so bare names count.
                if let Some(index) = param_subscript(expr) {
                    collect_word_expansions(index, parent, vars, subs);
                    collect_bare_arith_names(index, vars);
                }
                collect_param_expr_words(expr, |w| collect_word_expansions(w, parent, vars, subs));
            }
            // `$(( $x + $(cmd) ))` parses as an expansion; bare names (`$(( x ))`) also
            // read the variable, and only a lexical scan sees them.
            WordPiece::ArithmeticExpression(a) => {
                collect_word_expansions(&a.value, parent, vars, subs);
                collect_bare_arith_names(&a.value, vars);
            }
            _ => {}
        }
    }
}

pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Bare identifiers in arithmetic text (`x` in `x + 1`). `$x`/`${x}` forms are left
/// to the word parse: a run preceded by `$` is skipped, and dedup absorbs `${x}`.
/// A run starting with a digit (`0x1A`) is a number, not an identifier.
pub(crate) fn collect_bare_arith_names(text: &str, vars: &mut Vec<String>) {
    let mut prev: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !is_ident_char(c) {
            prev = Some(c);
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, c2)) = chars.peek() {
            if !is_ident_char(c2) {
                break;
            }
            end = i + c2.len_utf8();
            chars.next();
        }
        if (c.is_alphabetic() || c == '_') && prev != Some('$') {
            push_unique(vars, text[start..end].to_string());
        }
        prev = text[start..end].chars().next_back();
    }
}

/// Pulls the bodies of any command substitutions out of a single word, tagging each
/// with the `parent` stage the word belongs to (`None` for a container that runs
/// nothing). Variables are discarded: a stage-less context has no stage to hold them.
pub(crate) fn collect_word_subs(word: &str, parent: Option<StageId>, subs: &mut Vec<Sub>) {
    collect_word_expansions(word, parent, &mut Vec::new(), subs);
}
