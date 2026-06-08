use brush_parser::{Parser, ParserOptions};

use crate::types::{NestedStage, Stage, Sub, Walked};
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

/// Buckets the flat walk order into pipelines (flat mode): a command that reads an
/// upstream pipe continues the current pipeline; any other command starts a new one.
pub(crate) fn group_into_pipelines(commands: &[Walked]) -> Vec<Vec<Stage>> {
    let mut pipelines: Vec<Vec<Stage>> = Vec::new();
    for cmd in commands {
        let stage = Stage::from_walked(cmd);
        match pipelines.last_mut() {
            Some(current) if cmd.piped_from_previous => current.push(stage),
            _ => pipelines.push(vec![stage]),
        }
    }
    pipelines
}

/// Builds nested pipelines (nested mode): only root commands at the top level,
/// substitutions embedded recursively under their parent command.
pub(crate) fn build_nested_pipelines(commands: &[Walked]) -> Vec<Vec<NestedStage>> {
    let mut pipelines: Vec<Vec<NestedStage>> = Vec::new();

    for (idx, cmd) in commands.iter().enumerate() {
        // A command with a parent is embedded under it, not surfaced at the top level.
        if cmd.parent.is_some() {
            continue;
        }

        let stage = build_nested_stage(idx, commands);
        match pipelines.last_mut() {
            Some(current) if cmd.piped_from_previous => current.push(stage),
            _ => pipelines.push(vec![stage]),
        }
    }

    pipelines
}

/// Recursively builds a nested stage from its index, grouping its substitutions into
/// sub-pipelines by their own pipe flags.
fn build_nested_stage(idx: usize, commands: &[Walked]) -> NestedStage {
    let cmd = &commands[idx];

    let mut substitutions: Vec<Vec<NestedStage>> = Vec::new();
    for &child_idx in &cmd.children {
        let child_stage = build_nested_stage(child_idx, commands);
        match substitutions.last_mut() {
            Some(current) if commands[child_idx].piped_from_previous => current.push(child_stage),
            _ => substitutions.push(vec![child_stage]),
        }
    }

    NestedStage {
        stage: Stage::from_walked(cmd),
        substitutions,
    }
}
