# Coverage

The bash constructs bash-splitter understands, and what it does with each. Splitting follows bash's own grammar (via [brush](https://crates.io/crates/brush-parser)), not string matching, so these hold even when constructs nest.

## Structural splitting

How a command line is broken into pipelines and stages.

| Construct         | Example                                                                  | Behaviour                                                                                                                                            |
| ----------------- | ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Pipeline          | `a \| b \| c`                                                            | Each stage is a separate command; leftmost is index 0.                                                                                               |
| Sequence / list   | `a; b`, `a && b`, `a \|\| b`                                             | Each command becomes its own pipeline.                                                                                                               |
| Pipe continuation | `(echo a; echo b) \| foo`                                                | A stage reading an upstream pipe continues the pipeline; any other command starts a new one. Only a group's last command joins the downstream stage. |
| Grouping as stage | `foo \| (grep x)`, `foo \| { a \| b; }`                                  | Descends into subshells and brace groups; inner commands are listed individually, and the group's first command inherits the upstream pipe.          |
| Compound command  | `for`, `while`, `until`, `if`, `case`, functions, coprocesses, subshells | Never emitted whole. Bodies and conditions are inspected so every nested command is listed, including the command used as an `if`/`while` condition. In nested mode, loops group their contents as [loop objects](/guide/#loops-nested-mode). |
| Env assignment    | `LD_PRELOAD=x cmd -a`                                                    | Split into `assignments`, `name`, and `args`. A bare `FOO=bar` is listed with no `name`, since it invokes nothing.                                   |

## Hidden commands

A command hidden inside an expansion still runs, so it is surfaced too: in flat mode as a trailing pipeline, in nested mode embedded in the stage's `substitutions`. These are the places the splitter looks.

| Location             | Example                                                       | Notes                                                                                                   |
| -------------------- | ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Command substitution | `echo $(cmd)`, backquotes                                     | Included inside double quotes too.                                                                      |
| Process substitution | `diff <(sort a) <(sort b)`                                    | Both inner commands surfaced.                                                                           |
| Inside `[[ ... ]]`   | `[[ -n $(cmd) ]]`                                             | The test itself runs nothing, but a substitution in its words does.                                     |
| Nested               | `echo $(foo $(bar))`                                          | Every level is surfaced.                                                                                |
| Word expansion       | `${x:-$(cmd)}`, `${x/$(cmd)/y}`, `$(( $(cmd) ))`              | Parameter-expansion values and patterns, and arithmetic expansions.                                     |
| Redirect             | `> $(cmd)`, `<<< "$(cmd)"`, `> >(cmd)`, unquoted heredoc body | Targets and bodies are expanded. A quoted delimiter (`<<'EOF'`) suppresses expansion and is left alone. |

A substitution with no owning stage (one in `[[ ... ]]`, or in a redirect on a compound command) has no parent to embed under, so it surfaces as a top-level pipeline in both modes.

## Static loop unrolling

A `for` loop over plain literal values is unrolled into its individual commands:

```bash
for f in a b c; do echo $f; done
# echo a, echo b, echo c

# With `--no-unroll`, it will simply be: echo $f
```

Only fully-static loops qualify; anything needing real expansion or guesswork is not unrolled.

In flat mode the unrolled commands replace the loop body. In nested mode the [loop object](/guide/#loops-nested-mode) carries both: `body` as written, `unrolled` per iteration.

::: tip
Pass `-U` / `--no-unroll` to skip unrolling.
:::

| Case                     | Example                                         | Unrolled | Behaviour                                                                |
| ------------------------ | ----------------------------------------------- | :------: | ------------------------------------------------------------------------ |
| Plain literals           | `for f in a b c`                                |    ✅    | Each `$f` / `${f}` in the body is substituted (respecting quotes).       |
| Nested static loops      | `for a in x; do for b in y z; ...`              |    ✅    | Both variables are substituted.                                          |
| Non-literal values       | `for f in *.txt`, `$(ls)`, `$x`, `"a b"`        |    ❌    | A glob, expansion, quote, or brace is not evaluated.                     |
| Reassigned loop variable | `for f in a b; do f=x; ...`                     |    ❌    | `f=`, `read f`, `(( f++ ))`, or nested `for f` reassigns it.             |
| Flow control             | `for f in a b; do echo $f; break; done`         |    ❌    | `break` / `continue` / `return` / `exit` change how many iterations run. |
| Function definition      | `for f in a b; do g() { ...; }; done`           |    ❌    | The function body runs at call time, not per iteration.                  |
| Unrewritable reference   | `${f%.txt}`, `${f:-x}`, `$((f+1))`, `$(cmd $f)` |    ❌    | These reference `f` but only plain `$f` / `${f}` can be rewritten.       |

A form like `${f%.txt}` in a listed command leaves the loop unmodified. In a `[[ ]]` test or `case` selector - which don't impact output - it still unrolls:

```bash
for f in a b; do cat "${f%.txt}"; done                 # not unrolled: you see cat "${f%.txt}" once
for f in a b; do [[ ${f%.txt} = y ]]; cat "$f"; done   # unrolled: you see cat a, then cat b
```
