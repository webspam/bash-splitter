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

| Value         | Meaning                                                                                    |
| ------------- | ------------------------------------------------------------------------------------------ |
| `file`        | A file target. Includes `>&word` / `1>&word` when `word` is not digits or `-`.             |
| `fd`          | A descriptor duplication (`2>&1`), move (`3>&1-`), or close (`2>&-`, target `-`).          |
| `fd_or_file`  | `>&$x` / `1>&$x`: a descriptor if `$x` expands to digits or `-`, otherwise a file.         |
| `ambiguous`   | Bash rejects it at runtime with "ambiguous redirect" and the command does not run (below). |
| `process_sub` | A process-substitution target, e.g. `> >(cmd)`.                                            |
| `herestring`  | A here-string, `<<<`.                                                                      |
| `heredoc`     | A heredoc body.                                                                            |

### `ambiguous`

A file target is brace-expanded, word-split, and globbed, and must yield exactly one word:

- `> {a,b}`, `> x{a,b}y`, `&> {a,b}`: two words
- `> {a,}`: one word, resolves to file `a`

A non-numeric duplication target is a file only on stdout:

- `2>&word`, `3>&word`, `<&word`, `>&word-`: ambiguous
- `>&"1-"`: file `1-` (only an unquoted trailing `-` is a move)

## `dynamic`

| Source           | `target`     | `dynamic` | Meaning                                                         |
| ---------------- | ------------ | --------- | --------------------------------------------------------------- |
| `> "my log.txt"` | `my log.txt` | omitted   | The exact filename/fd bash uses, with any quotes stripped off.  |
| `> $out`         | `$out`       | `true`    | The source word as written; bash only resolves it when it runs. |

`true` when:

- Parameter / command / arithmetic expansion: `> $out`, `> $(date)`, `> f.$((n))`
- Glob / tilde: `> *.txt`, `> ~/out`, `> a=~`
- Duplication onto an expansion: `>&$fd`, `2>&$fd`
- Here-string tilde at the start or after `:`: `<<< ~`, `<<< a:~` (here-strings are never brace-expanded or globbed)
- Process substitution (always): `> >(cmd)`
- An unquoted heredoc containing expansions or escapes

## `heredoc` object

Carries the `delimiter`, an `expands` flag (false when the delimiter is quoted, e.g. `<<'EOF'`), and the raw `body`.

Input with a redirect on a compound command (`while ...; done > log`) is rejected via exit code 3.
