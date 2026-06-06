//! Tests for parsing pitfalls: substitutions hidden in word expansions and
//! redirects. Each asserts the hidden command surfaces; sentinel names (`innerN`)
//! make misses easy to spot.

mod common;
use common::surfaces;

use rstest::rstest;

#[rstest]
// Command substitution in a parameter default: `${x:-$(cmd)}` runs cmd.
#[case("echo ${x:-$(inner1)}", "inner1")]
// `${x:=$(cmd)}` assigns and runs cmd.
#[case("echo ${x:=$(inner2)}", "inner2")]
// `${x:+$(cmd)}` alternative value runs cmd.
#[case("echo ${x:+$(inner3)}", "inner3")]
// Pattern substitution `${x/$(cmd)/y}` runs cmd.
#[case("echo ${x/$(inner4)/y}", "inner4")]
// Redirect target is a word that can hold a substitution: `> $(cmd)`.
#[case("cat > $(inner5)", "inner5")]
// Here-string `<<< "$(cmd)"` runs cmd.
#[case("cat <<< \"$(inner6)\"", "inner6")]
// Here-document body (unquoted delimiter) expands `$(cmd)`.
#[case("cat <<EOF\n$(inner7)\nEOF\n", "inner7")]
// Arithmetic expansion `$(( $(cmd) ))` runs cmd.
#[case("echo $((1 + $(inner8)))", "inner8")]
// Output process substitution as a redirect target: `> >(cmd)`.
#[case("echo hi > >(inner9)", "inner9")]
// Redirect hanging off a compound command: `while :; do :; done > $(cmd)`.
#[case("while :; do :; done > $(inner10)", "inner10")]
fn hidden_command_surfaces(#[case] input: &str, #[case] inner: &str) {
    assert!(surfaces(input, inner), "{inner} should surface in {input:?}");
}
