//! Tests for the structured `redirects`, `in_loop`, and `variables` fields. Each
//! feeds a bash string and asserts on the metadata attached to the split stages.

mod common;
use common::{dynamic_variables, in_loop, name, redirects, split, variables};
use serde_json::Value;

/// The one stage named `n`.
fn stage<'a>(cmds: &'a [Value], n: &str) -> &'a Value {
    cmds.iter()
        .find(|c| name(c) == Some(n))
        .unwrap_or_else(|| panic!("no stage named {n}: {cmds:?}"))
}

fn field<'a>(redirect: &'a Value, key: &str) -> Option<&'a str> {
    redirect.get(key).and_then(Value::as_str)
}

fn is_dynamic(redirect: &Value) -> bool {
    redirect
        .get("dynamic")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

// ---- redirects ----

#[test]
fn plain_file_redirect_is_captured() {
    let cmds = split("cmd arg > out.txt");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "op"), Some(">"));
    assert_eq!(field(r, "kind"), Some("file"));
    assert_eq!(field(r, "target"), Some("out.txt"));
    // No explicit fd was written, so the field is omitted.
    assert!(r.get("fd").is_none(), "default fd should be omitted: {r:?}");
}

#[test]
fn explicit_fd_and_append_are_captured() {
    let cmds = split("cmd 2>> err.log");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(r.get("fd").and_then(Value::as_i64), Some(2));
    assert_eq!(field(r, "op"), Some(">>"));
    assert_eq!(field(r, "target"), Some("err.log"));
}

#[test]
fn fd_duplication_is_captured() {
    let cmds = split("cmd 2>&1");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(r.get("fd").and_then(Value::as_i64), Some(2));
    assert_eq!(field(r, "kind"), Some("fd"));
    assert_eq!(field(r, "target"), Some("1"));
}

#[test]
fn multiple_redirects_keep_source_order() {
    let cmds = split("cmd > out.txt 2> err.txt");
    let rs = redirects(&cmds[0]);
    assert_eq!(rs.len(), 2);
    assert_eq!(field(&rs[0], "target"), Some("out.txt"));
    assert_eq!(field(&rs[1], "target"), Some("err.txt"));
}

#[test]
fn process_substitution_target_is_tagged_and_surfaces() {
    let cmds = split("cmd > >(tee log)");
    let r = &redirects(stage(&cmds, "cmd"))[0];
    assert_eq!(field(r, "kind"), Some("process_sub"));
    // The inner command still surfaces as its own pipeline.
    assert!(cmds.iter().any(|c| name(c) == Some("tee")));
}

// ---- dynamic redirect targets ----

#[test]
fn static_file_target_is_not_dynamic() {
    let cmds = split("cmd > out.txt");
    let r = &redirects(&cmds[0])[0];
    assert!(!is_dynamic(r));
    assert_eq!(field(r, "target"), Some("out.txt"));
}

#[test]
fn quoted_file_target_resolves_and_is_static() {
    let cmds = split("cmd > \"out dir/log.txt\"");
    let r = &redirects(&cmds[0])[0];
    assert!(!is_dynamic(r));
    assert_eq!(field(r, "target"), Some("out dir/log.txt"));
}

#[test]
fn parameter_file_target_is_dynamic() {
    let cmds = split("cmd > $out");
    let r = &redirects(&cmds[0])[0];
    assert!(is_dynamic(r));
    assert_eq!(field(r, "target"), Some("$out"));
}

#[test]
fn command_substitution_target_is_dynamic() {
    let cmds = split("cmd > $(date).log");
    let r = &redirects(stage(&cmds, "cmd"))[0];
    assert!(is_dynamic(r));
}

#[test]
fn glob_target_is_dynamic() {
    let cmds = split("cmd > logs/*.txt");
    let r = &redirects(&cmds[0])[0];
    assert!(is_dynamic(r));
}

#[test]
fn tilde_target_is_dynamic() {
    let cmds = split("cmd > ~/out.txt");
    let r = &redirects(&cmds[0])[0];
    assert!(is_dynamic(r));
}

