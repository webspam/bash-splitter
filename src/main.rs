//! Splits a bash command string into its individual commands, grouped by the
//! pipeline each belongs to. Reads the raw command on stdin, writes a JSON array
//! of pipelines on stdout: each pipeline is an array of its stages in source
//! order, so a stage's pipe position is implied by its index (stage 0 reads no
//! pipe; the last stage feeds none). Commands hidden inside command and process
//! substitutions are surfaced too, as their own trailing pipelines. Rule evaluation
//! lives in the caller; this binary only splits.

use brush_parser::word::{self, WordPiece, WordPieceWithSource};
use brush_parser::{Parser, ParserOptions, ast};
use serde::Serialize;
use std::io::{Read, Write};

/// One command: a single stage of a pipeline.
#[derive(Serialize)]
struct Stage {
    /// Reconstructed text of this single command.
    command: String,
    /// Leading env assignments (`LD_PRELOAD=x` in `LD_PRELOAD=x cmd`), or the lone
    /// assignment of a bare `FOO=bar`. These set the environment, not argv.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    assignments: Vec<String>,
    /// The command actually invoked (`cmd` in `LD_PRELOAD=x cmd -a`). Absent for a
    /// bare assignment (`FOO=bar`), which runs nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// The command's arguments, in order. Redirects and process substitutions are
    /// excluded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
}

// A walked command plus whether its stdin comes from an upstream `|`. The flag is
// internal: it groups stages into pipelines and is not serialized (the output
// implies pipe position from a stage's index within its pipeline).
struct Walked {
    command: String,
    assignments: Vec<String>,
    name: Option<String>,
    args: Vec<String>,
    piped_from_previous: bool,
}

fn main() {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("bash-splitter: failed to read stdin: {e}");
        std::process::exit(1);
    }

    // PowerShell and other Windows shells feed CRLF; normalize to LF so a trailing
    // `\r` never clings to the last token. bash treats a bare `\r` as an ordinary
    // character, so leaving it in would corrupt argv.
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
        walk_compound_list(complete_command, &mut commands, &mut subs);
    }
    expand_substitutions(&mut commands, subs);

    let pipelines = group_into_pipelines(commands);

    let json = serde_json::to_string(&pipelines).expect("Stage is always serializable");
    // A downstream consumer closing stdout early is a normal end-of-work signal,
    // not a failure; exit cleanly rather than panicking on the broken pipe.
    if let Err(e) = writeln!(std::io::stdout(), "{json}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("bash-splitter: failed to write stdout: {e}");
        std::process::exit(1);
    }
}

// Parses each substitution's source and appends its commands after the ones already
// walked, level by level so nested substitutions surface too. They land at the end
// because they are separate programs; keeping them out of the main run preserves its
// pipeline grouping (which is positional).
fn expand_substitutions(commands: &mut Vec<Walked>, mut pending: Vec<String>) {
    while !pending.is_empty() {
        let mut next = Vec::new();
        for source in &pending {
            split_source(source, commands, &mut next);
        }
        pending = next;
    }
}

// Parses one source string and walks it. A substitution that does not parse on its
// own surfaces nothing rather than aborting the whole split.
fn split_source(input: &str, out: &mut Vec<Walked>, subs: &mut Vec<String>) {
    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let Ok(program) = parser.parse_program() else {
        return;
    };
    for complete_command in &program.complete_commands {
        walk_compound_list(complete_command, out, subs);
    }
}

// Buckets the flat walk order into pipelines: a command that reads an upstream
// pipe continues the current pipeline; any other command starts a new one.
fn group_into_pipelines(commands: Vec<Walked>) -> Vec<Vec<Stage>> {
    let mut pipelines: Vec<Vec<Stage>> = Vec::new();
    for cmd in commands {
        let stage = Stage {
            command: cmd.command,
            assignments: cmd.assignments,
            name: cmd.name,
            args: cmd.args,
        };
        match pipelines.last_mut() {
            Some(current) if cmd.piped_from_previous => current.push(stage),
            _ => pipelines.push(vec![stage]),
        }
    }
    pipelines
}

fn walk_compound_list(list: &ast::CompoundList, out: &mut Vec<Walked>, subs: &mut Vec<String>) {
    for item in &list.0 {
        walk_and_or_list(&item.0, out, subs);
    }
}

fn walk_and_or_list(list: &ast::AndOrList, out: &mut Vec<Walked>, subs: &mut Vec<String>) {
    walk_pipeline(&list.first, out, subs);
    for and_or in &list.additional {
        match and_or {
            ast::AndOr::And(pipeline) | ast::AndOr::Or(pipeline) => {
                walk_pipeline(pipeline, out, subs)
            }
        }
    }
}

