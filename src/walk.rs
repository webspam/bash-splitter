use brush_parser::ast;

use crate::expand::{self, Subst};
use crate::extended_test::collect_extended_test_subs;
use crate::redirect::collect_redirect_list_subs;
use crate::simple::{
    collect_simple_subs, prefix_assignments, simple_redirects, simple_vars, suffix_args,
};
use crate::types::{Stage, Sub, Walked};
use crate::word::collect_word_subs;

/// The walk's shared state: the stages walked out so far, the substitutions still to
/// surface, and whether static `for` loops should be unrolled. Threaded by `&mut`;
/// the per-call context (`in_loop`, `active` bindings, pipe position) stays in params.
pub(crate) struct Ctx<'a> {
    out: &'a mut Vec<Walked>,
    subs: &'a mut Vec<Sub>,
    expand: bool,
    /// Set when something under an unroll attempt proves it unfaithful; the
    /// attempt rolls back and re-walks the loop as a single body.
    poisoned: bool,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(out: &'a mut Vec<Walked>, subs: &'a mut Vec<Sub>, expand: bool) -> Self {
        Ctx {
            out,
            subs,
            expand,
            poisoned: false,
        }
    }
}

pub(crate) fn walk_compound_list(
    list: &ast::CompoundList,
    in_loop: bool,
    ctx: &mut Ctx,
    active: &[Subst],
) {
    for item in &list.0 {
        // A poisoned unroll attempt will be discarded
        if ctx.poisoned {
            return;
        }
        walk_and_or_list(&item.0, in_loop, ctx, active);
    }
}

fn walk_and_or_list(list: &ast::AndOrList, in_loop: bool, ctx: &mut Ctx, active: &[Subst]) {
    walk_pipeline(&list.first, in_loop, ctx, active);
    for and_or in &list.additional {
        match and_or {
            ast::AndOr::And(pipeline) | ast::AndOr::Or(pipeline) => {
                walk_pipeline(pipeline, in_loop, ctx, active)
            }
        }
    }
}

fn walk_pipeline(pipeline: &ast::Pipeline, in_loop: bool, ctx: &mut Ctx, active: &[Subst]) {
    for (i, command) in pipeline.seq.iter().enumerate() {
        walk_command(command, i > 0, in_loop, ctx, active);
    }
}

fn walk_command(
    command: &ast::Command,
    piped_from_previous: bool,
    in_loop: bool,
    ctx: &mut Ctx,
    active: &[Subst],
) {
    match command {
        ast::Command::Simple(simple) => {
            // Inside an unrolled loop, substitute the bindings so the stage is concrete.
            let substituted;
            let simple: &ast::SimpleCommand = if active.is_empty() {
                simple
            } else {
                substituted = expand::substitute_simple(simple, active);
                &substituted
            };
            let variables = simple_vars(simple);
            if !active.is_empty() && expand::stage_breaks_unroll(simple, &variables, active) {
                ctx.poisoned = true;
            }
            let id = ctx.out.len();
            ctx.out.push(Walked {
                stage: Stage {
                    command: simple.to_string(),
                    assignments: prefix_assignments(simple),
                    name: simple.word_or_name.as_ref().map(|w| w.value.clone()),
                    args: suffix_args(simple),
                    redirects: simple_redirects(simple),
                    in_loop,
                    variables,
                },
                parent: None,
                children: Vec::new(),
                piped_from_previous,
            });
            collect_simple_subs(simple, id, ctx.subs);
        }
        // Descend so commands nested in the compound surface individually. As a
        // pipeline stage (`foo | (grep x)`), its first inner command takes the pipe.
        ast::Command::Compound(compound, redirects) => {
            let start = ctx.out.len();
            walk_compound_command(compound, in_loop, ctx, active);
            apply_pipe_boundary(&mut ctx.out[start..], piped_from_previous);
            collect_redirect_list_subs(redirects.as_ref(), ctx.subs);
        }
        // The body runs when the function is later called, not per-iteration of any
        // enclosing loop: reset the loop context, and never bake an iteration's
        // value into the definition.
        ast::Command::Function(f) => {
            if !active.is_empty() {
                ctx.poisoned = true;
            }
            walk_compound_command(&f.body.0, false, ctx, &[])
        }
        // `[[ ... ]]` runs no command, but its words and redirects can hide substitutions.
        ast::Command::ExtendedTest(e, redirects) => {
            collect_extended_test_subs(&e.expr, ctx.subs);
            collect_redirect_list_subs(redirects.as_ref(), ctx.subs);
        }
    }
}

