//! Integration tests for the bash-splitter binary. Each test feeds a bash string
//! on stdin and asserts on the JSON array written to stdout. Input is passed as a
//! Rust string straight into the child's stdin, so there is no shell quoting layer.

mod common;
use common::{
    args, argv, assignments, name, piped_from_previous, pipes_to_next, split, split_pipelines,
};

#[test]
fn empty_input_is_empty_array() {
    assert!(split("").is_empty());
}

#[test]
fn single_command_has_no_pipe_flags() {
    let cmds = split("ls -la");
    assert_eq!(cmds.len(), 1);
    assert_eq!(argv(&cmds[0]), ["ls", "-la"]);
    assert!(!piped_from_previous(&cmds[0]));
    assert!(!pipes_to_next(&cmds[0]));
}

#[test]
fn plain_pipeline_sets_boundary_flags() {
    let cmds = split("a | b | c");
    assert_eq!(cmds.len(), 3);
    // first
    assert!(!piped_from_previous(&cmds[0]));
    assert!(pipes_to_next(&cmds[0]));
    // middle
    assert!(piped_from_previous(&cmds[1]));
    assert!(pipes_to_next(&cmds[1]));
    // last
    assert!(piped_from_previous(&cmds[2]));
    assert!(!pipes_to_next(&cmds[2]));
}

// Fix: grouped pipeline stages used to lose their pipe flags entirely.
#[test]
fn subshell_as_pipeline_stage_inherits_upstream_pipe() {
    let cmds = split("foo | (grep x)");
    assert_eq!(cmds.len(), 2);
    let grep = cmds
        .iter()
        .find(|c| argv(c).first() == Some(&"grep"))
        .unwrap();
    assert!(
        piped_from_previous(grep),
        "grep inside subshell should read the upstream pipe"
    );
}

#[test]
fn brace_group_as_pipeline_stage_inherits_upstream_pipe() {
    let cmds = split("foo | { a | b; }");
    // foo, a, b
    assert_eq!(cmds.len(), 3);
    let a = cmds.iter().find(|c| argv(c).first() == Some(&"a")).unwrap();
    let b = cmds.iter().find(|c| argv(c).first() == Some(&"b")).unwrap();
    // `a` is the group's first command: reads the outer pipe AND feeds `b`.
    assert!(
        piped_from_previous(a),
        "group's first command reads outer pipe"
    );
    assert!(
        pipes_to_next(a),
        "inner pipe a->b: `a` should feed downstream"
    );
    // `b` is the group's last command: reads from `a`, no outer downstream.
    assert!(
        piped_from_previous(b),
        "inner pipe a->b: `b` should read upstream"
    );
    assert!(!pipes_to_next(b), "no command downstream of the group");
}

#[test]
fn group_feeding_downstream_marks_last_inner_command() {
    let cmds = split("(echo a; echo b) | foo");
    // echo a, echo b, foo
    assert_eq!(cmds.len(), 3);
    // The group's last command feeds the downstream pipe.
    assert!(
        pipes_to_next(&cmds[1]),
        "last command in group feeds the pipe"
    );
    assert!(!pipes_to_next(&cmds[0]), "non-boundary command does not");
    assert!(piped_from_previous(&cmds[2]), "foo reads from the group");
}

// Fix: prefix assignments were invisible in argv.
#[test]
fn prefix_assignment_appears_before_command_name() {
    let cmds = split("LD_PRELOAD=evil cmd -a");
    assert_eq!(cmds.len(), 1);
    // The three parts are split out, not flattened into one list.
    assert_eq!(assignments(&cmds[0]), ["LD_PRELOAD=evil"]);
    assert_eq!(name(&cmds[0]), Some("cmd"));
    assert_eq!(args(&cmds[0]), ["-a"]);
}

#[test]
fn bare_assignment_is_surfaced() {
    let cmds = split("FOO=bar");
    assert_eq!(cmds.len(), 1);
    assert_eq!(assignments(&cmds[0]), ["FOO=bar"]);
    assert_eq!(name(&cmds[0]), None, "a bare assignment invokes nothing");
    assert!(args(&cmds[0]).is_empty());
}

#[test]
fn multiple_prefix_assignments_preserve_order() {
    let cmds = split("A=1 B=2 run --flag");
    assert_eq!(cmds.len(), 1);
    assert_eq!(argv(&cmds[0]), ["A=1", "B=2", "run", "--flag"]);
}

#[test]
fn redirects_excluded_from_argv() {
    let cmds = split("cmd arg > /dev/null");
    assert_eq!(cmds.len(), 1);
    assert_eq!(argv(&cmds[0]), ["cmd", "arg"]);
}