fn walk_pipeline(pipeline: &ast::Pipeline, out: &mut Vec<Walked>, subs: &mut Vec<String>) {
    for (i, command) in pipeline.seq.iter().enumerate() {
        walk_command(command, i > 0, out, subs);
    }
}

fn walk_command(
    command: &ast::Command,
    piped_from_previous: bool,
    out: &mut Vec<Walked>,
    subs: &mut Vec<String>,
) {
    match command {
        ast::Command::Simple(simple) => {
            out.push(Walked {
                command: simple.to_string(),
                assignments: prefix_assignments(simple),
                name: simple.word_or_name.as_ref().map(|w| w.value.clone()),
                args: suffix_args(simple),
                piped_from_previous,
            });
            collect_simple_subs(simple, subs);
        }
        // Descend so commands nested in the compound surface individually. As a
        // pipeline stage (`foo | (grep x)`), its first inner command takes the pipe.
        ast::Command::Compound(compound, redirects) => {
            let start = out.len();
            walk_compound_command(compound, out, subs);
            apply_pipe_boundary(&mut out[start..], piped_from_previous);
            collect_redirect_list_subs(redirects, subs);
        }
        // The body runs when the function is later called, so surface its commands.
        ast::Command::Function(f) => walk_compound_command(&f.body.0, out, subs),
        // `[[ ... ]]` runs no command, but its words and redirects can hide substitutions.
        ast::Command::ExtendedTest(e, redirects) => {
            collect_extended_test_subs(&e.expr, subs);
            collect_redirect_list_subs(redirects, subs);
        }
    }
}

// Surfaces every command nested in a compound's bodies and conditions; blocks are
// never emitted whole.
fn walk_compound_command(
    compound: &ast::CompoundCommand,
    out: &mut Vec<Walked>,
    subs: &mut Vec<String>,
) {
    use ast::CompoundCommand as C;
    match compound {
        C::Subshell(s) => walk_compound_list(&s.list, out, subs),
        C::BraceGroup(b) => walk_compound_list(&b.list, out, subs),
        C::ForClause(f) => {
            // The iterated values can hide substitutions (`for x in $(cmd)`).
            for value in f.values.iter().flatten() {
                collect_word_subs(&value.value, subs);
            }
            walk_compound_list(&f.body.list, out, subs);
        }
        C::ArithmeticForClause(f) => walk_compound_list(&f.body.list, out, subs),
        // Condition list, then `do` body.
        C::WhileClause(w) | C::UntilClause(w) => {
            walk_compound_list(&w.0, out, subs);
            walk_compound_list(&w.1.list, out, subs);
        }
        // The condition is itself a command (`if grep -q x; ...`).
        C::IfClause(i) => {
            walk_compound_list(&i.condition, out, subs);
            walk_compound_list(&i.then, out, subs);
            for else_clause in i.elses.iter().flatten() {
                if let Some(condition) = &else_clause.condition {
                    walk_compound_list(condition, out, subs);
                }
                walk_compound_list(&else_clause.body, out, subs);
            }
        }
        C::CaseClause(c) => {
            collect_word_subs(&c.value.value, subs);
            for item in &c.cases {
                for pattern in &item.patterns {
                    collect_word_subs(&pattern.value, subs);
                }
                if let Some(cmd) = &item.cmd {
                    walk_compound_list(cmd, out, subs);
                }
            }
        }
        C::Coprocess(c) => walk_command(&c.body, false, out, subs),
        // Arithmetic evaluates an expression; there is no command to surface.
        C::Arithmetic(_) => {}
    }
}

// When a grouping is itself a stage in a pipeline, only its first command reads
// the upstream pipe. `|=` so a connection from the group's own inner pipeline is
// preserved. The downstream side needs no handling: the command after the group
// records its own upstream connection, which is what grouping keys off.
fn apply_pipe_boundary(group: &mut [Walked], piped_from_previous: bool) {
    if let Some(first) = group.first_mut() {
        first.piped_from_previous |= piped_from_previous;
    }
}

// Env assignments preceding the command name. Only AssignmentWords count; a prefix
// is otherwise just redirects.
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

// The command's arguments. A suffix `foo=bar` is a literal arg, not an assignment.
// Redirects and process substitutions are excluded.
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

// `[[ ... ]]` words can hide substitutions (`[[ -n $(cmd) ]]`); surface them.
fn collect_extended_test_subs(expr: &ast::ExtendedTestExpr, subs: &mut Vec<String>) {
    use ast::ExtendedTestExpr as E;
    match expr {
        E::And(l, r) | E::Or(l, r) => {
            collect_extended_test_subs(l, subs);
            collect_extended_test_subs(r, subs);
        }
        E::Not(e) | E::Parenthesized(e) => collect_extended_test_subs(e, subs),
        E::UnaryTest(_, w) => collect_word_subs(&w.value, subs),
        E::BinaryTest(_, l, r) => {
            collect_word_subs(&l.value, subs);
            collect_word_subs(&r.value, subs);
        }
    }
}

