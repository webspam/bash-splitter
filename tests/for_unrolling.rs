//! Static `for`-loop unrolling: a loop over plain literals is unrolled into its
//! individual commands. Anything needing real expansion is left as a single body.

mod common;
use common::{args, command_text, in_loop, name, split, split_pipelines_with, variables};
use rstest::rstest;
use serde_json::Value;

/// Runs the binary with `--no-unroll` and returns the flattened stage list.
fn split_no_unroll(input: &str) -> Vec<Value> {
    split_pipelines_with(input, &["--no-unroll"])
        .into_iter()
        .flatten()
        .collect()
}

/// The reconstructed `command` text of each stage, in source order.
fn commands(cmds: &[Value]) -> Vec<&str> {
    cmds.iter().map(command_text).collect()
}

#[test]
fn plain_literals_are_unrolled() {
    let cmds = split("for f in a b c; do echo $f; done");
    assert_eq!(commands(&cmds), ["echo a", "echo b", "echo c"]);
    for cmd in &cmds {
        assert_eq!(name(cmd), Some("echo"));
    }
    assert_eq!(args(&cmds[0]), ["a"]);
}

#[test]
fn no_unroll_flag_keeps_single_body() {
    let cmds = split_no_unroll("for f in a b c; do echo $f; done");
    assert_eq!(commands(&cmds), ["echo $f"]);
    assert_eq!(variables(&cmds[0]), ["f"]);
}

#[test]
fn unrolled_stage_stays_flagged_in_loop() {
    let cmds = split("for f in a b; do echo $f; done");
    assert!(cmds.iter().all(in_loop));
}

#[test]
fn loop_variable_drops_out_of_variables() {
    // `$f` is substituted, so it is no longer a parameter the stage expands.
    let cmds = split("for f in a b; do echo $f; done");
    assert!(cmds.iter().all(|c| variables(c).is_empty()));
}

#[test]
fn other_variables_survive_unrolling() {
    let cmds = split("for f in a b; do grep \"$pat\" \"$f\"; done");
    assert_eq!(
        commands(&cmds),
        ["grep \"$pat\" \"a\"", "grep \"$pat\" \"b\""]
    );
    assert_eq!(variables(&cmds[0]), ["pat"]);
}

#[test]
fn braced_reference_in_double_quotes_is_substituted() {
    let cmds = split("for f in a b; do echo \"val=${f}!\"; done");
    assert_eq!(commands(&cmds), ["echo \"val=a!\"", "echo \"val=b!\""]);
}

#[test]
fn redirect_target_is_substituted() {
    let cmds = split("for f in a b; do echo hi > out/$f; done");
    let targets: Vec<&str> = cmds
        .iter()
        .map(|c| c["redirects"][0]["target"].as_str().unwrap())
        .collect();
    assert_eq!(targets, ["out/a", "out/b"]);
}

#[test]
fn body_without_the_variable_still_repeats() {
    let cmds = split("for f in a b c; do echo hi; done");
    assert_eq!(commands(&cmds), ["echo hi", "echo hi", "echo hi"]);
}

#[test]
fn multi_command_body_keeps_iteration_order() {
    let cmds = split("for f in a b; do echo $f; ls $f; done");
    assert_eq!(commands(&cmds), ["echo a", "ls a", "echo b", "ls b"]);
}

#[test]
fn nested_static_loops_compose() {
    let cmds = split("for a in x; do for b in y z; do touch $a$b; done; done");
    assert_eq!(commands(&cmds), ["touch xy", "touch xz"]);
}

#[test]
fn single_quoted_reference_is_left_literal() {
    // The loop unrolls, but `$f` inside single quotes does not expand.
    let cmds = split("for f in a b; do echo '$f'; done");
    assert_eq!(commands(&cmds), ["echo '$f'", "echo '$f'"]);
}

// ---- var referenced in a form we can't rewrite leaves the loop as one body ----

#[test]
fn parameter_operator_reference_blocks_unrolling() {
    // `${f%.txt}` is not a plain `$f`, so it would survive unsubstituted if the loop
    // were unrolled.
    let cmds = split("for f in a b; do echo ${f%.txt}; done");
    assert_eq!(commands(&cmds), ["echo ${f%.txt}"]);
}

#[test]
fn default_value_reference_blocks_unrolling() {
    let cmds = split("for f in a b; do echo ${f:-x}; done");
    assert_eq!(commands(&cmds), ["echo ${f:-x}"]);
}

#[test]
fn brace_formed_reference_blocks_unrolling() {
    let cmds = split("for f in a b; do echo {$,x}f; done");
    assert_eq!(commands(&cmds), ["echo {$,x}f"]);
}

