mod common;
use std::fs;
use std::path::Path;

/// Golden snapshot testing for both flat and nested modes.
/// To add a new snapshot test:
/// 1. Create `tests/snapshots/NAME.sh` with the input bash command
/// 2. Run the binary to get actual output: `echo '...' | target/release/bash-splitter > NAME.flat.json`
/// 3. Do the same for nested mode: `echo '...' | target/release/bash-splitter -n > NAME.nested.json`
/// 4. Add a test function below that calls `test_snapshot("NAME")`
/// Snapshots verify that the output format is stable across changes.
fn test_snapshot(name: &str) {
    let snapshot_dir = Path::new("tests/snapshots");
    let input_path = snapshot_dir.join(format!("{}.sh", name));
    let flat_path = snapshot_dir.join(format!("{}.flat.json", name));
    let nested_path = snapshot_dir.join(format!("{}.nested.json", name));

    let input = fs::read_to_string(&input_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", input_path.display()));

    // Test flat mode
    if flat_path.exists() {
        let expected = fs::read_to_string(&flat_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", flat_path.display()));
        let actual = run_flat(&input);
        assert_eq!(actual, expected, "flat mode snapshot mismatch for {}", name);
    }

    // Test nested mode
    if nested_path.exists() {
        let expected = fs::read_to_string(&nested_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", nested_path.display()));
        let actual = run_nested(&input);
        assert_eq!(actual, expected, "nested mode snapshot mismatch for {}", name);
    }
}

fn run_flat(input: &str) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(env!("CARGO_BIN_EXE_bash-splitter"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bash-splitter");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write");

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "flat mode failed for input {input:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("valid utf8")
}

fn run_nested(input: &str) -> String {
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
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write");

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "nested mode failed for input {input:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("valid utf8")
}

// Test cases: each uses snapshots/NAME.{sh,flat.json,nested.json}

#[test]
fn snapshot_simple_command() {
    test_snapshot("simple_command");
}

#[test]
fn snapshot_pipeline() {
    test_snapshot("pipeline");
}

#[test]
fn snapshot_sequence() {
    test_snapshot("sequence");
}

#[test]
fn snapshot_single_substitution() {
    test_snapshot("single_substitution");
}

#[test]
fn snapshot_nested_substitutions() {
    test_snapshot("nested_substitutions");
}

#[test]
fn snapshot_multiple_substitutions() {
    test_snapshot("multiple_substitutions");
}

#[test]
fn snapshot_loop_with_redirect_and_vars() {
    test_snapshot("loop_with_redirect_and_vars");
}

#[test]
fn snapshot_loop_with_nested_substitution() {
    test_snapshot("loop_with_nested_substitution");
}

#[test]
fn snapshot_orphan_substitution() {
    test_snapshot("orphan_substitution");
}

#[test]
fn snapshot_complex_pipeline_with_subs() {
    test_snapshot("complex_pipeline_with_subs");
}
