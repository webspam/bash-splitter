use brush_parser::ast;

use crate::redirect::{collect_redirect_expansions, redirect_of};
use crate::types::{Redirect, Sub};
use crate::word::collect_word_expansions;

/// Env assignments preceding the command name. Only [`AssignmentWord`](ast::CommandPrefixOrSuffixItem::AssignmentWord)s count; a prefix
/// is otherwise just redirects.
pub(crate) fn prefix_assignments(simple: &ast::SimpleCommand) -> Vec<String> {
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
pub(crate) fn suffix_args(simple: &ast::SimpleCommand) -> Vec<String> {
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

/// All prefix then suffix items, in source order.
pub(crate) fn prefix_suffix_items(
    simple: &ast::SimpleCommand,
) -> impl Iterator<Item = &ast::CommandPrefixOrSuffixItem> {
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    prefix.chain(suffix)
}

/// The command's I/O redirects, prefix then suffix, in source order. Process
/// substitutions inside them surface separately as their own pipelines.
pub(crate) fn simple_redirects(simple: &ast::SimpleCommand) -> Vec<Redirect> {
    prefix_suffix_items(simple)
        .filter_map(|item| match item {
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => Some(redirect_of(r)),
            _ => None,
        })
        .collect()
}

/// The parameters this command expands (deduped, first-seen) and the source of every
/// substitution in it, one pass in source order: prefix, name, suffix. Substitutions
/// are tagged with `parent`, the `id` of the stage they are emitted as.
pub(crate) fn collect_expansions(
    simple: &ast::SimpleCommand,
    parent: usize,
    subs: &mut Vec<Sub>,
) -> Vec<String> {
    let mut vars = Vec::new();
    for item in simple.prefix.iter().flat_map(|p| &p.0) {
        item_expansions(item, parent, &mut vars, subs);
    }
    if let Some(name) = &simple.word_or_name {
        collect_word_expansions(&name.value, Some(parent), &mut vars, subs);
    }
    for item in simple.suffix.iter().flat_map(|s| &s.0) {
        item_expansions(item, parent, &mut vars, subs);
    }
    vars
}

fn item_expansions(
    item: &ast::CommandPrefixOrSuffixItem,
    parent: usize,
    vars: &mut Vec<String>,
    subs: &mut Vec<Sub>,
) {
    match item {
        ast::CommandPrefixOrSuffixItem::Word(w)
        | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => {
            collect_word_expansions(&w.value, Some(parent), vars, subs)
        }
        ast::CommandPrefixOrSuffixItem::IoRedirect(r) => {
            collect_redirect_expansions(r, Some(parent), vars, subs)
        }
        // A process substitution's body is its own surfaced pipeline; its variables
        // belong to the stages walked out of it.
        ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => subs.push(Sub {
            source: sub.list.to_string(),
            parent: Some(parent),
        }),
    }
}
