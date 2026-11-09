use brush_parser::ast;

use crate::resolve::{RedirectWord, resolve_here_string, resolve_redirect_word};
use crate::types::{HereDoc, Redirect, StageId, Sub, Variables};
use crate::word::{
    collect_brace_expanded_expansions, collect_heredoc_expansions, collect_word_expansions,
};

/// Structures one redirect for output.
pub(crate) fn redirect_of(redirect: &ast::IoRedirect) -> Redirect {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(fd, op, target) => {
            let (kind, text, dynamic) = match target {
                T::Filename(w) => file_target(&w.value),
                T::Duplicate(w) => duplicate_target(*fd, op, &w.value),
                T::Fd(_) => ("fd", target.to_string(), false),
                T::ProcessSubstitution(_, _) => ("process_sub", target.to_string(), true),
            };
            Redirect {
                fd: *fd,
                op: op.to_string(),
                kind,
                target: Some(text),
                heredoc: None,
                dynamic,
            }
        }
        R::OutputAndError(target, append) => {
            let (kind, text, dynamic) = file_target(&target.value);
            Redirect {
                fd: None,
                op: if *append { "&>>" } else { "&>" }.to_string(),
                kind,
                target: Some(text),
                heredoc: None,
                dynamic,
            }
        }
        R::HereString(fd, word) => {
            let (text, dynamic) = resolve_here_string(&word.value);
            Redirect {
                fd: *fd,
                op: "<<<".to_string(),
                kind: "herestring",
                target: Some(text),
                heredoc: None,
                dynamic,
            }
        }
        R::HereDocument(fd, doc) => Redirect {
            fd: *fd,
            op: if doc.remove_tabs { "<<-" } else { "<<" }.to_string(),
            kind: "heredoc",
            target: None,
            heredoc: Some(HereDoc {
                delimiter: doc.here_end.value.clone(),
                expands: doc.requires_expansion,
                body: doc.doc.value.clone(),
            }),
            dynamic: heredoc_is_dynamic(doc),
        },
    }
}

type Target = (&'static str, String, bool);

fn file_target(raw: &str) -> Target {
    match resolve_redirect_word(raw) {
        RedirectWord::Static(text) => ("file", text, false),
        RedirectWord::Dynamic => ("file", raw.to_owned(), true),
        RedirectWord::Ambiguous => ("ambiguous", raw.to_owned(), false),
    }
}

fn duplicate_target(fd: Option<i32>, op: &ast::IoFileRedirectKind, raw: &str) -> Target {
    if raw == CLOSE {
        return ("fd", CLOSE.to_owned(), false);
    }
    if let Some(source) = raw.strip_suffix('-') {
        return move_target(source, raw);
    }
    let duplicates_stdout =
        matches!(op, ast::IoFileRedirectKind::DuplicateOutput) && fd.unwrap_or(1) == 1;
    match resolve_redirect_word(raw) {
        RedirectWord::Static(text) if text == CLOSE || is_fd_number(&text) => ("fd", text, false),
        RedirectWord::Static(text) if duplicates_stdout => ("file", text, false),
        RedirectWord::Dynamic if duplicates_stdout => ("fd_or_file", raw.to_owned(), true),
        RedirectWord::Dynamic => ("fd", raw.to_owned(), true),
        RedirectWord::Static(_) | RedirectWord::Ambiguous => ("ambiguous", raw.to_owned(), false),
    }
}

fn move_target(source: &str, raw: &str) -> Target {
    match resolve_redirect_word(source) {
        RedirectWord::Static(text) if is_fd_number(&text) => ("fd", format!("{text}-"), false),
        RedirectWord::Static(text) if text == CLOSE => ("fd", CLOSE.to_owned(), false),
        RedirectWord::Ambiguous => ("fd", CLOSE.to_owned(), false),
        RedirectWord::Dynamic => ("fd", raw.to_owned(), true),
        RedirectWord::Static(_) => ("ambiguous", raw.to_owned(), false),
    }
}

const CLOSE: &str = "-";

fn is_fd_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// A heredoc body is dynamic when its delimiter is unquoted (so it expands) and the
/// body actually references a parameter or command, or cannot be parsed.
fn heredoc_is_dynamic(doc: &ast::IoHereDocument) -> bool {
    if !doc.requires_expansion {
        return false;
    }
    let mut vars = Variables::default();
    let mut subs = Vec::new();
    collect_heredoc_expansions(&doc.doc.value, None, &mut vars, &mut subs);
    vars.dynamic || !vars.names.is_empty() || !subs.is_empty()
}

/// The one word a redirect expands: its target, or an unquoted heredoc body.
#[allow(clippy::match_same_arms)]
pub(crate) fn expanded_word(redirect: &ast::IoRedirect) -> Option<&ast::Word> {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => Some(w),
        R::File(_, _, T::Fd(_) | T::ProcessSubstitution(_, _)) => None,
        R::OutputAndError(w, _) | R::HereString(_, w) => Some(w),
        // Only an unquoted delimiter expands the body.
        R::HereDocument(_, doc) if doc.requires_expansion => Some(&doc.doc),
        R::HereDocument(_, _) => None,
    }
}