#[test]
fn unexpandable_brace_word_mentioning_loop_var_blocks_unrolling() {
    let cmds = split("for f in a b; do echo {$,x}f-{1..2}; done");
    assert_eq!(commands(&cmds), ["echo {$,x}f-{1..2}"]);
}

#[test]
fn unrelated_unexpandable_brace_word_still_unrolls() {
    let cmds = split("for f in a b; do echo $f {1..2}; done");
    assert_eq!(commands(&cmds), ["echo a {1..2}", "echo b {1..2}"]);
}

#[test]
fn arithmetic_reference_blocks_unrolling() {
    let cmds = split("for f in 1 2; do echo $((f + 1)); done");
    assert_eq!(commands(&cmds), ["echo $((f + 1))"]);
}

#[test]
fn command_substitution_referencing_var_blocks_unrolling() {
    let cmds = split("for f in a b; do echo $(grep $f); done");
    // One body; `grep $f` surfaces as its own pipeline, still referencing `$f`.
    assert_eq!(commands(&cmds), ["echo $(grep $f)", "grep $f"]);
}

#[test]
fn command_substitution_without_var_still_unrolls() {
    let cmds = split("for f in a b; do echo $(date); done");
    assert_eq!(
        commands(&cmds),
        ["echo $(date)", "echo $(date)", "date", "date"]
    );
}

#[test]
fn substitution_in_extended_test_blocks_unrolling() {
    // The `cmd $f` substitution inside surfaces as its own pipeline, still naming `f`.
    let cmds = split("for f in a b; do [[ -n $(cmd $f) ]]; done");
    assert_eq!(commands(&cmds), ["cmd $f"]);
}

#[test]
fn substitution_in_case_value_blocks_unrolling() {
    let cmds = split("for f in a b; do case $(kind $f) in x) run x;; esac; done");
    assert_eq!(commands(&cmds), ["run x", "kind $f"]);
}

#[test]
fn unrewritable_reference_in_extended_test_still_unrolls() {
    // A `[[ ]]` word never becomes a stage, so its `${f%.txt}` never reaches output; unroll.
    let cmds = split("for f in a b; do [[ ${f%.txt} = y ]]; cat \"$f\"; done");
    assert_eq!(commands(&cmds), ["cat \"a\"", "cat \"b\""]);
}

#[test]
fn unrewritable_reference_in_case_selector_still_unrolls() {
    // The case value never becomes a stage, so its `${f%.txt}` never reaches output; unroll.
    let cmds = split("for f in a b; do case ${f%.txt} in x) echo $f;; esac; done");
    assert_eq!(commands(&cmds), ["echo a", "echo b"]);
}

#[test]
fn substitution_in_nested_for_values_blocks_unrolling() {
    let cmds = split("for f in a b; do for g in $(list $f); do echo $g; done; done");
    assert_eq!(commands(&cmds), ["echo $g", "list $f"]);
}

#[test]
fn substitution_in_compound_redirect_blocks_unrolling() {
    let cmds = split("for f in a b; do { ls; } > $(dir $f); done");
    assert_eq!(commands(&cmds), ["ls", "dir $f"]);
}

#[test]
fn arithmetic_command_on_loop_var_blocks_unrolling() {
    // `(( i+=1 ))` reassigns `i` without becoming a stage, so it must still block.
    let cmds = split("for i in 1 2; do ((i+=1)); echo $i; done");
    assert_eq!(commands(&cmds), ["echo $i"]);
}

#[test]
fn arithmetic_command_on_other_var_still_unrolls() {
    let cmds = split("for f in a b; do ((x++)); echo $f; done");
    assert_eq!(commands(&cmds), ["echo a", "echo b"]);
}

#[test]
fn dynamic_arithmetic_in_stage_blocks_unrolling() {
    let cmds = split("for f in a b; do echo $(($x + 1)); echo $f; done");
    assert_eq!(commands(&cmds), ["echo $(($x + 1))", "echo $f"]);
}

#[test]
fn dynamic_arithmetic_command_blocks_unrolling() {
    let cmds = split("for f in a b; do (($x)); echo $f; done");
    assert_eq!(commands(&cmds), ["echo $f"]);
}

#[test]
fn function_definition_blocks_unrolling() {
    // The body runs at call time, so an iteration's value must not be substituted in.
    let cmds = split("for f in a b; do g() { echo $f; }; done");
    assert_eq!(commands(&cmds), ["echo $f"]);
}

#[test]
fn shadowed_inner_loop_unrolls_after_outer_falls_back() {
    let cmds = split("for f in a b; do for f in x y; do echo $f; done; done");
    assert_eq!(commands(&cmds), ["echo x", "echo y"]);
}

