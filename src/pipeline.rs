use brush_parser::{Parser, ParserOptions};

use crate::types::{Stage, Sub, Walked};
use crate::walk::walk_compound_list;

/// Appends each substitution's commands after the walked ones, level by level so
/// nested substitutions surface too.
pub(crate) fn expand_substitutions(commands: &mut Vec<Walked>, mut pending: Vec<Sub>) {
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
pub(crate) fn group_into_pipelines(commands: Vec<Walked>) -> Vec<Vec<Stage>> {
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
