use brush_parser::ast;

use crate::redirect::{collect_redirect_subs, collect_redirect_vars, redirect_of};
use crate::types::{Redirect, Sub};
use crate::word::{collect_word_subs, collect_word_vars};

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

/// The command's I/O redirects, prefix then suffix, in source order. Process
/// substitutions inside them surface separately as their own pipelines.
pub(crate) fn simple_redirects(simple: &ast::SimpleCommand) -> Vec<Redirect> {
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    prefix
        .chain(suffix)
        .filter_map(|item| match item {
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => Some(redirect_of(r)),
            _ => None,
        })
        .collect()
}

/// The parameters this command expands, deduped in first-seen order, across its
/// assignments, name, args, and redirect targets/heredoc bodies.
pub(crate) fn simple_vars(simple: &ast::SimpleCommand) -> Vec<String> {
    let mut vars = Vec::new();
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    if let Some(name) = &simple.word_or_name {
        collect_word_vars(&name.value, &mut vars);
    }
    for item in prefix.chain(suffix) {
        match item {
            ast::CommandPrefixOrSuffixItem::Word(w)
            | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => {
                collect_word_vars(&w.value, &mut vars)
            }
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => collect_redirect_vars(r, &mut vars),
            // A process substitution's body is its own surfaced pipeline.
            ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, _) => {}
        }
    }
    vars
}

/// Collects the source of every substitution in a simple command: command
/// substitutions in its words, the bodies of process substitutions, and anything in
/// its redirects. Each is tagged with `parent`, the `id` of the stage it is emitted as.
pub(crate) fn collect_simple_subs(simple: &ast::SimpleCommand, parent: usize, subs: &mut Vec<Sub>) {
    let prefix = simple.prefix.iter().flat_map(|p| &p.0);
    let suffix = simple.suffix.iter().flat_map(|s| &s.0);
    for item in prefix.chain(suffix) {
        match item {
            ast::CommandPrefixOrSuffixItem::Word(w)
            | ast::CommandPrefixOrSuffixItem::AssignmentWord(_, w) => {
                collect_word_subs(&w.value, Some(parent), subs)
            }
            ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => subs.push(Sub {
                source: sub.list.to_string(),
                parent: Some(parent),
            }),
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => {
                collect_redirect_subs(r, Some(parent), subs)
            }
        }
    }
    if let Some(name) = &simple.word_or_name {
        collect_word_subs(&name.value, Some(parent), subs);
    }
}
