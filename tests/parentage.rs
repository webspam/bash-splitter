//! Tests for the `id`/`parent`/`children` linkage that ties a surfaced substitution
//! back to the command it was hidden in. Each asserts the edges in both directions.

mod common;
use common::{children, id, name, parent, split};
use serde_json::Value;

/// The one stage named `n`.
fn stage<'a>(cmds: &'a [Value], n: &str) -> &'a Value {
    cmds.iter()
        .find(|c| name(c) == Some(n))
        .unwrap_or_else(|| panic!("no stage named {n}: {cmds:?}"))
}

#[test]
fn top_level_command_has_no_parent_or_children() {
    let cmds = split("ls -la");
    assert_eq!(parent(&cmds[0]), None);
    assert!(children(&cmds[0]).is_empty());
}

// The classic case: the echo hidden in cd's argument points back at cd, and cd
// claims it as a child, so a caller knows cd is a complex command.
#[test]
fn substitution_links_to_and_from_its_parent() {
    let cmds = split(r#"cd "$(echo pie)""#);
    let cd = stage(&cmds, "cd");
    let echo = stage(&cmds, "echo");
    assert_eq!(parent(echo), Some(id(cd)));
    assert_eq!(children(cd), vec![id(echo)]);
    assert_eq!(parent(cd), None, "cd itself is top-level");
}

// Each level points at the one above it, so the whole chain reconstructs.
#[test]
fn nested_substitutions_chain_parents() {
    let cmds = split(r#"cd "$(echo $(date))""#);
    let cd = stage(&cmds, "cd");
    let echo = stage(&cmds, "echo");
    let date = stage(&cmds, "date");
    assert_eq!(parent(echo), Some(id(cd)));
    assert_eq!(parent(date), Some(id(echo)));
    assert_eq!(children(echo), vec![id(date)]);
}

// One command, two hidden substitutions: both children are claimed, in order.
#[test]
fn multiple_substitutions_are_all_children() {
    let cmds = split(r#"echo "$(whoami)" "$(hostname)""#);
    let echo = stage(&cmds, "echo");
    let whoami = stage(&cmds, "whoami");
    let hostname = stage(&cmds, "hostname");
    assert_eq!(children(echo), vec![id(whoami), id(hostname)]);
    assert_eq!(parent(whoami), Some(id(echo)));
    assert_eq!(parent(hostname), Some(id(echo)));
}

// A substitution hidden in a container that runs nothing (`[[ ]]`) has no parent
// stage to claim it, so it degrades to a parent-less top-level command.
#[test]
fn substitution_in_extended_test_has_no_parent() {
    let cmds = split("[[ -n $(whoami) ]]");
    let whoami = stage(&cmds, "whoami");
    assert_eq!(parent(whoami), None);
    assert!(
        cmds.iter().all(|c| children(c).is_empty()),
        "nothing claims the test's substitution as a child: {cmds:?}"
    );
}
