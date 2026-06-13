# Redirects

Each entry in a stage's `redirects` array describes one I/O redirect, in source order, so you do not have to text-search the `command`.

| Field     | Meaning                                                                     |
| --------- | --------------------------------------------------------------------------- |
| `op`      | The operator: `>`, `>>`, `<`, `<<`, `<<<`, `>&`, `&>`, and so on.           |
| `fd`      | The explicit file descriptor, when one is given. Optional.                  |
| `kind`    | The target family (below).                                                  |
| `target`  | The target text. Present for `file`, `fd`, `herestring`, and `process_sub`. |
| `heredoc` | The heredoc payload (below). Present for `heredoc`.                         |

## `kind`

| Value         | Meaning                                         |
| ------------- | ----------------------------------------------- |
| `file`        | A file target.                                  |
| `fd`          | A descriptor duplication, e.g. `2>&1`.          |
| `process_sub` | A process-substitution target, e.g. `> >(cmd)`. |
| `herestring`  | A here-string, `<<<`.                           |
| `heredoc`     | A heredoc body.                                 |

## `heredoc` object

Carries the `delimiter`, an `expands` flag (false when the delimiter is quoted, e.g. `<<'EOF'`), and the raw `body`.

A redirect hanging off a compound command (`while ...; done > log`) is not attached to any stage, but a command it hides is still surfaced.