// Collects the source of every substitution in a simple command: command
// substitutions inside its words, and the bodies of process substitutions.
fn collect_simple_subs(simple: &ast::SimpleCommand, subs: &mut Vec<String>) {
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    for item in prefix.chain(suffix) {
        match item {
            ast::CommandPrefixOrSuffixItem::Word(w)
            | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => {
                collect_word_subs(&w.value, subs)
            }
            ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => {
                subs.push(sub.list.to_string())
            }
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => collect_redirect_subs(r, subs),
        }
    }
    if let Some(name) = &simple.word_or_name {
        collect_word_subs(&name.value, subs);
    }
}

// Pulls the bodies of any command substitutions out of a single word.
fn collect_word_subs(word: &str, subs: &mut Vec<String>) {
    let Ok(pieces) = word::parse(word, &ParserOptions::default()) else {
        return;
    };
    collect_piece_subs(&pieces, subs);
}

fn collect_piece_subs(pieces: &[WordPieceWithSource], subs: &mut Vec<String>) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::CommandSubstitution(s) | WordPiece::BackquotedCommandSubstitution(s) => {
                subs.push(s.clone())
            }
            // Substitutions can nest inside double quotes (`"$(...)"`).
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => collect_piece_subs(inner, subs),
            // `${x:-$(cmd)}`, `${x/$(cmd)/y}`: the value/pattern words expand.
            WordPiece::ParameterExpansion(expr) => collect_param_expr_subs(expr, subs),
            // `$(( $(cmd) ))` runs cmd while evaluating the expression.
            WordPiece::ArithmeticExpression(a) => collect_word_subs(&a.value, subs),
            _ => {}
        }
    }
}

// A parameter expansion's value and pattern words are themselves expanded, so a
// substitution in one runs (`${x:-$(cmd)}`, `${x/$(cmd)/y}`, `${x:$(cmd)}`).
fn collect_param_expr_subs(expr: &word::ParameterExpr, subs: &mut Vec<String>) {
    use word::ParameterExpr as P;
    match expr {
        P::UseDefaultValues { default_value: Some(s), .. }
        | P::AssignDefaultValues { default_value: Some(s), .. }
        | P::UseAlternativeValue { alternative_value: Some(s), .. }
        | P::IndicateErrorIfNullOrUnset { error_message: Some(s), .. }
        | P::RemoveSmallestSuffixPattern { pattern: Some(s), .. }
        | P::RemoveLargestSuffixPattern { pattern: Some(s), .. }
        | P::RemoveSmallestPrefixPattern { pattern: Some(s), .. }
        | P::RemoveLargestPrefixPattern { pattern: Some(s), .. }
        | P::UppercaseFirstChar { pattern: Some(s), .. }
        | P::UppercasePattern { pattern: Some(s), .. }
        | P::LowercaseFirstChar { pattern: Some(s), .. }
        | P::LowercasePattern { pattern: Some(s), .. } => collect_word_subs(s, subs),
        P::ReplaceSubstring { pattern, replacement, .. } => {
            collect_word_subs(pattern, subs);
            if let Some(r) = replacement {
                collect_word_subs(r, subs);
            }
        }
        P::Substring { offset, length, .. } => {
            collect_word_subs(&offset.value, subs);
            if let Some(l) = length {
                collect_word_subs(&l.value, subs);
            }
        }
        _ => {}
    }
}

// Redirect targets are expanded, so a substitution in one runs (`> $(cmd)`,
// `<<< "$(cmd)"`, `> >(cmd)`, or an unquoted heredoc body).
fn collect_redirect_subs(redirect: &ast::IoRedirect, subs: &mut Vec<String>) {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => collect_word_subs(&w.value, subs),
        R::File(_, _, T::ProcessSubstitution(_, sub)) => subs.push(sub.list.to_string()),
        R::File(_, _, T::Fd(_)) => {}
        R::OutputAndError(w, _) | R::HereString(_, w) => collect_word_subs(&w.value, subs),
        // Only an unquoted delimiter expands the body.
        R::HereDocument(_, doc) if doc.requires_expansion => {
            collect_word_subs(&doc.doc.value, subs)
        }
        R::HereDocument(_, _) => {}
    }
}

// Redirects can also hang off a compound command or `[[ ]]` (`while ...; done > $(cmd)`).
fn collect_redirect_list_subs(redirects: &Option<ast::RedirectList>, subs: &mut Vec<String>) {
    for redirect in redirects.iter().flat_map(|r| &r.0) {
        collect_redirect_subs(redirect, subs);
    }
}
