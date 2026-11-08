use serde::Serialize;

/// One command: a single stage of a pipeline (flat mode).
/// All commands appear as top-level pipelines, including those surfaced from substitutions.
#[derive(Clone, Serialize)]
pub struct Stage {
    /// Reconstructed text of this single command.
    pub(crate) command: String,
    /// Leading env assignments (the `LD_PRELOAD=x` in `LD_PRELOAD=x cmd`, or a bare `FOO=bar`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) assignments: Vec<String>,
    #[serde(flatten)]
    pub(crate) argv: ResolvedArgv,
    /// The command's I/O redirects, in source order (heredoc bodies included).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) redirects: Vec<Redirect>,
    /// True when this command sits inside a `for`/`while`/`until` loop, so it runs
    /// once per iteration. Not tracked across substitution boundaries.
    #[serde(skip_serializing_if = "<&bool as std::ops::Not>::not")]
    pub(crate) in_loop: bool,
    /// Names of the parameters this command expands (`$f`, `${x}`, `$1`), deduped in
    /// first-seen order. A single-quoted or quoted-heredoc reference contributes none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) variables: Vec<String>,
    /// True when the stage contains variables that could not be statically analysed.
    #[serde(skip_serializing_if = "<&bool as std::ops::Not>::not")]
    pub(crate) dynamic_variables: bool,
}

#[derive(Default)]
pub(crate) struct Variables {
    pub(crate) names: Vec<String>,
    pub(crate) dynamic: bool,
}

impl Variables {
    pub(crate) fn push(&mut self, name: &str) {
        if !self.names.iter().any(|n| n == name) {
            self.names.push(name.to_owned());
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct ResolvedArgv {
    /// The command actually invoked (`cmd` in `LD_PRELOAD=x cmd -a`), brace-expanded and
    /// quote-removed. Absent for a bare assignment (`FOO=bar`), which runs nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// The command's arguments, in order, resolved like `name`. Redirects are excluded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<String>,
    /// The source words of `name` and `args`; present only when resolution changed them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) raw: Option<RawArgv>,
    /// True when `name` could not be statically resolved and is left as written.
    #[serde(skip_serializing_if = "<&bool as std::ops::Not>::not")]
    pub(crate) dynamic_name: bool,
    /// Indices into `args` of words that could not be statically resolved.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) dynamic_args: Vec<usize>,
}

/// A command's name and arguments exactly as written.
#[derive(Clone, Serialize)]
pub(crate) struct RawArgv {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<String>,
}

/// One command: a single stage of a pipeline (nested mode). Carries the same fields
/// as [`Stage`], plus the substitutions embedded recursively under it.
/// Only root commands appear at the top level.
#[derive(Serialize)]
pub struct NestedStage {
    #[serde(flatten)]
    pub(crate) stage: Stage,
    /// Items surfaced from this command's substitutions. Empty means no substitutions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) substitutions: Vec<NestedItem>,
}

/// One item of nested output: a pipeline of stages (a JSON array) or a loop
/// construct (a JSON object).
#[derive(Serialize)]
#[serde(untagged)]
pub enum NestedItem {
    Pipeline(Vec<NestedStage>),
    Loop(NestedLoop),
}

/// A loop construct (nested mode): its contents as written and, for a static
/// `for`, the unrolled per-iteration commands.
#[derive(Serialize)]
pub struct NestedLoop {
    pub(crate) kind: LoopKind,
    /// The iteration variable of `for v in ...`; absent for the other loop forms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) variable: Option<String>,
    /// The condition's commands (while/until), re-run each iteration.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) condition: Vec<NestedItem>,
    /// The body as written, loop variables unexpanded.
    pub(crate) body: Vec<NestedItem>,
    /// The iterations of a statically unrolled `for`; absent when not unrollable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unrolled: Option<Vec<Iteration>>,
}

/// One unrolled iteration: the bound value and the body with it substituted.
#[derive(Serialize)]
pub struct Iteration {
    pub(crate) value: String,
    pub(crate) commands: Vec<NestedItem>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LoopKind {
    For,
    While,
    Until,
}

/// One I/O redirect on a command. `kind` tags the target family: `file`, `fd`,
/// `process_sub`, `herestring`, `heredoc`.
#[derive(Clone, Serialize)]
pub(crate) struct Redirect {
    /// The explicit fd, if the source gave one (`2>` -> 2). Absent means bash's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fd: Option<i32>,
    /// The redirect operator as written (`>`, `>>`, `<`, `<<`, `<<<`, `>&`, `&>`, ...).
    pub(crate) op: String,
    /// Target family; see the struct doc.
    pub(crate) kind: &'static str,
    /// The target text (filename, fd number, or rendered process substitution). Absent
    /// for a heredoc, whose payload lives in `heredoc`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target: Option<String>,
    /// Present only for heredocs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) heredoc: Option<HereDoc>,
    /// True when `target` (or a heredoc body) could not be statically resolved: it
    /// expands a parameter, command, arithmetic, glob, or tilde, so the real contents
    /// are only known at runtime. A process substitution is always dynamic.
    #[serde(skip_serializing_if = "<&bool as std::ops::Not>::not")]
    pub(crate) dynamic: bool,
}

/// A heredoc body and how bash treats it.
#[derive(Clone, Serialize)]
pub(crate) struct HereDoc {
    /// The end delimiter, raw (`EOF`, `'EOF'`); quoting is reflected in `expands`.
    pub(crate) delimiter: String,
    /// False when the delimiter is quoted (`<<'EOF'`), which suppresses expansion.
    pub(crate) expands: bool,
    /// The raw body text; leading tabs are stripped for `<<-` (shown in `op`), as bash does.
    pub(crate) body: String,
}

/// Identifies a stage: its index in walk output order.
#[derive(Clone, Copy)]
pub(crate) struct StageId(pub(crate) usize);

/// Identifies a loop: its index in walk order.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LoopId(pub(crate) usize);

/// Which part of a loop a walked element sits in.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Section {
    Condition,
    Body,
    /// One unroll pass, by iteration index.
    Iteration(usize),
}

/// A structural slot: one section of a specific loop.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Loc {
    pub(crate) loop_id: LoopId,
    pub(crate) section: Section,
}

/// A walked command plus internal bookkeeping used to group stages into pipelines.
pub(crate) struct Walked {
    pub(crate) stage: Stage,
    pub(crate) parent: Option<StageId>,
    /// The innermost loop slot the command sits in, within its own walk.
    pub(crate) loc: Option<Loc>,
    pub(crate) piped_from_previous: bool,
}

/// A walked loop construct plus bookkeeping to place it in nested output.
pub(crate) struct LoopWalked {
    pub(crate) kind: LoopKind,
    pub(crate) variable: Option<String>,
    /// Slot in the enclosing loop, if any (within the same walk).
    pub(crate) parent: Option<Loc>,
    /// The stage whose substitution this loop's walk came from.
    pub(crate) sub_parent: Option<StageId>,
    /// Stage index where the construct began; orders it among sibling stages.
    pub(crate) position: usize,
    /// The literal iteration values; `Some` only when the loop unrolled.
    pub(crate) values: Option<Vec<String>>,
}

/// A substitution source still to be walked, tagged with the `id` of the stage it
/// came from (`None` for a container that runs nothing, e.g. `[[ ]]`).
pub(crate) struct Sub {
    pub(crate) source: String,
    pub(crate) parent: Option<StageId>,
}
