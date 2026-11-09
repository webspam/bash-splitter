mod common;
use common::{args, name, redirects, split, split_unsupported};

#[test]
fn heredoc_end_tag_split_by_line_continuation_is_unsupported() {
    split_unsupported("cat <<EOF\nbody\nE\\\nOF\necho after\nEOF\n");
}

#[test]
fn heredoc_body_line_continued_onto_end_tag_is_unsupported() {
    split_unsupported("cat <<EOF\nbody\\\nEOF\nEOF\n");
}

#[test]
fn quoted_heredoc_with_line_continuation_is_supported() {
    let cmds = split("cat <<'EOF'\nbody\\\nEOF\necho after\n");
    assert!(cmds.iter().any(|c| name(c) == Some("echo")));
}

#[test]
fn heredoc_line_continuation_not_forming_end_tag_is_supported() {
    let cmds = split("cat <<EOF\nalpha\\\nbeta\nEOF\necho after\n");
    assert!(cmds.iter().any(|c| name(c) == Some("echo")));
}

#[test]
fn heredoc_escaped_backslash_before_newline_is_not_a_continuation() {
    let cmds = split("cat <<EOF\nbody\\\\\nEOF\necho after\n");
    assert!(cmds.iter().any(|c| name(c) == Some("echo")));
}

#[test]
fn ansi_c_quote_with_line_continuation_is_unsupported() {
    split_unsupported("printf '<%s>' $'a\\\nb'\n");
}

#[test]
fn ansi_c_quote_in_substitution_with_line_continuation_is_unsupported() {
    split_unsupported("echo $(printf '<%s>' $'a\\\nb')\n");
}

#[test]
fn tab_stripped_heredoc_end_tag_split_by_line_continuation_hiding_command_is_unsupported() {
    split("EOF() { :; }\ncat <<-EOF\n\tbody\n\tEOF\necho HIDDEN-RAN\n");
    split_unsupported("EOF() { :; }\ncat <<-EOF\n\tbody\n\tE\\\nOF\necho HIDDEN-RAN\nEOF\n");
}

#[test]
fn heredoc_end_tag_split_by_line_continuation_hiding_command_is_unsupported() {
    split("EOF() { :; }\ncat <<EOF\nbody\nEOF\necho HIDDEN-RAN\n");
    split_unsupported("EOF() { :; }\ncat <<EOF\nbody\nE\\\nOF\necho HIDDEN-RAN\nEOF\n");
}

#[test]
fn ansi_c_quote_with_line_continuation_after_escaped_newline_format_is_unsupported() {
    split("printf '<%s>\\n' $'ab'\n");
    split_unsupported("printf '<%s>\\n' $'a\\\nb'\n");
}

#[test]
fn ansi_c_quote_with_line_continuation_after_other_ansi_c_quote_is_unsupported() {
    split("printf '<%s>\\n' $'x'$'ab'\n");
    split_unsupported("printf '<%s>\\n' $'x'$'a\\\nb'\n");
}

#[test]
fn ansi_c_quote_in_nested_substitution_with_line_continuation_is_unsupported() {
    split("echo \"$(echo $(printf '<%s>' $'ab'))\"\n");
    split_unsupported("echo \"$(echo $(printf '<%s>' $'a\\\nb'))\"\n");
}

#[test]
fn ansi_c_quote_in_backticks_with_escaped_backslash_newline_matches_bash() {
    let cmds = split("echo `printf '<%s>' $'a\\\\\nb'`\n");
    let printf = cmds
        .iter()
        .find(|c| name(c) == Some("printf"))
        .expect("printf surfaces");
    assert_eq!(args(printf), ["<%s>", "a\\\nb"]);
}

#[test]
fn unquoted_heredoc_line_continuation_is_supported_and_dynamic() {
    let cmds = split("cat <<EOF\na\\\nb\nEOF\n");
    let cat = cmds
        .iter()
        .find(|c| name(c) == Some("cat"))
        .expect("cat surfaces");
    let heredoc = redirects(cat).first().expect("heredoc redirect");
    assert_eq!(
        heredoc.get("dynamic").and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn ansi_c_quote_in_parameter_default_substitution_with_line_continuation_is_unsupported() {
    split_unsupported("echo ${x:-$(printf '<%s>' $'a\\\nb')}\n");
}

#[test]
fn line_continuation_outside_ansi_c_quote_is_supported() {
    let cmds = split("echo $'a' \\\n b\n");
    assert_eq!(name(&cmds[0]), Some("echo"));
}
