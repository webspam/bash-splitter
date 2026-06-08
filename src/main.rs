//! Splits a bash command (on stdin) into pipelines, as a JSON array of stage arrays
//! in source order. Commands inside command and process substitutions surface as
//! their own trailing pipelines, each with a `parent` id back to the stage it came
//! from (which lists it in `children`). This only splits; the caller evaluates rules.

use brush_parser::word::{self, WordPiece, WordPieceWithSource};
use brush_parser::{Parser, ParserOptions, ast};
use serde::Serialize;
use std::io::{Read, Write};

/// One command: a single stage of a pipeline.
#[derive(Serialize)]
struct Stage {
    /// Stable index across the whole flattened output; `parent`/`children` reference it.
    id: usize,
    /// Reconstructed text of this single command.
    command: String,
    /// Leading env assignments (the `LD_PRELOAD=x` in `LD_PRELOAD=x cmd`, or a bare `FOO=bar`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    assignments: Vec<String>,
    /// The command actually invoked (`cmd` in `LD_PRELOAD=x cmd -a`). Absent for a
    /// bare assignment (`FOO=bar`), which runs nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// The command's arguments, in order. Redirects and process substitutions are excluded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    /// The command's I/O redirects, in source order (heredoc bodies included).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    redirects: Vec<Redirect>,
    /// True when this command sits inside a `for`/`while`/`until` loop, so it runs
    /// once per iteration. Not tracked across substitution boundaries.
    #[serde(skip_serializing_if = "<&bool as std::ops::Not>::not")]
    in_loop: bool,
    /// Names of the parameters this command expands (`$f`, `${x}`, `$1`), deduped in
    /// first-seen order. A single-quoted or quoted-heredoc reference contributes none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variables: Vec<String>,
    /// The `id` of the stage this one surfaced from (`cd`, for the `echo` in `cd "$(echo pie)"`).
    /// Absent for top-level, or a substitution in a container that runs nothing (`[[ ]]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<usize>,
    /// The `id`s of the stages surfaced from this one's substitutions. Non-empty marks
    /// a complex command whose words or redirects hide other commands.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<usize>,
}

/// One I/O redirect on a command. `kind` tags the target family: `file`, `fd`,
/// `process_sub`, `herestring`, `heredoc`.
#[derive(Serialize)]
struct Redirect {
    /// The explicit fd, if the source gave one (`2>` -> 2). Absent means bash's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    fd: Option<i32>,
    /// The redirect operator as written (`>`, `>>`, `<`, `<<`, `<<<`, `>&`, `&>`, ...).
    op: String,
    /// Target family; see the struct doc.
    kind: &'static str,
    /// The target text (filename, fd number, or rendered process substitution). Absent
    /// for a heredoc, whose payload lives in `heredoc`.
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    /// Present only for heredocs.
    #[serde(skip_serializing_if = "Option::is_none")]
    heredoc: Option<HereDoc>,
}

/// A heredoc body and how bash treats it.
#[derive(Serialize)]
struct HereDoc {
    /// The end delimiter, raw (`EOF`, `'EOF'`); quoting is reflected in `expands`.
    delimiter: String,
    /// False when the delimiter is quoted (`<<'EOF'`), which suppresses expansion.
    expands: bool,
    /// The raw body text; leading tabs are stripped for `<<-` (shown in `op`), as bash does.
    body: String,
}

/// A walked command plus internal bookkeeping used to group stages into pipelines.
struct Walked {
    command: String,
    assignments: Vec<String>,
    name: Option<String>,
    args: Vec<String>,
    redirects: Vec<Redirect>,
    in_loop: bool,
    variables: Vec<String>,
    parent: Option<usize>,
    children: Vec<usize>,
    piped_from_previous: bool,
}

/// A substitution source still to be walked, tagged with the `id` of the stage it
/// came from (`None` for a container that runs nothing, e.g. `[[ ]]`).
struct Sub {
    source: String,
    parent: Option<usize>,
}

