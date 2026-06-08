use brush_parser::ast;

use crate::types::{HereDoc, Redirect, Sub};
use crate::word::{collect_word_subs, collect_word_vars};

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

/// Variables in a redirect's expanded target, or an unquoted heredoc body.
#[allow(clippy::match_same_arms)]
pub(crate) fn collect_redirect_vars(redirect: &ast::IoRedirect, vars: &mut Vec<String>) {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => collect_word_vars(&w.value, vars),
        R::File(_, _, T::Fd(_) | T::ProcessSubstitution(_, _)) => {}
        R::OutputAndError(w, _) | R::HereString(_, w) => collect_word_vars(&w.value, vars),
        R::HereDocument(_, doc) if doc.requires_expansion => collect_word_vars(&doc.doc.value, vars),
        R::HereDocument(_, _) => {}
    }
}

/// Redirect targets are expanded, so a substitution in one runs (`> $(cmd)`,
/// `<<< "$(cmd)"`, `> >(cmd)`, or an unquoted heredoc body).
#[allow(clippy::match_same_arms)]
pub(crate) fn collect_redirect_subs(redirect: &ast::IoRedirect, parent: Option<usize>, subs: &mut Vec<Sub>) {
    use ast::IoFileRedirectTarget as T;
    use ast::IoRedirect as R;
    match redirect {
        R::File(_, _, T::Filename(w) | T::Duplicate(w)) => collect_word_subs(&w.value, parent, subs),
        R::File(_, _, T::ProcessSubstitution(_, sub)) => subs.push(Sub {
            source: sub.list.to_string(),
            parent,
        }),
        R::File(_, _, T::Fd(_)) => {}
        R::OutputAndError(w, _) | R::HereString(_, w) => collect_word_subs(&w.value, parent, subs),
        // Only an unquoted delimiter expands the body.
        R::HereDocument(_, doc) if doc.requires_expansion => {
            collect_word_subs(&doc.doc.value, parent, subs)
        }
        R::HereDocument(_, _) => {}
    }
}

/// Redirects can also hang off a compound command or `[[ ]]` (`while ...; done > $(cmd)`).
/// The compound runs no command of its own, so its substitutions have no parent stage.
pub(crate) fn collect_redirect_list_subs(redirects: Option<&ast::RedirectList>, subs: &mut Vec<Sub>) {
    for redirect in redirects.iter().flat_map(|r| &r.0) {
        collect_redirect_subs(redirect, None, subs);
    }
}
