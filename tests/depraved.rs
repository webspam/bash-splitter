//! Adversarial inputs of the kind people post online specifically to baffle naive
//! command parsers. Each test states why the input is horrid right above it. Inputs
//! are inline Rust strings fed straight to stdin, so there is no shell-quoting layer.
//! "Truly depraved inputs"

mod common;
use common::{argv, name, split};

/// True if any split command has `token` anywhere in its argv.
fn any_argv_has(cmds: &[serde_json::Value], token: &str) -> bool {
    cmds.iter()
        .any(|c| argv(c).iter().any(|a| a.contains(token)))
}

// a heredoc flips the parser into a literal-text state. Everything between
// `<<EOF` and `EOF` is data, not commands. A line-oriented splitter would emit the
// body as real commands -- so `rm -rf /` inside a heredoc would register with the
// rule engine as something actually run. It must not.
#[test]
fn heredoc_body_is_not_split_into_commands() {
    let input = "cat <<EOF\nrm -rf / --no-preserve-root\nsudo dd if=/dev/zero of=/dev/sda\nEOF\n";
    let cmds = split(input);
    assert_eq!(cmds.len(), 1, "heredoc is one command: {cmds:?}");
    assert_eq!(argv(&cmds[0]), ["cat"]);
    assert!(
        !any_argv_has(&cmds, "rm"),
        "heredoc body `rm` leaked as a command"
    );
    assert!(
        !any_argv_has(&cmds, "dd"),
        "heredoc body `dd` leaked as a command"
    );
    assert!(
        !any_argv_has(&cmds, "sudo"),
        "heredoc body `sudo` leaked as a command"
    );
}

// `if`, `then`, `for`, `done`, ... are reserved words ONLY in command
// position (the first word). As later words they are ordinary arguments. A parser
// that keys off the token tries to open a control-flow block and falls over.
#[test]
fn reserved_words_as_arguments_are_plain_words() {
    let cmds = split("echo if then else fi for do done while");
    assert_eq!(cmds.len(), 1);
    assert_eq!(
        argv(&cmds[0]),
        [
            "echo", "if", "then", "else", "fi", "for", "do", "done", "while"
        ]
    );
}

// `$((` opens an arithmetic expansion; the whole thing is part of one word.
// It must not be mistaken for a command substitution wrapping a subshell.
#[test]
fn arithmetic_expansion_stays_in_argv() {
    let cmds = split("echo $((1 + 2))");
    assert_eq!(
        cmds.len(),
        1,
        "arithmetic is not a separate command: {cmds:?}"
    );
    assert_eq!(argv(&cmds[0]).first(), Some(&"echo"));
    assert!(
        argv(&cmds[0])
            .iter()
            .any(|a| a.contains('1') && a.contains('2')),
        "the arithmetic word should be preserved: {:?}",
        argv(&cmds[0])
    );
}

// one space flips the meaning. `$( (cmd) )` is a command substitution whose
// body is a subshell -- POSIX literally recommends the space to disambiguate it from
// `$(( ))`. The substitution runs a command, so that command is surfaced for the
// rule engine: `exit` appears as its own entry.
#[test]
fn command_sub_of_subshell_is_descended() {
    let cmds = split("echo $( (exit 1) )");
    // The outer echo keeps the substitution verbatim as its argument...
    let echo = cmds.iter().find(|c| name(c) == Some("echo")).unwrap();
    assert_eq!(argv(echo), ["echo", "$( (exit 1) )"]);
    // ...and the command inside the substitution surfaces too.
    assert!(
        cmds.iter().any(|c| name(c) == Some("exit")),
        "command inside the substitution should surface: {cmds:?}"
    );
}

// backticks use the same delimiter to open and close, so nesting requires
// backslash-escaping the inner pair. The canonical snippet for breaking a tokenizer.
// Both nesting levels must surface as their own commands.
#[test]
fn nested_escaped_backticks_are_descended() {
    // Raw string so the backslashes reach bash verbatim: echo `echo \`date\``
    let cmds = split(r#"echo `echo \`date\`` "#);
    assert!(
        cmds.iter().any(|c| name(c) == Some("date")),
        "the doubly-nested `date` should surface: {cmds:?}"
    );
}

// `#` only starts a comment at the start of a word. Word-internal `#` is
// literal. A naive comment-stripper truncates the URL at the fragment.
#[test]
fn hash_in_word_is_literal() {
    let cmds = split("curl https://example.com/page#section");
    assert_eq!(cmds.len(), 1);
    assert!(
        any_argv_has(&cmds, "page#section"),
        "the `#section` fragment must survive: {:?}",
        argv(&cmds[0])
    );
}
