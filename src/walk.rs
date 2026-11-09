use brush_parser::ast;

use crate::Error;
use crate::brush_workarounds::splits_heredoc_end_tag;
use crate::extended_test::collect_extended_test_subs;
use crate::redirect::collect_redirect_list_subs;
use crate::resolve::resolve_argv;
use crate::simple::{
    collect_expansions, prefix_assignments, prefix_suffix_items, simple_redirects, suffix_args,
};
use crate::types::{Loc, LoopId, LoopKind, LoopWalked, Section, Stage, StageId, Sub, Walked};
use crate::unroll::{self, Subst};
use crate::word::collect_word_subs;

/// State shared across the whole walk: the stages and loops emitted so far, the
/// substitutions still to surface, and the mode flags. Context that varies per
/// call is passed as a [`Scope`] instead.
pub(crate) struct Ctx<'a> {
    out: &'a mut Vec<Walked>,
    subs: &'a mut Vec<Sub>,
    loops: &'a mut Vec<LoopWalked>,
    unroll: bool,
    /// Nested mode: an unrolled loop keeps its original body as a second
    /// representation; flat mode replaces the body with the unrolled commands.
    nested: bool,
    /// Set when something under an unroll attempt shows it would be incorrect; the
    /// attempt rolls back and re-walks the loop as a single body.
    poisoned: bool,
    rejection: Option<&'static str>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(
        out: &'a mut Vec<Walked>,
        subs: &'a mut Vec<Sub>,
        loops: &'a mut Vec<LoopWalked>,
        unroll: bool,
        nested: bool,
    ) -> Self {
        Ctx {
            out,
            subs,
            loops,
            unroll,
            nested,
            poisoned: false,
            rejection: None,
        }
    }

    pub(crate) fn reject_unsupported(&self) -> Result<(), Error> {
        self.rejection
            .map_or(Ok(()), |construct| Err(Error::Unsupported(construct)))
    }

    fn note_redirects(&mut self, redirects: Option<&ast::RedirectList>) {
        if redirects.is_some_and(|r| !r.0.is_empty()) {
            self.rejection = Some("redirect on a compound command, [[ ]], or function definition");
        }
    }

    fn note_heredocs(&mut self, simple: &ast::SimpleCommand) {
        let splits_end_tag = prefix_suffix_items(simple).any(|item| {
            matches!(item, ast::CommandPrefixOrSuffixItem::IoRedirect(r) if splits_heredoc_end_tag(r))
        });
        if splits_end_tag {
            self.rejection = Some("heredoc end tag joined by a line continuation");
        }
    }

    fn record_loop(&mut self, kind: LoopKind, variable: Option<String>, scope: Scope) -> LoopId {
        let id = LoopId(self.loops.len());
        self.loops.push(LoopWalked {
            kind,
            variable,
            parent: scope.loc,
            sub_parent: None,
            position: self.out.len(),
            values: None,
        });
        id
    }
}

/// Per-call walk context: whether commands run per loop iteration, the active
/// unroll bindings, and the loop slot the commands structurally sit in.
#[derive(Clone, Copy)]
pub(crate) struct Scope<'a> {
    /// True inside a `for`/`while`/`until` loop, so commands run once per
    /// iteration. Not tracked across substitution boundaries.
    in_loop: bool,
    active: &'a [Subst<'a>],
    loc: Option<Loc>,
}

impl Scope<'_> {
    pub(crate) fn root() -> Self {
        Scope {
            in_loop: false,
            active: &[],
            loc: None,
        }
    }

    /// The scope for one section of a just-recorded loop.
    fn enter(self, loop_id: LoopId, section: Section) -> Self {
        Scope {
            in_loop: true,
            loc: Some(Loc { loop_id, section }),
            ..self
        }
    }
}

pub(crate) fn walk_compound_list(list: &ast::CompoundList, ctx: &mut Ctx, scope: Scope) {
    for item in &list.0 {
        if ctx.poisoned {
            return;
        }
        walk_and_or_list(&item.0, ctx, scope);
    }
}

fn walk_and_or_list(list: &ast::AndOrList, ctx: &mut Ctx, scope: Scope) {
    walk_pipeline(&list.first, ctx, scope);
    for and_or in &list.additional {
        match and_or {
            ast::AndOr::And(pipeline) | ast::AndOr::Or(pipeline) => {
                walk_pipeline(pipeline, ctx, scope)
            }
        }
    }
}

