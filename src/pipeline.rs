use brush_parser::{Parser, ParserOptions};

use crate::types::{NestedStage, Stage, StageId, Sub, Walked};
use crate::walk::{Ctx, walk_compound_list};

/// Appends each substitution's commands after the walked ones, level by level so
/// nested substitutions surface too.
pub(crate) fn expand_substitutions(
    commands: &mut Vec<Walked>,
    mut pending: Vec<Sub>,
    unroll: bool,
) {
    while !pending.is_empty() {
        let mut next = Vec::new();
        for sub in &pending {
            // Every command this source walks out is a direct child of the stage the
            // substitution sat in; deeper substitutions surface in the next round.
            let start = commands.len();
            split_source(&sub.source, commands, &mut next, unroll);
            for walked in &mut commands[start..] {
                walked.parent = sub.parent;
            }
        }
        pending = next;
    }
}

/// Parses one source string and walks it. A substitution that does not parse on its
/// own surfaces nothing rather than aborting the whole split.
fn split_source(input: &str, out: &mut Vec<Walked>, subs: &mut Vec<Sub>, unroll: bool) {
    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let Ok(program) = parser.parse_program() else {
        return;
    };
    // A re-parsed substitution starts a fresh tree; loop bindings do not cross in.
    let mut ctx = Ctx::new(out, subs, unroll);
    for complete_command in &program.complete_commands {
        walk_compound_list(complete_command, false, &mut ctx, &[]);
    }
}

/// Buckets items into pipelines: a piped item continues the current pipeline; any
/// other starts a new one.
fn bucket_piped<T>(items: impl Iterator<Item = (T, bool)>) -> Vec<Vec<T>> {
    let mut pipelines: Vec<Vec<T>> = Vec::new();
    for (item, piped) in items {
        match pipelines.last_mut() {
            Some(current) if piped => current.push(item),
            _ => pipelines.push(vec![item]),
        }
    }
    pipelines
}

/// Buckets the flat walk order into pipelines (flat mode).
pub(crate) fn group_into_pipelines(commands: &[Walked]) -> Vec<Vec<Stage>> {
    bucket_piped(
        commands
            .iter()
            .map(|cmd| (cmd.stage.clone(), cmd.piped_from_previous)),
    )
}

/// Builds nested pipelines (nested mode): only root commands at the top level,
/// substitutions embedded recursively under their parent command.
pub(crate) fn build_nested_pipelines(commands: &[Walked]) -> Vec<Vec<NestedStage>> {
    bucket_piped(
        commands
            .iter()
            .enumerate()
            // A command with a parent is embedded under it, not surfaced at the top level.
            .filter(|(_, cmd)| cmd.parent.is_none())
            .map(|(idx, cmd)| (build_nested_stage(idx, commands), cmd.piped_from_previous)),
    )
}

/// Recursively builds a nested stage from its index, grouping its substitutions into
/// sub-pipelines by their own pipe flags.
fn build_nested_stage(idx: usize, commands: &[Walked]) -> NestedStage {
    let cmd = &commands[idx];
    NestedStage {
        stage: cmd.stage.clone(),
        substitutions: bucket_piped(cmd.children.iter().map(|&StageId(child_idx)| {
            (
                build_nested_stage(child_idx, commands),
                commands[child_idx].piped_from_previous,
            )
        })),
    }
}