#[test]
fn quoted_tilde_target_is_static() {
    let cmds = split("cmd > \"~/out.txt\"");
    let r = &redirects(&cmds[0])[0];
    assert!(!is_dynamic(r), "quotes suppress tilde expansion");
    assert_eq!(field(r, "target"), Some("~/out.txt"));
}

#[test]
fn fd_duplication_is_static() {
    let cmds = split("cmd 2>&1");
    let r = &redirects(&cmds[0])[0];
    assert!(!is_dynamic(r));
}

#[test]
fn stdout_duplication_to_variable_may_be_fd_or_file() {
    for input in ["cmd >&$fd", "cmd 1>&$fd"] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert_eq!(field(r, "kind"), Some("fd_or_file"), "{input}");
        assert_eq!(field(r, "target"), Some("$fd"), "{input}");
        assert!(is_dynamic(r), "{input}");
    }
}

#[test]
fn non_stdout_duplication_to_variable_is_dynamic_fd() {
    for input in ["cmd 2>&$fd", "cmd <&$fd", "cmd >&$fd-"] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert_eq!(field(r, "kind"), Some("fd"), "{input}");
        assert!(is_dynamic(r), "{input}");
    }
}

#[test]
fn explicit_stdout_duplication_to_word_is_a_file() {
    let cmds = split("cmd 1>&fail");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "kind"), Some("file"));
    assert_eq!(field(r, "target"), Some("fail"));
}

#[test]
fn non_stdout_duplication_to_word_is_ambiguous() {
    for input in ["cmd 2>&fail", "cmd 3>&fail", "cmd <&fail", "cmd >&fail-"] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert_eq!(field(r, "kind"), Some("ambiguous"), "{input}");
        assert!(!is_dynamic(r), "{input}");
    }
}

#[test]
fn quoted_dash_suffix_is_a_filename_not_a_move() {
    let cmds = split("cmd >&\"1-\"");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "kind"), Some("file"));
    assert_eq!(field(r, "target"), Some("1-"));
}

#[test]
fn quoted_digits_and_dash_are_descriptors() {
    for (input, target) in [
        ("cmd >&\"1\"", "1"),
        ("cmd >&\"-\"", "-"),
        ("cmd >&{1,}", "1"),
    ] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert_eq!(field(r, "kind"), Some("fd"), "{input}");
        assert_eq!(field(r, "target"), Some(target), "{input}");
    }
}

#[test]
fn brace_expansion_to_one_word_resolves() {
    for (input, kind) in [
        ("cmd > {a,}", "file"),
        ("cmd &> {a,}", "file"),
        ("cmd >&{a,}", "file"),
    ] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert_eq!(field(r, "kind"), Some(kind), "{input}");
        assert_eq!(field(r, "target"), Some("a"), "{input}");
        assert!(!is_dynamic(r), "{input}");
    }
}

#[test]
fn brace_expansion_to_many_words_is_ambiguous() {
    for input in [
        "cmd > {a,b}",
        "cmd > x{a,b}y",
        "cmd > {a,\"\"}",
        "cmd &> {a,b}",
        "cmd >&{a,b}",
        "cmd > {$x,a,b}",
    ] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert_eq!(field(r, "kind"), Some("ambiguous"), "{input}");
        assert!(!is_dynamic(r), "{input}");
    }
}

#[test]
fn move_onto_many_words_closes_the_descriptor() {
    let cmds = split("cmd >&{a,b}-");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "kind"), Some("fd"));
    assert_eq!(field(r, "target"), Some("-"));
    assert!(!is_dynamic(r));
}

#[test]
fn brace_expansion_with_one_static_word_and_a_variable_is_dynamic() {
    let cmds = split("cmd > {$x,a}");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "kind"), Some("file"));
    assert!(is_dynamic(r));
}

#[test]
fn here_string_is_not_brace_or_glob_expanded() {
    for (input, target) in [
        ("cat <<< {a,b}", "{a,b}"),
        ("cat <<< g*", "g*"),
        ("cat <<< a=~", "a=~"),
    ] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert_eq!(field(r, "target"), Some(target), "{input}");
        assert!(!is_dynamic(r), "{input}");
    }
}

