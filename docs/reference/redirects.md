# Redirects

Each entry in a stage's `redirects` array describes one I/O redirect, in source order, so you do not have to text-search the `command`.

| Field     | Meaning                                                                     |
| --------- | --------------------------------------------------------------------------- |
| `op`      | The operator: `>`, `>>`, `<`, `<<`, `<<<`, `>&`, `&>`, and so on.           |
| `fd`      | The explicit file descriptor, when one is given. Optional.                  |
| `kind`    | The target family (below).                                                  |
| `target`  | The target text. Present for `file`, `fd`, `herestring`, and `process_sub`. |
| `heredoc` | The heredoc payload (below). Present for `heredoc`.                         |
| `dynamic` | `true` when the target cannot be statically resolved (below). Optional.     |

## `kind`

| Value         | Meaning                                         |
| ------------- | ----------------------------------------------- |
| `file`        | A file target.                                  |
| `fd`          | A descriptor duplication, e.g. `2>&1`.          |
| `process_sub` | A process-substitution target, e.g. `> >(cmd)`. |
| `herestring`  | A here-string, `<<<`.                           |
| `heredoc`     | A heredoc body.                                 |

## `dynamic`

| Source           | `target`     | `dynamic` | Meaning                                                         |
| ---------------- | ------------ | --------- | --------------------------------------------------------------- |
| `> "my log.txt"` | `my log.txt` | omitted   | The exact filename/fd bash uses, with any quotes stripped off.  |
| `> $out`         | `$out`       | `true`    | The source word as written; bash only resolves it when it runs. |

`true` when:

- Parameter / command / arithmetic expansion: `> $out`, `> $(date)`, `> f.$((n))`
- Glob / tilde: `> *.txt`, `> ~/out`
- Duplication onto an expansion: `>&$fd`
- Process substitution (always): `> >(cmd)`
- Heredoc with an unquoted delimiter whose body references a parameter or command

## `heredoc` object

Carries the `delimiter`, an `expands` flag (false when the delimiter is quoted, e.g. `<<'EOF'`), and the raw `body`.

Input with a redirect on a compound command (`while ...; done > log`) is rejected via exit code 3.
