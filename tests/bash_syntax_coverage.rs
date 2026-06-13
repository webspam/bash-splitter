//! Bash constructs that run a command in a non-obvious place. The splitter exists
//! so an auto-deny pipeline sees every command that executes, so a missed one is a
//! security hole. Each case names its hidden command `innerN` and asserts it
//! surfaces; the quoted-heredoc case asserts inert data stays data.

mod common;
use common::{name, split, surfaces};

use rstest::rstest;

#[rstest]
// Unquoted heredoc body expands backticks, as it does `$(...)`.
#[case("cat <<EOF\n`inner1`\nEOF\n", "inner1")]
// Here-string expands backticks.
#[case("cat <<< `inner2`", "inner2")]
// Parameter default holds a backquoted command.
#[case("echo ${x:-`inner3`}", "inner3")]
// Redirect target word expands backticks.
#[case("cat > `inner4`", "inner4")]
// Arithmetic is command-substituted, so `(( ))` runs an embedded `$(cmd)`.
#[case("(( x = $(inner5) ))", "inner5")]
// C-style for: initializer expression.
#[case("for ((i=$(inner6); i<1; i++)); do :; done", "inner6")]
// C-style for: loop condition.
#[case("for ((i=0; i<$(inner7); i++)); do :; done", "inner7")]
// C-style for: updater expression.
#[case("for ((i=0; i<1; i+=$(inner8))); do :; done", "inner8")]
// A subscript is an arithmetic context: `${arr[$(cmd)]}` runs cmd.
#[case("echo ${arr[$(inner9)]}", "inner9")]
// Same for an assigned subscript.
#[case("arr[$(inner10)]=v", "inner10")]
// `arr=($(cmd))`: the lone element is a substitution.
#[case("arr=($(inner11))", "inner11")]
// `arr=(a $(cmd) b)`: a substitution among literals.
#[case("arr=(a $(inner12) b)", "inner12")]
// Declaration builtin with an array value.
#[case("declare -a arr=($(inner13))", "inner13")]
// Append assignment `x+=$(cmd)`.
#[case("x+=$(inner14)", "inner14")]
// Declaration builtins take assignment words: `export x=$(cmd)`.
#[case("export x=$(inner15)", "inner15")]
// `local x=$(cmd)` in a function body.
#[case("f() { local x=$(inner16); }", "inner16")]
// `readonly x=$(cmd)`.
#[case("readonly x=$(inner17)", "inner17")]
// Tab-stripped heredoc `<<-` still expands its body.
#[case("cat <<-EOF\n\t$(inner18)\nEOF\n", "inner18")]
// Coprocess body runs commands like any block.
#[case("coproc { echo $(inner21); }", "inner21")]
// A backgrounded substitution still runs.
#[case("echo $(inner22) &", "inner22")]
// `time` prefixes a pipeline; substitutions inside it still run.
#[case("time echo $(inner23)", "inner23")]
// Negated pipeline `! cmd` still runs the command.
#[case("! echo $(inner24)", "inner24")]
// Substitution nested inside a process substitution's body.
#[case("diff <(echo $(inner25)) x", "inner25")]
fn hidden_command_surfaces(#[case] input: &str, #[case] inner: &str) {
    assert!(
        surfaces(input, inner),
        "{inner} should surface in {input:?}"
    );
}

// Two heredocs on one command: both bodies expand.
#[test]
fn two_heredocs_both_expand() {
    let cmds = split("cat <<A <<B\n$(inner19)\nA\n$(inner20)\nB\n");
    assert!(cmds.iter().any(|c| name(c) == Some("inner19")));
    assert!(cmds.iter().any(|c| name(c) == Some("inner20")));
}

// A quoted delimiter suppresses expansion: the body must NOT surface as a command.
#[test]
fn quoted_heredoc_delimiter_does_not_expand() {
    assert!(!surfaces("cat <<'EOF'\n$(notrun)\nEOF\n", "notrun"));
}
