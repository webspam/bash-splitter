//! Substitution bodies are re-parsed as standalone programs. Every input here is
//! valid bash, so the inner command must surface and the split must not fail.

mod common;
use common::{split_error, surfaces};
use rstest::rstest;

#[rstest]
#[case::heredoc_body_with_lone_single_quote("x=\"$(cat <<'EOF'\nit's here\nEOF\n)\"", "cat")]
#[case::heredoc_with_nested_substitution("x=$(cat <<EOF\n$(date)\nEOF\n)", "date")]
#[case::paren_in_parameter_replacement("x=$(echo ${y/)/z})", "echo")]
#[case::quoted_paren_in_nested_substitution("x=$(echo \"$(printf ')')\")", "printf")]
#[case::extglob_inside_substitution("x=$(ls !(a|b))", "ls")]
#[case::ansi_c_quote_inside_substitution("x=$(echo $'it\\'s')", "echo")]
#[case::line_continuation_inside_substitution("x=$(echo a \\\n b)", "echo")]
#[case::trailing_semicolon("x=$(echo a;)", "echo")]
#[case::trailing_ampersand("x=$(echo a &)", "echo")]
#[case::brace_group("x=$( { echo a; } )", "echo")]
#[case::negated_pipeline("x=$(! true)", "true")]
#[case::timed_command("x=$(time ls)", "ls")]
#[case::multiline_body("x=$(\n  echo a\n  echo b\n)", "echo")]
#[case::comment_line_in_body("x=$(\n  # note\n  echo a\n)", "echo")]
#[case::backticks_in_double_quotes_with_escaped_quotes("x=\"`echo \\\"hi\\\"`\"", "echo")]
#[case::backticks_with_escaped_backslash("x=`printf '%s\\\\n' a`", "printf")]
#[case::process_sub_heredoc("cat <(cat <<EOF\nhi\nEOF\n)", "cat")]
#[case::process_sub_heredoc_remove_tabs("cat <(cat <<-EOF\n\thi\n\tEOF\n)", "cat")]
#[case::process_sub_for_loop("cat <(for f in a b; do echo $f; done)", "echo")]
#[case::process_sub_arithmetic_for("cat <(for ((i=0; i<3; i++)); do echo $i; done)", "echo")]
#[case::process_sub_extended_test("cat <(if [[ -n $x && $y == a* ]]; then echo y; fi)", "echo")]
#[case::process_sub_function("cat <(f() { echo y; }; f)", "echo")]
#[case::process_sub_pipeline_with_and_or("cat <(ls | sort && echo ok || echo no)", "sort")]
#[case::process_sub_nested_substitution("cat <(echo $(date))", "date")]
#[case::process_sub_subshell("cat <( (cd /tmp && ls) )", "ls")]
#[case::process_sub_background("cat <(sleep 1 & wait)", "sleep")]
fn valid_substitution_body_surfaces_inner_command(#[case] input: &str, #[case] inner: &str) {
    assert!(
        surfaces(input, inner),
        "{inner:?} should surface from {input:?}"
    );
}

#[test]
#[should_panic(expected = "unterminated here document")]
fn known_brush_limitation_close_paren_in_heredoc_inside_substitution_is_rejected() {
    let input = "x=$(cat <<EOF\n)\nEOF\n)";
    surfaces(input, "cat");
}

#[test]
#[should_panic(expected = "syntax error at end of input")]
fn known_brush_limitation_case_pattern_inside_process_substitution_is_rejected() {
    let input = "cat <(case $x in a) echo 1;; b|c) echo 2;; esac)";
    surfaces(input, "echo");
}

#[rstest]
#[case::empty_substitution("echo $()")]
#[case::whitespace_only_substitution("echo $(   )")]
#[case::newline_only_substitution("echo $(\n)")]
#[case::comment_only_substitution("echo $(\n# nothing\n)")]
fn empty_substitution_body_is_not_an_error(#[case] input: &str) {
    assert!(surfaces(input, "echo"));
}

#[rstest]
#[case::dangling_pipe("echo $(ls |)")]
#[case::unterminated_if("echo $(if true; then ls)")]
#[case::stray_fi("echo $(ls; fi)")]
fn malformed_substitution_body_fails_the_split(#[case] input: &str) {
    let stderr = split_error(input);
    assert!(stderr.contains("in substitution"), "stderr: {stderr}");
}
