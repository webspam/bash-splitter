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
| Compound command  | `for`, `while`, `until`, `if`, `case`, functions, coprocesses, subshells | Never emitted whole. Bodies and conditions are inspected so every nested command is listed, including the command used as an `if`/`while` condition. |
| Env assignment    | `LD_PRELOAD=x cmd -a`                                                    | Split into `assignments`, `name`, and `args`. A bare `FOO=bar` is listed with no `name`, since it invokes nothing.                                   |

## Hidden commands

A command hidden inside an expansion still runs, so it is surfaced too: in flat mode as a trailing pipeline, in nested mode embedded in the stage's `substitutions`. These are the places the splitter looks.

| Location             | Example                                                       | Notes                                                                                                   |
| -------------------- | ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Command substitution | `echo $(rm -rf /)`, backquotes                                | Included inside double quotes too.                                                                      |
| Process substitution | `diff <(sort a) <(sort b)`                                    | Both inner commands surfaced.                                                                           |
| Inside `[[ ... ]]`   | `[[ -n $(cmd) ]]`                                             | The test itself runs nothing, but a substitution in its words does.                                     |
| Nested               | `echo $(foo $(bar))`                                          | Every level is surfaced.                                                                                |
| Word expansion       | `${x:-$(cmd)}`, `${x/$(cmd)/y}`, `$(( $(cmd) ))`              | Parameter-expansion values and patterns, and arithmetic expansions.                                     |
| Redirect             | `> $(cmd)`, `<<< "$(cmd)"`, `> >(cmd)`, unquoted heredoc body | Targets and bodies are expanded. A quoted delimiter (`<<'EOF'`) suppresses expansion and is left alone. |

A substitution with no owning stage (one in `[[ ... ]]`, or in a redirect on a compound command) has no parent to embed under, so it surfaces as a top-level pipeline in both modes.
