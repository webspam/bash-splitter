//! Runs the abhorrent fixture (tests/fixtures/horrible.sh) through the splitter to
//! verify the nasty constructs all survive: nested control flow, `$(...)` nested in
//! backticks, prefix assignments, subshell/brace-group pipeline stages, redirects,
//! process substitution, and arithmetic expansion.

mod common;
use common::{argv, command_text, piped_from_previous, pipes_to_next, split};

const HORRIBLE: &str = include_str!("fixtures/horrible.sh");

/// Finds the first split command whose argv begins with `name`.
fn by_name<'a>(cmds: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    cmds.iter()
        .find(|c| argv(c).first() == Some(&name))
        .unwrap_or_else(|| panic!("no command starting with {name:?}"))
}

/// Finds the first split command whose argv contains `token` (anywhere). Needed
/// when a prefix assignment, not the command name, occupies argv[0].
fn by_argv_token<'a>(cmds: &'a [serde_json::Value], token: &str) -> &'a serde_json::Value {
    cmds.iter()
        .find(|c| argv(c).contains(&token))
        .unwrap_or_else(|| panic!("no command with {token:?} in argv"))
}

#[test]
fn the_monster_parses_and_splits() {
    let cmds = split(HORRIBLE);
    // Dump the breakdown so `cargo test -- --nocapture` shows the awful pieces.
    for (i, c) in cmds.iter().enumerate() {
        eprintln!(
            "[{i}] from={} to={} argv={:?}\n     {}",
            u8::from(piped_from_previous(c)),
            u8::from(pipes_to_next(c)),
            argv(c),
            command_text(c).replace('\n', "\n     ")
        );
    }
    assert!(!cmds.is_empty(), "fixture should split into something");
}

#[test]
fn backticks_and_nested_substitution_do_not_break_parsing() {
    // grep's first arg embeds `whoami`-$(id -un); it must come through as one word.
    let cmds = split(HORRIBLE);
    let grep = by_argv_token(&cmds, "grep");
    assert!(
        argv(grep)
            .iter()
            .any(|a| a.contains("whoami") && a.contains("id -un")),
        "backtick + $() argument should be preserved verbatim: {:?}",
        argv(grep)
    );
}

#[test]
fn prefix_assignment_survives_in_a_pipeline() {
    let cmds = split(HORRIBLE);
    let grep = by_argv_token(&cmds, "grep");
    assert_eq!(
        argv(grep).first(),
        Some(&"LD_PRELOAD=./evil.so"),
        "prefix assignment should lead the grep argv: {:?}",
        argv(grep)
    );
}

#[test]
fn top_level_pipeline_flags_are_correct() {
    let cmds = split(HORRIBLE);
    // LD_PRELOAD=... grep ... | (sort -u) | wc -l
    let grep = by_argv_token(&cmds, "grep");
    assert!(!piped_from_previous(grep), "grep is the first stage");
    assert!(pipes_to_next(grep), "grep feeds the next stage");

    // The subshell stage `(sort -u)` must inherit the upstream pipe (the fix).
    let sort = by_name(&cmds, "sort");
    assert!(
        piped_from_previous(sort),
        "sort inside the subshell reads the upstream pipe"
    );

    let wc = by_name(&cmds, "wc");
    assert!(piped_from_previous(wc), "wc is the final stage");
    assert!(!pipes_to_next(wc), "nothing downstream of wc");
}

#[test]
fn brace_group_stage_inherits_pipe() {
    let cmds = split(HORRIBLE);
    // producer | { read -r first; echo "$first" | tr a-z A-Z; } | cat -n
    let read = by_name(&cmds, "read");
    assert!(
        piped_from_previous(read),
        "the brace group's first command reads producer's output"
    );
}

#[test]
fn control_flow_blocks_are_descended_into() {
    let cmds = split(HORRIBLE);
    assert!(
        cmds.iter().all(|c| !argv(c).is_empty()),
        "a compound command leaked as an opaque whole: {cmds:?}"
    );
    // Each lives only deep inside the monster, reachable only by descending.
    for name in ["uniq", "tar", "ping", "sleep", "nohup"] {
        assert!(
            cmds.iter().any(|c| argv(c).first() == Some(&name)),
            "{name:?} nested in the for loop should surface as its own command"
        );
    }
}
