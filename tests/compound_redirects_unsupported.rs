//! KNOWN FAILING: redirects on compound commands, `[[ ]]`, and function definitions
//! are currently rejected. Each test asserts the correct behaviour and is marked
//! `#[should_panic]` on the rejection.
//!
//! If support is added, update tests and restore `horrible.sh` back to `while < <(redirect)`.

mod common;
use common::{command_text, redirects, split, surfaces};
use serde_json::Value;

const REJECTED: &str = "unsupported: redirect on a compound command";

fn has_redirect(cmd: &Value, op: &str, target: &str) -> bool {
    redirects(cmd)
        .iter()
        .any(|r| r["op"] == op && r["target"] == target)
}

fn all_have_redirect(input: &str, op: &str, target: &str) {
    let cmds = split(input);
    assert!(!cmds.is_empty(), "{input:?} produced no stages");
    for cmd in &cmds {
        assert!(
            has_redirect(cmd, op, target),
            "{} is missing {op} {target}",
            command_text(cmd)
        );
    }
}

#[test]
#[should_panic(expected = "unsupported: redirect on a compound command")]
fn failing_subshell_redirect_attaches_to_inner_stages() {
    all_have_redirect("(cd a; ls) > x", ">", "x");
}

#[test]
#[should_panic(expected = "unsupported: redirect on a compound command")]
fn failing_brace_group_redirect_attaches_to_inner_stages() {
    let cmds = split("{ ls; } 2>err");
    assert!(
        redirects(&cmds[0])
            .iter()
            .any(|r| r["fd"] == 2 && r["op"] == ">" && r["target"] == "err"),
        "ls is missing 2>err"
    );
}

#[test]
#[should_panic(expected = "unsupported: redirect on a compound command")]
fn failing_loop_redirect_attaches_to_inner_stages() {
    all_have_redirect("while :; do echo hi; done > log", ">", "log");
}

#[test]
#[should_panic(expected = "unsupported: redirect on a compound command")]
fn failing_while_read_from_process_substitution() {
    let input = r#"while read -r line; do echo "$line"; done < <(cat "$d"/*.log 2>/dev/null)"#;
    assert!(surfaces(input, "cat"), "cat should surface");
    let cmds = split(input);
    let echo = cmds
        .iter()
        .find(|c| c["name"] == "echo")
        .expect("echo stage");
    assert!(
        redirects(echo).iter().any(|r| r["op"] == "<"),
        "echo is missing the loop's < redirect"
    );
}

#[test]
#[should_panic(expected = "unsupported: redirect on a compound command")]
fn failing_substitution_in_compound_redirect_surfaces() {
    assert!(surfaces("while :; do :; done > $(inner10)", "inner10"));
}

#[test]
#[should_panic(expected = "unsupported: redirect on a compound command")]
fn failing_substitution_in_compound_redirect_blocks_unrolling() {
    let cmds = split("for f in a b; do { ls; } > $(dir $f); done");
    let commands: Vec<&str> = cmds.iter().map(command_text).collect();
    assert_eq!(commands, ["ls", "dir $f"]);
}

#[test]
#[should_panic(expected = "unsupported: redirect on a compound command")]
fn failing_process_sub_while_with_redirect_surfaces() {
    assert!(surfaces(
        "cat <(while read l; do echo $l; done < file)",
        "echo"
    ));
}

#[test]
fn rejection_message_is_stable() {
    let stderr = common::split_unsupported("{ ls; } > x");
    assert!(stderr.contains(REJECTED), "{stderr}");
}
