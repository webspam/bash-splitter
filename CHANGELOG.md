# Changelog

## 0.2.0

### Features

- Nested output mode (`-n` / `--nested`): root commands stay top-level, with substituted commands embedded under their owning stage in a recursive `substitutions` tree. Flat stays the default.
- New `redirects` field: I/O redirects in source order; heredocs keep their `delimiter`, `body`, and an `expands` flag.
- New `in_loop` field: `true` when the stage runs inside a `for` / `while` / `until` loop.
- New `variables` field: the `$var`-style params the stage expands (`$f`, `${x}`, `$1`, `$?`), deduped in first-seen order.
- Detects commands hidden in more contexts: arithmetic `for` loops, `(( ... ))` commands, and array subscripts (`${arr[$(cmd)]}`).
- Library API: `split` and `split_nested` are exposed for use as a crate, not just the CLI.

### Documentation

- README rewritten around the two modes; coverage table and redirect detail moved to `docs/reference/`.

## 0.1.1

### Improvements

- Smaller, faster CLI binary: release builds use LTO, a single codegen unit, stripped symbols, and `panic = "abort"`.

### Distribution

- Prebuilt binaries for Linux, macOS (x64 / arm64), and Windows x64, with CI on every push.

## 0.1.0

First release. Reads a bash command on stdin and writes a JSON array of pipelines, splitting each command into `assignments`, `name`, and `args`. Uses the [brush](https://crates.io/crates/brush-parser) parser to descend into pipelines, sequences, control flow, compound commands, and substitutions. Flat output only.
