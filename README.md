# bash-splitter

Splits a bash command string into its individual commands so each can be inspected on its own (for example, to evaluate allow/deny rules against every command a line would actually run).

It reads the raw command on stdin and writes a JSON array of pipelines on stdout. Each pipeline is an array of its stages in source order; each stage breaks the command into its `assignments`, `name`, and `args`.

Output is flattened - an ordered array of arrays, regardless of nesting.

Parsing is done with [brush](https://crates.io/crates/brush-parser), a proper bash parser, so the split reflects bash's own grammar rather than ad-hoc string splitting. This is what lets it correctly handle pipelines, compound commands, and commands hidden in substitutions, expansions, and redirects.

## Simple example

```sh
echo 'ls -la' | bash-splitter
```

```json
[[{ "command": "ls -la", "name": "ls", "args": ["-la"] }]]
```

## What it splits, and how

- **Pipelines** (`a | b | c`): each stage becomes a separate command. This is represented as a JSON array, with the leftmost command being index 0 (`a`).
- **Sequences and lists** (`a; b`, `a && b`, `a || b`): each command is listed separately, as its own pipeline.
- **Logical boundaries**: a command reading an upstream pipe continues the current pipeline; any other command starts a new one. Groupings keep this correct, so `(echo a; echo b) | foo` splits at the group's internal `;` (only the group's last command joins the downstream stage).
- **Groupings as pipeline stages** (`foo | (grep x)`, `foo | { a | b; }`): the parser descends into subshells and brace groups so inner commands are listed individually, while the group's first command still inherits the upstream pipe.
- **Compound commands**: `for`, `while`, `until`, `if`, `case`, function bodies, coprocesses, and subshells are never emitted whole. Their bodies and conditions are inspected so every nested command is listed (including the command used as an `if`/`while` condition).
- **Command substitutions** (`echo $(rm -rf /)`, backquotes): the hidden command still runs, so it is listed as its own pipeline at the end. Substitutions inside double quotes are included too.
- **Process substitutions** (`diff <(sort a) <(sort b)`): the inner commands are listed too.
- **Substitutions inside `[[ ... ]]`** (`[[ -n $(cmd) ]]`): the test runs no command itself, but a substitution in its words still runs, so it is listed.
- **Nested substitutions** (`echo $(foo $(bar))`): every level is listed.
- **Substitutions in word expansions**: a command hidden in a parameter expansion's value or pattern (`${x:-$(cmd)}`, `${x/$(cmd)/y}`) or in an arithmetic expansion (`$(( $(cmd) ))`) is listed.
- **Substitutions in redirects**: redirect targets and bodies are expanded, so a command in one is listed too: `> $(cmd)`, here-strings (`<<< "$(cmd)"`), process-substitution targets (`> >(cmd)`), and unquoted heredoc bodies. A quoted heredoc delimiter (`<<'EOF'`) suppresses expansion and is left alone.
- **Env assignments**: a prefix like `LD_PRELOAD=x cmd -a` is split into `assignments`, `name`, and `args` rather than flattened. A bare `FOO=bar` is listed with no `name`, since it invokes nothing.

## What it excludes

- **Redirects** (`> /dev/null`) are not part of argv.
- The `[[ ... ]]` extended test is not emitted as a command (it runs none), but substitutions hidden in its words are included.

## Scope

This binary only splits; rule evaluation is left to the caller. CRLF input (e.g. from PowerShell on Windows) is normalized to LF so no stray `\r` is left on the last token. A parse error exits non-zero so the caller can decide what an unparseable command means.

## Usage

```sh
echo 'foo | (grep x) && bar' | bash-splitter
```

## Example

Input: a standalone command, then a pipeline, then another standalone command.

```sh
echo 'echo "pie" && grep -i foo access.log | sort -u; echo done' | bash-splitter
```

Actual output (one line):

```json
[[{"command":"echo \"pie\"","name":"echo","args":["\"pie\""]}],[{"command":"grep -i foo access.log","name":"grep","args":["-i","foo","access.log"]},{"command":"sort -u","name":"sort","args":["-u"]}],[{"command":"echo done","name":"echo","args":["done"]}]]
```

Same output, pretty-printed: the outer array holds three pipelines; `echo "pie"` and `echo done` are standalone, while the middle one has two piped stages.

```json
[
  [
    {
      "command": "echo \"pie\"",
      "name": "echo",
      "args": ["\"pie\""]
    }
  ],
  [
    {
      "command": "grep -i foo access.log",
      "name": "grep",
      "args": ["-i", "foo", "access.log"]
    },
    {
      "command": "sort -u",
      "name": "sort",
      "args": ["-u"]
    }
  ],
  [
    {
      "command": "echo done",
      "name": "echo",
      "args": ["done"]
    }
  ]
]
```