#[test]
fn here_string_tilde_at_start_or_after_colon_is_dynamic() {
    for input in ["cat <<< ~", "cat <<< a:~"] {
        let cmds = split(input);
        let r = &redirects(&cmds[0])[0];
        assert!(is_dynamic(r), "{input}");
    }
}

#[test]
fn file_target_tilde_after_equals_is_dynamic() {
    let cmds = split("cmd > a=~");
    let r = &redirects(&cmds[0])[0];
    assert!(is_dynamic(r));
}

#[test]
fn fd_word_filename_is_a_static_file_not_a_descriptor() {
    let cmds = split("cmd >&fail");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "op"), Some(">&"));
    assert_eq!(field(r, "kind"), Some("file"));
    assert_eq!(field(r, "target"), Some("fail"));
    assert!(!is_dynamic(r));
}

#[test]
fn fd_close_dash_is_a_descriptor() {
    let cmds = split("cmd 2>&-");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "kind"), Some("fd"));
    assert_eq!(field(r, "target"), Some("-"));
    assert!(!is_dynamic(r));
}

#[test]
fn fd_move_is_a_descriptor() {
    let cmds = split("cmd 3>&1-");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "kind"), Some("fd"));
    assert_eq!(field(r, "target"), Some("1-"));
}

#[test]
fn process_substitution_target_is_dynamic() {
    let cmds = split("cmd > >(tee log)");
    let r = &redirects(stage(&cmds, "cmd"))[0];
    assert!(is_dynamic(r));
}

#[test]
fn here_string_with_variable_is_dynamic() {
    let cmds = split("cat <<< \"$foo\"");
    let r = &redirects(&cmds[0])[0];
    assert!(is_dynamic(r));
}

#[test]
fn literal_here_string_is_static() {
    let cmds = split("cat <<< hello");
    let r = &redirects(&cmds[0])[0];
    assert!(!is_dynamic(r));
    assert_eq!(field(r, "target"), Some("hello"));
}

#[test]
fn expanding_heredoc_with_variable_is_dynamic() {
    let cmds = split("cat <<EOF\nhello $name\nEOF\n");
    let r = &redirects(&cmds[0])[0];
    assert!(is_dynamic(r));
}

#[test]
fn expanding_heredoc_with_escape_or_arithmetic_is_dynamic() {
    for body in [
        "echo \\$(id)",
        "a\\\\b",
        "x \\`y\\`",
        "ab\\\ncd",
        "$((1+2))",
    ] {
        let cmds = split(&format!("cat <<EOF\n{body}\nEOF\n"));
        let r = &redirects(&cmds[0])[0];
        assert!(is_dynamic(r), "{body}");
    }
}

#[test]
fn expanding_heredoc_without_refs_is_static() {
    let cmds = split("cat <<EOF\nhello world\nEOF\n");
    let r = &redirects(&cmds[0])[0];
    assert!(!is_dynamic(r));
}

#[test]
fn quoted_heredoc_is_static() {
    let cmds = split("cat <<'EOF'\nliteral $x\nEOF\n");
    let r = &redirects(&cmds[0])[0];
    assert!(!is_dynamic(r));
}

// ---- heredocs ----

#[test]
fn unquoted_heredoc_captures_body_and_expands() {
    let cmds = split("cat <<EOF\nhello $name\nEOF\n");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "kind"), Some("heredoc"));
    let doc = &r["heredoc"];
    assert_eq!(field(doc, "delimiter"), Some("EOF"));
    assert_eq!(doc["expands"].as_bool(), Some(true));
    assert!(
        field(doc, "body").unwrap().contains("hello $name"),
        "body should hold the raw text: {doc:?}"
    );
    // An unquoted heredoc expands, so its variable is collected.
    assert!(variables(&cmds[0]).contains(&"name"));
}

#[test]
fn quoted_heredoc_does_not_expand() {
    let cmds = split("cat <<'EOF'\nliteral $x\nEOF\n");
    let doc = &redirects(&cmds[0])[0]["heredoc"];
    assert_eq!(doc["expands"].as_bool(), Some(false));
    // A quoted delimiter suppresses expansion: $x is literal, not a variable.
    assert!(
        variables(&cmds[0]).is_empty(),
        "quoted heredoc expands nothing: {:?}",
        variables(&cmds[0])
    );
}

