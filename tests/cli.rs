//! CLI contract: flag handling and exit codes, not the split logic itself.

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// Runs the binary with `args` and `input` on stdin, returning the full output.
fn run(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bash-splitter"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bash-splitter");

    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");

    child.wait_with_output().expect("wait for child")
}

// `--nested` is an alias for `-n`; both must produce identical output.
#[test]
fn long_nested_flag_matches_short() {
    let input = r#"cd "$(echo pie)""#;
    let short = run(&["-n"], input);
    let long = run(&["--nested"], input);
    assert!(short.status.success() && long.status.success());
    assert_eq!(short.stdout, long.stdout);
}

// A parse error exits non-zero (2) and writes nothing to stdout.
#[test]
fn parse_error_exits_nonzero_with_empty_stdout() {
    let output = run(&[], "echo $((");
    assert_eq!(output.status.code(), Some(2), "parse error should exit 2");
    assert!(output.stdout.is_empty(), "no stdout on parse error");
}