fn walk_pipeline(pipeline: &ast::Pipeline, ctx: &mut Ctx, scope: Scope) {
    for (i, command) in pipeline.seq.iter().enumerate() {
        walk_command(command, i > 0, ctx, scope);
    }
}

fn walk_command(command: &ast::Command, piped_from_previous: bool, ctx: &mut Ctx, scope: Scope) {
    match command {
        ast::Command::Simple(simple) => {
            // Inside an unrolled loop, substitute the bindings.
            let substituted;
            let simple: &ast::SimpleCommand = if scope.active.is_empty() {
                simple
            } else {
                substituted = unroll::substitute_simple(simple, scope.active);
                &substituted
            };
            ctx.note_heredocs(simple);
            let id = StageId(ctx.out.len());
            let variables = collect_expansions(simple, id, ctx.subs);
            let argv = resolve_argv(
                simple.word_or_name.as_ref().map(|w| w.value.clone()),
                suffix_args(simple),
            );
            if !scope.active.is_empty()
                && unroll::stage_breaks_unroll(simple, &argv, &variables, scope.active)
            {
                ctx.poisoned = true;
            }
            ctx.out.push(Walked {
                stage: Stage {
                    command: simple.to_string(),
                    assignments: prefix_assignments(simple),
                    argv,
                    redirects: simple_redirects(simple),
                    in_loop: scope.in_loop,
                    variables: variables.names,
                    dynamic_variables: variables.dynamic,
                },
                parent: None,
                loc: scope.loc,
                piped_from_previous,
            });
        }
        // Descend so commands nested in the compound surface individually. As a
        // pipeline stage (`foo | (grep x)`), its first inner command takes the pipe.
        ast::Command::Compound(compound, redirects) => {
            ctx.note_redirects(redirects.as_ref());
            let start = ctx.out.len();
            walk_compound_command(compound, ctx, scope);
            apply_pipe_boundary(&mut ctx.out[start..], piped_from_previous);
            collect_redirect_list_subs(redirects.as_ref(), ctx.subs);
        }
        // The body runs when the function is later called, not per-iteration of any
        // enclosing loop: reset the loop context, and never substitute an
        // iteration's value into the definition. The definition still sits in the
        // enclosing loop object structurally, so `loc` is kept.
        ast::Command::Function(f) => {
            ctx.note_redirects(f.body.1.as_ref());
            if !scope.active.is_empty() {
                ctx.poisoned = true;
            }
            walk_compound_command(
                &f.body.0,
                ctx,
                Scope {
                    in_loop: false,
                    active: &[],
                    loc: scope.loc,
                },
            )
        }
        // `[[ ... ]]` runs no command, but its words and redirects can hide substitutions.
        ast::Command::ExtendedTest(e, redirects) => {
            ctx.note_redirects(redirects.as_ref());
            collect_extended_test_subs(&e.expr, ctx.subs);
            collect_redirect_list_subs(redirects.as_ref(), ctx.subs);
        }
    }
}

/// Surfaces every command nested in a compound's bodies and conditions; blocks are
/// never emitted whole, but each loop is recorded so nested mode can rebuild it as
/// an object. The loop clauses set `in_loop` for their own sections.
fn walk_compound_command(compound: &ast::CompoundCommand, ctx: &mut Ctx, scope: Scope) {
    use ast::CompoundCommand as C;
    match compound {
        C::Subshell(s) => walk_compound_list(&s.list, ctx, scope),
        C::BraceGroup(b) => walk_compound_list(&b.list, ctx, scope),
        C::ForClause(f) => walk_for(f, ctx, scope),
        // The init/cond/update expressions are arithmetic, and arithmetic is
        // command-substituted (`for ((i=$(cmd); ...))`); scan them, then the body.
        C::ArithmeticForClause(f) => {
            let id = ctx.record_loop(LoopKind::For, None, scope);
            for expr in [&f.initializer, &f.condition, &f.updater]
                .into_iter()
                .flatten()
            {
                if unroll::arithmetic_breaks_unroll(&expr.value, scope.active) {
                    ctx.poisoned = true;
                }
                collect_word_subs(&expr.value, None, ctx.subs);
            }
            walk_compound_list(&f.body.list, ctx, scope.enter(id, Section::Body));
        }
        C::WhileClause(w) => walk_while_until(w, LoopKind::While, ctx, scope),
        C::UntilClause(w) => walk_while_until(w, LoopKind::Until, ctx, scope),
        // The condition is itself a command (`if grep -q x; ...`).
        C::IfClause(i) => {
            walk_compound_list(&i.condition, ctx, scope);
            walk_compound_list(&i.then, ctx, scope);
            for else_clause in i.elses.iter().flatten() {
                if let Some(condition) = &else_clause.condition {
                    walk_compound_list(condition, ctx, scope);
                }
                walk_compound_list(&else_clause.body, ctx, scope);
            }
        }
        C::CaseClause(c) => {
            collect_word_subs(&c.value.value, None, ctx.subs);
            for item in &c.cases {
                for pattern in &item.patterns {
                    collect_word_subs(&pattern.value, None, ctx.subs);
                }
                if let Some(cmd) = &item.cmd {
                    walk_compound_list(cmd, ctx, scope);
                }
            }
        }
        C::Coprocess(c) => walk_command(&c.body, false, ctx, scope),
        // No command to *run*, but arithmetic is command-substituted before it is
        // evaluated, so `(( x=$(cmd) ))` runs cmd. It can also read or assign a bound
        // variable without ever becoming a stage.
        C::Arithmetic(a) => {
            if unroll::arithmetic_breaks_unroll(&a.expr.value, scope.active) {
                ctx.poisoned = true;
            }
            collect_word_subs(&a.expr.value, None, ctx.subs)
        }
    }
}

