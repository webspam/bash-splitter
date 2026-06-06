//! Tests for parsing pitfalls: substitutions hidden in word expansions and
//! redirects. Each asserts the hidden command surfaces; sentinel names (`innerN`)
//! make misses easy to spot.

mod common;
use common::{name, split};

fn surfaces(input: &str, inner: &str) -> bool {
    split(input).iter().any(|c| name(c) == Some(inner))
}

// Command substitution in a parameter default: `${x:-$(cmd)}` runs cmd.
#[test]
fn param_default_value() {
    assert!(surfaces("echo ${x:-$(inner1)}", "inner1"));
}

// `${x:=$(cmd)}` assigns and runs cmd.
#[test]
fn param_assign_default() {
    assert!(surfaces("echo ${x:=$(inner2)}", "inner2"));
}

// `${x:+$(cmd)}` alternative value runs cmd.
#[test]
fn param_alternative_value() {
    assert!(surfaces("echo ${x:+$(inner3)}", "inner3"));
}

// Pattern substitution `${x/$(cmd)/y}` runs cmd.
#[test]
fn param_pattern_substitution() {
    assert!(surfaces("echo ${x/$(inner4)/y}", "inner4"));
}

// Redirect target is a word that can hold a substitution: `> $(cmd)`.
#[test]
fn redirect_target_substitution() {
    assert!(surfaces("cat > $(inner5)", "inner5"));
}

// Here-string `<<< "$(cmd)"` runs cmd.
#[test]
fn here_string_substitution() {
    assert!(surfaces("cat <<< \"$(inner6)\"", "inner6"));
}

// Here-document body (unquoted delimiter) expands `$(cmd)`.
#[test]
fn heredoc_body_substitution() {
    assert!(surfaces("cat <<EOF\n$(inner7)\nEOF\n", "inner7"));
}

// Arithmetic expansion `$(( $(cmd) ))` runs cmd.
#[test]
fn arithmetic_substitution() {
    assert!(surfaces("echo $((1 + $(inner8)))", "inner8"));
}

// Output process substitution as a redirect target: `> >(cmd)`.
#[test]
fn redirect_process_substitution() {
    assert!(surfaces("echo hi > >(inner9)", "inner9"));
}

// Redirect hanging off a compound command: `while :; do :; done > $(cmd)`.
#[test]
fn compound_redirect_substitution() {
    assert!(surfaces("while :; do :; done > $(inner10)", "inner10"));
}
