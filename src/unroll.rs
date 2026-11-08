//! Static `for`-loop unrolling: `for f in a b c; do echo $f; done` becomes the
//! individual commands `echo a`, `echo b`, `echo c`.
//!
//! Qualification is validate-and-rollback, not a pre-scan: the walk unrolls as it
//! goes, and if anything it emits shows the unroll would be incorrect (a leftover
//! reference to the loop variable, a reassignment, flow control) the attempt is
//! discarded and the loop re-walked as a single body. The walk is the only
//! traversal, so the check can never disagree with it about coverage.

use std::borrow::Cow;

use brush_parser::ParserOptions;
use brush_parser::ast;
use brush_parser::word::{self, Parameter, ParameterExpr, WordPiece, WordPieceWithSource};

use crate::arithmetic::collect_arithmetic_names;
use crate::redirect::expanded_word_mut;
use crate::resolve::{brace_expand, expands_unquoted};
use crate::simple::prefix_suffix_items;
use crate::types::{ResolvedArgv, Variables};
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
    // Glob and brace metacharacters would expand further; reject them.
    if expands_unquoted("", text) || text.contains(['{', '}']) {
        return None;
    }
    // A trailing `$` would capture whatever follows the splice point (`a$` + `x`
    // -> `a$x`); reject it.
    if word.ends_with('$') {
        return None;
    }
    Some(word)
}

/// The literals to unroll a `for` loop over, or `None` when a value is anything but
/// a plain literal. Whether the *body* can be unrolled is decided by the walk itself.
pub(crate) fn for_values(f: &ast::ForClauseCommand, unroll: bool) -> Option<Vec<&str>> {
    if !unroll {
        return None;
    }
    let words = f.values.as_ref()?;
    words.iter().map(|w| literal_value(&w.value)).collect()
}

/// Whether an emitted stage breaks the enclosing unroll: it still expands
/// a bound variable after substitution (an unrewritable form like `${f%.txt}`),
/// reassigns one, or is a var-binding builtin or flow-control keyword.
pub(crate) fn stage_breaks_unroll(
    simple: &ast::SimpleCommand,
    argv: &ResolvedArgv,
    variables: &Variables,
    active: &[Subst],
) -> bool {
    if variables.dynamic || binds_any(&variables.names, active) {
        return true;
    }
    if invokes_unroll_breaking_command(argv) || has_unexpandable_brace_word(simple, active) {
        return true;
    }
    prefix_suffix_items(simple).any(|item| match item {
        ast::CommandPrefixOrSuffixItem::AssignmentWord(a, _) => {
            active.iter().any(|s| assigns_var(a, s.var))
        }
        _ => false,
    })
}

const UNROLL_BREAKING_COMMANDS: [&str; 10] = [
    "read",
    "readarray",
    "mapfile",
    "getopts",
    "let",
    "eval",
    "break",
    "continue",
    "return",
    "exit",
];

const BUILTIN_WRAPPERS: [&str; 2] = ["builtin", "command"];

fn invokes_unroll_breaking_command(argv: &ResolvedArgv) -> bool {
    let mut after_wrapper = false;
    for (word, dynamic) in argv.words() {
        if dynamic {
            return true;
        }
        if after_wrapper && word.starts_with('-') {
            continue;
        }
        if BUILTIN_WRAPPERS.contains(&word) {
            after_wrapper = true;
            continue;
        }
        return UNROLL_BREAKING_COMMANDS.contains(&word);
    }
    false
}

fn has_unexpandable_brace_word(simple: &ast::SimpleCommand, active: &[Subst]) -> bool {
    let suffix_words = simple
        .suffix
        .iter()
        .flat_map(|s| &s.0)
        .filter_map(|item| match item {
            ast::CommandPrefixOrSuffixItem::Word(w)
            | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => Some(w),
            _ => None,
        });
    simple
        .word_or_name
        .iter()
        .chain(suffix_words)
        .any(|w| mentions_any(&w.value, active) && brace_expand(&w.value).is_none())
}

fn assigns_var(assignment: &ast::Assignment, var: &str) -> bool {
    match &assignment.name {
        ast::AssignmentName::VariableName(n) | ast::AssignmentName::ArrayElementName(n, _) => {
            n == var
        }
    }
}

fn binds_any(names: &[String], active: &[Subst]) -> bool {
    active.iter().any(|s| names.iter().any(|n| n == s.var))
}

