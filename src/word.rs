use brush_parser::ParserOptions;
use brush_parser::word::{self, WordPiece, WordPieceWithSource};

use crate::params::{collect_param_expr_words, param_name, param_of, param_subscript};
use crate::types::Sub;

/// Appends `v` unless already present, keeping first-seen order.
pub(crate) fn push_unique(vars: &mut Vec<String>, v: String) {
    if !vars.contains(&v) {
        vars.push(v);
    }
}

/// Pulls the names of any parameter expansions out of a single word.
pub(crate) fn collect_word_vars(word: &str, vars: &mut Vec<String>) {
    let Ok(pieces) = word::parse(word, &ParserOptions::default()) else {
        return;
    };
    collect_piece_vars(&pieces, vars);
}

pub(crate) fn collect_piece_vars(pieces: &[WordPieceWithSource], vars: &mut Vec<String>) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::ParameterExpansion(expr) => {
                if let Some(parameter) = param_of(expr) {
                    push_unique(vars, param_name(parameter));
                }
                // `${arr[$i]}` and `${x:-$y}`: the subscript and value/pattern words expand too.
                if let Some(index) = param_subscript(expr) {
                    collect_word_vars(index, vars);
                }
                collect_param_expr_words(expr, |w| collect_word_vars(w, vars));
            }
            // Variables expand inside double quotes (`"$x"`) but not single quotes.
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => collect_piece_vars(inner, vars),
            // `$(( $x ))` references x; bare arithmetic names (`$(( x ))`) are missed.
            WordPiece::ArithmeticExpression(a) => collect_word_vars(&a.value, vars),
            _ => {}
        }
    }
}

/// Pulls the bodies of any command substitutions out of a single word, tagging each
/// with the `parent` stage the word belongs to (`None` for a container that runs nothing).
pub(crate) fn collect_word_subs(word: &str, parent: Option<usize>, subs: &mut Vec<Sub>) {
    let Ok(pieces) = word::parse(word, &ParserOptions::default()) else {
        return;
    };
    collect_piece_subs(&pieces, parent, subs);
}

pub(crate) fn collect_piece_subs(
    pieces: &[WordPieceWithSource],
    parent: Option<usize>,
    subs: &mut Vec<Sub>,
) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::CommandSubstitution(s) | WordPiece::BackquotedCommandSubstitution(s) => subs
                .push(Sub {
                    source: s.clone(),
                    parent,
                }),
            // Substitutions can nest inside double quotes (`"$(...)"`).
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => {
                collect_piece_subs(inner, parent, subs)
            }
            // `${x:-$(cmd)}`, `${x/$(cmd)/y}`: the value/pattern words expand.
            WordPiece::ParameterExpansion(expr) => {
                // An array subscript is an arithmetic context (`${arr[$(cmd)]}`).
                if let Some(index) = param_subscript(expr) {
                    collect_word_subs(index, parent, subs);
                }
                collect_param_expr_words(expr, |w| collect_word_subs(w, parent, subs));
            }
            // `$(( $(cmd) ))` runs cmd while evaluating the expression.
            WordPiece::ArithmeticExpression(a) => collect_word_subs(&a.value, parent, subs),
            _ => {}
        }
    }
}
