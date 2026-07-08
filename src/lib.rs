//! Splits a bash command into pipelines of stages, in source order.
//! Two modes: flat (default) lists all commands including those from substitutions;
//! nested embeds substitutions recursively under their parent command.

mod extended_test;
mod params;
mod pipeline;
mod redirect;
mod simple;
mod types;
mod unroll;
mod walk;
mod word;

use brush_parser::{Parser, ParserOptions};

use pipeline::{build_nested_pipelines, expand_substitutions, group_into_pipelines};
use types::Walked;
pub use types::{NestedStage, Stage};
use walk::{Ctx, walk_compound_list};

/// A parse error from the input command.
#[derive(Debug)]
pub struct ParseError(String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for ParseError {}

/// Splits `input` into flat pipelines of stages (default mode).
/// All commands, including those from substitutions, appear as top-level pipelines.
///
/// When `unroll` is set, statically-analysable `for` loops are unrolled into their
/// individual commands (`for f in a b; do echo $f; done` -> `echo a`, `echo b`).
///
/// # Errors
///
/// Returns [`ParseError`] when `input` does not parse as a bash program.
pub fn split(input: &str, unroll: bool) -> Result<Vec<Vec<Stage>>, ParseError> {
    let commands = walk_and_unroll(input, unroll)?;
    Ok(group_into_pipelines(&commands))
}

/// Splits `input` into nested pipelines of stages.
/// Only root commands appear at the top level; substitutions are embedded recursively.
/// `unroll` toggles static `for`-loop unrolling, as in [`split`].
///
/// # Errors
///
/// Returns [`ParseError`] when `input` does not parse as a bash program.
pub fn split_nested(input: &str, unroll: bool) -> Result<Vec<Vec<NestedStage>>, ParseError> {
    let commands = walk_and_unroll(input, unroll)?;
    Ok(build_nested_pipelines(&commands))
}

fn walk_and_unroll(input: &str, unroll: bool) -> Result<Vec<Walked>, ParseError> {
    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let program = parser
        .parse_program()
        .map_err(|e| ParseError(e.to_string()))?;

    let mut commands = Vec::new();
    let mut subs = Vec::new();
    for complete_command in &program.complete_commands {
        let mut ctx = Ctx::new(&mut commands, &mut subs, unroll);
        walk_compound_list(complete_command, false, &mut ctx, &[]);
    }
    expand_substitutions(&mut commands, subs, unroll);

    // Backfill the reverse edge: each parent learns which stages surfaced from it.
    let edges: Vec<(usize, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(id, w)| w.parent.map(|p| (p.0, id)))
        .collect();
    for (parent, child) in edges {
        commands[parent].children.push(types::StageId(child));
    }

    Ok(commands)
}
