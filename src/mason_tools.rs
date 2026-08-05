//! Mason's opt-in tool-call loop.
//!
//! Every other Mason transport in this codebase is single-shot: one prompt, one
//! reply, and whatever the reply contains is the whole of the model's output.
//! That works when the model already knows everything it needs. It does not
//! work when the model needs to *look* — read a file it was not given, or list
//! a directory to find out what is there — before it can decide what to write.
//!
//! This module adds that missing capability, and nothing else. It is dead
//! unless a spec sets `worker_harness.tool_loop: true`.
//!
//! Two rules shape everything here, both learned the expensive way from the six
//! transports that came before:
//!
//! 1. **Never return `Ok` having silently skipped content.** If the model emits
//!    something this parser does not understand, the parse fails loudly and
//!    names what and where. A rejected response costs a turn; a silently
//!    dropped `write_file` costs the operator a file they believe was written.
//! 2. **Return writes, do not perform them.** The loop hands back
//!    `(path, content)` pairs. The caller converts them to `MasonEdit` and
//!    routes them through `validate_mason_edits` and the one existing apply
//!    path. Two write paths would mean two places for the safety check to be
//!    forgotten, and one of them would eventually be.

use anyhow::{bail, Result};
use std::path::{Component, Path, PathBuf};

use crate::llm::{LlmProvider, LlmRequest, Message};
// One matcher, two lanes. `near_miss_marker_word` was written and tuned here,
// then found to be exactly what the single-shot transports were missing; it
// lives in `mason_transport` now so this lane and that one cannot drift into
// different ideas of what "nearly a marker" means. See
// `mason_transport::unreadable_edit_marker` for the sibling caller.
use crate::mason_transport::near_miss_marker_word;

const TOOL_MARKER: &str = "### TOOL:";
const CONTENT_MARKER: &str = "### CONTENT";
const END_CONTENT_MARKER: &str = "### END CONTENT";

/// Wording handed to the model, kept next to the parser for the same reason
/// `FENCED_FORMAT_INSTRUCTION` is: a prompt describing one format while the
/// parser expects another is the exact failure this transport exists to remove.
pub const TOOL_LOOP_INSTRUCTION: &str = "\
You are working in a tool loop. You may inspect the workspace before deciding \
what to write, one message at a time.

To inspect a file:

### TOOL: read_file
path: <relative/path/from/the/workspace/root>

To list a directory:

### TOOL: list_dir
path: <relative/path/from/the/workspace/root>

To write a file (this is how you make every change — there is no other way):

### TOOL: write_file
path: <relative/path/from/the/workspace/root>
### CONTENT
<the complete contents of the file, written literally>
### END CONTENT

Rules:
- Paths are always relative to the workspace root. Absolute paths and '..' are \
refused outright and end the run.
- read_file and list_dir can reach any file in the workspace, not only the ones \
you may edit. Read what you need to understand the change; do not go looking \
through unrelated files. Build output, VCS internals and anything holding \
credentials are refused — you will get an ERROR result naming the reason, which \
is not a failure and not something to retry.
- You may emit more than one tool call in a single message; each is executed in \
order and every result is returned to you.
- write_file replaces the whole file. Include the complete contents, exactly as \
they should appear on disk. Do not escape quotes, backslashes or newlines, and \
do not wrap contents in backticks.
- File content must not contain any line whose only non-whitespace text is \
'### END CONTENT', and must not contain a line whose first non-whitespace text \
is '### TOOL:'. Indenting such a line does NOT make it safe: the parser trims \
each line before comparing it, so an indented '### END CONTENT' still ends your \
write and everything after it is lost. If the file you are writing genuinely \
needs one of those lines, say so instead of writing the file.
- Markers are matched exactly: three hashes, uppercase, spelled as shown. \
'#### TOOL:' or '### Tool:' are not tool calls, and a message containing one is \
rejected rather than guessed at.
- Do NOT use '### FILE:' or '### PATCH:' blocks, and do NOT send a JSON edit \
proposal, in this mode. None of them are read here, and a message containing \
one is rejected.
- When you have written everything the spec requires, reply with a short summary \
and no tool calls at all. That ends the loop.";

/// The three things Mason may do inside the staged workspace.
///
/// `WriteFile` is deliberately not an action: it is a *request* to write, which
/// the loop records and returns rather than performing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MasonTool {
    ReadFile { path: String },
    ListDir { path: String },
    WriteFile { path: String, content: String },
}

impl MasonTool {
    pub fn name(&self) -> &'static str {
        match self {
            MasonTool::ReadFile { .. } => "read_file",
            MasonTool::ListDir { .. } => "list_dir",
            MasonTool::WriteFile { .. } => "write_file",
        }
    }

    pub fn path(&self) -> &str {
        match self {
            MasonTool::ReadFile { path }
            | MasonTool::ListDir { path }
            | MasonTool::WriteFile { path, .. } => path,
        }
    }
}

/// Convenience wrapper for the single-call case, and for tests.
///
/// Returns `None` both when the message carries no tool call at all and when it
/// carries something this parser rejects — which makes it unsafe to drive the
/// loop with, because those two cases demand opposite responses ("the model is
/// finished" versus "the model emitted something we could not read"). The loop
/// uses [`parse_tool_calls`], which keeps them apart. This exists because a
/// single call is the overwhelmingly common shape and reads far better in a
/// test than a `Vec` destructure.
pub fn parse_tool_call(raw: &str) -> Option<MasonTool> {
    match parse_tool_calls(raw) {
        Ok(mut calls) if calls.len() == 1 => calls.pop(),
        _ => None,
    }
}

