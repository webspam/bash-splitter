//! Splits a bash command into pipelines of stages, in source order.
//! Two modes: flat (default) lists all commands including those from substitutions;
//! nested embeds substitutions recursively under their parent command.

mod extended_test;
mod params;
mod pipeline;
mod redirect;
mod simple;
mod types;
mod walk;
mod word;

use brush_parser::{Parser, ParserOptions};

use pipeline::{build_nested_pipelines, expand_substitutions, group_into_pipelines};
use types::Walked;
pub use types::{NestedStage, Stage};
use walk::walk_compound_list;

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
/// # Errors
///
/// Returns [`ParseError`] when `input` does not parse as a bash program.
pub fn split(input: &str) -> Result<Vec<Vec<Stage>>, ParseError> {
    let commands = walk_and_expand(input)?;
    Ok(group_into_pipelines(&commands))
}

/// Splits `input` into nested pipelines of stages.
/// Only root commands appear at the top level; substitutions are embedded recursively.
///
/// # Errors
///
/// Returns [`ParseError`] when `input` does not parse as a bash program.
pub fn split_nested(input: &str) -> Result<Vec<Vec<NestedStage>>, ParseError> {
    let commands = walk_and_expand(input)?;
    Ok(build_nested_pipelines(&commands))
}

fn walk_and_expand(input: &str) -> Result<Vec<Walked>, ParseError> {
    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let program = parser
        .parse_program()
        .map_err(|e| ParseError(e.to_string()))?;

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

    Ok(commands)
}
