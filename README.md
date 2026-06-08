# bash-splitter

Splits a bash command string into its individual commands so each can be inspected on its own (for example, to evaluate allow/deny rules against every command a line would actually run).

It reads the raw command on stdin and writes a JSON array of pipelines on stdout. Each pipeline is an array of its stages in source order; each stage breaks the command into its `assignments`, `name`, and `args`, plus its `redirects`, an `in_loop` flag, and the `variables` it expands.

Parsing is done with [brush](https://crates.io/crates/brush-parser), a proper bash parser, so the split reflects bash's own grammar rather than ad-hoc string splitting. This is what lets it correctly handle pipelines, compound commands, and commands hidden in substitutions, expansions, and redirects.

## Two modes

The tool answers two different questions, one per mode:

- **Flat (default)**: every command, nested or not, listed as a top-level pipeline. Use this to check each command a line would run against a filter, without caring where it came from.
- **Nested (`-n` / `--nested`)**: only root commands at the top level; a command hidden in a substitution is embedded under the stage it came from, in `substitutions`. Use this to tell a genuine top-level command apart from one that only runs inside an expansion.

Both modes carry the same per-stage metadata. Only nested mode shows the tree; flat mode never does.

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
- **Command substitutions** (`echo $(rm -rf /)`, backquotes): the hidden command still runs, so it is surfaced too. In flat mode it is its own trailing pipeline; in nested mode it is embedded in the parent's `substitutions`. Substitutions inside double quotes are included.
- **Process substitutions** (`diff <(sort a) <(sort b)`): the inner commands are surfaced too.
- **Substitutions inside `[[ ... ]]`** (`[[ -n $(cmd) ]]`): the test runs no command itself, but a substitution in its words still runs, so it is surfaced.
- **Nested substitutions** (`echo $(foo $(bar))`): every level is surfaced.
- **Substitutions in word expansions**: a command hidden in a parameter expansion's value or pattern (`${x:-$(cmd)}`, `${x/$(cmd)/y}`) or in an arithmetic expansion (`$(( $(cmd) ))`) is surfaced.
- **Substitutions in redirects**: redirect targets and bodies are expanded, so a command in one is surfaced too: `> $(cmd)`, here-strings (`<<< "$(cmd)"`), process-substitution targets (`> >(cmd)`), and unquoted heredoc bodies. A quoted heredoc delimiter (`<<'EOF'`) suppresses expansion and is left alone.
- **Env assignments**: a prefix like `LD_PRELOAD=x cmd -a` is split into `assignments`, `name`, and `args` rather than flattened. A bare `FOO=bar` is listed with no `name`, since it invokes nothing.

## Per-stage metadata

Beyond the split words, a stage may carry the fields below. The optional ones are omitted when empty, so a plain command stays `{ command, name, args }`.

- **`redirects`**: the stage's I/O redirects, in source order, so you no longer have to text-search the `command`. Each entry has the operator `op` (`>`, `>>`, `<`, `<<`, `<<<`, `>&`, `&>`, ...), an optional explicit `fd`, and a `kind` that tags the target family: `file`, `fd` (a duplication like `2>&1`), `process_sub`, `herestring`, or `heredoc`. A `file`/`fd`/`herestring`/`process_sub` carries its `target` text; a `heredoc` carries a `heredoc` object with the `delimiter`, an `expands` flag (false when the delimiter is quoted, e.g. `<<'EOF'`), and the raw `body`. Redirects hanging off a compound command (`while ...; done > log`) are not attached to a stage, but a command they hide is still surfaced.
- **`in_loop`**: `true` when the stage runs inside a `for`/`while`/`until` loop (body or condition), so it executes once per iteration. An `if`/`case` does not count. Loop context is not tracked across a substitution boundary, so a command surfaced out of `$(...)` reports `false`.
- **`variables`**: the parameters the stage expands (`$f`, `${x}`, `$1`, `$?`), deduped in first-seen order, gathered from its assignments, name, args, redirect targets, and expanded heredoc bodies. Quoting is respected: a single-quoted word or a quoted-delimiter heredoc contributes nothing. Command substitutions are surfaced separately (see the modes above); this field is only `$var`-style expansion.
- **`substitutions`** (nested mode only): the pipelines surfaced from this stage's substitutions, in source order, each embedded recursively. Omitted when empty, so its presence doubles as the "this is a complex command, don't evaluate it in isolation" signal.

## What it excludes

- **Redirects** are not part of `args` (they are reported separately in `redirects`).
- The `[[ ... ]]` extended test is not emitted as a command (it runs none), but substitutions hidden in its words are included.

## Scope

This binary only splits; rule evaluation is left to the caller. A parse error exits non-zero so the caller can decide what an unparseable command means.

## Usage

```sh
echo 'foo | (grep x) && bar' | bash-splitter
echo 'foo | (grep x) && bar' | bash-splitter -n
```

## Flat example

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

## Metadata example

A loop whose body redirects to a file built from variables exercises all three extra fields:

```sh
printf 'for f in *.txt; do\n  grep "$pattern" "$f" > "out/$f"\ndone\n' | bash-splitter
```

Pretty-printed: one pipeline with the single `grep` stage, flagged `in_loop`, its redirect captured, and the variables it expands listed.

```json
[
  [
    {
      "command": "grep \"$pattern\" \"$f\" > \"out/$f\"",
      "name": "grep",
      "args": ["\"$pattern\"", "\"$f\""],
      "redirects": [{ "op": ">", "kind": "file", "target": "\"out/$f\"" }],
      "in_loop": true,
      "variables": ["pattern", "f"]
    }
  ]
]
```

A heredoc keeps its body, with `expands` reflecting whether the delimiter was quoted:

```json
{
  "command": "cat <<EOF\nprocessing $f\nEOF\n >> log.txt",
  "name": "cat",
  "redirects": [
    { "op": "<<", "kind": "heredoc", "heredoc": { "delimiter": "EOF", "expands": true, "body": "processing $f\n" } },
    { "op": ">>", "kind": "file", "target": "log.txt" }
  ],
  "variables": ["f"]
}
```

## Nesting example

In flat mode, a command hidden in another's argument surfaces as its own trailing pipeline:

```sh
echo 'cd "$(echo pie)"' | bash-splitter
```

```json
[
  [{ "command": "cd \"$(echo pie)\"", "name": "cd", "args": ["\"$(echo pie)\""] }],
  [{ "command": "echo pie", "name": "echo", "args": ["pie"] }]
]
```

In nested mode (`-n`), the same `echo` is embedded under the `cd` it was hidden in, rather than floated to the top level:

```sh
echo 'cd "$(echo pie)"' | bash-splitter -n
```

```json
[
  [{
    "command": "cd \"$(echo pie)\"",
    "name": "cd",
    "args": ["\"$(echo pie)\""],
    "substitutions": [
      [{ "command": "echo pie", "name": "echo", "args": ["pie"] }]
    ]
  }]
]
```

Nesting chains: `echo $(foo $(bar))` embeds `bar` inside `foo`'s `substitutions`, which is itself inside `echo`'s, so the full tree is structural with no ids to reassemble.

```sh
echo 'echo $(foo $(bar))' | bash-splitter -n
```

```json
[
  [{
    "command": "echo $(foo $(bar))",
    "name": "echo",
    "args": ["$(foo $(bar))"],
    "substitutions": [
      [{
        "command": "foo $(bar)",
        "name": "foo",
        "args": ["$(bar)"],
        "substitutions": [
          [{ "command": "bar", "name": "bar" }]
        ]
      }]
    ]
  }]
]
```

A substitution with no owning stage (a substitution in `[[ ... ]]`, or in a redirect on a compound command) has no parent to embed it under, so it surfaces as a top-level pipeline in both modes.
