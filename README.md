# bash-splitter

Splits a bash command string into its individual commands, so each can be inspected on its own (for example, against allow/deny rules).

## Simple example

```sh
echo 'ls -la' | bash-splitter
```

```json
[[{ "command": "ls -la", "name": "ls", "args": ["-la"] }]]
```

## Output

Reads the raw command on stdin and writes a JSON array of pipelines on stdout, in source order. Each pipeline is an array of stages, and each stage splits a command into its `assignments`, `name`, and `args`, plus optional [per-stage metadata](#per-stage-metadata).

## Two modes

- **Flat (default)**: every command, nested or not, listed as a top-level pipeline. Use this to check each command a line would run against a filter, without caring where it came from.
- **Nested (`-n` / `--nested`)**: only root commands at the top level; a command hidden in a substitution is embedded under the stage it came from, in `substitutions`. Use this to tell a genuine top-level command apart from one that only runs inside an expansion.

All other metadata is the same in either mode.

## Coverage

Parsing uses [brush](https://crates.io/crates/brush-parser), a proper bash parser.

That lets bash-splitter descend into every construct: pipelines, sequences, control flow, commands hidden in substitutions, and several others. See the [coverage reference](docs/reference/coverage.md) for the full table.

## Optional fields

Omitted when empty / false.

- **`substitutions`** (`--nested` only): recursive tree of all substituted commands / pipelines.
- **`redirects`**: I/O redirects (e.g. `>`), in source order. See the [redirects reference](docs/reference/redirects.md).
- **`in_loop`**: `true` when the stage runs inside a `for`/`while`/`until` loop (caveat: any substitutions in the loop **will not** have this flag set, but in nested mode the parent(s) can be checked).
- **`variables`**: all `$var`-style parameters the stage will expand when executed (`$f`, `${x}`, `$1`, `$?`), deduped in first-seen order.

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