/// Parse every tool call in one model message.
///
/// Every reading of the input this parser does not understand — an unknown tool
/// name, a missing `path:`, a `write_file` with no content block, a content
/// block that never closes, a second `path:` for one call — is an `Err`. None
/// of them are recoverable by guessing, and every guess would drop or invent a
/// file write.
///
/// `Ok(vec![])` means only that no *exact* `### TOOL:` header was found. That is
/// **not** on its own sufficient to conclude the model has finished, and callers
/// must not treat it that way: a message can carry a write in a format this
/// parser does not read at all — a JSON edit proposal, a `### FILE:` block, or
/// the right vocabulary one character off (`#### TOOL:`, `### Tool:`) — and
/// every one of those parses to zero calls here. [`unreadable_write_marker`]
/// exists to catch that, and [`run_mason_tool_loop_with_recorder`] consults it
/// before ever concluding the model is done, because "I could not read this" and
/// "the model is finished" must never be the same outcome.
///
/// Be clear about what that buys, though: termination is **not** positive. It is
/// still absence-of-markers, only with a longer list of markers. These are known
/// to slip through and be read as the model signing off, dropping whatever they
/// carried:
///
/// - no hashes at all (`TOOL: write_file`, `FILE: js/a.js`);
/// - a single hash (`# TOOL:`) or seven or more;
/// - a fenced ` ```js ` block with the filename in a comment;
/// - unquoted YAML (`edits:` / `- path:`);
/// - a pretty-printed JSON proposal whose `"edits"` and `[` land on separate
///   lines *and* whose `"path":` and `"content":` land on separate lines.
///
/// A genuinely positive test — requiring an explicit end-of-work token before
/// accepting termination — is the real fix and is not what this does. Do not
/// read the guard as more than it is.
pub fn parse_tool_calls(raw: &str) -> Result<Vec<MasonTool>> {
    let mut calls: Vec<MasonTool> = Vec::new();
    let mut pending: Option<Pending> = None;
    let mut in_content = false;

    // Split on '\n' rather than using `lines()` so a trailing '\r' survives
    // into content, matching how `collect_fenced_edits` treats CRLF bodies.
    for (index, line) in raw.split('\n').enumerate() {
        let line_num = index + 1;
        let for_markers = line.trim_end_matches('\r');
        let trimmed = for_markers.trim();

        if in_content {
            if trimmed == END_CONTENT_MARKER {
                in_content = false;
            } else if trimmed.starts_with(TOOL_MARKER) {
                // A new tool header while a content block is open means the
                // model forgot the closer. Everything from here to end of input
                // would otherwise be swallowed into the previous file's
                // content, silently, and the tool call on this line would
                // vanish with it.
                bail!(
                    "line {line_num}: a '{TOOL_MARKER}' header appeared while the \
                     '{CONTENT_MARKER}' block was still open — close it with \
                     '{END_CONTENT_MARKER}' first. File content may not contain a line \
                     starting with '{TOOL_MARKER}'."
                );
            } else if let Some(body) = pending.as_mut().and_then(|p| p.content.as_mut()) {
                body.push(line.to_string());
            } else {
                // Unreachable in practice: `in_content` is only ever set
                // alongside a pending call with an open body. Refusing beats
                // an `unwrap`, and beats dropping the line.
                bail!("line {line_num}: content block has no owning tool call");
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix(TOOL_MARKER) {
            if let Some(previous) = pending.take() {
                calls.push(finish_pending(previous)?);
            }
            let kind = rest.trim().to_string();
            if kind.is_empty() {
                bail!("line {line_num}: '{TOOL_MARKER}' header named no tool");
            }
            pending = Some(Pending {
                kind,
                line: line_num,
                path: None,
                content: None,
            });
        } else if let Some(rest) = trimmed.strip_prefix("path:") {
            let Some(current) = pending.as_mut() else {
                // A bare `path:` line in prose, with no tool call open, is not
                // a tool call and is not treated as one. It is also not
                // silently dropped content: with no header there is nothing to
                // dispatch, and a message with no headers at all is terminal
                // by definition.
                continue;
            };
            if current.path.is_some() {
                bail!(
                    "line {line_num}: '{}' tool call declared a second path — one call, one path",
                    current.kind
                );
            }
            let path = rest.trim().to_string();
            if path.is_empty() {
                bail!(
                    "line {line_num}: '{}' tool call declared an empty path",
                    current.kind
                );
            }
            if path.len() > 200 {
                bail!(
                    "line {line_num}: '{}' tool call declared an implausible {} character path",
                    current.kind,
                    path.len()
                );
            }
            current.path = Some(path);
        } else if trimmed == CONTENT_MARKER {
            let Some(current) = pending.as_mut() else {
                bail!(
                    "line {line_num}: '{CONTENT_MARKER}' appeared outside any tool call — a \
                     content block belongs to a '{TOOL_MARKER} write_file' header."
                );
            };
            if current.content.is_some() {
                bail!(
                    "line {line_num}: '{}' tool call opened a second '{CONTENT_MARKER}' block",
                    current.kind
                );
            }
            current.content = Some(Vec::new());
            in_content = true;
        } else if trimmed == END_CONTENT_MARKER {
            bail!(
                "line {line_num}: stray '{END_CONTENT_MARKER}' with no open content block — file \
                 content may not contain a line consisting solely of '{END_CONTENT_MARKER}'."
            );
        }
        // Anything else at top level is prose. The model is allowed to think
        // out loud around its tool calls.
    }

    // An unterminated content block swallowed every line after it. Returning
    // the calls collected before it would hand back a truncated file as if it
    // were whole — the single worst outcome this module can produce.
    if in_content {
        let path = pending
            .as_ref()
            .and_then(|p| p.path.clone())
            .unwrap_or_else(|| "<unknown>".to_string());
        bail!(
            "the '{CONTENT_MARKER}' block for {path:?} was never closed with \
             '{END_CONTENT_MARKER}' — the response was cut off, so the file content is \
             incomplete. Raise the token budget or write a smaller file."
        );
    }

    if let Some(previous) = pending.take() {
        calls.push(finish_pending(previous)?);
    }

    Ok(calls)
}

/// A call being accumulated by [`parse_tool_calls`]. `content` is `None` until
/// a `### CONTENT` marker opens one, which is what separates "write_file with
/// no content block" (an error) from "write_file whose content is empty" (a
/// legal request to create an empty file).
struct Pending {
    kind: String,
    line: usize,
    path: Option<String>,
    content: Option<Vec<String>>,
}

fn finish_pending(pending: Pending) -> Result<MasonTool> {
    let Pending {
        kind,
        line,
        path,
        content,
    } = pending;
    let Some(path) = path else {
        bail!("line {line}: '{kind}' tool call is missing its 'path:' line");
    };
    match kind.as_str() {
        "read_file" | "list_dir" => {
            if content.is_some() {
                bail!(
                    "line {line}: '{kind}' tool call carried a '{CONTENT_MARKER}' block, which \
                     only '{TOOL_MARKER} write_file' accepts — refusing rather than guessing \
                     which tool was meant."
                );
            }
            if kind == "read_file" {
                Ok(MasonTool::ReadFile { path })
            } else {
                Ok(MasonTool::ListDir { path })
            }
        }
        "write_file" => {
            let Some(body) = content else {
                bail!(
                    "line {line}: 'write_file' tool call for {path:?} has no '{CONTENT_MARKER}' \
                     block, so there is nothing to write. Emit the complete file contents \
                     between '{CONTENT_MARKER}' and '{END_CONTENT_MARKER}'."
                );
            };
            Ok(MasonTool::WriteFile {
                path,
                content: body.join("\n"),
            })
        }
        other => bail!(
            "line {line}: unknown tool {other:?} — the only tools are read_file, list_dir and \
             write_file. Refusing rather than ignoring the call, since ignoring it would look \
             exactly like the model finishing."
        ),
    }
}

/// Render one tool's result in the shape the model was told to expect.
pub fn render_tool_result(tool: &MasonTool, result: &str) -> String {
    format!(
        "### TOOL RESULT: {}\n{result}\n### END TOOL RESULT",
        tool.name()
    )
}

/// Recorded side effect of a proposed write.
///
/// The loop does not own the invocation gateway — that lives on `AppContext`,
/// which this module deliberately does not depend on so the parser stays
/// testable without bootstrapping a whole factory. The orchestrator implements
/// this trait over the same gateway host commands already go through, so tool
/// writes land in `tool_invocations.json` beside them.
#[async_trait::async_trait]
pub trait MasonToolRecorder: Send + Sync {
    /// Record a proposed write. `Ok(false)` means the gateway refused it, which
    /// ends the loop rather than dropping the write.
    async fn record_write(&self, path: &str, byte_len: usize) -> Result<bool>;
}

/// Run the loop with no invocation recording. Used by tests and by any caller
/// with no run context; production always goes through
/// [`run_mason_tool_loop_with_recorder`].
pub async fn run_mason_tool_loop(
    provider: &dyn LlmProvider,
    req: LlmRequest,
    staged: &Path,
    max_turns: u32,
) -> Result<Vec<(String, String)>> {
    run_mason_tool_loop_with_recorder(provider, req, staged, max_turns, None).await
}

/// Drive the model until it stops asking for tools, then hand back what it
/// wants written.
///
/// The returned pairs are `(path, content)` in the order the model settled on
/// them, one entry per distinct file: a later write to a path already written
/// *supersedes* the earlier one rather than colliding with it, because in a
/// sequential loop the second write is the model revising its own work, not two
/// competing edits in one batch. The supersession is reported back to the model
/// in the tool result and each write is recorded separately in the invocation
/// log, so nothing about it is silent — and the batch handed to the caller
/// still has one entry per path, which keeps `validate_mason_edits`'s
/// duplicate-path rejection meaningful for every other transport.
pub async fn run_mason_tool_loop_with_recorder(
    provider: &dyn LlmProvider,
    req: LlmRequest,
    staged: &Path,
    max_turns: u32,
    recorder: Option<&dyn MasonToolRecorder>,
) -> Result<Vec<(String, String)>> {
    let turns = max_turns.max(1);
    // Canonical form of the workspace root, for the "this write targets the
    // root itself" check below. `join_workspace_relative_path` canonicalizes
    // the base it returns, so the two are directly comparable.
    let staged_root = std::fs::canonicalize(staged).unwrap_or_else(|_| staged.to_path_buf());
    let mut messages = req.messages.clone();
    // Keyed by the *resolved* path so two spellings of one file
    // (`js/a.js`, `js/./a.js`) supersede each other here exactly as they would
    // collide on disk, rather than surviving as two entries.
    let mut writes: Vec<(PathBuf, String, String)> = Vec::new();
    let mut last_problem: Option<String> = None;

    // Said out loud, once per run, where an operator reading logs will meet it.
    // The residual is documented on `parse_tool_calls`, but a doc comment is
    // only visible to someone already reading the source of the thing they are
    // worried about. This lane decides it is finished by *not recognizing* a
    // write, so the operator should know that before trusting a clean result.
    tracing::warn!(
        turns,
        "Mason tool loop enabled. Termination is heuristic: the loop stops when a message \
         contains no tool call it recognizes and nothing it recognizes as a write in another \
         format. A write in a shape neither check knows (see the residual list on \
         mason_tools::parse_tool_calls) is read as the model finishing, and is dropped. Review \
         mason_edit_application.json against the spec rather than assuming a clean run wrote \
         everything asked for."
    );

    for _turn in 0..turns {
        let response = provider
            .complete(LlmRequest {
                messages: messages.clone(),
                max_tokens: req.max_tokens,
                temperature: req.temperature,
            })
            .await?;

        let calls = match parse_tool_calls(&response.content) {
            Ok(calls) => calls,
            Err(error) => {
                // Same contract as the single-shot retry lane: show the model
                // exactly how it failed and ask again. Nothing from this turn
                // is kept — `parse_tool_calls` is all-or-nothing — so no
                // half-read call can leak through.
                let problem = format!("{error:#}");
                messages.push(Message::assistant(response.content.clone()));
                messages.push(Message::user(format!(
                    "Your previous message could not be used: {problem}\n\n{TOOL_LOOP_INSTRUCTION}"
                )));
                last_problem = Some(problem);
                continue;
            }
        };

        // Checked on every turn, not only the terminal one, and before a single
        // call is executed. Two failures live here:
        //
        // - a mixed message, carrying one `write_file` call *and* a `### FILE:`
        //   block for a second file. Executing the call and ignoring the block
        //   applies one file, drops the other, and reports success.
        // - a terminal message that is not terminal at all — a JSON proposal, or
        //   `#### TOOL:` one hash off. Zero calls parse out of it, and without
        //   this check the loop reads it as the model signing off.
        //
        // Either way the whole message is rejected and re-asked.
        if let Some(problem) = unreadable_write_marker(&response.content) {
            messages.push(Message::assistant(response.content.clone()));
            messages.push(Message::user(format!(
                "Your previous message could not be used: {problem}\n\n{TOOL_LOOP_INSTRUCTION}"
            )));
            last_problem = Some(problem);
            continue;
        }

        if calls.is_empty() {
            // No tool calls, and nothing in the message this lane recognized as
            // an unreadable write. That is the best available evidence the model
            // is finished — not proof; see `parse_tool_calls` for the shapes
            // that still get through.
            return Ok(writes
                .into_iter()
                .map(|(_resolved, path, content)| (path, content))
                .collect());
        }

        let mut results = Vec::new();
        for tool in &calls {
            // Confinement first, for every tool, before anything is read,
            // listed or recorded. `join_workspace_relative_path` is the same
            // function the apply path uses: it rejects absolute paths and any
            // `..` component. A path that escapes ends the run rather than
            // returning an error the model could iterate against — a model
            // reaching outside its workspace has misunderstood its boundary,
            // and the operator should see that immediately.
            //
            // Fed the *normalized* path, because that is what the apply path
            // feeds it. Handing one implementation two different inputs defeats
            // the entire reason for sharing it: `js\..\..\x` has no `..`
            // component on Linux (backslashes are ordinary characters) and
            // sailed through here, only to be rejected at apply time — after
            // the model had been told "recorded write of N bytes" and a
            // `mason_tool_write` record claiming success had been written for a
            // write that could never land.
            //
            // Absolute paths are refused *before* normalizing, because
            // normalizing strips the leading `/` and would quietly reinterpret
            // `/etc/cron.d/evil` as a workspace-relative write. Confined, but
            // not what the model asked for, and not what it was told would
            // happen. Refusing is louder and keeps the instruction honest.
            //
            // Only paths absolute *on this platform* are caught. `\etc\passwd`
            // and `C:\windows\x` are not absolute on Linux, so they normalize to
            // `etc/passwd` and `C:/windows/x` and are read or written inside the
            // workspace — the same silent re-rooting this comment says it wanted
            // to avoid, for the spellings `Path` does not recognize. Harmless
            // (still confined, still scope-checked at apply time) and consistent
            // with every other transport, but it is re-rooting, not refusal.
            if Path::new(tool.path()).is_absolute() {
                bail!(
                    "Mason's '{}' tool call uses the absolute path {:?}. Tool paths must be \
                     relative to the staged workspace root.",
                    tool.name(),
                    tool.path()
                );
            }
            let normalized = crate::orchestrator::normalize_project_path(tool.path());
            let resolved = crate::orchestrator::join_workspace_relative_path(staged, &normalized)
                .map_err(|error| {
                anyhow::anyhow!(
                    "Mason's '{}' tool call for {:?} does not resolve inside the staged \
                         workspace: {error:#}",
                    tool.name(),
                    tool.path()
                )
            })?;

            // Computed once, for reads *and* listings. A listing is a read of
            // the names: `list_dir .ssh` returning `id_ecdsa` is still Mason
            // being shown what these rules promise it is never shown, and a
            // promise the code does not keep is worse than no promise at all.
            // Not an escape — the path is inside the workspace — so this is a
            // soft refusal the model can work around, not a boundary violation
            // that ends the run.
            let refusal = read_refusal(&normalized);

            let result = match tool {
                MasonTool::ReadFile { path } | MasonTool::ListDir { path } if refusal.is_some() => {
                    let reason = refusal.unwrap_or_default();
                    format!("ERROR: {} {path}: {reason}", tool.name())
                }
                MasonTool::ReadFile { path } => match std::fs::read_to_string(&resolved) {
                    Ok(text) => text,
                    // A missing file is information the model asked for, not a
                    // failure of the loop: it is allowed to probe for a file and
                    // learn it is not there.
                    Err(error) => format!("ERROR: reading {path}: {error}"),
                },
                MasonTool::ListDir { path } => match std::fs::read_dir(&resolved) {
                    Ok(entries) => {
                        let mut names = Vec::new();
                        for entry in entries {
                            // Soft, like the failure to open the directory at
                            // all. A single unreadable entry — a race with
                            // another process, a permission quirk — is not a
                            // reason to end a run that has real writes in it.
                            let entry = match entry {
                                Ok(entry) => entry,
                                Err(error) => {
                                    names.push(format!("(unreadable entry: {error})"));
                                    continue;
                                }
                            };
                            let suffix = if entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
                                "/"
                            } else {
                                ""
                            };
                            names.push(format!("{}{suffix}", entry.file_name().to_string_lossy()));
                        }
                        names.sort();
                        if names.is_empty() {
                            format!("(empty directory: {path})")
                        } else {
                            names.join("\n")
                        }
                    }
                    Err(error) => format!("ERROR: listing {path}: {error}"),
                },
                MasonTool::WriteFile { path, content } => {
                    // A write that resolves to the workspace root is not a
                    // write. `validate_mason_edits` rejects it at apply time
                    // (its normalized path is empty), so letting it through
                    // here would again promise the model a write that can never
                    // land, and log a successful invocation for it.
                    if resolved == staged_root {
                        bail!(
                            "Mason's write_file call for {path:?} resolves to the workspace root \
                             itself, which is a directory, not a file."
                        );
                    }
                    // Read and write must agree about what is off limits. A
                    // path Mason may not read is a path it may not blindly
                    // overwrite either — a whole-file write to a file it was
                    // never shown is a rewrite from nothing, and `.env` is
                    // exactly the file where that is worst. Loud, because a
                    // soft refusal here would let the model sign off believing
                    // the write landed.
                    if let Some(reason) = &refusal {
                        bail!(
                            "Mason's write_file call targets {path:?}, which it is not allowed to \
                             read: {reason} Refusing to overwrite a file whole that could not be \
                             read first."
                        );
                    }
                    if let Some(recorder) = recorder {
                        if !recorder.record_write(path, content.len()).await? {
                            bail!(
                                "the invocation gateway refused Mason's tool write to {path:?}. \
                                 No edits were returned."
                            );
                        }
                    }
                    let superseded = writes.iter().position(|(key, _, _)| key == &resolved);
                    match superseded {
                        Some(index) => {
                            let previous = writes[index].2.len();
                            writes[index] = (resolved.clone(), path.clone(), content.clone());
                            format!(
                                "recorded write of {} bytes to {path} (replaces the earlier {} \
                                 byte write to the same file in this loop)",
                                content.len(),
                                previous
                            )
                        }
                        None => {
                            writes.push((resolved.clone(), path.clone(), content.clone()));
                            format!("recorded write of {} bytes to {path}", content.len())
                        }
                    }
                }
            };
            results.push(render_tool_result(tool, &result));
        }

        messages.push(Message::assistant(response.content.clone()));
        messages.push(Message::user(results.join("\n\n")));
    }

    // Partial results are not success. The model asked for a tool on the last
    // turn it had, which means it did not consider itself finished, which means
    // whatever it had written so far is not the set of writes it intended.
    match last_problem {
        Some(problem) => bail!(
            "Mason used all {turns} tool turns without settling on a final set of writes. The \
             last turn was rejected: {problem}"
        ),
        None => bail!(
            "Mason used all {turns} tool turns without settling on a final set of writes — it \
             was still calling tools when the budget ran out. Raise the turn budget or narrow \
             the spec."
        ),
    }
}

/// Why a read is refused, or `None` if it is allowed.
///
/// Before this lane existed, Mason saw a harness-chosen, filtered set of at most
/// eight files (`build_mason_context_files` / `is_mason_context_candidate`).
/// The loop makes the selection *model-chosen*, which is the point — but it also
/// means a `read_file` on `.env` would ship `API_KEY=sk-live-…` verbatim into
/// the provider conversation, from a file no operator ever chose to share.
///
/// Three rules, applied to reads *and* listings — a directory listing is a read
/// of the names, and `list_dir .ssh` returning `id_ecdsa` is still Mason being
/// shown something these rules promise it is never shown:
///
/// - the same blocked prefixes the single-shot lanes apply, shared through
///   `orchestrator::mason_blocked_path_component` so the read filter and the
///   write boundary cannot drift apart. Matched on *any* path component, so
///   `target` (the directory itself) is refused, and so are `sub/.git/config`
///   and `a/target/x` — checking only the first component let both of those
///   through;
/// - credential directories, anywhere in the path. Directory rules age better
///   than filename rules: `id_rsa` was blocked while `.ssh/id_ecdsa` — one
///   letter different — was not;
/// - credential-bearing filenames, for the ones that sit loose in a project.
///
/// This is a filter, not a boundary: refusing is reported to the model as a
/// normal tool error and it can carry on. Nothing the model produced is
/// discarded, so the loud-failure invariant does not apply here. It is also a
/// *denylist*, and therefore incomplete by construction — a secret in a file
/// named nothing in particular is still readable. The disclosure on
/// `WorkerHarnessConfig::tool_loop` is what makes that the operator's decision
/// rather than a silent one.
///
/// Scope note, so the sharing claim above is not read as more than it is: only
/// the *blocked prefixes* are shared with the single-shot lanes. The credential
/// rules below — credential directories, key material by extension, `.netrc`
/// and friends — exist in this lane alone, because only this lane lets the
/// model choose what it reads. The single-shot lane's context files are picked
/// by the harness (`build_mason_context_files`), so there is nothing there for
/// these rules to filter.
fn read_refusal(normalized: &str) -> Option<String> {
    let components = Path::new(normalized)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect::<Vec<_>>();

    if let Some(prefix) = crate::orchestrator::mason_blocked_path_component(normalized) {
        return Some(format!(
            "{prefix} is excluded from Mason's reads — it holds build output, VCS \
             internals or factory state, not product source"
        ));
    }

    const CREDENTIAL_DIRS: [&str; 7] = [
        ".ssh", ".aws", ".gnupg", ".gpg", ".docker", ".kube", ".azure",
    ];
    for component in &components {
        if CREDENTIAL_DIRS.contains(&component.as_str()) {
            return Some(format!(
                "{component}/ holds credentials and is never shared with a model. Ask the \
                 operator for any value you need from it."
            ));
        }
    }

    let Some(file_name) = components.last() else {
        return None;
    };
    let extension = Path::new(file_name.as_str())
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = Path::new(file_name.as_str())
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let secret =
        // Any `*.env`, not only `.env` — `secrets.env` and `prod.env` are the
        // same file by another name. Plus `.env.production`, `.envrc`.
        file_name == ".env"
            || file_name.starts_with(".env")
            || extension == "env"
            // Key material, by extension.
            || matches!(
                extension.as_str(),
                "pem" | "key" | "p12" | "pfx" | "p8" | "jks" | "keystore" | "asc"
            )
            // Credential bundles, whatever they are serialized as — but only
            // when serialized as *data*. `src/secrets.rs`, `lib/secrets.ts` and
            // `docs/secrets.md` are source and documentation about secrets, not
            // secrets, and refusing them was actively harmful: reads are
            // filtered and writes are not, so Mason asked to modify
            // `src/secrets.rs` could not read it but could still overwrite it
            // whole — a blind rewrite of a file it was never allowed to see,
            // manufactured by the filter meant to protect it.
            || (matches!(stem.as_str(), "credentials" | "secrets" | "service-account")
                && matches!(
                    extension.as_str(),
                    "" | "json" | "yaml" | "yml" | "toml" | "ini" | "env" | "txt" | "properties"
                ))
            || matches!(
                file_name.as_str(),
                ".netrc" | ".npmrc" | ".pypirc" | ".htpasswd" | ".pgpass" | ".git-credentials"
            )
            // `id_rsa`, `id_ecdsa`, `id_ed25519` and their `.pub` siblings —
            // extensionless or `.pub` only, so `id_utils.py` is untouched.
            || (file_name.starts_with("id_") && (extension.is_empty() || extension == "pub"));
    if secret {
        return Some(format!(
            "{file_name} holds credentials and is never shared with a model. Ask the operator \
             for any value you need from it."
        ));
    }

    None
}

/// Detects a message that is trying to write files in a way this lane cannot
/// read — which is the difference between a model that has finished and a model
/// whose output was thrown away.
///
/// This is the guard that stands between "I could not read this" and "the model
/// is finished". It does **not** make termination positive — see the note at
/// the end of [`parse_tool_calls`], which enumerates the shapes still known to
/// slip through and be read as a sign-off. It only lengthens the list of
/// markers whose *absence* is required, which is a narrower claim and the only
/// one this function can support. `parse_tool_calls` returning zero calls means
/// no exact `### TOOL:` header was found, and there are far more ways to miss
/// that header than to hit it:
///
/// - another live Mason transport (`### FILE:`, `### PATCH:`, a JSON edit
///   proposal) — JSON is the likeliest, since it is still a supported transport
///   and several providers revert to it under pressure;
/// - the right vocabulary one character off — `#### TOOL:` (four hashes),
///   `### Tool:` (case), `## CONTENT`. This is the sharpest case: the model used
///   exactly the vocabulary it was taught, and without this check its write is
///   read as it signing off.
///
/// Every one of those parses to zero calls. Treating that as "finished" drops
/// the write and reports the run applied. So a near-miss marker rejects the
/// message and re-asks instead.
///
/// Block-aware on purpose, and exact markers are explicitly allowed through. A
/// `### FILE:` line inside a `write_file` content block is not a foreign
/// transport — it is the file's own text, and this repo contains several files
/// that legitimately document those markers. A naive substring scan would
/// reject such a write forever, on every retry, which is the mistake
/// `has_top_level_patch_header` was fixed for in the patch lane.
///
/// Only called after `parse_tool_calls` has already succeeded, so the block
/// structure is known to be well-formed and this toggle cannot desynchronize
/// from the parser's.
fn unreadable_write_marker(raw: &str) -> Option<String> {
    let mut in_content = false;
    for (index, line) in raw.split('\n').enumerate() {
        let trimmed = line.trim_end_matches('\r').trim();
        if in_content {
            if trimmed == END_CONTENT_MARKER {
                in_content = false;
            }
            continue;
        }
        if trimmed == CONTENT_MARKER {
            in_content = true;
            continue;
        }
        // The exact markers this lane *does* read are fine — they are why the
        // message parsed. Only near-misses and foreign transports get here.
        if trimmed.starts_with(TOOL_MARKER) || trimmed == END_CONTENT_MARKER {
            continue;
        }
        if let Some(word) = near_miss_marker_word(trimmed) {
            return Some(format!(
                "line {}: {trimmed:?} looks like a '{word}' marker but is not one — this lane \
                 reads only '{TOOL_MARKER}', '{CONTENT_MARKER}' and '{END_CONTENT_MARKER}', \
                 spelled exactly, with three hashes and in upper case",
                index + 1
            ));
        }
        if looks_like_json_edit_proposal(trimmed) {
            return Some(format!(
                "line {}: {trimmed:?} looks like a JSON edit proposal. That transport is not \
                 read in the tool loop — every write must be a '{TOOL_MARKER} write_file' call",
                index + 1
            ));
        }
    }
    None
}

/// A line that is structurally part of a JSON edit proposal — still a live
/// Mason transport, and the one providers revert to most readily.
///
/// Same asymmetry as [`near_miss_marker_word`], and the reason this needs two
/// pieces of evidence on one line rather than one: a single key name appears in
/// ordinary prose all the time. `Done. I set the "path": key in the manifest.`
/// is a perfectly good sign-off, and rejecting it deterministically costs the
/// whole run. So a match requires a key *and* the punctuation that can only
/// come from the real document:
///
/// - `"edits"` together with the `[` that opens its array, or
/// - `"path":` together with the `"content":` that always accompanies it in an
///   edit object.
fn looks_like_json_edit_proposal(trimmed: &str) -> bool {
    (trimmed.contains("\"edits\"") && trimmed.contains('['))
        || (trimmed.contains("\"path\":") && trimmed.contains("\"content\":"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct ScriptedProvider {
        responses: Mutex<Vec<String>>,
        seen: Mutex<Vec<Vec<Message>>>,
    }

    impl ScriptedProvider {
        fn new(responses: &[&str]) -> Self {
            Self {
                responses: Mutex::new(responses.iter().map(|r| r.to_string()).collect()),
                seen: Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> usize {
            self.seen.lock().expect("lock").len()
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn complete(&self, req: LlmRequest) -> Result<crate::llm::LlmResponse> {
            self.seen.lock().expect("lock").push(req.messages.clone());
            let mut responses = self.responses.lock().expect("lock");
            let content = if responses.is_empty() {
                // A model that never stops asking for tools.
                "### TOOL: list_dir\npath: .\n".to_string()
            } else {
                responses.remove(0)
            };
            Ok(crate::llm::LlmResponse {
                content,
                usage: None,
            })
        }
    }

    fn request() -> LlmRequest {
        LlmRequest {
            messages: vec![Message::user("do the work")],
            max_tokens: 1000,
            temperature: 0.1,
        }
    }

    #[test]
    fn tool_calls_parse_from_the_wire_format() {
        assert_eq!(
            parse_tool_call("### TOOL: read_file\npath: js/data.js\n"),
            Some(MasonTool::ReadFile {
                path: "js/data.js".to_string()
            })
        );
        assert_eq!(
            parse_tool_call("### TOOL: list_dir\npath: js\n"),
            Some(MasonTool::ListDir {
                path: "js".to_string()
            })
        );
        assert_eq!(parse_tool_call("SUMMARY: done\n"), None);
    }

    #[test]
    fn write_file_tool_carries_its_content_block() {
        let raw = "### TOOL: write_file\npath: js/bonus.js\n### CONTENT\nG.rooms.bonus = {};\n### END CONTENT\n";
        let Some(MasonTool::WriteFile { path, content }) = parse_tool_call(raw) else {
            panic!("expected a write_file call");
        };
        assert_eq!(path, "js/bonus.js");
        assert_eq!(content.trim(), "G.rooms.bonus = {};");
    }

    #[test]
    fn a_message_with_no_tool_header_is_terminal_not_an_error() {
        let calls = parse_tool_calls("SUMMARY: done\nI wrote everything already.\n")
            .expect("prose is not an error");
        assert!(calls.is_empty(), "no header means the model is finished");
    }

    #[test]
    fn several_tool_calls_in_one_message_all_survive() {
        let raw = "\
### TOOL: read_file
path: js/data.js
### TOOL: write_file
path: js/a.js
### CONTENT
a
### END CONTENT
### TOOL: list_dir
path: js
";
        let calls = parse_tool_calls(raw).expect("three calls must parse");
        assert_eq!(
            calls,
            vec![
                MasonTool::ReadFile {
                    path: "js/data.js".to_string()
                },
                MasonTool::WriteFile {
                    path: "js/a.js".to_string(),
                    content: "a".to_string()
                },
                MasonTool::ListDir {
                    path: "js".to_string()
                },
            ],
            "a second call must not overwrite the first"
        );
    }

    #[test]
    fn an_unknown_tool_name_is_refused_rather_than_read_as_finishing() {
        let error = parse_tool_calls("### TOOL: delete_file\npath: js/a.js\n")
            .expect_err("an unknown tool must not parse");
        let message = format!("{error:#}");
        assert!(
            message.contains("delete_file"),
            "the error must name the tool: {message}"
        );
    }

    #[test]
    fn a_tool_call_without_a_path_is_refused() {
        let error =
            parse_tool_calls("### TOOL: read_file\nI forgot the path\n").expect_err("no path");
        assert!(format!("{error:#}").contains("missing its 'path:' line"));
    }

    #[test]
    fn a_write_without_a_content_block_is_refused() {
        let error =
            parse_tool_calls("### TOOL: write_file\npath: js/a.js\n").expect_err("no content");
        assert!(format!("{error:#}").contains("nothing to write"));
    }

    #[test]
    fn an_empty_content_block_writes_an_empty_file() {
        let call =
            parse_tool_call("### TOOL: write_file\npath: js/a.js\n### CONTENT\n### END CONTENT\n")
                .expect("an empty file is a legal write");
        assert_eq!(
            call,
            MasonTool::WriteFile {
                path: "js/a.js".to_string(),
                content: String::new()
            }
        );
    }

    #[test]
    fn an_unclosed_content_block_is_refused_rather_than_truncated() {
        let raw = "### TOOL: write_file\npath: js/a.js\n### CONTENT\nline one\nline two\n";
        let error = parse_tool_calls(raw).expect_err("a cut-off write must not parse");
        let message = format!("{error:#}");
        assert!(
            message.contains("never closed"),
            "the error must say the block never closed: {message}"
        );
    }

    #[test]
    fn a_tool_header_inside_an_open_content_block_is_refused() {
        // The failure this guard exists for: without it, the second call and
        // every line after it is swallowed into the first file's content, and
        // the loop reports success having written a corrupted file and dropped
        // a write entirely.
        let raw = "\
### TOOL: write_file
path: js/a.js
### CONTENT
a
### TOOL: write_file
path: js/b.js
### CONTENT
b
### END CONTENT
";
        let error = parse_tool_calls(raw).expect_err("a missing closer must not parse");
        assert!(format!("{error:#}").contains("still open"));
    }

    #[test]
    fn a_stray_end_content_marker_is_refused() {
        let error = parse_tool_calls("### TOOL: read_file\npath: js/a.js\n### END CONTENT\n")
            .expect_err("a stray closer must not parse");
        assert!(format!("{error:#}").contains("stray"));
    }

    #[test]
    fn a_content_block_on_a_read_call_is_refused() {
        let raw = "### TOOL: read_file\npath: js/a.js\n### CONTENT\nx\n### END CONTENT\n";
        let error = parse_tool_calls(raw).expect_err("read_file takes no content");
        assert!(format!("{error:#}").contains("only '### TOOL: write_file' accepts"));
    }

    #[test]
    fn tool_calls_and_prose_coexist() {
        let raw = "\
Let me look at the data file first.

### TOOL: read_file
path: js/data.js

I will decide what to write once I see it.
";
        assert_eq!(
            parse_tool_calls(raw).expect("prose around a call is fine"),
            vec![MasonTool::ReadFile {
                path: "js/data.js".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn the_loop_reads_then_writes_and_returns_the_pairs() {
        let staged = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(staged.path().join("js")).expect("mkdir");
        std::fs::write(staged.path().join("js/data.js"), "G.rooms = {};").expect("write");

        let provider = ScriptedProvider::new(&[
            "### TOOL: read_file\npath: js/data.js\n",
            "### TOOL: write_file\npath: js/bonus.js\n### CONTENT\nG.rooms.bonus = {};\n### END CONTENT\n",
            "SUMMARY: added the bonus room\n",
        ]);

        let writes = run_mason_tool_loop(&provider, request(), staged.path(), 6)
            .await
            .expect("the loop must finish");

        assert_eq!(
            writes,
            vec![("js/bonus.js".to_string(), "G.rooms.bonus = {};".to_string())]
        );
        assert_eq!(provider.calls(), 3);

        // The write is returned, never performed: the caller owns the one apply
        // path, and a second one is how a safety check gets forgotten.
        assert!(
            !staged.path().join("js/bonus.js").exists(),
            "the loop must not touch the workspace"
        );

        // The file it read must have reached it.
        let seen = provider.seen.lock().expect("lock");
        let second_turn = &seen[1];
        assert!(
            second_turn
                .iter()
                .any(|m| m.content.contains("G.rooms = {};")),
            "the read result must be fed back to the model"
        );
    }

    #[tokio::test]
    async fn a_path_escaping_the_workspace_ends_the_loop() {
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&["### TOOL: read_file\npath: ../../etc/passwd\n"]);

        let error = run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect_err("an escaping path must end the run");
        let message = format!("{error:#}");
        assert!(
            message.contains("staged") && message.contains("passwd"),
            "the error must name the offending path: {message}"
        );
    }

    #[tokio::test]
    async fn an_absolute_path_is_refused_too() {
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: /etc/cron.d/evil\n### CONTENT\nboom\n### END CONTENT\n",
        ]);

        let error = run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect_err("an absolute path must end the run");
        assert!(format!("{error:#}").contains("absolute"));
    }

    #[tokio::test]
    async fn a_write_that_escapes_is_refused_before_it_is_recorded() {
        struct CountingRecorder {
            writes: Mutex<Vec<String>>,
        }
        #[async_trait::async_trait]
        impl MasonToolRecorder for CountingRecorder {
            async fn record_write(&self, path: &str, _byte_len: usize) -> Result<bool> {
                self.writes.lock().expect("lock").push(path.to_string());
                Ok(true)
            }
        }

        let staged = tempfile::tempdir().expect("tempdir");
        let recorder = CountingRecorder {
            writes: Mutex::new(Vec::new()),
        };
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: ../escape.js\n### CONTENT\nx\n### END CONTENT\n",
        ]);

        run_mason_tool_loop_with_recorder(&provider, request(), staged.path(), 4, Some(&recorder))
            .await
            .expect_err("an escaping write must end the run");
        assert!(
            recorder.writes.lock().expect("lock").is_empty(),
            "confinement must run before the gateway, not after"
        );
    }

    #[tokio::test]
    async fn a_model_that_never_finishes_fails_rather_than_returning_partial_writes() {
        let staged = tempfile::tempdir().expect("tempdir");
        // One real write, then an endless list_dir (the scripted provider's
        // fallback), so there *is* a partial result available to return.
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\na\n### END CONTENT\n",
        ]);

        let error = run_mason_tool_loop(&provider, request(), staged.path(), 3)
            .await
            .expect_err("an unfinished loop must not report success");
        let message = format!("{error:#}");
        assert!(
            message.contains("without settling on a final set of writes"),
            "the exhaustion message must say what went wrong: {message}"
        );
        assert_eq!(provider.calls(), 3, "the turn budget must be honoured");
    }

    #[tokio::test]
    async fn a_malformed_tool_call_is_fed_back_and_the_loop_recovers() {
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            // Unknown tool: the failure that, if parsed as `None`, would look
            // exactly like the model finishing with no writes at all.
            "### TOOL: delete_file\npath: js/a.js\n",
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\na\n### END CONTENT\n",
            "SUMMARY: done\n",
        ]);

        let writes = run_mason_tool_loop(&provider, request(), staged.path(), 5)
            .await
            .expect("the loop must recover after the correction");
        assert_eq!(writes, vec![("js/a.js".to_string(), "a".to_string())]);

        let seen = provider.seen.lock().expect("lock");
        assert!(
            seen[1]
                .iter()
                .any(|m| m.content.contains("could not be used")
                    && m.content.contains("delete_file")),
            "the model must be told exactly what it got wrong"
        );
    }

    #[tokio::test]
    async fn a_file_block_in_the_final_message_is_not_read_as_finishing() {
        // The silent-skip this guard exists for: the model reverts to the
        // single-shot transport, the loop sees no tool call, and every file in
        // those blocks is dropped while the run reports success.
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "SUMMARY: done\n### FILE: js/a.js\nvar a = 1;\n### END FILE\n",
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\nvar a = 1;\n### END CONTENT\n",
            "SUMMARY: done\n",
        ]);

        let writes = run_mason_tool_loop(&provider, request(), staged.path(), 5)
            .await
            .expect("the loop must recover");
        assert_eq!(
            writes,
            vec![("js/a.js".to_string(), "var a = 1;".to_string())]
        );
    }

    #[tokio::test]
    async fn a_file_block_beside_a_tool_call_rejects_the_whole_message() {
        // The nastier half of the same failure: the model emits one write_file
        // call *and* a `### FILE:` block for a second file. Executing the call
        // and ignoring the block would apply one file, drop the other, and
        // report success — the operator would never learn the second file was
        // never written.
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\na\n### END CONTENT\n\
             ### FILE: js/b.js\nb\n### END FILE\n",
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\na\n### END CONTENT\n\
             ### TOOL: write_file\npath: js/b.js\n### CONTENT\nb\n### END CONTENT\n",
            "SUMMARY: done\n",
        ]);

        let writes = run_mason_tool_loop(&provider, request(), staged.path(), 5)
            .await
            .expect("the loop must recover");
        assert_eq!(
            writes,
            vec![
                ("js/a.js".to_string(), "a".to_string()),
                ("js/b.js".to_string(), "b".to_string()),
            ],
            "the mixed message must be re-asked, not half-applied"
        );
    }

    /// Every one of these returned `Ok([("js/a.js", "a")])` with the second
    /// file silently gone, and the run reporting `applied`, when termination was
    /// merely "no `### TOOL:` header, and no exact `### FILE:`/`### PATCH:`".
    /// The `#### TOOL:` case is the sharpest: the model uses the exact
    /// vocabulary it was taught, one character off, and its write is read as it
    /// signing off. The JSON case is the likeliest, since JSON is still a live
    /// Mason transport several providers revert to.
    #[tokio::test]
    async fn a_terminal_message_carrying_an_unreadable_write_is_never_read_as_finishing() {
        let cases: &[(&str, &str)] = &[
            (
                "json edit proposal",
                "{\"summary\":\"s\",\"edits\":[{\"path\":\"js/b.js\",\"content\":\"b\"}]}",
            ),
            ("four-hash FILE", "#### FILE: js/b.js\nb\n#### END FILE"),
            ("wrong-case FILE", "### File: js/b.js\nb\n### End File"),
            (
                "four-hash TOOL",
                "#### TOOL: write_file\npath: js/b.js\n#### CONTENT\nb\n#### END CONTENT",
            ),
            (
                "wrong-case TOOL",
                "### Tool: write_file\npath: js/b.js\n### Content\nb\n### End Content",
            ),
            ("two-hash PATCH", "## PATCH: js/b.js\n<<<<<<< SEARCH"),
        ];

        for (label, terminal) in cases {
            let staged = tempfile::tempdir().expect("tempdir");
            let provider = ScriptedProvider::new(&[
                "### TOOL: write_file\npath: js/a.js\n### CONTENT\na\n### END CONTENT\n",
                terminal,
                "### TOOL: write_file\npath: js/b.js\n### CONTENT\nb\n### END CONTENT\n",
                "SUMMARY: done\n",
            ]);

            let writes = run_mason_tool_loop(&provider, request(), staged.path(), 6)
                .await
                .unwrap_or_else(|error| panic!("{label}: loop must recover: {error:#}"));
            assert_eq!(
                writes,
                vec![
                    ("js/a.js".to_string(), "a".to_string()),
                    ("js/b.js".to_string(), "b".to_string()),
                ],
                "{label}: the unreadable write must be re-asked, not read as finishing"
            );
        }
    }

    /// The guard's *edge*, not its coverage.
    ///
    /// The first version of this test picked only strings from the passing side
    /// of the boundary (`Files`, `Patching`, `Toolchain` — all suffix cases),
    /// which documented what the fix handled and gave false assurance about the
    /// exact risk it exists to guard. Every case below is one the shipped guard
    /// actually rejected, each of which cost a whole run: the rejection is
    /// deterministic, so the model re-emits the same sign-off, the guard refuses
    /// it identically, the budget drains and the loop bails discarding writes it
    /// had already collected.
    #[test]
    fn realistic_sign_offs_are_not_read_as_unreadable_writes() {
        for terminal in [
            // Marker word + space + prose: how markdown headings actually read.
            // A word boundary alone does not catch these; the boundary IS the
            // space.
            "## Patch notes\n\nAdded the bonus room; no patches were needed.\n",
            "## File changes\n\n- js/a.js\n",
            "## Content overview\n\nThe room now has a lamp.\n",
            "## Tool usage\n\nI read two files.\n",
            "### End of summary\n",
            // A single JSON key in prose is prose.
            "Done. I set the \"path\": key in the manifest.\n",
            "The spec's \"edits\" array is unchanged.\n",
            "I used \"content\": as the field name.\n",
            // The bare marker word as a whole heading — round 3's residual,
            // found by enumerating the accepting set rather than by listing
            // more prose.
            "## File\n\nOnly js/a.js changed.\n",
            "## Patch\n\nNone needed.\n",
            "## Tool\n\nread_file, twice.\n",
            "## Content\n\nThe room now has a lamp.\n",
            "#### CONTENT\n",
            "## End content\n",
            "### END FILE\n",
            // Suffix cases, kept from the first version.
            "## Files changed\n\n- js/bonus.js\n",
            "### Patching notes\n\nNone needed.\n",
            "#### Toolchain\n\nNo change.\n",
            "### Contents of the room\n\nA lamp.\n",
            // Plain sign-offs.
            "SUMMARY: done\n",
            "I have written both files. Nothing else is needed.\n",
            "## Summary\n\nAdded the bonus room and wired it up.\n",
            "### Notes\n\n- the room id is `bonus`\n",
        ] {
            assert_eq!(
                unreadable_write_marker(terminal),
                None,
                "a realistic sign-off must terminate, not cost the run: {terminal:?}"
            );
        }
    }

    /// The other half of the same boundary: tightening the matcher must not have
    /// let the Critical back in.
    #[test]
    fn genuine_unreadable_writes_are_still_caught() {
        for raw in [
            "#### FILE: js/b.js\nb\n#### END FILE\n",
            "### File: js/b.js\nb\n",
            "#### TOOL: write_file\npath: js/b.js\n",
            "### Tool: write_file\n",
            "## PATCH: js/b.js\n",
            // Bare content markers are caught by the header beside them, which
            // is the only thing that makes them mean a write.
            "#### FILE: js/b.js\nb\n#### CONTENT\nb\n#### END CONTENT\n",
            "{\"summary\":\"s\",\"edits\":[{\"path\":\"js/b.js\",\"content\":\"b\"}]}\n",
            "  \"edits\": [\n",
            "    {\"path\": \"js/b.js\", \"content\": \"b\"}\n",
        ] {
            assert!(
                unreadable_write_marker(raw).is_some(),
                "must still be caught: {raw:?}"
            );
        }
    }

    /// Built the way the previous three boundary tests were not.
    ///
    /// Rounds 1-3 each drew the boundary set from the *passing* side: list some
    /// prose, check it passes. Each set reached exactly as far as that round's
    /// fix and stopped, so each round shipped a new false positive that cost a
    /// run — the suffix case, then word-plus-prose, then the bare word. The
    /// method was the defect, not the strings.
    ///
    /// So this enumerates what the matcher *accepts* and asks, of each, whether
    /// a model could write it while finished. `unreadable_write_marker` accepts
    /// exactly two families:
    ///
    /// A. 2-6 `#`, optional space, then TOOL|FILE|PATCH|END FILE, then `:`
    /// B. one line holding (`"edits"` and `[`) or (`"path":` and `"content":`)
    ///
    /// Family A's accepting set is `{2..6 hashes} x {4 words} x {any case} x
    /// {anything after the colon}`. The judgement below covers each axis; the
    /// exhaustive hash/case sweep is mechanical and done in the loop.
    #[test]
    fn the_accepting_set_is_enumerated_and_each_shape_judged() {
        // Axis 1 and 2: hash count and case. Every combination is a marker
        // spelling, none is prose — a heading does not end in a bare keyword
        // plus colon by accident.
        for hashes in ["##", "###", "####", "#####", "######"] {
            for word in ["TOOL", "tool", "Tool", "FILE", "file", "PATCH", "END FILE"] {
                let line = format!("{hashes} {word}: js/b.js");
                // Three hashes plus the exact uppercase marker is this lane's
                // own vocabulary, not a near-miss, and is handled before the
                // matcher ever runs.
                let is_this_lanes_own = hashes == "###" && word == "TOOL";
                assert_eq!(
                    unreadable_write_marker(&line).is_some(),
                    !is_this_lanes_own,
                    "unexpected verdict for {line:?}"
                );
            }
        }

        // Axis 3: what follows the colon. All of these are writes.
        for line in [
            "## FILE:js/b.js",
            "##FILE: js/b.js",
            "##   file:   js/b.js",
            "#### TOOL:",
        ] {
            assert!(
                unreadable_write_marker(line).is_some(),
                "must be caught: {line:?}"
            );
        }

        // The accepting set's genuinely ambiguous members. Each is prose a model
        // could plausibly write, and each is rejected anyway because it is
        // shape-identical to a real marker. Rejecting them costs the run, so
        // they are listed here as known, accepted losses rather than left to be
        // discovered as a fourth round's Critical.
        for ambiguous in [
            "## Patch: none needed",
            "## File: js/a.js is the only one I touched",
            "## Tool: none used",
            "I set both \"path\": and \"content\": in the manifest.",
            "The \"edits\" list is [empty].",
        ] {
            assert!(
                unreadable_write_marker(ambiguous).is_some(),
                "documenting a known false positive; if this now passes, that is an \
                 improvement — update the list: {ambiguous:?}"
            );
        }

        // Family B needs both halves on one line. One half is prose.
        for allowed in [
            "The \"edits\" array is unchanged.",
            "I set the \"path\": key.",
            "Its \"content\": field is a string.",
            "edits: [",
            "The array [1, 2] holds ids.",
        ] {
            assert_eq!(
                unreadable_write_marker(allowed),
                None,
                "one half of the shape is prose: {allowed:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_realistic_sign_off_after_a_write_keeps_the_write() {
        // End-to-end proof of the Critical, on the shape that caused it: one
        // real write, then a summary with an ordinary heading. Before the fix
        // this failed the run and discarded the write.
        for sign_off in [
            "## Patch notes\n\nAdded the bonus room; no patches were needed.\n",
            "## File changes\n\n- js/a.js\n",
            "Done. I set the \"path\": key in the manifest.\n",
            "## Content overview\n\nThe room now has a lamp.\n",
            // Round 3's residual, at run level.
            "## File\n\nOnly js/a.js changed.\n",
            "## Patch\n\nNone needed.\n",
            "## Tool\n\nread_file, twice.\n",
            "## Content\n\nThe room now has a lamp.\n",
        ] {
            let staged = tempfile::tempdir().expect("tempdir");
            let provider = ScriptedProvider::new(&[
                "### TOOL: write_file\npath: js/a.js\n### CONTENT\na\n### END CONTENT\n",
                sign_off,
            ]);

            let writes = run_mason_tool_loop(&provider, request(), staged.path(), 12)
                .await
                .unwrap_or_else(|error| panic!("{sign_off:?} must end the loop: {error:#}"));
            assert_eq!(
                writes,
                vec![("js/a.js".to_string(), "a".to_string())],
                "the write must survive the sign-off: {sign_off:?}"
            );
            assert_eq!(provider.calls(), 2, "no re-ask: {sign_off:?}");
        }
    }

    #[tokio::test]
    async fn an_ordinary_heading_beside_a_real_tool_call_does_not_reject_the_turn() {
        // The guard fires mid-loop too, so this lane's stated allowance for the
        // model to think out loud around its calls was conditional on avoiding
        // common headings.
        let staged = tempfile::tempdir().expect("tempdir");
        std::fs::write(staged.path().join("data.js"), "G = {};").expect("write");
        let provider = ScriptedProvider::new(&[
            "## Patch notes\n\nLet me look first.\n\n### TOOL: read_file\npath: data.js\n",
            "SUMMARY: done\n",
        ]);

        run_mason_tool_loop(&provider, request(), staged.path(), 6)
            .await
            .expect("prose around a call must not reject the turn");
        let seen = provider.seen.lock().expect("lock");
        assert!(
            seen[1].iter().any(|m| m.content.contains("G = {};")),
            "the read must actually have happened"
        );
    }

    #[test]
    fn a_near_miss_marker_inside_written_content_is_not_a_near_miss() {
        // Block-awareness has to survive the looser matcher too, or Mason can
        // never write a file that documents these formats — this repo's own
        // `mason_transport.rs` being the obvious example.
        let raw = "### TOOL: write_file\npath: docs/f.md\n### CONTENT\n\
                   #### TOOL: write_file\n#### FILE: x\n{\"edits\":[]}\n### END CONTENT\n";
        assert_eq!(unreadable_write_marker(raw), None);
    }

    #[test]
    fn an_indented_end_content_marker_closes_the_block_and_the_instruction_says_so() {
        // The parser trims before comparing, symmetrically with
        // `collect_fenced_edits`, so a markdown-indented marker DOES end the
        // write and everything after it is lost. That behaviour is deliberate
        // and shared; the fix is that the model is now warned about it rather
        // than left to discover it by losing a file.
        let raw =
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\nkept\n    ### END CONTENT\nlost\n";
        let call = parse_tool_call(raw).expect("the indented marker closes the block");
        assert_eq!(
            call,
            MasonTool::WriteFile {
                path: "js/a.js".to_string(),
                content: "kept".to_string()
            },
            "indentation does not exempt a marker — pinning the parser's actual behaviour"
        );

        assert!(
            TOOL_LOOP_INSTRUCTION.contains("Indenting such a line does NOT make it safe"),
            "the instruction must warn about the indented case, since the parser cannot"
        );
        assert!(
            TOOL_LOOP_INSTRUCTION.contains("only non-whitespace text"),
            "the instruction must describe the trim-then-compare rule, not 'consists solely of'"
        );
    }

    #[tokio::test]
    async fn credential_files_are_refused_and_the_refusal_is_told_to_the_model() {
        let staged = tempfile::tempdir().expect("tempdir");
        std::fs::write(staged.path().join(".env"), "API_KEY=sk-live-xyz").expect("write");
        std::fs::create_dir_all(staged.path().join("target")).expect("mkdir");
        std::fs::write(staged.path().join("target/build.log"), "noise").expect("write");

        let provider = ScriptedProvider::new(&[
            "### TOOL: read_file\npath: .env\n",
            "### TOOL: read_file\npath: target/build.log\n",
            "SUMMARY: nothing to do\n",
        ]);

        run_mason_tool_loop(&provider, request(), staged.path(), 5)
            .await
            .expect("a refused read is not a failure");

        let seen = provider.seen.lock().expect("lock");
        let after_env = seen[1]
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        assert!(
            after_env.contains("credentials") && !after_env.contains("sk-live-xyz"),
            "the secret must never reach the conversation: {after_env}"
        );
        let after_target = seen[2]
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        assert!(
            after_target.contains("excluded from Mason's reads"),
            "the shared blocked-prefix list must apply to reads too: {after_target}"
        );
    }

    #[test]
    fn ordinary_source_reads_are_not_refused() {
        for allowed in [
            "js/data.js",
            "src/lib.rs",
            "environment/setup.md",
            "docs/id_generation.md",
            "src/id_utils.py",
            "keys.md",
            "src/monkey.rs",
            // Source and documentation *about* secrets is not a secret. And
            // refusing it was worse than a nuisance: reads are filtered and
            // writes were not, so Mason could not read `src/secrets.rs` but
            // could still overwrite it whole.
            "src/secrets.rs",
            "src/credentials.rs",
            "lib/secrets.ts",
            "docs/secrets.md",
            "service-account.go",
        ] {
            assert_eq!(read_refusal(allowed), None, "must be readable: {allowed}");
        }
    }

    #[test]
    fn credential_paths_are_refused_including_the_gaps_the_filename_list_left() {
        for refused in [
            // Was blocked before.
            ".env",
            ".env.local",
            "certs/server.pem",
            "node_modules/left-pad/index.js",
            "id_rsa",
            // Was NOT blocked: one letter from `id_rsa`.
            ".ssh/id_ecdsa",
            ".ssh/id_ed25519.pub",
            ".aws/credentials",
            ".gnupg/secring.gpg",
            ".kube/config",
            // Was NOT blocked: `.env` by another name.
            "secrets.env",
            "prod.env",
            ".envrc",
            // Was NOT blocked: credential bundles by serialization.
            "config/credentials.yaml",
            "secrets.yaml",
            "service-account.json",
            ".git-credentials",
            // Directories themselves, not only files inside them — the prefix
            // rule used to need a trailing slash to match.
            "target",
            ".git",
        ] {
            assert!(
                read_refusal(refused).is_some(),
                "must be refused: {refused}"
            );
        }
    }

    #[tokio::test]
    async fn a_write_to_a_path_that_cannot_be_read_is_refused() {
        // Read and write must agree. A whole-file write to a file Mason was
        // never allowed to see is a rewrite from nothing.
        let staged = tempfile::tempdir().expect("tempdir");
        std::fs::write(staged.path().join(".env"), "API_KEY=sk-live-xyz").expect("write");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: .env\n### CONTENT\nAPI_KEY=\n### END CONTENT\n",
        ]);

        let error = run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect_err("a blind overwrite of an unreadable file must be refused");
        assert!(format!("{error:#}").contains("not allowed to read"));
        assert_eq!(
            std::fs::read_to_string(staged.path().join(".env")).expect("read"),
            "API_KEY=sk-live-xyz",
            "and nothing may have touched it"
        );
    }

    #[tokio::test]
    async fn list_dir_is_filtered_by_the_same_rules_as_read_file() {
        let staged = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(staged.path().join(".ssh")).expect("mkdir");
        std::fs::write(staged.path().join(".ssh/id_ecdsa"), "PRIVATE").expect("write");
        std::fs::create_dir_all(staged.path().join("target")).expect("mkdir");
        std::fs::write(staged.path().join("target/build.log"), "noise").expect("write");

        let provider = ScriptedProvider::new(&[
            "### TOOL: list_dir\npath: .ssh\n",
            "### TOOL: list_dir\npath: target\n",
            "SUMMARY: done\n",
        ]);

        run_mason_tool_loop(&provider, request(), staged.path(), 6)
            .await
            .expect("a refused listing is not a failure");

        let seen = provider.seen.lock().expect("lock");
        let after_ssh = seen[1]
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        assert!(
            !after_ssh.contains("id_ecdsa") && after_ssh.contains("credentials"),
            "a listing is a read of the names: {after_ssh}"
        );
        let after_target = seen[2]
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        assert!(
            !after_target.contains("build.log"),
            "the shared prefix list must cover the directory itself: {after_target}"
        );
    }

    #[tokio::test]
    async fn a_backslash_escape_is_refused_by_the_loop_not_by_the_apply_path() {
        // Backslashes are ordinary characters on Linux, so `js\..\..\x` has no
        // `Component::ParentDir` and sailed through confinement — until the
        // apply path normalized it to `js/../../x` and rejected it, long after
        // the model had been told the write was recorded and a successful
        // invocation had been logged for it.
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: js\\..\\..\\x\n### CONTENT\nx\n### END CONTENT\n",
        ]);

        let error = run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect_err("the loop must refuse what the apply path would refuse");
        assert!(format!("{error:#}").contains("does not resolve inside the staged workspace"));
    }

    #[tokio::test]
    async fn a_write_to_the_workspace_root_is_refused() {
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: .\n### CONTENT\nx\n### END CONTENT\n",
        ]);

        let error = run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect_err("a write with no filename can never land");
        assert!(format!("{error:#}").contains("workspace root"));
    }

    #[tokio::test]
    async fn listing_the_workspace_root_is_still_allowed() {
        // The root check is scoped to writes on purpose: `list_dir .` is the
        // loop's most useful first move.
        let staged = tempfile::tempdir().expect("tempdir");
        std::fs::write(staged.path().join("index.html"), "x").expect("write");
        let provider = ScriptedProvider::new(&["### TOOL: list_dir\npath: .\n", "SUMMARY: seen\n"]);

        run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect("listing the root must work");
        let seen = provider.seen.lock().expect("lock");
        assert!(seen[1]
            .last()
            .map(|m| m.content.contains("index.html"))
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn a_file_marker_inside_written_content_is_not_a_foreign_transport() {
        // Mason writing documentation about the fenced transport — which the
        // files in this very repo do. A substring scan would reject this write
        // forever, on every retry, for containing text it was asked to write.
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: docs/format.md\n### CONTENT\n\
             Emit blocks like:\n### FILE: path\ncontent\n### END FILE\n### END CONTENT\n",
            "SUMMARY: documented the format\n",
        ]);

        let writes = run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect("documenting a marker is a legal write");
        assert_eq!(writes.len(), 1);
        assert!(
            writes[0].1.contains("### FILE: path"),
            "the marker must survive into the written content: {:?}",
            writes[0].1
        );
        assert_eq!(
            provider.calls(),
            2,
            "the write must be accepted on the first try, not re-asked"
        );
    }

    #[tokio::test]
    async fn a_second_write_to_one_path_supersedes_rather_than_duplicating() {
        // Sent to the caller as one entry per file, because two entries for one
        // path is exactly what `validate_mason_edits` refuses — and here the
        // model's intent is unambiguous: the later write is the revision.
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\nfirst\n### END CONTENT\n",
            "### TOOL: write_file\npath: js/./a.js\n### CONTENT\nsecond\n### END CONTENT\n",
            "SUMMARY: done\n",
        ]);

        let writes = run_mason_tool_loop(&provider, request(), staged.path(), 5)
            .await
            .expect("the loop must finish");
        assert_eq!(
            writes,
            vec![("js/./a.js".to_string(), "second".to_string())],
            "one entry per real file, carrying the latest content"
        );

        let seen = provider.seen.lock().expect("lock");
        assert!(
            seen[2]
                .iter()
                .any(|m| m.content.contains("replaces the earlier")),
            "supersession must be reported back, never silent"
        );
    }

    #[tokio::test]
    async fn a_refused_gateway_write_ends_the_loop() {
        struct RefusingRecorder;
        #[async_trait::async_trait]
        impl MasonToolRecorder for RefusingRecorder {
            async fn record_write(&self, _path: &str, _byte_len: usize) -> Result<bool> {
                Ok(false)
            }
        }

        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\na\n### END CONTENT\n",
            "SUMMARY: done\n",
        ]);

        let error = run_mason_tool_loop_with_recorder(
            &provider,
            request(),
            staged.path(),
            5,
            Some(&RefusingRecorder),
        )
        .await
        .expect_err("a refused write must not be returned as an edit");
        assert!(format!("{error:#}").contains("gateway refused"));
    }

    #[tokio::test]
    async fn every_write_reaches_the_invocation_gateway() {
        struct CountingRecorder {
            writes: Mutex<Vec<(String, usize)>>,
        }
        #[async_trait::async_trait]
        impl MasonToolRecorder for CountingRecorder {
            async fn record_write(&self, path: &str, byte_len: usize) -> Result<bool> {
                self.writes
                    .lock()
                    .expect("lock")
                    .push((path.to_string(), byte_len));
                Ok(true)
            }
        }

        let staged = tempfile::tempdir().expect("tempdir");
        let recorder = CountingRecorder {
            writes: Mutex::new(Vec::new()),
        };
        let provider = ScriptedProvider::new(&[
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\naa\n### END CONTENT\n",
            "### TOOL: write_file\npath: js/a.js\n### CONTENT\nbbb\n### END CONTENT\n",
            "SUMMARY: done\n",
        ]);

        run_mason_tool_loop_with_recorder(&provider, request(), staged.path(), 5, Some(&recorder))
            .await
            .expect("the loop must finish");

        assert_eq!(
            *recorder.writes.lock().expect("lock"),
            vec![("js/a.js".to_string(), 2), ("js/a.js".to_string(), 3)],
            "a superseded write is still a write that happened, and must be logged"
        );
    }

    #[tokio::test]
    async fn a_missing_file_is_reported_to_the_model_not_fatal() {
        let staged = tempfile::tempdir().expect("tempdir");
        let provider = ScriptedProvider::new(&[
            "### TOOL: read_file\npath: js/absent.js\n",
            "SUMMARY: nothing to do\n",
        ]);

        let writes = run_mason_tool_loop(&provider, request(), staged.path(), 4)
            .await
            .expect("a missing file is information, not a failure");
        assert!(writes.is_empty());

        let seen = provider.seen.lock().expect("lock");
        assert!(seen[1].iter().any(|m| m.content.contains("ERROR: reading")));
    }

    #[test]
    fn the_tool_loop_is_off_unless_a_spec_asks_for_it() {
        // The whole safety argument for this module rests on this: no existing
        // spec sets `tool_loop`, so no existing run reaches any of the code
        // above. If `#[serde(default)]` ever came off the field, a spec without
        // it would fail to load instead of defaulting to the single-shot lane.
        let existing: crate::models::WorkerHarnessConfig =
            serde_yaml::from_str("adapter: harkonnen\nllm_edits: true\ngit_branch: true\n")
                .expect("a spec written before this feature must still load");
        assert!(
            !existing.tool_loop,
            "a spec that never heard of the tool loop must not run it"
        );
        assert!(!crate::models::WorkerHarnessConfig::default().tool_loop);

        let opted_in: crate::models::WorkerHarnessConfig =
            serde_yaml::from_str("adapter: harkonnen\nllm_edits: true\ntool_loop: true\n")
                .expect("opting in must parse");
        assert!(
            opted_in.tool_loop,
            "and a spec that asks for it must get it"
        );
    }

    #[test]
    fn tool_results_render_in_the_shape_the_model_was_promised() {
        let tool = MasonTool::ListDir {
            path: "js".to_string(),
        };
        assert_eq!(
            render_tool_result(&tool, "a.js\nb.js"),
            "### TOOL RESULT: list_dir\na.js\nb.js\n### END TOOL RESULT"
        );
    }
}