fn main() {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("bash-splitter: failed to read stdin: {e}");
        std::process::exit(1);
    }

    // Windows shells feed CRLF; normalize to LF so a stray `\r` doesn't cling to the
    // last token and corrupt argv (bash treats a bare `\r` as an ordinary character).
    let input = input.replace("\r\n", "\n").replace('\r', "\n");

    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let program = match parser.parse_program() {
        Ok(program) => program,
        // The caller decides what an unparseable command means; we just signal it.
        Err(e) => {
            eprintln!("bash-splitter: parse error: {e}");
            std::process::exit(2);
        }
    };

    let mut commands = Vec::new();
    let mut subs = Vec::new();
    for complete_command in &program.complete_commands {
        walk_compound_list(complete_command, false, &mut commands, &mut subs);
    }
    expand_substitutions(&mut commands, subs);

    // Backfill the reverse edge: each parent learns which stages surfaced from it.
    let edges: Vec<(usize, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(id, w)| w.parent.map(|p| (p, id)))
        .collect();
    for (parent, child) in edges {
        commands[parent].children.push(child);
    }

    let pipelines = group_into_pipelines(commands);

    let json = serde_json::to_string(&pipelines).expect("Stage is always serializable");
    // A consumer closing stdout early is normal, not a failure; exit cleanly on the
    // broken pipe rather than panicking.
    if let Err(e) = writeln!(std::io::stdout(), "{json}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("bash-splitter: failed to write stdout: {e}");
        std::process::exit(1);
    }
}

/// Appends each substitution's commands after the walked ones, level by level so
/// nested substitutions surface too.
fn expand_substitutions(commands: &mut Vec<Walked>, mut pending: Vec<Sub>) {
    while !pending.is_empty() {
        let mut next = Vec::new();
        for sub in &pending {
            // Every command this source walks out is a direct child of the stage the
            // substitution sat in; deeper substitutions surface in the next round.
            let start = commands.len();
            split_source(&sub.source, commands, &mut next);
            for walked in &mut commands[start..] {
                walked.parent = sub.parent;
            }
        }
        pending = next;
    }
}

/// Parses one source string and walks it. A substitution that does not parse on its
/// own surfaces nothing rather than aborting the whole split.
fn split_source(input: &str, out: &mut Vec<Walked>, subs: &mut Vec<Sub>) {
    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let Ok(program) = parser.parse_program() else {
        return;
    };
    // A re-parsed substitution starts a fresh tree; loop context does not cross in.
    for complete_command in &program.complete_commands {
        walk_compound_list(complete_command, false, out, subs);
    }
}

/// Buckets the flat walk order into pipelines: a command that reads an upstream
/// pipe continues the current pipeline; any other command starts a new one.
fn group_into_pipelines(commands: Vec<Walked>) -> Vec<Vec<Stage>> {
    let mut pipelines: Vec<Vec<Stage>> = Vec::new();
    for (id, cmd) in commands.into_iter().enumerate() {
        let stage = Stage {
            id,
            command: cmd.command,
            assignments: cmd.assignments,
            name: cmd.name,
            args: cmd.args,
            redirects: cmd.redirects,
            in_loop: cmd.in_loop,
            variables: cmd.variables,
            parent: cmd.parent,
            children: cmd.children,
        };
        match pipelines.last_mut() {
            Some(current) if cmd.piped_from_previous => current.push(stage),
            _ => pipelines.push(vec![stage]),
        }
    }
    pipelines
}

fn walk_compound_list(
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

/// Env assignments preceding the command name. Only [`AssignmentWord`](ast::CommandPrefixOrSuffixItem::AssignmentWord)s count; a prefix
/// is otherwise just redirects.
fn prefix_assignments(simple: &ast::SimpleCommand) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(prefix) = &simple.prefix {
        for item in &prefix.0 {
            if let ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) = item {
                out.push(w.value.clone());
            }
        }
    }
    out
}

