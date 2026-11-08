//! Tests for parsing pitfalls: substitutions hidden in word expansions and
//! redirects. Each asserts the hidden command surfaces; sentinel names (`innerN`)
//! make misses easy to spot.

mod common;
use bash_splitter::{Error, split as lib_split, split_nested};
use common::{split_unsupported, surfaces};

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
fn hidden_command_surfaces(#[case] input: &str, #[case] inner: &str) {
    assert!(
        surfaces(input, inner),
        "{inner} should surface in {input:?}"
    );
}

#[rstest]
#[case("(cd a; ls) > x")]
#[case("{ ls; } 2>err")]
#[case("while :; do echo hi; done > log")]
#[case("until false; do :; done >> log")]
#[case("for f in a b; do echo $f; done > log")]
#[case("for ((i=0; i<3; i++)); do :; done > log")]
#[case("if true; then :; fi > log")]
#[case("case x in x) :;; esac > log")]
#[case("(( x )) > log")]
#[case("[[ -f a ]] > log")]
#[case("f() { :; } > log")]
#[case("echo a | { cat; } > log")]
#[case("while :; do :; done > $(inner10)")]
#[case("for f in a b; do { ls; } > $(dir $f); done")]
#[case("echo $( { ls; } > x )")]
#[case("cat <(while :; do :; done > log)")]
#[case("cat <(while read l; do echo $l; done < file)")]
#[case("while read l; do echo $l; done < <(cat file)")]
fn compound_redirect_is_rejected(#[case] input: &str) {
    let stderr = split_unsupported(input);
    assert!(
        stderr.contains("unsupported: redirect on a compound command"),
        "{input:?}: {stderr}"
    );
    assert!(
        matches!(split_nested(input, true), Err(Error::Unsupported(_))),
        "nested {input:?}"
    );
    assert!(
        split_nested(input, false).is_err(),
        "nested no-unroll {input:?}"
    );
    assert!(lib_split(input, false).is_err(), "flat no-unroll {input:?}");
}

#[rstest]
#[case("{ ls > x; }")]
#[case("(ls 2>err)")]
#[case("while :; do echo hi > log; done")]
#[case("f() { :; }")]
fn redirect_inside_compound_is_accepted(#[case] input: &str) {
    assert!(lib_split(input, true).is_ok(), "{input:?}");
}