#[test]
fn and_or_list_yields_each_command() {
    let cmds = split("a && b || c");
    assert_eq!(cmds.len(), 3);
    assert_eq!(argv(&cmds[0]), ["a"]);
    assert_eq!(argv(&cmds[1]), ["b"]);
    assert_eq!(argv(&cmds[2]), ["c"]);
}

#[test]
fn sequence_yields_each_command() {
    let cmds = split("a; b; c");
    assert_eq!(cmds.len(), 3);
}

// The output groups by pipeline: one `|` chain is a single inner array.
#[test]
fn pipeline_is_one_group() {
    let pipelines = split_pipelines("a | b | c");
    assert_eq!(pipelines.len(), 1, "one pipeline: {pipelines:?}");
    assert_eq!(pipelines[0].len(), 3, "three stages in it");
}

// Sequenced / and-or commands are separate pipelines, each a singleton group.
#[test]
fn unpiped_commands_are_separate_groups() {
    let pipelines = split_pipelines("a; b && c");
    assert_eq!(pipelines.len(), 3, "three pipelines: {pipelines:?}");
    assert!(pipelines.iter().all(|p| p.len() == 1));
}

// A grouping that feeds a pipe splits at the group's internal sequence boundary:
// only its last command joins the downstream stage.
#[test]
fn group_feeding_pipe_splits_at_sequence_boundary() {
    let pipelines = split_pipelines("(echo a; echo b) | foo");
    assert_eq!(
        pipelines.len(),
        2,
        "echo a alone, then echo b | foo: {pipelines:?}"
    );
    assert_eq!(pipelines[0].len(), 1);
    assert_eq!(pipelines[1].len(), 2);
}

// A command substitution hides a command that still runs; it must surface so a rule
// can block it. `echo $(rm -rf /)` is not just a plain `echo`.
#[test]
fn command_substitution_surfaces_inner_command() {
    let cmds = split("echo $(rm -rf /)");
    assert!(cmds.iter().any(|c| name(c) == Some("echo")));
    let rm = cmds
        .iter()
        .find(|c| name(c) == Some("rm"))
        .expect("rm inside $() should surface");
    assert_eq!(args(rm), ["-rf", "/"]);
}

// Process substitutions run commands too (`<(...)`, `>(...)`).
#[test]
fn process_substitution_surfaces_inner_command() {
    let cmds = split("diff <(sort a) <(sort b)");
    assert_eq!(
        cmds.iter().filter(|c| name(c) == Some("sort")).count(),
        2,
        "both process substitutions should surface: {cmds:?}"
    );
}

// Substitutions nest; every level must surface.
#[test]
fn nested_substitutions_surface_every_level() {
    let cmds = split("echo $(foo $(bar))");
    assert!(cmds.iter().any(|c| name(c) == Some("foo")));
    assert!(cmds.iter().any(|c| name(c) == Some("bar")));
}

// A substitution feeding a pipeline stage must not corrupt that pipeline's grouping:
// the inner command lands in its own trailing pipeline.
#[test]
fn substitution_does_not_disturb_outer_pipeline() {
    let pipelines = split_pipelines("echo $(rm) | grep x");
    assert_eq!(
        pipelines[0].len(),
        2,
        "echo | grep stays intact: {pipelines:?}"
    );
    assert_eq!(pipelines[1].len(), 1, "rm is its own pipeline");
    assert_eq!(pipelines[1][0]["name"], "rm");
}

// `[[ ... ]]` runs no command, but a substitution in its words still executes.
#[test]
fn extended_test_surfaces_substitution() {
    let cmds = split("[[ -n $(rm -rf /) ]]");
    let rm = cmds
        .iter()
        .find(|c| name(c) == Some("rm"))
        .expect("rm inside [[ ]] should surface");
    assert_eq!(args(rm), ["-rf", "/"]);
}

// Both sides of a binary test are scanned.
#[test]
fn extended_test_scans_both_operands() {
    let cmds = split("[[ $(foo) == $(bar) ]]");
    assert!(cmds.iter().any(|c| name(c) == Some("foo")));
    assert!(cmds.iter().any(|c| name(c) == Some("bar")));
}

// CRLF from PowerShell on Windows must not leave a `\r` on the last token.
#[test]
fn crlf_line_endings_are_normalized() {
    let cmds = split("ls -la\r\n");
    assert_eq!(cmds.len(), 1);
    assert_eq!(argv(&cmds[0]), ["ls", "-la"]);
}

// CRLF between commands splits cleanly, with no stray `\r` words.
#[test]
fn crlf_between_commands_splits_cleanly() {
    let cmds = split("echo one\r\necho two\r\n");
    assert_eq!(cmds.len(), 2);
    assert_eq!(argv(&cmds[0]), ["echo", "one"]);
    assert_eq!(argv(&cmds[1]), ["echo", "two"]);
}
