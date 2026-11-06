use brush_parser::ast;

use crate::types::{HereDoc, Redirect, StageId, Sub};
use crate::word::{collect_brace_expanded_expansions, collect_word_expansions, collect_word_subs};

/// Structures one redirect for output.
pub(crate) fn redirect_of(redirect: &ast::IoRedirect) -> Redirect {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(fd, op, target) => {
            let kind = match target {
                T::Filename(_) => "file",
                T::Fd(_) | T::Duplicate(_) => "fd",
                T::ProcessSubstitution(_, _) => "process_sub",
            };
            Redirect {
                fd: *fd,
                op: op.to_string(),
                kind,
                target: Some(target.to_string()),
                heredoc: None,
            }
        }
        R::OutputAndError(target, append) => Redirect {
            fd: None,
            op: if *append { "&>>" } else { "&>" }.to_string(),
            kind: "file",
            target: Some(target.value.clone()),
            heredoc: None,
        },
        R::HereString(fd, word) => Redirect {
            fd: *fd,
            op: "<<<".to_string(),
            kind: "herestring",
            target: Some(word.value.clone()),
            heredoc: None,
        },
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
        },
    }
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
    vars: &mut Vec<String>,
    subs: &mut Vec<Sub>,
) {
    if let Some(w) = expanded_word(redirect) {
        if is_brace_expanded(redirect) {
            collect_brace_expanded_expansions(&w.value, parent, vars, subs);
        } else {
            collect_word_expansions(&w.value, parent, vars, subs);
        }
    }
    collect_process_sub(redirect, parent, subs);
}

fn is_brace_expanded(redirect: &ast::IoRedirect) -> bool {
    !matches!(
        redirect,
        ast::IoRedirect::HereString(..) | ast::IoRedirect::HereDocument(..)
    )
}

/// As [`collect_redirect_expansions`], substitutions only (for stage-less contexts).
pub(crate) fn collect_redirect_subs(
    redirect: &ast::IoRedirect,
    parent: Option<StageId>,
    subs: &mut Vec<Sub>,
) {
    if let Some(w) = expanded_word(redirect) {
        collect_word_subs(&w.value, parent, subs);
    }
    collect_process_sub(redirect, parent, subs);
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
