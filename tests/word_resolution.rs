//! `name` and `args` carry what bash would actually pass (brace expansion, quote
//! removal); `raw` keeps the source words, `dynamic_*` flag unresolvable ones.

mod common;
use common::{args, name, split};

use rstest::rstest;
use serde_json::{Value, json};

fn only(input: &str) -> Value {
    let mut cmds = split(input);
    assert_eq!(cmds.len(), 1, "expected one command for {input:?}");
    cmds.remove(0)
}

fn words(cmd: &Value) -> Vec<&str> {
    name(cmd).into_iter().chain(args(cmd)).collect()
}

fn dynamic_name(cmd: &Value) -> bool {
    cmd.get("dynamic_name")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn dynamic_args(cmd: &Value) -> Vec<u64> {
    cmd.get("dynamic_args")
        .map(|v| {
            v.as_array()
                .expect("array")
                .iter()
                .map(|i| i.as_u64().expect("index"))
                .collect()
        })
        .unwrap_or_default()
}

#[rstest]
#[case::double_quoted_prefix(r#""-"echo"#, &["-echo"])]
#[case::empty_double_quotes(r#"""-echo"#, &["-echo"])]
#[case::escaped_dash(r"\-echo", &["-echo"])]
#[case::ansi_c_plain(r"$'-echo'", &["-echo"])]
#[case::brace_trailing_empty("{-echo,}", &["-echo"])]
#[case::brace_leading_empty("{,-echo} hi", &["-echo", "hi"])]
#[case::brace_splits_name("{echo,hi} there", &["echo", "hi", "there"])]
#[case::mixed_quotes(r#"e"c"'h'o hi"#, &["echo", "hi"])]
#[case::ansi_c_hex(r"$'\x2decho'", &["-echo"])]
#[case::ansi_c_octal(r"$'\055echo'", &["-echo"])]
#[case::ansi_c_unicode(concat!(r"$'\", "u002decho'"), &["-echo"])]
#[case::ansi_c_wide_unicode(r"x $'A\U0001F600'", &["x", "A\u{1F600}"])]
#[case::ansi_c_control(r"x $'\cA'y", &["x", "\u{1}y"])]
#[case::ansi_c_escaped_quote(r"x $'it\'s'", &["x", "it's"])]
#[case::ansi_c_unknown_escape(r"x $'a\qb'", &["x", r"a\qb"])]
#[case::ansi_c_bare_hex(r"x $'\x'", &["x", r"\x"])]
#[case::brace_infix("x a{b,c}d", &["x", "abd", "acd"])]
#[case::brace_nested("x a{b,{c,d}}e", &["x", "abe", "ace", "ade"])]
#[case::brace_empty_members("x {,}y", &["x", "y", "y"])]
#[case::brace_quoted_member(r#"x {"a b",c}"#, &["x", "a b", "c"])]
#[case::brace_escaped_comma(r"x {a\,b,c}", &["x", "a,b", "c"])]
#[case::brace_single_quoted("x '{a,b}'", &["x", "{a,b}"])]
#[case::brace_double_quoted(r#"x "{a,b}""#, &["x", "{a,b}"])]
#[case::brace_escaped(r"x \{a,b}", &["x", "{a,b}"])]
#[case::brace_unclosed("x {a,b", &["x", "{a,b"])]
#[case::brace_single_member("x {a}", &["x", "{a}"])]
#[case::dq_escaped_dollar(r#"x "a\$b""#, &["x", "a$b"])]
#[case::dq_backslash_kept(r#"x "a\b""#, &["x", r"a\b"])]
#[case::dq_escaped_backslash(r#"x "a\\b""#, &["x", r"a\b"])]
#[case::dq_escaped_quote(r#"x "a\"b""#, &["x", "a\"b"])]
#[case::sq_backslash_kept(r"x 'a\b'", &["x", r"a\b"])]
#[case::escaped_space(r"x a\ b", &["x", "a b"])]
#[case::quoted_empty_kept(r#"x """#, &["x", ""])]
#[case::suffix_assignment("x a=b", &["x", "a=b"])]
#[case::quoted_glob("x '*'", &["x", "*"])]
#[case::escaped_glob(r"x \*", &["x", "*"])]
#[case::quoted_tilde(r#"x "~""#, &["x", "~"])]
#[case::infix_tilde("x a~b", &["x", "a~b"])]
#[case::escaped_dollar_brace(r"x \${a,b}", &["x", "$a", "$b"])]
#[case::ansi_c_comma_in_brace(r"x {$'a,b',c}", &["x", "a,b", "c"])]
#[case::ansi_c_comma_not_separator(r"x {a$'\x2c'b}", &["x", "{a,b}"])]
#[case::quoted_close_brace(r#"x {a,"b}"}"#, &["x", "a", "b}"])]
#[case::escaped_close_brace(r"x {a,b\}}", &["x", "a", "b}"])]
#[case::trailing_close_brace("x {a,b}}", &["x", "a}", "b}"])]
#[case::sequence_text_member("x {a..c,d}", &["x", "a..c", "d"])]
#[case::ansi_c_close_brace(r"x {a,$'}'}", &["x", "a", "}"])]
#[case::unrolled_brace_then_var(
    "for f in zz; do echo {x,y}$f; done",
    &["echo", "xzz", "yzz"]
)]
fn resolves_statically(#[case] input: &str, #[case] expected: &[&str]) {
    let cmd = only(input);
    assert_eq!(words(&cmd), expected, "{input:?}");
    assert!(!dynamic_name(&cmd), "{input:?} name should be static");
    assert!(
        dynamic_args(&cmd).is_empty(),
        "{input:?} args should be static"
    );
}

