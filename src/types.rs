use serde::Serialize;

/// One command: a single stage of a pipeline.
#[derive(Serialize)]
pub struct Stage {
    /// Stable index across the whole flattened output; `parent`/`children` reference it.
    pub(crate) id: usize,
    /// Reconstructed text of this single command.
    pub(crate) command: String,
    /// Leading env assignments (the `LD_PRELOAD=x` in `LD_PRELOAD=x cmd`, or a bare `FOO=bar`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) assignments: Vec<String>,
    /// The command actually invoked (`cmd` in `LD_PRELOAD=x cmd -a`). Absent for a
    /// bare assignment (`FOO=bar`), which runs nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// The command's arguments, in order. Redirects and process substitutions are excluded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<String>,
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
    /// The `id` of the stage this one surfaced from (`cd`, for the `echo` in `cd "$(echo pie)"`).
    /// Absent for top-level, or a substitution in a container that runs nothing (`[[ ]]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent: Option<usize>,
    /// The `id`s of the stages surfaced from this one's substitutions. Non-empty marks
    /// a complex command whose words or redirects hide other commands.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) children: Vec<usize>,
}

/// One I/O redirect on a command. `kind` tags the target family: `file`, `fd`,
/// `process_sub`, `herestring`, `heredoc`.
#[derive(Serialize)]
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
}

/// A heredoc body and how bash treats it.
#[derive(Serialize)]
pub(crate) struct HereDoc {
    /// The end delimiter, raw (`EOF`, `'EOF'`); quoting is reflected in `expands`.
    pub(crate) delimiter: String,
    /// False when the delimiter is quoted (`<<'EOF'`), which suppresses expansion.
    pub(crate) expands: bool,
    /// The raw body text; leading tabs are stripped for `<<-` (shown in `op`), as bash does.
    pub(crate) body: String,
}

/// A walked command plus internal bookkeeping used to group stages into pipelines.
pub(crate) struct Walked {
    pub(crate) command: String,
    pub(crate) assignments: Vec<String>,
    pub(crate) name: Option<String>,
    pub(crate) args: Vec<String>,
    pub(crate) redirects: Vec<Redirect>,
    pub(crate) in_loop: bool,
    pub(crate) variables: Vec<String>,
    pub(crate) parent: Option<usize>,
    pub(crate) children: Vec<usize>,
    pub(crate) piped_from_previous: bool,
}

/// A substitution source still to be walked, tagged with the `id` of the stage it
/// came from (`None` for a container that runs nothing, e.g. `[[ ]]`).
pub(crate) struct Sub {
    pub(crate) source: String,
    pub(crate) parent: Option<usize>,
}