#[test]
fn heredoc_single_quotes_are_literal_and_body_expands() {
    let cmds = split("cat <<EOF\n'$x' \"$y\"\nEOF\n");
    assert_eq!(
        variables(&cmds[0]),
        ["x", "y"],
        "quotes don't quote in a heredoc"
    );
}

#[test]
fn heredoc_unbalanced_quote_still_expands() {
    let cmds = split("cat <<EOF\nit's $x\nEOF\n");
    assert_eq!(variables(&cmds[0]), ["x"], "a stray apostrophe hid $x");
}

#[test]
fn unrolled_heredoc_substitutes_inside_single_quotes() {
    let cmds = split("for f in a; do cat <<EOF\n'$f'\nEOF\ndone");
    let doc = &redirects(&cmds[0])[0]["heredoc"];
    assert_eq!(field(doc, "body"), Some("'a'\n"), "$f should become a");
}

#[test]
fn here_string_is_tagged() {
    let cmds = split("cat <<< \"$foo\"");
    let r = &redirects(&cmds[0])[0];
    assert_eq!(field(r, "op"), Some("<<<"));
    assert_eq!(field(r, "kind"), Some("herestring"));
    assert!(variables(&cmds[0]).contains(&"foo"));
}

// ---- in_loop ----

#[test]
fn for_loop_body_is_flagged() {
    let cmds = split("for f in a b; do echo $f; done");
    assert!(in_loop(stage(&cmds, "echo")));
}

#[test]
fn while_condition_and_body_are_flagged() {
    let cmds = split("while read l; do echo $l; done");
    assert!(
        in_loop(stage(&cmds, "read")),
        "condition re-runs each iteration"
    );
    assert!(in_loop(stage(&cmds, "echo")));
}

#[test]
fn nested_loop_keeps_flag() {
    let cmds = split("for a in x; do for b in y; do touch $a$b; done; done");
    assert!(in_loop(stage(&cmds, "touch")));
}

#[test]
fn if_outside_loop_is_not_flagged() {
    let cmds = split("if true; then echo hi; fi");
    assert!(!in_loop(stage(&cmds, "echo")));
}

#[test]
fn command_outside_any_loop_is_not_flagged() {
    let cmds = split("echo hi");
    assert!(!in_loop(&cmds[0]));
}

// ---- variables ----

#[test]
fn variables_in_args_are_collected() {
    let cmds = split("grep \"$pattern\" \"$f\"");
    assert_eq!(variables(&cmds[0]), ["pattern", "f"]);
}

#[test]
fn variables_are_deduped_in_first_seen_order() {
    let cmds = split("echo $a $b $a");
    assert_eq!(variables(&cmds[0]), ["a", "b"]);
}

#[test]
fn bare_arithmetic_names_are_collected() {
    let cmds = split("echo $((count + 1))");
    assert_eq!(variables(&cmds[0]), ["count"]);
}

#[test]
fn parsed_arithmetic_is_not_dynamic() {
    let cmds = split("echo $((count + 1))");
    assert!(!dynamic_variables(&cmds[0]));
}

#[test]
fn arithmetic_assignment_targets_and_indices_are_collected() {
    let cmds = split("echo $((a = b + c[i], d++))");
    assert_eq!(variables(&cmds[0]), ["a", "b", "c", "i", "d"]);
}

#[test]
#[should_panic(expected = "known limitation: bare name beside an expansion is not collected")]
fn bare_name_beside_expansion_is_collected() {
    let cmds = split("echo $(($x + y))");
    assert!(
        dynamic_variables(&cmds[0]),
        "expansion flags variables dynamic"
    );
    assert_eq!(
        variables(&cmds[0]),
        ["x", "y"],
        "known limitation: bare name beside an expansion is not collected"
    );
}