/// Surfaces every command nested in a compound's bodies and conditions; blocks are
/// never emitted whole. `in_loop` is whether the compound itself sits in a loop;
/// the loop clauses set it for their own bodies.
fn walk_compound_command(
    compound: &ast::CompoundCommand,
    in_loop: bool,
    ctx: &mut Ctx,
    active: &[Subst],
) {
    use ast::CompoundCommand as C;
    match compound {
        C::Subshell(s) => walk_compound_list(&s.list, in_loop, ctx, active),
        C::BraceGroup(b) => walk_compound_list(&b.list, in_loop, ctx, active),
        C::ForClause(f) => {
            // Re-binding a variable an enclosing unroll holds shadows it; that
            // unroll cannot be faithful. Once it falls back, this loop gets its
            // own clean unroll attempt.
            if active.iter().any(|s| s.var == f.variable_name) {
                ctx.poisoned = true;
            }
            if !ctx.poisoned
                && let Some(values) = expand::for_values(f, ctx.expand)
                && try_unroll(f, &values, ctx, active)
            {
                return;
            }
            // The iterated values can hide substitutions (`for x in $(cmd)`).
            for value in f.values.iter().flatten() {
                collect_word_subs(&value.value, None, ctx.subs);
            }
            walk_compound_list(&f.body.list, true, ctx, active);
        }
        // The init/cond/update expressions are arithmetic, and arithmetic is
        // command-substituted (`for ((i=$(cmd); ...))`); scan them, then the body.
        C::ArithmeticForClause(f) => {
            for expr in [&f.initializer, &f.condition, &f.updater]
                .into_iter()
                .flatten()
            {
                if expand::mentions_any(&expr.value, active) {
                    ctx.poisoned = true;
                }
                collect_word_subs(&expr.value, None, ctx.subs);
            }
            walk_compound_list(&f.body.list, true, ctx, active);
        }
        // Condition list, then `do` body. Both re-run each iteration, so both count.
        C::WhileClause(w) | C::UntilClause(w) => {
            walk_compound_list(&w.0, true, ctx, active);
            walk_compound_list(&w.1.list, true, ctx, active);
        }
        // The condition is itself a command (`if grep -q x; ...`).
        C::IfClause(i) => {
            walk_compound_list(&i.condition, in_loop, ctx, active);
            walk_compound_list(&i.then, in_loop, ctx, active);
            for else_clause in i.elses.iter().flatten() {
                if let Some(condition) = &else_clause.condition {
                    walk_compound_list(condition, in_loop, ctx, active);
                }
                walk_compound_list(&else_clause.body, in_loop, ctx, active);
            }
        }
        C::CaseClause(c) => {
            collect_word_subs(&c.value.value, None, ctx.subs);
            for item in &c.cases {
                for pattern in &item.patterns {
                    collect_word_subs(&pattern.value, None, ctx.subs);
                }
                if let Some(cmd) = &item.cmd {
                    walk_compound_list(cmd, in_loop, ctx, active);
                }
            }
        }
        C::Coprocess(c) => walk_command(&c.body, false, in_loop, ctx, active),
        // No command to *run*, but arithmetic is command-substituted before it is
        // evaluated, so `(( x=$(cmd) ))` runs cmd. It can also read or assign a
        // bound variable while emitting no stage the unroll checks could see.
        C::Arithmetic(a) => {
            if expand::mentions_any(&a.expr.value, active) {
                ctx.poisoned = true;
            }
            collect_word_subs(&a.expr.value, None, ctx.subs)
        }
    }
}

/// Walks the loop body once per literal, each pass with the variable bound. Anything
/// emitted that proves the unroll unfaithful poisons it: the attempt is rolled back
/// (truncated) and `false` returned so the caller walks the loop as a single body.
/// A problem owed to an *outer* binding re-poisons during that re-walk and cascades.
fn try_unroll(f: &ast::ForClauseCommand, values: &[&str], ctx: &mut Ctx, active: &[Subst]) -> bool {
    let start = ctx.out.len();
    let substart = ctx.subs.len();
    for value in values {
        if ctx.poisoned {
            break;
        }
        let mut bound = active.to_vec();
        bound.push(Subst {
            var: &f.variable_name,
            value,
        });
        walk_compound_list(&f.body.list, true, ctx, &bound);
    }
    // A substitution source still naming the variable would surface as a stage
    // mentioning it after the loop defining it is gone. This covers every position
    // subs are collected from, stage-less ones included.
    if !ctx.poisoned {
        ctx.poisoned = ctx.subs[substart..]
            .iter()
            .any(|s| expand::mentions_identifier(&s.source, &f.variable_name));
    }
    if ctx.poisoned {
        ctx.out.truncate(start);
        ctx.subs.truncate(substart);
        ctx.poisoned = false;
        return false;
    }
    true
}

/// When a grouping is itself a pipeline stage, its first inner command is the one
/// that reads the upstream pipe; the group's internal pipe flags are already set.
fn apply_pipe_boundary(group: &mut [Walked], piped_from_previous: bool) {
    if let Some(first) = group.first_mut() {
        first.piped_from_previous |= piped_from_previous;
    }
}
