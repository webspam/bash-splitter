mod common;
use common::{name, split, split_unsupported};

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
fn line_continuation_outside_ansi_c_quote_is_supported() {
    let cmds = split("echo $'a' \\\n b\n");
    assert_eq!(name(&cmds[0]), Some("echo"));
}