// ---- non-static loops are left as a single body ----

#[test]
fn quoted_multiword_value_is_not_unrolled() {
    let cmds = split("for f in \"a b\" c; do echo $f; done");
    assert_eq!(commands(&cmds), ["echo $f"]);
}

#[test]
fn glob_value_is_not_unrolled() {
    let cmds = split("for f in *.txt; do echo $f; done");
    assert_eq!(commands(&cmds), ["echo $f"]);
}

#[test]
fn substitution_value_is_not_unrolled() {
    let cmds = split("for f in $(ls); do echo $f; done");
    // The body is walked once; `ls` surfaces as its own pipeline.
    assert_eq!(commands(&cmds), ["echo $f", "ls"]);
}

#[test]
fn reassigned_variable_blocks_unrolling() {
    let cmds = split("for f in a b; do f=x; echo $f; done");
    assert_eq!(commands(&cmds), ["f=x", "echo $f"]);
}

#[test]
fn break_blocks_unrolling() {
    // `break` exits after the first iteration; unrolling would be incorrect.
    let cmds = split("for f in a b c; do echo $f; break; done");
    assert_eq!(commands(&cmds), ["echo $f", "break"]);
}

#[test]
fn continue_blocks_unrolling() {
    let cmds = split("for f in a b; do continue; echo $f; done");
    assert_eq!(commands(&cmds), ["continue", "echo $f"]);
}

#[test]
fn return_blocks_unrolling() {
    let cmds = split("for f in a b; do echo $f; return; done");
    assert_eq!(commands(&cmds), ["echo $f", "return"]);
}

#[test]
fn exit_blocks_unrolling() {
    let cmds = split("for f in a b; do echo $f; exit; done");
    assert_eq!(commands(&cmds), ["echo $f", "exit"]);
}

#[test]
fn break_nested_in_condition_blocks_unrolling() {
    let cmds = split("for f in a b; do if grep -q x y; then break; fi; done");
    assert_eq!(commands(&cmds), ["grep -q x y", "break"]);
}

#[rstest]
#[case::quoted_builtin(r#""read" x"#)]
#[case::escaped_keyword(r"\break")]
#[case::brace_formed_keyword("{exit,}")]
#[case::builtin_wrapper("builtin read x")]
#[case::command_wrapper_with_option("command -p eval x")]
#[case::dynamic_name("$cmd x")]
fn resolved_loop_breaking_command_blocks_unrolling(#[case] command: &str) {
    let cmds = split(&format!("for f in a b; do {command}; echo $f; done"));
    assert!(
        commands(&cmds).contains(&"echo $f"),
        "{command:?}: {:?}",
        commands(&cmds)
    );
}

#[test]
fn wrapped_ordinary_command_still_unrolls() {
    let cmds = split("for f in a b; do command ls $f; done");
    assert_eq!(commands(&cmds), ["command ls a", "command ls b"]);
}

#[test]
fn break_as_argument_does_not_block_unrolling() {
    // `break` here is an argument to echo, not the flow-control keyword.
    let cmds = split("for f in a b; do echo break; done");
    assert_eq!(commands(&cmds), ["echo break", "echo break"]);
}

#[test]
fn value_after_bare_reference_blocks_unrolling() {
    // Splicing `x` after `$f` would produce `$fx`, a different variable.
    let cmds = split("for g in x y; do echo $f$g; done");
    assert_eq!(commands(&cmds), ["echo $f$g"]);
    assert_eq!(variables(&cmds[0]), ["f", "g"]);
}

#[test]
fn value_after_braced_reference_still_unrolls() {
    let cmds = split("for g in x y; do echo ${f}$g; done");
    assert_eq!(commands(&cmds), ["echo ${f}x", "echo ${f}y"]);
}

#[test]
fn value_after_positional_still_unrolls() {
    // `$1` expands one digit regardless of what follows; nothing to merge with.
    let cmds = split("for g in x y; do echo $1$g; done");
    assert_eq!(commands(&cmds), ["echo $1x", "echo $1y"]);
}

#[test]
fn value_with_trailing_dollar_is_not_unrolled() {
    // Splicing `a$` before `x` would produce `a$x`, a new expansion.
    let cmds = split("for g in a$; do echo ${g}x; done");
    assert_eq!(commands(&cmds), ["echo ${g}x"]);
}

#[test]
fn variable_valued_loop_is_not_unrolled() {
    let cmds = split("for f in $x $y; do echo $f; done");
    assert_eq!(commands(&cmds), ["echo $f"]);
}
