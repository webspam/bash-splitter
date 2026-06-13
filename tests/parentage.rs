//! Tests for the nested structural relationships: substitutions embedded recursively in
//! their parent command's `substitutions` field (nested mode only).

mod common;
use common::name;
use serde_json::Value;

/// Flatten a nested structure into a list to find a stage by name.
fn flatten_nested(pipelines: &[Vec<Value>]) -> Vec<Value> {
    fn recurse(stage: &Value, out: &mut Vec<Value>) {
        out.push(stage.clone());
        if let Some(subs) = stage.get("substitutions").and_then(Value::as_array) {
            for pipeline in subs {
                if let Some(stages) = pipeline.as_array() {
                    for s in stages {
                        recurse(s, out);
                    }
                }
            }
        }
    }
    let mut result = Vec::new();
    for pipeline in pipelines {
        for stage in pipeline {
            recurse(stage, &mut result);
        }
    }
    result
}

/// The one stage named `n` in a flattened nested structure.
fn find_stage(pipelines: &[Vec<Value>], n: &str) -> Value {
    let cmds = flatten_nested(pipelines);
    cmds.iter()
        .find(|c| name(c) == Some(n))
        .cloned()
        .unwrap_or_else(|| panic!("no stage named {n}"))
}

/// Whether a stage is embedded in the `substitutions` field of its parent.
fn is_child_of(child: &Value, parent: &Value) -> bool {
    if let Some(subs) = parent.get("substitutions").and_then(Value::as_array) {
        for pipeline in subs {
            if let Some(stages) = pipeline.as_array()
                && stages.iter().any(|s| is_same_stage(s, child))
            {
                return true;
            }
        }
    }
    false
}

/// Whether two stages refer to the same command (by comparing command text and name).
fn is_same_stage(a: &Value, b: &Value) -> bool {
    a.get("command") == b.get("command") && a.get("name") == b.get("name")
}

/// Run the binary in nested mode and return the raw pipelines.
fn split_nested(input: &str) -> Vec<Vec<Value>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(env!("CARGO_BIN_EXE_bash-splitter"))
        .arg("-n")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bash-splitter -n");

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

#[test]
fn top_level_command_has_no_substitutions() {
    let pipelines = split_nested("ls -la");
    let cmds = flatten_nested(&pipelines);
    let ls = &cmds[0];
    assert_eq!(
        ls.get("substitutions"),
        None,
        "top-level command has no substitutions"
    );
}

// The classic case: echo is embedded in cd's substitutions, indicating cd is the parent.
#[test]
fn substitution_links_to_and_from_its_parent() {
    let pipelines = split_nested(r#"cd "$(echo pie)""#);
    let cd = find_stage(&pipelines, "cd");
    let echo = find_stage(&pipelines, "echo");
    assert!(
        is_child_of(&echo, &cd),
        "echo should be embedded in cd's substitutions"
    );
    assert_eq!(cd.get("name").and_then(Value::as_str), Some("cd"));
}

// Each level of substitution nests one deeper.
#[test]
fn nested_substitutions_chain_parents() {
    let pipelines = split_nested(r#"cd "$(echo $(date))""#);
    let cd = find_stage(&pipelines, "cd");
    let echo = find_stage(&pipelines, "echo");
    let date = find_stage(&pipelines, "date");
    assert!(
        is_child_of(&echo, &cd),
        "echo should be embedded in cd's substitutions"
    );
    assert!(
        is_child_of(&date, &echo),
        "date should be embedded in echo's substitutions"
    );
}

// One command, two hidden substitutions: both embedded in the parent.
#[test]
fn multiple_substitutions_are_all_children() {
    let pipelines = split_nested(r#"echo "$(whoami)" "$(hostname)""#);
    let echo = find_stage(&pipelines, "echo");
    let whoami = find_stage(&pipelines, "whoami");
    let hostname = find_stage(&pipelines, "hostname");
    assert!(
        is_child_of(&whoami, &echo),
        "whoami should be embedded in echo's substitutions"
    );
    assert!(
        is_child_of(&hostname, &echo),
        "hostname should be embedded in echo's substitutions"
    );
}

// A substitution hidden in a container that runs nothing (`[[ ]]`) has no parent
// stage to claim it, so it appears as a top-level pipeline.
#[test]
fn substitution_in_extended_test_has_no_parent() {
    let pipelines = split_nested("[[ -n $(whoami) ]]");
    let whoami = find_stage(&pipelines, "whoami");
    assert!(
        pipelines
            .iter()
            .flat_map(|p| p.iter())
            .any(|cmd| is_same_stage(cmd, &whoami)),
        "whoami should appear as a top-level command (not embedded)"
    );
}