/// As [`expanded_word`], mutably (the borrow rules force a second match).
#[allow(clippy::match_same_arms)]
pub(crate) fn expanded_word_mut(redirect: &mut ast::IoRedirect) -> Option<&mut ast::Word> {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => Some(w),
        R::File(_, _, T::Fd(_) | T::ProcessSubstitution(_, _)) => None,
        R::OutputAndError(w, _) | R::HereString(_, w) => Some(w),
        R::HereDocument(_, doc) if doc.requires_expansion => Some(&mut doc.doc),
        R::HereDocument(_, _) => None,
    }
}

/// Variables and substitutions in a redirect's expanded word (`> $(cmd)`, `<<< "$x"`,
/// an unquoted heredoc body), plus a process substitution's body (`> >(cmd)`).
pub(crate) fn collect_redirect_expansions(
    redirect: &ast::IoRedirect,
    parent: Option<StageId>,
    vars: &mut Variables,
    subs: &mut Vec<Sub>,
) {
    if let Some(w) = expanded_word(redirect) {
        match redirect {
            ast::IoRedirect::HereDocument(..) => {
                collect_heredoc_expansions(&w.value, parent, vars, subs)
            }
            ast::IoRedirect::HereString(..) => {
                collect_word_expansions(&w.value, parent, vars, subs)
            }
            _ => collect_brace_expanded_expansions(&w.value, parent, vars, subs),
        }
    }
    collect_process_sub(redirect, parent, subs);
}

/// As [`collect_redirect_expansions`], substitutions only (for stage-less contexts).
pub(crate) fn collect_redirect_subs(
    redirect: &ast::IoRedirect,
    parent: Option<StageId>,
    subs: &mut Vec<Sub>,
) {
    collect_redirect_expansions(redirect, parent, &mut Variables::default(), subs);
}

fn collect_process_sub(redirect: &ast::IoRedirect, parent: Option<StageId>, subs: &mut Vec<Sub>) {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    if let R::File(_, _, T::ProcessSubstitution(_, sub)) = redirect {
        subs.push(Sub {
            source: sub.list.to_string(),
            parent,
        });
    }
}

/// Redirects can also hang off a compound command or `[[ ]]` (`while ...; done > $(cmd)`).
/// The compound runs no command of its own, so its substitutions have no parent stage.
pub(crate) fn collect_redirect_list_subs(
    redirects: Option<&ast::RedirectList>,
    subs: &mut Vec<Sub>,
) {
    for redirect in redirects.iter().flat_map(|r| &r.0) {
        collect_redirect_subs(redirect, None, subs);
    }
}
