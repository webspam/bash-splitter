//! Splits a bash command into pipelines of stages, in source order.
//! Two modes: flat (default) lists all commands including those from substitutions;
//! nested embeds substitutions recursively under their parent command and
//! represents each loop as an object holding its contents.

mod arithmetic;
mod brush_workarounds;
mod extended_test;
mod params;
mod pipeline;
mod redirect;
mod resolve;
mod simple;
mod types;
mod unroll;
mod walk;
mod word;

use brush_parser::{Parser, ParserOptions};

use pipeline::{build_flat_pipelines, build_nested, expand_substitutions};
pub use types::{Iteration, NestedItem, NestedLoop, NestedStage, Stage};
use types::{LoopWalked, Walked};
use walk::{Ctx, Scope, walk_compound_list};

#[derive(Debug)]
pub enum Error {
    /// Unable to parse the input command(s).
    Parse(String),
    /// bash syntax that the splitter does not support was detected.
    Unsupported(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(message) => write!(f, "parse error: {message}"),
            Error::Unsupported(construct) => write!(f, "unsupported: {construct}"),
        }
    }
}

impl std::error::Error for Error {}

/// Splits `input` into flat pipelines of stages (default mode).
/// All commands, including those from substitutions, appear as top-level pipelines.
///
/// When `unroll` is set, statically-analysable `for` loops are unrolled into their
/// individual commands (`for f in a b; do echo $f; done` -> `echo a`, `echo b`).
///
/// # Errors
///
/// Returns [`Error`] when `input` cannot be split.
pub fn split(input: &str, unroll: bool) -> Result<Vec<Vec<Stage>>, Error> {
    let (commands, _) = walk_and_unroll(input, unroll, false)?;
    Ok(build_flat_pipelines(&commands))
}

/// Splits `input` into nested items: pipelines of stages, and loop objects.
/// Only root items appear at the top level; substitutions are embedded recursively.
/// A loop carries its body as written; with `unroll` set, a statically-analysable
/// `for` loop also carries its unrolled per-iteration commands.
///
/// # Errors
///
/// Returns [`Error`] when `input` cannot be split.
pub fn split_nested(input: &str, unroll: bool) -> Result<Vec<NestedItem>, Error> {
    let (commands, loops) = walk_and_unroll(input, unroll, true)?;
    Ok(build_nested(&commands, &loops))
}

fn walk_and_unroll(
    input: &str,
    unroll: bool,
    nested: bool,
) -> Result<(Vec<Walked>, Vec<LoopWalked>), Error> {
    brush_workarounds::reject_ansi_c_line_continuation(input)?;
    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let program = parser
        .parse_program()
        .map_err(|e| Error::Parse(e.to_string()))?;

    let mut commands = Vec::new();
    let mut subs = Vec::new();
    let mut loops = Vec::new();
    for complete_command in &program.complete_commands {
        let mut ctx = Ctx::new(&mut commands, &mut subs, &mut loops, unroll, nested);
        walk_compound_list(complete_command, &mut ctx, Scope::root());
        ctx.reject_unsupported()?;
    }
    expand_substitutions(&mut commands, &mut loops, subs, unroll, nested)?;

    Ok((commands, loops))
}
