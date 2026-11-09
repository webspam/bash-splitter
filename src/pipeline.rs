use std::collections::HashMap;

use brush_parser::{Parser, ParserOptions};

use crate::Error;
use crate::brush_workarounds::reject_ansi_c_line_continuation;
use crate::types::{
    Iteration, Loc, LoopId, LoopWalked, NestedItem, NestedLoop, NestedStage, Section, Stage,
    StageId, Sub, Walked,
};
use crate::walk::{Ctx, Scope, walk_compound_list};

/// Appends each substitution's commands (and loops) after the walked ones, level by
/// level so nested substitutions surface too.
pub(crate) fn expand_substitutions(
    commands: &mut Vec<Walked>,
    loops: &mut Vec<LoopWalked>,
    mut pending: Vec<Sub>,
    unroll: bool,
    nested: bool,
) -> Result<(), Error> {
    while !pending.is_empty() {
        let mut next = Vec::new();
        for sub in &pending {
            // Everything this source walks out is a direct child of the stage the
            // substitution sat in; deeper substitutions surface in the next round.
            let start = commands.len();
            let loopstart = loops.len();
            split_source(&sub.source, commands, loops, &mut next, unroll, nested)?;
            for walked in &mut commands[start..] {
                walked.parent = sub.parent;
            }
            for l in &mut loops[loopstart..] {
                l.sub_parent = sub.parent;
            }
        }
        pending = next;
    }
    Ok(())
}

/// Parses one substitution source and walks it.
fn split_source(
    input: &str,
    out: &mut Vec<Walked>,
    loops: &mut Vec<LoopWalked>,
    subs: &mut Vec<Sub>,
    unroll: bool,
    nested: bool,
) -> Result<(), Error> {
    reject_ansi_c_line_continuation(input)?;
    let mut parser = Parser::new(input.as_bytes(), &ParserOptions::default());
    let program = parser
        .parse_program()
        .map_err(|e| Error::Parse(format!("in substitution `{input}`: {e}")))?;
    // A re-parsed substitution starts a fresh tree; loop bindings do not cross in.
    let mut ctx = Ctx::new(out, subs, loops, unroll, nested);
    for complete_command in &program.complete_commands {
        walk_compound_list(complete_command, &mut ctx, Scope::root());
    }
    ctx.reject_unsupported()
}

/// Groups a flagged sequence into pipelines: a piped item continues the current
/// pipeline; any other starts a new one.
fn group_into_pipelines<T>(items: impl Iterator<Item = (T, bool)>) -> Vec<Vec<T>> {
    let mut pipelines: Vec<Vec<T>> = Vec::new();
    for (item, piped) in items {
        match pipelines.last_mut() {
            Some(current) if piped => current.push(item),
            _ => pipelines.push(vec![item]),
        }
    }
    pipelines
}

/// Groups the flat walk order into pipelines (flat mode).
pub(crate) fn build_flat_pipelines(commands: &[Walked]) -> Vec<Vec<Stage>> {
    group_into_pipelines(
        commands
            .iter()
            .map(|cmd| (cmd.stage.clone(), cmd.piped_from_previous)),
    )
}

/// Builds nested output: only root items at the top level, substitutions embedded
/// recursively under their parent command, loops rebuilt as objects.
pub(crate) fn build_nested(commands: &[Walked], loops: &[LoopWalked]) -> Vec<NestedItem> {
    Builder::new(commands, loops).items(Level::Root)
}

/// Where an element sits in the nested output tree.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Level {
    /// The top level of the output.
    Root,
    /// The `substitutions` of the stage with this index.
    Sub(usize),
    /// One section of a loop object.
    Slot(Loc),
}

/// A stage or loop at one level of the tree.
#[derive(Clone, Copy)]
enum Element {
    Stage(usize),
    Loop(usize),
}

struct Builder<'a> {
    commands: &'a [Walked],
    loops: &'a [LoopWalked],
    by_level: HashMap<Level, Vec<Element>>,
}

impl<'a> Builder<'a> {
    fn new(commands: &'a [Walked], loops: &'a [LoopWalked]) -> Self {
        let mut by_level: HashMap<Level, Vec<Element>> = HashMap::new();
        for (idx, cmd) in commands.iter().enumerate() {
            let level = match (cmd.loc, cmd.parent) {
                (Some(loc), _) => Level::Slot(loc),
                (None, Some(StageId(p))) => Level::Sub(p),
                (None, None) => Level::Root,
            };
            by_level.entry(level).or_default().push(Element::Stage(idx));
        }
        for (id, l) in loops.iter().enumerate() {
            let level = match (l.parent, l.sub_parent) {
                (Some(loc), _) => Level::Slot(loc),
                (None, Some(StageId(p))) => Level::Sub(p),
                (None, None) => Level::Root,
            };
            by_level.entry(level).or_default().push(Element::Loop(id));
        }
        // Order siblings by stage index; on a tie (a loop that emitted no stages)
        // the loop comes first.
        for elements in by_level.values_mut() {
            elements.sort_by_key(|el| match *el {
                Element::Stage(idx) => (idx, 1, 0),
                Element::Loop(id) => (loops[id].position, 0, id),
            });
        }
        Builder {
            commands,
            loops,
            by_level,
        }
    }

    /// The ordered items at one level: stages grouped into pipelines by their pipe
    /// flags, loops as their own objects. A loop breaks the pipeline around it.
    fn items(&self, level: Level) -> Vec<NestedItem> {
        let mut out: Vec<NestedItem> = Vec::new();
        for &el in self.by_level.get(&level).map_or(&[][..], Vec::as_slice) {
            match el {
                Element::Stage(idx) => {
                    let stage = self.stage(idx);
                    match out.last_mut() {
                        Some(NestedItem::Pipeline(p)) if self.commands[idx].piped_from_previous => {
                            p.push(stage)
                        }
                        _ => out.push(NestedItem::Pipeline(vec![stage])),
                    }
                }
                Element::Loop(id) => out.push(NestedItem::Loop(self.build_loop(id))),
            }
        }
        out
    }

    fn stage(&self, idx: usize) -> NestedStage {
        NestedStage {
            stage: self.commands[idx].stage.clone(),
            substitutions: self.items(Level::Sub(idx)),
        }
    }

    fn build_loop(&self, id: usize) -> NestedLoop {
        let l = &self.loops[id];
        let slot = |section| {
            self.items(Level::Slot(Loc {
                loop_id: LoopId(id),
                section,
            }))
        };
        NestedLoop {
            kind: l.kind,
            variable: l.variable.clone(),
            condition: slot(Section::Condition),
            body: slot(Section::Body),
            unrolled: l.values.as_ref().map(|values| {
                values
                    .iter()
                    .enumerate()
                    .map(|(i, value)| Iteration {
                        value: value.clone(),
                        commands: slot(Section::Iteration(i)),
                    })
                    .collect()
            }),
        }
    }
}