fn walk_for(f: &ast::ForClauseCommand, ctx: &mut Ctx, scope: Scope) {
    // Re-binding a variable an enclosing unroll holds shadows it; that unroll
    // would be incorrect. Once it falls back, this loop gets its own clean
    // unroll attempt.
    if scope.active.iter().any(|s| s.var == f.variable_name) {
        ctx.poisoned = true;
    }
    let id = ctx.record_loop(LoopKind::For, Some(f.variable_name.clone()), scope);
    if !ctx.poisoned
        && let Some(values) = unroll::for_values(f, ctx.unroll)
        && try_unroll(f, &values, id, ctx, scope)
    {
        ctx.loops[id.0].values = Some(values.iter().map(ToString::to_string).collect());
        // Flat mode replaces the loop with the unrolled commands; nested mode
        // also walks the original body below.
        if !ctx.nested {
            return;
        }
    }
    // The iterated values can hide substitutions (`for x in $(cmd)`).
    for value in f.values.iter().flatten() {
        collect_word_subs(&value.value, None, ctx.subs);
    }
    walk_compound_list(&f.body.list, ctx, scope.enter(id, Section::Body));
}

/// Walks the condition list, then the `do` body; both re-run each iteration.
fn walk_while_until(
    w: &ast::WhileOrUntilClauseCommand,
    kind: LoopKind,
    ctx: &mut Ctx,
    scope: Scope,
) {
    let id = ctx.record_loop(kind, None, scope);
    walk_compound_list(&w.0, ctx, scope.enter(id, Section::Condition));
    walk_compound_list(&w.1.list, ctx, scope.enter(id, Section::Body));
}

/// Walks the loop body once per literal, each pass with the variable bound. If anything
/// emitted shows the unroll would be incorrect, the attempt is rolled back (truncated) and
/// `false` returned, so the caller walks the loop as a single body. A failure caused by
/// an *outer* binding recurs on the re-walk, so the outer loop falls back too.
fn try_unroll(
    f: &ast::ForClauseCommand,
    values: &[&str],
    id: LoopId,
    ctx: &mut Ctx,
    scope: Scope,
) -> bool {
    let start = ctx.out.len();
    let substart = ctx.subs.len();
    let loopstart = ctx.loops.len();
    for (i, value) in values.iter().enumerate() {
        if ctx.poisoned {
            break;
        }
        let mut bound = scope.active.to_vec();
        bound.push(Subst {
            var: &f.variable_name,
            value,
        });
        let iteration = Scope {
            active: &bound,
            ..scope.enter(id, Section::Iteration(i))
        };
        walk_compound_list(&f.body.list, ctx, iteration);
    }
    // A substitution source still naming the variable would surface as a stage
    // mentioning it after the loop defining it is gone. This covers every position
    // subs are collected from, stage-less ones included.
    if !ctx.poisoned {
        ctx.poisoned = ctx.subs[substart..]
            .iter()
            .any(|s| unroll::mentions_identifier(&s.source, &f.variable_name));
    }
    if ctx.poisoned {
        ctx.out.truncate(start);
        ctx.subs.truncate(substart);
        ctx.loops.truncate(loopstart);
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
