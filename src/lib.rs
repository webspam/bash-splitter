//! Splits a bash command into pipelines of stages, in source order. Commands inside
//! command and process substitutions surface as their own trailing pipelines, each
//! with a `parent` id back to the stage it came from (which lists it in `children`).
//! This only splits; the caller evaluates rules.

mod extended_test;
mod params;
mod pipeline;
mod redirect;
mod simple;
mod types;
mod walk;
mod word;

use brush_parser::{Parser, ParserOptions};

use pipeline::{expand_substitutions, group_into_pipelines};
pub use types::Stage;
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

/// Splits `input` into pipelines of stages.
///
/// # Errors
///
/// Returns [`ParseError`] when `input` does not parse as a bash program.
pub fn split(input: &str) -> Result<Vec<Vec<Stage>>, ParseError> {
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

    Ok(group_into_pipelines(commands))
}
