use brush_parser::ast;

use crate::extended_test::collect_extended_test_subs;
use crate::redirect::collect_redirect_list_subs;
use crate::simple::{collect_simple_subs, prefix_assignments, simple_redirects, simple_vars, suffix_args};
use crate::types::{Sub, Walked};
use crate::word::collect_word_subs;

pub(crate) fn walk_compound_list(
    list: &ast::CompoundList,
    in_loop: bool,
    out: &mut Vec<Walked>,
    subs: &mut Vec<Sub>,
) {
    for item in &list.0 {
        walk_and_or_list(&item.0, in_loop, out, subs);
    }
}

fn walk_and_or_list(
    list: &ast::AndOrList,
    in_loop: bool,
    out: &mut Vec<Walked>,
    subs: &mut Vec<Sub>,
) {
    walk_pipeline(&list.first, in_loop, out, subs);
    for and_or in &list.additional {
        match and_or {
            ast::AndOr::And(pipeline) | ast::AndOr::Or(pipeline) => {
                walk_pipeline(pipeline, in_loop, out, subs)
            }
        }
    }
}

fn walk_pipeline(
    pipeline: &ast::Pipeline,
    in_loop: bool,
    out: &mut Vec<Walked>,
    subs: &mut Vec<Sub>,
) {
    for (i, command) in pipeline.seq.iter().enumerate() {
        walk_command(command, i > 0, in_loop, out, subs);
    }
}

fn walk_command(
    command: &ast::Command,
    piped_from_previous: bool,
    in_loop: bool,
    out: &mut Vec<Walked>,
    subs: &mut Vec<Sub>,
) {
    match command {
        ast::Command::Simple(simple) => {
            let id = out.len();
            out.push(Walked {
                command: simple.to_string(),
                assignments: prefix_assignments(simple),
                name: simple.word_or_name.as_ref().map(|w| w.value.clone()),
                args: suffix_args(simple),
                redirects: simple_redirects(simple),
                in_loop,
                variables: simple_vars(simple),
                parent: None,
                children: Vec::new(),
                piped_from_previous,
            });
            collect_simple_subs(simple, id, subs);
        }
        // Descend so commands nested in the compound surface individually. As a
        // pipeline stage (`foo | (grep x)`), its first inner command takes the pipe.
        ast::Command::Compound(compound, redirects) => {
            let start = out.len();
            walk_compound_command(compound, in_loop, out, subs);
            apply_pipe_boundary(&mut out[start..], piped_from_previous);
            collect_redirect_list_subs(redirects.as_ref(), subs);
        }
        // The body runs when the function is later called, not per-iteration of any
        // enclosing loop, so reset the loop context.
        ast::Command::Function(f) => walk_compound_command(&f.body.0, false, out, subs),
        // `[[ ... ]]` runs no command, but its words and redirects can hide substitutions.
        ast::Command::ExtendedTest(e, redirects) => {
            collect_extended_test_subs(&e.expr, subs);
            collect_redirect_list_subs(redirects.as_ref(), subs);
        }
    }
}

/// Surfaces every command nested in a compound's bodies and conditions; blocks are
/// never emitted whole. `in_loop` is whether the compound itself sits in a loop;
/// the loop clauses set it for their own bodies.
fn walk_compound_command(
    compound: &ast::CompoundCommand,
    in_loop: bool,
    out: &mut Vec<Walked>,
    subs: &mut Vec<Sub>,
) {
    use ast::CompoundCommand as C;
    match compound {
        C::Subshell(s) => walk_compound_list(&s.list, in_loop, out, subs),
        C::BraceGroup(b) => walk_compound_list(&b.list, in_loop, out, subs),
        C::ForClause(f) => {
            // The iterated values can hide substitutions (`for x in $(cmd)`).
            for value in f.values.iter().flatten() {
                collect_word_subs(&value.value, None, subs);
            }
            walk_compound_list(&f.body.list, true, out, subs);
        }
        // The init/cond/update expressions are arithmetic, and arithmetic is
        // command-substituted (`for ((i=$(cmd); ...))`); scan them, then the body.
        C::ArithmeticForClause(f) => {
            for expr in [&f.initializer, &f.condition, &f.updater].into_iter().flatten() {
                collect_word_subs(&expr.value, None, subs);
            }
            walk_compound_list(&f.body.list, true, out, subs);
        }
        // Condition list, then `do` body. Both re-run each iteration, so both count.
        C::WhileClause(w) | C::UntilClause(w) => {
            walk_compound_list(&w.0, true, out, subs);
            walk_compound_list(&w.1.list, true, out, subs);
        }
        // The condition is itself a command (`if grep -q x; ...`).
        C::IfClause(i) => {
            walk_compound_list(&i.condition, in_loop, out, subs);
            walk_compound_list(&i.then, in_loop, out, subs);
            for else_clause in i.elses.iter().flatten() {
                if let Some(condition) = &else_clause.condition {
                    walk_compound_list(condition, in_loop, out, subs);
                }
                walk_compound_list(&else_clause.body, in_loop, out, subs);
            }
        }
        C::CaseClause(c) => {
            collect_word_subs(&c.value.value, None, subs);
            for item in &c.cases {
                for pattern in &item.patterns {
                    collect_word_subs(&pattern.value, None, subs);
                }
                if let Some(cmd) = &item.cmd {
                    walk_compound_list(cmd, in_loop, out, subs);
                }
            }
        }
        C::Coprocess(c) => walk_command(&c.body, false, in_loop, out, subs),
        // No command to *run*, but arithmetic is command-substituted before it is
        // evaluated, so `(( x=$(cmd) ))` runs cmd.
        C::Arithmetic(a) => collect_word_subs(&a.expr.value, None, subs),
    }
}

/// When a grouping is itself a pipeline stage, its first inner command is the one
/// that reads the upstream pipe; the group's internal pipe flags are already set.
fn apply_pipe_boundary(group: &mut [Walked], piped_from_previous: bool) {
    if let Some(first) = group.first_mut() {
        first.piped_from_previous |= piped_from_previous;
    }
}