/// The command's arguments. A suffix `foo=bar` is a literal arg, not an assignment.
/// Redirects and process substitutions are excluded.
fn suffix_args(simple: &ast::SimpleCommand) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(suffix) = &simple.suffix {
        for item in &suffix.0 {
            match item {
                ast::CommandPrefixOrSuffixItem::Word(w)
                | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => out.push(w.value.clone()),
                ast::CommandPrefixOrSuffixItem::IoRedirect(_)
                | ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, _) => {}
            }
        }
    }
    out
}

/// The command's I/O redirects, prefix then suffix, in source order. Process
/// substitutions inside them surface separately as their own pipelines.
fn simple_redirects(simple: &ast::SimpleCommand) -> Vec<Redirect> {
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    prefix
        .chain(suffix)
        .filter_map(|item| match item {
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => Some(redirect_of(r)),
            _ => None,
        })
        .collect()
}

/// Structures one redirect for output.
fn redirect_of(redirect: &ast::IoRedirect) -> Redirect {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(fd, op, target) => {
            let kind = match target {
                T::Filename(_) => "file",
                T::Fd(_) | T::Duplicate(_) => "fd",
                T::ProcessSubstitution(_, _) => "process_sub",
            };
            Redirect {
                fd: *fd,
                op: op.to_string(),
                kind,
                target: Some(target.to_string()),
                heredoc: None,
            }
        }
        R::OutputAndError(target, append) => Redirect {
            fd: None,
            op: if *append { "&>>" } else { "&>" }.to_string(),
            kind: "file",
            target: Some(target.value.clone()),
            heredoc: None,
        },
        R::HereString(fd, word) => Redirect {
            fd: *fd,
            op: "<<<".to_string(),
            kind: "herestring",
            target: Some(word.value.clone()),
            heredoc: None,
        },
        R::HereDocument(fd, doc) => Redirect {
            fd: *fd,
            op: if doc.remove_tabs { "<<-" } else { "<<" }.to_string(),
            kind: "heredoc",
            target: None,
            heredoc: Some(HereDoc {
                delimiter: doc.here_end.value.clone(),
                expands: doc.requires_expansion,
                body: doc.doc.value.clone(),
            }),
        },
    }
}

/// The parameters this command expands, deduped in first-seen order, across its
/// assignments, name, args, and redirect targets/heredoc bodies.
fn simple_vars(simple: &ast::SimpleCommand) -> Vec<String> {
    let mut vars = Vec::new();
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    if let Some(name) = &simple.word_or_name {
        collect_word_vars(&name.value, &mut vars);
    }
    for item in prefix.chain(suffix) {
        match item {
            ast::CommandPrefixOrSuffixItem::Word(w)
            | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => {
                collect_word_vars(&w.value, &mut vars)
            }
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => collect_redirect_vars(r, &mut vars),
            // A process substitution's body is its own surfaced pipeline.
            ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, _) => {}
        }
    }
    vars
}

/// Variables in a redirect's expanded target, or an unquoted heredoc body.
#[allow(clippy::match_same_arms)]
fn collect_redirect_vars(redirect: &ast::IoRedirect, vars: &mut Vec<String>) {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => collect_word_vars(&w.value, vars),
        R::File(_, _, T::Fd(_) | T::ProcessSubstitution(_, _)) => {}
        R::OutputAndError(w, _) | R::HereString(_, w) => collect_word_vars(&w.value, vars),
        R::HereDocument(_, doc) if doc.requires_expansion => collect_word_vars(&doc.doc.value, vars),
        R::HereDocument(_, _) => {}
    }
}

/// Pulls the names of any parameter expansions out of a single word.
fn collect_word_vars(word: &str, vars: &mut Vec<String>) {
    let Ok(pieces) = word::parse(word, &ParserOptions::default()) else {
        return;
    };
    collect_piece_vars(&pieces, vars);
}

