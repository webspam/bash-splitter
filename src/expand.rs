//! Static `for`-loop unrolling: `for f in a b c; do echo $f; done` becomes the
//! individual commands `echo a`, `echo b`, `echo c`. Only fully-static loops
//! qualify (every value a plain literal, the variable never reassigned); anything
//! needing real expansion or guesswork is left for the normal walk.

use brush_parser::ParserOptions;
use brush_parser::ast;
use brush_parser::word::{self, Parameter, ParameterExpr, WordPiece, WordPieceWithSource};

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

/// The literals to unroll a `for` loop over, or `None` when it is not statically
/// expandable (disabled, non-literal values, a shadowed or reassigned variable).
pub(crate) fn for_values<'a>(
    f: &'a ast::ForClauseCommand,
    expand: bool,
    active: &[Subst],
) -> Option<Vec<&'a str>> {
    if !expand {
        return None;
    }
    // A variable already bound by an enclosing loop would be shadowed here; leave it
    // for the normal walk rather than substitute the wrong binding.
    if active.iter().any(|s| s.var == f.variable_name) {
        return None;
    }
    let words = f.values.as_ref()?;
    let mut values = Vec::with_capacity(words.len());
    for word in words {
        values.push(literal_value(&word.value)?);
    }
    // A body that reassigns the loop variable, or can cut the loop short, cannot be
    // faithfully unrolled into one command per value.
    if body_disqualifies(&f.body.list, &f.variable_name) {
        return None;
    }
    Some(values)
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

/// Whether the loop body cannot be faithfully unrolled. Two reasons: it reassigns
/// `var` (direct assignment, var-setting builtin, nested `for` over the same name, or
/// arithmetic, which can assign anything), or it contains flow control
/// (`break`/`continue`/`return`/`exit`) that changes how many iterations run.
fn body_disqualifies(list: &ast::CompoundList, var: &str) -> bool {
    list.0.iter().any(|item| and_or_disqualifies(&item.0, var))
}

fn and_or_disqualifies(list: &ast::AndOrList, var: &str) -> bool {
    pipeline_disqualifies(&list.first, var)
        || list.additional.iter().any(|a| match a {
            ast::AndOr::And(p) | ast::AndOr::Or(p) => pipeline_disqualifies(p, var),
        })
}

fn pipeline_disqualifies(pipeline: &ast::Pipeline, var: &str) -> bool {
    pipeline.seq.iter().any(|c| command_disqualifies(c, var))
}

fn command_disqualifies(command: &ast::Command, var: &str) -> bool {
    match command {
        ast::Command::Simple(s) => simple_disqualifies(s, var),
        ast::Command::Compound(c, _) => compound_disqualifies(c, var),
        // A function body runs when later called, not per iteration.
        ast::Command::Function(_) | ast::Command::ExtendedTest(_, _) => false,
    }
}

fn simple_disqualifies(simple: &ast::SimpleCommand, var: &str) -> bool {
    let assigns = simple
        .prefix
        .iter()
        .flat_map(|p| p.0.iter())
        .chain(simple.suffix.iter().flat_map(|s| s.0.iter()))
        .any(|item| match item {
            ast::CommandPrefixOrSuffixItem::AssignmentWord(a, _) => assigns_var(a, var),
            _ => false,
        });
    // Builtins that bind a variable from their arguments, and keywords that alter loop
    // flow; both are treated conservatively regardless of arguments.
    let name_disqualifies = simple.word_or_name.as_ref().is_some_and(|w| {
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
    });
    assigns || name_disqualifies
}

fn assigns_var(assignment: &ast::Assignment, var: &str) -> bool {
    match &assignment.name {
        ast::AssignmentName::VariableName(n) | ast::AssignmentName::ArrayElementName(n, _) => {
            n == var
        }
    }
}

fn compound_disqualifies(compound: &ast::CompoundCommand, var: &str) -> bool {
    use ast::CompoundCommand as C;
    match compound {
        C::Subshell(s) => body_disqualifies(&s.list, var),
        C::BraceGroup(b) => body_disqualifies(&b.list, var),
        // A nested loop over the same name rebinds it; otherwise scan its body.
        C::ForClause(f) => f.variable_name == var || body_disqualifies(&f.body.list, var),
        C::WhileClause(w) | C::UntilClause(w) => {
            body_disqualifies(&w.0, var) || body_disqualifies(&w.1.list, var)
        }
        C::IfClause(i) => {
            body_disqualifies(&i.condition, var)
                || body_disqualifies(&i.then, var)
                || i.elses.iter().flatten().any(|e| {
                    e.condition
                        .as_ref()
                        .is_some_and(|c| body_disqualifies(c, var))
                        || body_disqualifies(&e.body, var)
                })
        }
        C::CaseClause(c) => c.cases.iter().any(|item| {
            item.cmd
                .as_ref()
                .is_some_and(|cmd| body_disqualifies(cmd, var))
        }),
        C::Coprocess(c) => command_disqualifies(&c.body, var),
        // Arithmetic assignment (`(( f=1 ))`, `for (( ))`) can set any variable.
        C::Arithmetic(_) | C::ArithmeticForClause(_) => true,
    }
}
