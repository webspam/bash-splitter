//! Static `for`-loop unrolling: `for f in a b c; do echo $f; done` becomes the
//! individual commands `echo a`, `echo b`, `echo c`.
//!
//! Qualification is validate-and-rollback, not a pre-scan: the walk unrolls
//! speculatively, and anything it emits that proves the unroll unfaithful (a
//! residual reference to the bound variable, a rebind, flow control) poisons the
//! attempt, which is then discarded and re-walked as a single body. The walk is
//! the only traversal, so the check can never disagree with it about coverage.

use brush_parser::ParserOptions;
use brush_parser::ast;
use brush_parser::word::{self, Parameter, ParameterExpr, WordPiece, WordPieceWithSource};

use crate::simple::prefix_suffix_items;
use crate::word::is_ident_char;

/// An active loop binding: the variable and the literal it currently expands to.
#[derive(Clone, Copy)]
pub(crate) struct Subst<'a> {
    pub var: &'a str,
    pub value: &'a str,
}

/// The literal a `for` value expands to, or `None` if it is anything but a plain
/// literal. Quotes, expansions, globs, and braces all make the loop non-static.
fn literal_value(word: &str) -> Option<&str> {
    let pieces = word::parse(word, &ParserOptions::default()).ok()?;
    let [piece] = pieces.as_slice() else {
        return None;
    };
    let WordPiece::Text(text) = &piece.piece else {
        return None;
    };
    // Glob and brace metacharacters would expand further; a plain literal has none.
    if text.contains(['*', '?', '[', ']', '{', '}']) {
        return None;
    }
    Some(word)
}

/// The literals to unroll a `for` loop over, or `None` when a value is anything but
/// a plain literal. Whether the *body* can be unrolled is decided by the walk itself.
pub(crate) fn for_values(f: &ast::ForClauseCommand, expand: bool) -> Option<Vec<&str>> {
    if !expand {
        return None;
    }
    let words = f.values.as_ref()?;
    words.iter().map(|w| literal_value(&w.value)).collect()
}

/// Whether an emitted stage proves the enclosing unroll unfaithful: it still expands
/// a bound variable after substitution (an unrewritable form like `${f%.txt}`),
/// reassigns one, or is a var-binding builtin or flow-control keyword.
pub(crate) fn stage_breaks_unroll(
    simple: &ast::SimpleCommand,
    variables: &[String],
    active: &[Subst],
) -> bool {
    if active.iter().any(|s| variables.iter().any(|v| v == s.var)) {
        return true;
    }
    // Var-binding builtins and flow-control keywords, regardless of arguments.
    if simple.word_or_name.as_ref().is_some_and(|w| {
        matches!(
            w.value.as_str(),
            "read"
                | "readarray"
                | "mapfile"
                | "getopts"
                | "let"
                | "eval"
                | "break"
                | "continue"
                | "return"
                | "exit"
        )
    }) {
        return true;
    }
    prefix_suffix_items(simple).any(|item| match item {
        ast::CommandPrefixOrSuffixItem::AssignmentWord(a, _) => {
            active.iter().any(|s| assigns_var(a, s.var))
        }
        _ => false,
    })
}

fn assigns_var(assignment: &ast::Assignment, var: &str) -> bool {
    match &assignment.name {
        ast::AssignmentName::VariableName(n) | ast::AssignmentName::ArrayElementName(n, _) => {
            n == var
        }
    }
}

/// Whether arithmetic text reads or assigns any bound variable (`(( f++ ))`), which
/// the walk cannot see through a stage: arithmetic emits none.
pub(crate) fn mentions_any(text: &str, active: &[Subst]) -> bool {
    active.iter().any(|s| mentions_identifier(text, s.var))
}

/// Whether `text` contains `ident` as a whole identifier token (`$f`, `f+1`, `${f%x}`
/// all match `f`; `find`, `$foo` do not). Deliberately conservative: a false positive
/// only skips an unroll.
pub(crate) fn mentions_identifier(text: &str, ident: &str) -> bool {
    text.match_indices(ident).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + ident.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

/// A copy of `simple` with every `$var`/`${var}` in its words replaced by the active
/// bindings, so the walk reads it as a concrete command.
pub(crate) fn substitute_simple(
    simple: &ast::SimpleCommand,
    active: &[Subst],
) -> ast::SimpleCommand {
    let mut cloned = simple.clone();
    if let Some(word) = &mut cloned.word_or_name {
        substitute(word, active);
    }
    for item in cloned.prefix.iter_mut().flat_map(|p| p.0.iter_mut()) {
        substitute_item(item, active);
    }
    for item in cloned.suffix.iter_mut().flat_map(|s| s.0.iter_mut()) {
        substitute_item(item, active);
    }
    cloned
}

fn substitute_item(item: &mut ast::CommandPrefixOrSuffixItem, active: &[Subst]) {
    use ast::CommandPrefixOrSuffixItem as I;
    match item {
        I::Word(w) | I::AssignmentWord(_, w) => substitute(w, active),
        I::IoRedirect(r) => substitute_redirect(r, active),
        // Its body is a separate surfaced pipeline, walked without this binding.
        I::ProcessSubstitution(_, _) => {}
    }
}

#[allow(clippy::match_same_arms)]
fn substitute_redirect(redirect: &mut ast::IoRedirect, active: &[Subst]) {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => substitute(w, active),
        R::File(_, _, T::Fd(_) | T::ProcessSubstitution(_, _)) => {}
        R::OutputAndError(w, _) | R::HereString(_, w) => substitute(w, active),
        // Only an unquoted delimiter expands the body.
        R::HereDocument(_, doc) if doc.requires_expansion => substitute(&mut doc.doc, active),
        R::HereDocument(_, _) => {}
    }
}

fn substitute(word: &mut ast::Word, active: &[Subst]) {
    word.value = substitute_str(&word.value, active);
}

/// Replaces each `$var`/`${var}` in one word's text with its bound literal. brush's
/// piece spans give quote-correct boundaries, so `'$f'` and `$other` are left alone.
fn substitute_str(value: &str, active: &[Subst]) -> String {
    let Ok(pieces) = word::parse(value, &ParserOptions::default()) else {
        return value.to_string();
    };
    let mut spans = Vec::new();
    collect_spans(&pieces, active, &mut spans);
    if spans.is_empty() {
        return value.to_string();
    }
    spans.sort_by_key(|&(start, ..)| start);
    let mut out = String::with_capacity(value.len());
    let mut pos = 0;
    for (start, end, repl) in spans {
        out.push_str(&value[pos..start]);
        out.push_str(repl);
        pos = end;
    }
    out.push_str(&value[pos..]);
    out
}

/// Collects the source spans of bare `$var`/`${var}` references (including inside
/// double quotes), paired with the literal to splice in.
fn collect_spans<'a>(
    pieces: &[WordPieceWithSource],
    active: &[Subst<'a>],
    out: &mut Vec<(usize, usize, &'a str)>,
) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::ParameterExpansion(ParameterExpr::Parameter {
                parameter: Parameter::Named(name),
                indirect: false,
            }) => {
                if let Some(subst) = active.iter().find(|s| s.var == name) {
                    out.push((piece.start_index, piece.end_index, subst.value));
                }
            }
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => {
                collect_spans(inner, active, out);
            }
            _ => {}
        }
    }
}