fn collect_piece_vars(pieces: &[WordPieceWithSource], vars: &mut Vec<String>) {
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

/// Appends `v` unless already present, keeping first-seen order.
fn push_unique(vars: &mut Vec<String>, v: String) {
    if !vars.contains(&v) {
        vars.push(v);
    }
}

/// `[[ ... ]]` words can hide substitutions (`[[ -n $(cmd) ]]`); surface them.
fn collect_extended_test_subs(expr: &ast::ExtendedTestExpr, subs: &mut Vec<Sub>) {
    use ast::ExtendedTestExpr as E;
    match expr {
        E::And(l, r) | E::Or(l, r) => {
            collect_extended_test_subs(l, subs);
            collect_extended_test_subs(r, subs);
        }
        E::Not(e) | E::Parenthesized(e) => collect_extended_test_subs(e, subs),
        // The test itself is never an emitted stage, so its substitutions have no parent.
        E::UnaryTest(_, w) => collect_word_subs(&w.value, None, subs),
        E::BinaryTest(_, l, r) => {
            collect_word_subs(&l.value, None, subs);
            collect_word_subs(&r.value, None, subs);
        }
    }
}

/// Collects the source of every substitution in a simple command: command
/// substitutions in its words, the bodies of process substitutions, and anything in
/// its redirects. Each is tagged with `parent`, the `id` of the stage it is emitted as.
fn collect_simple_subs(simple: &ast::SimpleCommand, parent: usize, subs: &mut Vec<Sub>) {
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    for item in prefix.chain(suffix) {
        match item {
            ast::CommandPrefixOrSuffixItem::Word(w)
            | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => {
                collect_word_subs(&w.value, Some(parent), subs)
            }
            ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => subs.push(Sub {
                source: sub.list.to_string(),
                parent: Some(parent),
            }),
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => {
                collect_redirect_subs(r, Some(parent), subs)
            }
        }
    }
    if let Some(name) = &simple.word_or_name {
        collect_word_subs(&name.value, Some(parent), subs);
    }
}

/// Pulls the bodies of any command substitutions out of a single word, tagging each
/// with the `parent` stage the word belongs to (`None` for a container that runs nothing).
fn collect_word_subs(word: &str, parent: Option<usize>, subs: &mut Vec<Sub>) {
    let Ok(pieces) = word::parse(word, &ParserOptions::default()) else {
        return;
    };
    collect_piece_subs(&pieces, parent, subs);
}

fn collect_piece_subs(pieces: &[WordPieceWithSource], parent: Option<usize>, subs: &mut Vec<Sub>) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::CommandSubstitution(s) | WordPiece::BackquotedCommandSubstitution(s) => {
                subs.push(Sub {
                    source: s.clone(),
                    parent,
                })
            }
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

/// The parameter an expansion acts on (`x` in `${x:-y}`), or `None` for the
/// prefix/key listings (`${!pre*}`, `${!arr[@]}`), which name no single parameter.
fn param_of(expr: &word::ParameterExpr) -> Option<&word::Parameter> {
    use word::ParameterExpr as P;
    match expr {
        P::Parameter { parameter, .. }
        | P::UseDefaultValues { parameter, .. }
        | P::AssignDefaultValues { parameter, .. }
        | P::IndicateErrorIfNullOrUnset { parameter, .. }
        | P::UseAlternativeValue { parameter, .. }
        | P::ParameterLength { parameter, .. }
        | P::RemoveSmallestSuffixPattern { parameter, .. }
        | P::RemoveLargestSuffixPattern { parameter, .. }
        | P::RemoveSmallestPrefixPattern { parameter, .. }
        | P::RemoveLargestPrefixPattern { parameter, .. }
        | P::Substring { parameter, .. }
        | P::Transform { parameter, .. }
        | P::UppercaseFirstChar { parameter, .. }
        | P::UppercasePattern { parameter, .. }
        | P::LowercaseFirstChar { parameter, .. }
        | P::LowercasePattern { parameter, .. }
        | P::ReplaceSubstring { parameter, .. } => Some(parameter),
        P::VariableNames { .. } | P::MemberKeys { .. } => None,
    }
}

/// The name of the variable a parameter references; positionals and specials render
/// as their token (`1`, `?`).
fn param_name(parameter: &word::Parameter) -> String {
    use word::Parameter as P;
    match parameter {
        P::Named(name)
        | P::NamedWithIndex { name, .. }
        | P::NamedWithAllIndices { name, .. } => name.clone(),
        P::Positional(n) => n.to_string(),
        P::Special(s) => s.to_string(),
    }
}

/// The subscript text of an array-indexed parameter (`arr[idx]`), if any. The
/// index is arithmetic-evaluated, so a substitution in it runs (`${arr[$(cmd)]}`).
fn param_subscript(expr: &word::ParameterExpr) -> Option<&str> {
    match param_of(expr)? {
        word::Parameter::NamedWithIndex { index, .. } => Some(index),
        _ => None,
    }
}

/// A parameter expansion's value and pattern words are themselves expanded, so a
/// command or variable in one is reachable (`${x:-$(cmd)}`, `${x/$y/z}`, `${x:$n}`).
/// `sink` receives each such word.
fn collect_param_expr_words(expr: &word::ParameterExpr, mut sink: impl FnMut(&str)) {
    use word::ParameterExpr as P;
    match expr {
        P::UseDefaultValues {
            default_value: Some(s),
            ..
        }
        | P::AssignDefaultValues {
            default_value: Some(s),
            ..
        }
        | P::UseAlternativeValue {
            alternative_value: Some(s),
            ..
        }
        | P::IndicateErrorIfNullOrUnset {
            error_message: Some(s),
            ..
        }
        | P::RemoveSmallestSuffixPattern {
            pattern: Some(s), ..
        }
        | P::RemoveLargestSuffixPattern {
            pattern: Some(s), ..
        }
        | P::RemoveSmallestPrefixPattern {
            pattern: Some(s), ..
        }
        | P::RemoveLargestPrefixPattern {
            pattern: Some(s), ..
        }
        | P::UppercaseFirstChar {
            pattern: Some(s), ..
        }
        | P::UppercasePattern {
            pattern: Some(s), ..
        }
        | P::LowercaseFirstChar {
            pattern: Some(s), ..
        }
        | P::LowercasePattern {
            pattern: Some(s), ..
        } => sink(s),
        P::ReplaceSubstring {
            pattern,
            replacement,
            ..
        } => {
            sink(pattern);
            if let Some(r) = replacement {
                sink(r);
            }
        }
        P::Substring { offset, length, .. } => {
            sink(&offset.value);
            if let Some(l) = length {
                sink(&l.value);
            }
        }
        _ => {}
    }
}

/// Redirect targets are expanded, so a substitution in one runs (`> $(cmd)`,
/// `<<< "$(cmd)"`, `> >(cmd)`, or an unquoted heredoc body).
#[allow(clippy::match_same_arms)]
fn collect_redirect_subs(redirect: &ast::IoRedirect, parent: Option<usize>, subs: &mut Vec<Sub>) {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => collect_word_subs(&w.value, parent, subs),
        R::File(_, _, T::ProcessSubstitution(_, sub)) => subs.push(Sub {
            source: sub.list.to_string(),
            parent,
        }),
        R::File(_, _, T::Fd(_)) => {}
        R::OutputAndError(w, _) | R::HereString(_, w) => collect_word_subs(&w.value, parent, subs),
        // Only an unquoted delimiter expands the body.
        R::HereDocument(_, doc) if doc.requires_expansion => {
            collect_word_subs(&doc.doc.value, parent, subs)
        }
        R::HereDocument(_, _) => {}
    }
}

/// Redirects can also hang off a compound command or `[[ ]]` (`while ...; done > $(cmd)`).
/// The compound runs no command of its own, so its substitutions have no parent stage.
fn collect_redirect_list_subs(redirects: Option<&ast::RedirectList>, subs: &mut Vec<Sub>) {
    for redirect in redirects.iter().flat_map(|r| &r.0) {
        collect_redirect_subs(redirect, None, subs);
    }
}
