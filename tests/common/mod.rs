//! Shared helpers for the integration tests. Each `tests/*.rs` file is its own
//! crate, so this module is pulled in via `mod common;` where needed.
#![allow(dead_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

/// Runs the binary with `input` on stdin and returns the raw output: an array of
/// pipelines, each an array of its stages in source order. Panics with the
/// captured stderr if the process fails or output isn't valid JSON.
pub fn split_pipelines(input: &str) -> Vec<Vec<Value>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bash-splitter"))
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

    let output = child.wait_with_output().expect("wait for child");
    assert!(
        output.status.success(),
        "non-zero exit for {input:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid JSON for {input:?}: {e}\nstdout: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// Runs the binary and flattens the grouped output to a single command list, in
/// source order. Each stage is augmented with `piped_from_previous`/`pipes_to_next`
/// re-derived from its index in its pipeline, so the position-agnostic tests assert
/// on connectivity without caring about the grouped shape.
pub fn split(input: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for pipeline in split_pipelines(input) {
        let last = pipeline.len().saturating_sub(1);
        for (i, mut stage) in pipeline.into_iter().enumerate() {
            let obj = stage.as_object_mut().expect("stage is a JSON object");
            obj.insert("piped_from_previous".into(), Value::Bool(i > 0));
            obj.insert("pipes_to_next".into(), Value::Bool(i < last));
            out.push(stage);
        }
    }
    out
}

/// A string array field, empty when the field is absent (the binary omits empties).
fn string_array<'a>(cmd: &'a Value, key: &str) -> Vec<&'a str> {
    cmd.get(key)
        .map(|v| {
            v.as_array()
                .expect("array")
                .iter()
                .map(|e| e.as_str().expect("string entry"))
                .collect()
        })
        .unwrap_or_default()
}

/// Test convenience: the full word list in source order (assignments, name, args),
/// reassembled from the structured fields so word-presence tests stay simple.
pub fn argv(cmd: &Value) -> Vec<&str> {
    let mut out = assignments(cmd);
    out.extend(name(cmd));
    out.extend(args(cmd));
    out
}

/// Leading env assignments of a split command.
pub fn assignments(cmd: &Value) -> Vec<&str> {
    string_array(cmd, "assignments")
}

/// The command's arguments.
pub fn args(cmd: &Value) -> Vec<&str> {
    string_array(cmd, "args")
}

/// The reconstructed `command` text of a split command.
pub fn command_text(cmd: &Value) -> &str {
    cmd["command"].as_str().expect("command is string")
}

/// The invoked command name, or `None` for a bare assignment.
pub fn name(cmd: &Value) -> Option<&str> {
    cmd.get("name").map(|v| v.as_str().expect("name is string"))
}

/// True if splitting `input` surfaces a command named `inner`. The coverage tests
/// hide a command behind some expansion and assert it still shows up.
pub fn surfaces(input: &str, inner: &str) -> bool {
    split(input).iter().any(|c| name(c) == Some(inner))
}

/// The command's redirects (empty when the field is absent).
pub fn redirects(cmd: &Value) -> &[Value] {
    cmd.get("redirects")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Whether the command is flagged as running inside a loop.
pub fn in_loop(cmd: &Value) -> bool {
    cmd.get("in_loop").and_then(Value::as_bool).unwrap_or(false)
}

/// The names of the parameters the command expands.
pub fn variables(cmd: &Value) -> Vec<&str> {
    string_array(cmd, "variables")
}

pub fn piped_from_previous(cmd: &Value) -> bool {
    cmd["piped_from_previous"].as_bool().unwrap_or(false)
}

pub fn pipes_to_next(cmd: &Value) -> bool {
    cmd["pipes_to_next"].as_bool().unwrap_or(false)
}