#[test]
fn bare_subscript_names_are_collected() {
    let cmds = split("echo ${arr[i]}");
    assert_eq!(variables(&cmds[0]), ["arr", "i"]);
    assert!(!dynamic_variables(&cmds[0]));
}

#[test]
fn expansion_in_subscript_flags_variables_dynamic() {
    let cmds = split("echo ${arr[$i]}");
    assert_eq!(variables(&cmds[0]), ["arr", "i"]);
    assert!(dynamic_variables(&cmds[0]));
}

#[test]
fn empty_arithmetic_is_not_dynamic() {
    let cmds = split("echo $(( ))");
    assert!(variables(&cmds[0]).is_empty());
    assert!(!dynamic_variables(&cmds[0]));
}

#[test]
fn indexed_array_subscript_is_read_as_arithmetic() {
    let cmds = split("echo ${map[key]}");
    assert_eq!(variables(&cmds[0]), ["map", "key"]);
    assert!(!dynamic_variables(&cmds[0]));
}

#[test]
#[should_panic(expected = "known limitation: associative array key is reported as a variable")]
fn associative_array_key_is_not_a_variable() {
    let cmds = split("declare -A map; echo ${map[key]}");
    assert_eq!(
        variables(stage(&cmds, "echo")),
        ["map"],
        "known limitation: associative array key is reported as a variable"
    );
}

#[test]
fn quoted_subscript_flags_variables_dynamic() {
    let cmds = split("echo ${map[\"a b\"]}");
    assert_eq!(variables(&cmds[0]), ["map"]);
    assert!(dynamic_variables(&cmds[0]));
}

#[test]
#[should_panic(expected = "known limitation: assigned values are not tracked")]
fn assigned_value_is_substituted_into_arithmetic() {
    let cmds = split("x=5; echo $(( $x + y ))");
    let echo = stage(&cmds, "echo");
    assert!(
        !dynamic_variables(echo),
        "known limitation: assigned values are not tracked"
    );
    assert_eq!(variables(echo), ["x", "y"]);
}

#[test]
fn single_quoted_text_yields_no_variables() {
    let cmds = split("echo '$a'");
    assert!(variables(&cmds[0]).is_empty());
}

#[test]
fn assignment_rhs_variable_is_collected() {
    let cmds = split("FOO=$bar cmd");
    assert!(variables(stage(&cmds, "cmd")).contains(&"bar"));
}

#[test]
fn default_value_variable_is_collected() {
    let cmds = split("echo ${x:-$y}");
    let vars = variables(&cmds[0]);
    assert!(vars.contains(&"x"), "the parameter itself: {vars:?}");
    assert!(
        vars.contains(&"y"),
        "the default-value word expands too: {vars:?}"
    );
}

#[test]
fn brace_expansion_runs_before_parameter_expansion() {
    let cmds = split("echo $f{danger,}");
    assert_eq!(variables(&cmds[0]), ["fdanger", "f"]);
}

#[test]
fn brace_expansion_glues_every_alternative() {
    let cmds = split("echo $f{a,b}");
    assert_eq!(variables(&cmds[0]), ["fa", "fb"]);
}

#[test]
fn brace_expansion_can_form_a_parameter() {
    let cmds = split("echo {$,x}f");
    assert_eq!(variables(&cmds[0]), ["f"]);
}

#[test]
fn redirect_target_is_brace_expanded() {
    let cmds = split("echo hi > $f{danger,}");
    assert_eq!(variables(&cmds[0]), ["fdanger", "f"]);
}

#[test]
fn here_string_is_not_brace_expanded() {
    let cmds = split("cat <<< $f{a,b}");
    assert_eq!(variables(&cmds[0]), ["f"]);
}

#[test]
fn prefix_assignment_is_not_brace_expanded() {
    let cmds = split("FOO=$f{a,b} cmd");
    assert_eq!(variables(stage(&cmds, "cmd")), ["f"]);
}

// ---- backward compatibility ----

#[test]
fn plain_command_omits_new_fields() {
    let cmds = split("ls -la");
    let c = &cmds[0];
    assert!(c.get("redirects").is_none());
    assert!(c.get("in_loop").is_none());
    assert!(c.get("variables").is_none());
}