#[rstest]
#[case::parameter_name("$x -la", &["$x", "-la"], true, &[])]
#[case::parameter_arg("echo $x", &["echo", "$x"], false, &[0])]
#[case::partially_quoted_parameter(r#"echo "$x"y"#, &["echo", r#""$x"y"#], false, &[0])]
#[case::command_substitution("echo $(id)", &["echo", "$(id)"], false, &[0])]
#[case::arithmetic("echo $((1+2))", &["echo", "$((1+2))"], false, &[0])]
#[case::tilde_prefix("ls ~/foo", &["ls", "~/foo"], false, &[0])]
#[case::tilde_after_equals("ls x=~", &["ls", "x=~"], false, &[0])]
#[case::glob("ls *.txt", &["ls", "*.txt"], false, &[0])]
#[case::extglob("ls @(a|b)", &["ls", "@(a|b)"], false, &[0])]
#[case::number_sequence("echo {1..3}", &["echo", "{1..3}"], false, &[0])]
#[case::char_sequence("echo x{a..c}", &["echo", "x{a..c}"], false, &[0])]
#[case::gettext(r#"echo $"hi""#, &["echo", r#"$"hi""#], false, &[0])]
#[case::ansi_c_nul(r"echo $'\x00'", &["echo", r"$'\x00'"], false, &[0])]
#[case::ansi_c_invalid_utf8(r"echo $'\xff'", &["echo", r"$'\xff'"], false, &[0])]
#[case::brace_then_parameter("{a,$x}", &["a", "$x"], false, &[0])]
#[case::brace_prefix_parameter("echo a{b,c}$x", &["echo", "ab$x", "ac$x"], false, &[0, 1])]
#[case::brace_explosion(
    "echo {a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}",
    &["echo", "{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}{a,b}"],
    false,
    &[0]
)]
#[case::many_unclosed_braces(
    "echo {{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{",
    &["echo", "{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{"],
    false,
    &[0]
)]
#[case::sequence_overflows_i64(
    "echo {1..99999999999999999999}",
    &["echo", "{1..99999999999999999999}"],
    false,
    &[0]
)]
#[case::unrolled_sequence_overflows_i64(
    "for f in a; do echo {1..99999999999999999999}$f; done",
    &["echo", "{1..99999999999999999999}$f"],
    false,
    &[0]
)]
#[case::unrolled_var_glues_to_brace(
    "for f in zz; do $f{danger,} -n; done",
    &["$fdanger", "$f", "-n"],
    true,
    &[0]
)]
#[case::unrolled_comma_forms_brace(
    "for f in ,; do echo {a$f-n}; done",
    &["echo", "{a$f-n}"],
    false,
    &[0]
)]
#[case::unrolled_extglob_value(
    r#"for f in @(zz|yy); do echo "$f"; done"#,
    &["echo", r#""$f""#],
    false,
    &[0]
)]
#[case::process_substitution_arg("cat <(true)", &["cat", "<(true)"], false, &[0])]
#[case::output_process_substitution_keeps_indices(
    "tee >(cat) -a",
    &["tee", ">(cat)", "-a"],
    false,
    &[0]
)]
#[case::unrolled_tilde_value(
    r#"for d in ~; do echo "$d"; done"#,
    &["echo", r#""$d""#],
    false,
    &[0]
)]
fn flags_dynamic_words(
    #[case] input: &str,
    #[case] expected: &[&str],
    #[case] name_dynamic: bool,
    #[case] dynamic: &[u64],
) {
    let cmd = split(input).remove(0);
    assert_eq!(words(&cmd), expected, "{input:?}");
    assert_eq!(dynamic_name(&cmd), name_dynamic, "{input:?} dynamic_name");
    assert_eq!(dynamic_args(&cmd), dynamic, "{input:?} dynamic_args");
}

#[rstest]
#[case::unchanged("ls -la", None)]
#[case::dynamic_unchanged("echo $x", None)]
#[case::quoted_name(r#""-"echo hi"#, Some(json!({"name": "\"-\"echo", "args": ["hi"]})))]
#[case::brace_name_only("{echo,hi}", Some(json!({"name": "{echo,hi}"})))]
#[case::quoted_arg(r#"echo "pie""#, Some(json!({"name": "echo", "args": ["\"pie\""]})))]
fn raw_present_only_when_changed(#[case] input: &str, #[case] expected: Option<Value>) {
    assert_eq!(only(input).get("raw").cloned(), expected, "{input:?}");
}

#[test]
fn unrolled_loop_value_resolves() {
    let names: Vec<_> = split(r#"for f in a b; do "$f"x; done"#)
        .iter()
        .map(|c| name(c).expect("name").to_owned())
        .collect();
    assert_eq!(names, ["ax", "bx"]);
}