/// Whether a stage-less arithmetic expression (`(( f++ ))`) may read or assign a
/// bound variable. Arithmetic never becomes a stage, so `stage_breaks_unroll` can't
/// catch it.
pub(crate) fn arithmetic_breaks_unroll(text: &str, active: &[Subst]) -> bool {
    if active.is_empty() {
        return false;
    }
    let mut vars = Variables::default();
    collect_arithmetic_names(&substitute_str(text, active), &mut vars);
    vars.dynamic || binds_any(&vars.names, active)
}

fn mentions_any(text: &str, active: &[Subst]) -> bool {
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
        I::IoRedirect(ast::IoRedirect::HereDocument(_, doc)) if doc.requires_expansion => {
            doc.doc.value = substitute_heredoc(&doc.doc.value, active);
        }
        I::IoRedirect(r) => {
            if let Some(w) = expanded_word_mut(r) {
                substitute(w, active);
            }
        }
        // Its body is a separate surfaced pipeline, walked without this binding.
        I::ProcessSubstitution(_, _) => {}
    }
}

fn substitute(word: &mut ast::Word, active: &[Subst]) {
    let substituted = substitute_str(&word.value, active);
    if brace_expansion_commutes(&word.value, &substituted, active) {
        word.value = substituted;
    }
}

fn brace_expansion_commutes(raw: &str, substituted: &str, active: &[Subst]) -> bool {
    let Some(expanded) = brace_expand(raw) else {
        return false;
    };
    let expanded_then_substituted: Vec<_> =
        expanded.iter().map(|w| substitute_str(w, active)).collect();
    brace_expand(substituted) == Some(expanded_then_substituted)
}

/// Converts loop variables to their values.
fn substitute_str(value: &str, active: &[Subst]) -> String {
    match word::parse(value, &ParserOptions::default()) {
        Ok(pieces) => splice(value, &pieces, active),
        Err(_) => value.to_string(),
    }
}

/// Swaps loop variables in a heredoc body for their values.
fn substitute_heredoc(body: &str, active: &[Subst]) -> String {
    match word::parse_heredoc(body, &ParserOptions::default()) {
        Ok(pieces) => splice(body, &pieces, active),
        Err(_) => body.to_string(),
    }
}

fn splice(value: &str, pieces: &[WordPieceWithSource], active: &[Subst]) -> String {
    let mut spans = Vec::new();
    collect_spans(value, pieces, active, &mut spans);
    if spans.is_empty() {
        return value.to_string();
    }
    spans.sort_by_key(|&(start, ..)| start);
    let mut out = String::with_capacity(value.len());
    let mut pos = 0;
    for (start, end, repl) in spans {
        out.push_str(&value[pos..start]);
        // Pasting a value directly after another `$var` would glue onto that
        // variable's name; refuse to paste there.
        if repl.starts_with(is_ident_char) && ends_with_bare_ref(&out) {
            out.push_str(&value[start..end]);
        } else {
            out.push_str(&repl);
        }
        pos = end;
    }
    out.push_str(&value[pos..]);
    out
}

/// Whether `text` ends with a bare named reference (`...$f`). A positional (`$1`)
/// expands one digit regardless of what follows, so only a name can merge.
fn ends_with_bare_ref(text: &str) -> bool {
    let trimmed = text.trim_end_matches(is_ident_char);
    let name = &text[trimmed.len()..];
    name.chars().next().is_some_and(|c| !c.is_ascii_digit()) && trimmed.ends_with('$')
}

/// Collects the source spans of bare `$var`/`${var}` references (including inside
/// double quotes), paired with the literal to splice in.
fn collect_spans<'a>(
    source: &str,
    pieces: &[WordPieceWithSource],
    active: &[Subst<'a>],
    out: &mut Vec<(usize, usize, Cow<'a, str>)>,
) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::ParameterExpansion(ParameterExpr::Parameter {
                parameter: Parameter::Named(name),
                indirect: false,
            }) => {
                if let Some(subst) = active.iter().find(|s| s.var == name) {
                    out.push((piece.start_index, piece.end_index, subst.value.into()));
                }
            }
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => {
                collect_spans(source, inner, active, out);
            }
            WordPiece::ArithmeticExpression(a) => {
                let substituted = substitute_str(&a.value, active);
                if substituted != a.value {
                    let opener_len = if source.as_bytes()[piece.start_index + 1] == b'[' {
                        2
                    } else {
                        3
                    };
                    let body_start = piece.start_index + opener_len;
                    out.push((body_start, body_start + a.value.len(), substituted.into()));
                }
            }
            _ => {}
        }
    }
}
