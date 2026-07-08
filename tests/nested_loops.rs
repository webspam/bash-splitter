//! Nested-mode loop objects: a loop is an object holding its contents; a
//! pipeline is an array. A static `for` carries both the body as written and
//! the unrolled per-iteration commands.

mod common;
use common::split_json;
use serde_json::Value;

/// Top-level nested items for `input`: pipeline arrays and loop objects.
fn nested(input: &str) -> Vec<Value> {
    nested_with(input, &["-n"])
}

fn nested_with(input: &str, args: &[&str]) -> Vec<Value> {
    split_json(input, args)
        .as_array()
        .expect("top level is an array")
        .clone()
}

#[test]
fn static_for_has_both_representations() {
    let items = nested("for f in a b; do echo $f; done");
    let [l] = &items[..] else {
        panic!("expected one loop object, got {items:?}")
    };
    assert_eq!(l["kind"], "for");
    assert_eq!(l["variable"], "f");
    assert_eq!(l["body"][0][0]["command"], "echo $f");
    assert_eq!(l["body"][0][0]["variables"][0], "f");

    let unrolled = l["unrolled"].as_array().expect("unrolled");
    assert_eq!(unrolled.len(), 2);
    assert_eq!(unrolled[0]["value"], "a");
    assert_eq!(unrolled[0]["commands"][0][0]["command"], "echo a");
    assert_eq!(unrolled[1]["value"], "b");
    assert_eq!(unrolled[1]["commands"][0][0]["command"], "echo b");
}

#[test]
fn no_unroll_flag_keeps_body_only() {
    let items = nested_with("for f in a b; do echo $f; done", &["-n", "--no-unroll"]);
    assert_eq!(items[0]["body"][0][0]["command"], "echo $f");
    assert!(items[0].get("unrolled").is_none());
}

#[test]
fn non_static_for_has_no_unrolled() {
    let items = nested("for f in $x; do echo $f; done");
    assert_eq!(items[0]["body"][0][0]["command"], "echo $f");
    assert!(items[0].get("unrolled").is_none());
}

#[test]
fn while_has_condition_and_body() {
    let items = nested("while read -r x; do echo hi; done");
    let l = &items[0];
    assert_eq!(l["kind"], "while");
    assert!(l.get("variable").is_none());
    assert_eq!(l["condition"][0][0]["command"], "read -r x");
    assert_eq!(l["body"][0][0]["command"], "echo hi");
}

#[test]
fn until_is_tagged() {
    let items = nested("until test -f flag; do sleep 1; done");
    assert_eq!(items[0]["kind"], "until");
    assert_eq!(items[0]["condition"][0][0]["command"], "test -f flag");
}

#[test]
fn arithmetic_for_is_a_for_object() {
    let items = nested("for ((i=0; i<3; i++)); do date; done");
    let l = &items[0];
    assert_eq!(l["kind"], "for");
    assert!(l.get("variable").is_none());
    assert!(l.get("unrolled").is_none());
    assert_eq!(l["body"][0][0]["command"], "date");
}

#[test]
fn loop_interleaves_with_pipelines_in_source_order() {
    let items = nested("echo a; for f in x; do ls; done; echo b");
    assert_eq!(items[0][0]["command"], "echo a");
    assert_eq!(items[1]["kind"], "for");
    assert_eq!(items[2][0]["command"], "echo b");
}

#[test]
fn body_pipelines_group_by_pipes() {
    let items = nested("while :; do foo | bar; baz; done");
    let body = items[0]["body"].as_array().expect("body");
    assert_eq!(body.len(), 2);
    assert_eq!(body[0][0]["command"], "foo");
    assert_eq!(body[0][1]["command"], "bar");
    assert_eq!(body[1][0]["command"], "baz");
}

// A loop cannot sit inside a pipeline array, so a pipe into (or out of) a loop
// splits the pipeline at the loop boundary.
#[test]
fn pipe_into_loop_splits_at_the_loop() {
    let items = nested("seq 3 | while read -r x; do echo hi; done");
    assert_eq!(items[0][0]["command"], "seq 3");
    assert_eq!(items[1]["kind"], "while");
}

#[test]
fn loop_in_substitution_embeds_under_parent() {
    let items = nested("echo $(for g in x; do cat $g; done)");
    let sub = &items[0][0]["substitutions"][0];
    assert_eq!(sub["kind"], "for");
    assert_eq!(sub["body"][0][0]["command"], "cat $g");
    assert_eq!(sub["unrolled"][0]["commands"][0][0]["command"], "cat x");
}

#[test]
fn substitution_in_body_embeds_in_both_representations() {
    let items = nested("for f in a; do echo $(date); done");
    let l = &items[0];
    assert_eq!(l["body"][0][0]["substitutions"][0][0]["command"], "date");
    let iteration = &l["unrolled"][0]["commands"][0][0];
    assert_eq!(iteration["substitutions"][0][0]["command"], "date");
}

#[test]
fn nested_loops_nest_in_both_representations() {
    let items = nested("for f in a; do for g in x; do echo $f-$g; done; done");
    let outer = &items[0];
    // In the outer body, the inner loop unrolls but `$f` stays.
    let inner = &outer["body"][0];
    assert_eq!(inner["kind"], "for");
    assert_eq!(inner["body"][0][0]["command"], "echo $f-$g");
    assert_eq!(
        inner["unrolled"][0]["commands"][0][0]["command"],
        "echo $f-x"
    );
    // In the outer iteration, `$f` is bound too.
    let inner = &outer["unrolled"][0]["commands"][0];
    assert_eq!(inner["body"][0][0]["command"], "echo a-$g");
    assert_eq!(
        inner["unrolled"][0]["commands"][0][0]["command"],
        "echo a-x"
    );
}
