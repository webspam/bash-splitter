use brush_parser::ParserOptions;
use brush_parser::word::{self, WordPiece, WordPieceWithSource};

use crate::arithmetic::collect_arithmetic_names;
use crate::params::{collect_param_expr_words, param_name, param_of, param_subscript};
use crate::resolve::brace_expand;
use crate::types::{StageId, Sub, Variables};

/// Pulls the parameter names and command-substitution sources out of a single word.
pub(crate) fn collect_word_expansions(
    word: &str,
    parent: Option<StageId>,
    vars: &mut Variables,
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
    vars: &mut Variables,
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
    vars: &mut Variables,
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
                    vars.push(&param_name(parameter));
                }
                // `${arr[$i]}` and `${x:-$y}`: the subscript and value/pattern words expand
                // too. A subscript is an arithmetic context, so bare names count.
                if let Some(index) = param_subscript(expr) {
                    collect_word_expansions(index, parent, vars, subs);
                    collect_arithmetic_names(index, vars);
                }
                collect_param_expr_words(expr, |w| collect_word_expansions(w, parent, vars, subs));
            }
            WordPiece::ArithmeticExpression(a) => {
                collect_word_expansions(&a.value, parent, vars, subs);
                collect_arithmetic_names(&a.value, vars);
            }
            _ => {}
        }
    }
}

pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Pulls the bodies of any command substitutions out of a single word, tagging each
/// with the `parent` stage the word belongs to (`None` for a container that runs
/// nothing). Variables are discarded: a stage-less context has no stage to hold them.
pub(crate) fn collect_word_subs(word: &str, parent: Option<StageId>, subs: &mut Vec<Sub>) {
    collect_word_expansions(word, parent, &mut Variables::default(), subs);
}
