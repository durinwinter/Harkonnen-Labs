# Mason Edit Transport Implementation Plan

> ## ⚠️ THE REFERENCE CODE IN THIS PLAN IS NOT AUTHORITATIVE — THE COMMITTED SOURCE IS
>
> Every `rust` snippet below was **written but never executed** before it was
> put in this document. Four of them shipped **Critical, silent** defects —
> defects that made the parser return `Ok` while discarding content the model
> had actually produced. None were found by reading; all were found by
> compiling the logic standalone and running adversarial input against it.
>
> The known-bad reference implementations are in **Tasks 2, 5, 6 and 7**, and
> each is annotated inline below with a `⚠️ KNOWN-BAD` note. The list is not
> guaranteed complete — the same method that found four would likely find more.
>
> If you are re-deriving this work, or reviewing a diff against it:
>
> - **Read `src/mason_transport.rs`, `src/mason_tools.rs` and
>   `src/orchestrator.rs`, not this file.** They carry the corrected logic and
>   the reasoning for each correction.
> - Read the ledger at
>   `.superpowers/sdd/2026-08-03-mason-edit-transport/progress.md` and the final
>   fix report beside it for what changed and why.
> - Treat any snippet here as a sketch of *intent*. Do not copy one into source
>   without executing it against adversarial input first.
>
> A whole-branch review after all seven tasks passed found defects that
> per-task review structurally could not: the per-task reviews checked each
> reference implementation against its own brief, and the briefs were derived
> from the same unexecuted code.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-shot JSON-blob edit transport with a retry loop, a fenced-file envelope, a patch format, and finally real tool calls — so Mason can reliably turn a model response into files on disk.

**Architecture:** Mason currently sends one prompt and demands one JSON object containing every touched file's complete contents, JSON-string-escaped. This plan adds a retry that feeds parse errors back, then changes the envelope so source code is never escaped, then adds search/replace patches so output scales with change size rather than file size, and finally lets Mason call read/write tools through the invocation gateway the factory already has. Each tier is independently shippable and leaves the system working.

**Tech Stack:** Rust 2021, tokio, anyhow, serde/serde_json, existing `crate::llm` provider abstraction.

## Global Constraints

- Rust edition 2021, async-first (tokio). Error propagation via `anyhow::Result`; no `unwrap()` in non-test code.
- Gate for every task: `cargo fmt --check`, `cargo check --workspace --all-targets`, `cargo test -q --workspace`. All must pass before commit.
- **Three call sites, not one.** Every transport change applies to `mason_generate_and_apply_edits` (`src/orchestrator.rs:7896`), `mason_fix_from_build_failure` (`:8509`), and `mason_fix_from_validation_failure` (`:8607`).
- **Never apply a mis-recovered proposal.** `validate_mason_edits` is the last line of defence before writing to the operator's files; every new parse path must route through it.
- Existing behaviour must keep working at every step. The legacy JSON path is never removed by this plan — it stays as the fallback in `parse_mason_edit_response`, so a model that ignores the new format, or a cached prompt from an older run, still produces a usable proposal. Removing it is a separate decision once telemetry shows nothing uses it.
- No new third-party crates without checking `Cargo.toml` first — `serde_json`, `anyhow`, `tokio` are already present.
- Model-facing instruction text and parser must always be changed in the same commit. A parser expecting a format the prompt never requested is the failure mode this whole plan exists to remove.

---

## File Structure

| File | Responsibility |
|---|---|
| `src/orchestrator.rs` | Existing home of the edit lane, parser, and the three call sites. Tasks 1 and 4 modify it in place. |
| `src/mason_transport.rs` (new) | Envelope formats: fenced-file parsing (Task 2), patch blocks (Task 5). Pure functions, no I/O, no `AppContext`. This is where format logic goes so `orchestrator.rs` does not grow further. |
| `src/mason_tools.rs` (new) | Tool-call loop and tool definitions (Task 7). |
| `src/lib.rs` | Module registration for the two new files. |

`orchestrator.rs` is already very large. Everything that can be a pure function over strings belongs in the new modules, which keeps it testable without spinning up an `AppContext`.

---

## Task 1: Retry a malformed model response

**Files:**
- Modify: `src/orchestrator.rs` — add helper near `parse_mason_edit_proposal` (~`:28833`); wire into `:8081`, `:8577`, `:8719`
- Test: `src/orchestrator.rs` `mod tests`

**Interfaces:**
- Produces: `async fn complete_edit_proposal_with_retry(provider: &dyn LlmProvider, req: LlmRequest, attempts: u32) -> (Result<MasonEditProposal>, String)` — returns the parsed proposal and the last raw response body (for `mason_raw_response.txt`).
- Consumes: existing `parse_mason_edit_proposal`, `LlmRequest`, `Message`, `LlmProvider::complete`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn edit_proposal_retry_feeds_the_parse_error_back_and_succeeds() {
    struct FlakyProvider {
        calls: std::sync::Mutex<Vec<String>>,
    }
    #[async_trait::async_trait]
    impl crate::llm::LlmProvider for FlakyProvider {
        async fn complete(&self, req: crate::llm::LlmRequest) -> Result<crate::llm::LlmResponse> {
            let mut calls = self.calls.lock().expect("lock");
            let last_user = req
                .messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default();
            calls.push(last_user);
            let content = if calls.len() == 1 {
                "{\"summary\":\"x\",\"edits\":[".to_string()
            } else {
                r#"{"summary":"ok","rationale":[],"edits":[{"path":"js/a.js","action":"write","summary":"s","content":"x"}]}"#.to_string()
            };
            Ok(crate::llm::LlmResponse { content, usage: None })
        }
    }

    let provider = FlakyProvider { calls: std::sync::Mutex::new(Vec::new()) };
    let req = crate::llm::LlmRequest {
        messages: vec![crate::llm::Message::user("edit please".to_string())],
        max_tokens: 4000,
        temperature: 0.1,
    };

    let (result, _raw) = complete_edit_proposal_with_retry(&provider, req, 2).await;
    let proposal = result.expect("second attempt must succeed");

    assert_eq!(proposal.edits.len(), 1);
    let calls = provider.calls.lock().expect("lock");
    assert_eq!(calls.len(), 2, "must retry exactly once");
    assert!(
        calls[1].contains("cut off mid-response") || calls[1].contains("did not parse"),
        "the retry must carry the parse failure back to the model, got: {}",
        calls[1]
    );
}
```

- [ ] **Step 2: Run it to make sure it fails**

Run: `cargo test -q --lib -- edit_proposal_retry_feeds`
Expected: FAIL — `cannot find function complete_edit_proposal_with_retry`

- [ ] **Step 3: Write the minimal implementation**

```rust
/// Ask once, and if the response does not parse, show the model exactly how it
/// failed and ask again. Models correct malformed output reliably when told
/// what was wrong; before this, a single bad response ended the run.
///
/// Returns the last raw body alongside the result so the caller can still write
/// `mason_raw_response.txt` for the attempt that actually failed.
async fn complete_edit_proposal_with_retry(
    provider: &dyn crate::llm::LlmProvider,
    req: crate::llm::LlmRequest,
    attempts: u32,
) -> (Result<MasonEditProposal>, String) {
    let mut messages = req.messages.clone();
    let mut last_raw = String::new();
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..attempts.max(1) {
        let this_req = crate::llm::LlmRequest {
            messages: messages.clone(),
            max_tokens: req.max_tokens,
            temperature: req.temperature,
        };
        let response = match provider.complete(this_req).await {
            Ok(response) => response,
            Err(error) => return (Err(error), last_raw),
        };
        last_raw = response.content.clone();

        let (_reasoning, body) = extract_reasoning(&response.content);
        match parse_mason_edit_proposal(body) {
            Ok(proposal) => return (Ok(proposal), last_raw),
            Err(error) => {
                if attempt + 1 < attempts.max(1) {
                    messages.push(crate::llm::Message::assistant(response.content.clone()));
                    messages.push(crate::llm::Message::user(format!(
                        "Your previous response could not be used: {error:#}\n\n\
                         Return the same work again as a single valid JSON object with keys \
                         summary, rationale and edits. Escape every newline, quote and backslash \
                         inside string values. Emit nothing outside the JSON object.",
                    )));
                }
                last_error = Some(error);
            }
        }
    }

    (
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no attempts were made"))),
        last_raw,
    )
}
```

If `Message::assistant` does not exist, add it next to `Message::system` / `Message::user` in `src/llm.rs` following the same shape.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -q --lib -- edit_proposal_retry_feeds`
Expected: PASS

- [ ] **Step 5: Wire all three call sites**

At `:8081` in `mason_generate_and_apply_edits`, replace the single `provider.complete(...)` + `parse_mason_edit_proposal(edit_body)` pair with:

```rust
let (parsed, raw_body) = complete_edit_proposal_with_retry(provider.as_ref(), req, 2).await;
let raw_response_path = run_dir.join("mason_raw_response.txt");
let _ = tokio::fs::write(&raw_response_path, &raw_body).await;
let proposal = match parsed {
    Ok(proposal) => proposal,
    Err(error) => { /* keep the existing invalid_llm_edit_response artifact branch, using `error` */ }
};
```

Apply the same replacement in `mason_fix_from_build_failure` (`:8577`) and `mason_fix_from_validation_failure` (`:8719`).

- [ ] **Step 6: Fix the hardcoded provider label while here**

`:8077` records every cost event as `"gemini"` regardless of which provider ran:

```rust
self.record_llm_cost_event(run_id, "mason", "edits", "gemini", "", usage)
```

Replace the literal with the resolved provider name already available from `setup.resolve_agent_provider_name("mason", "default")`. Cost attribution is currently wrong for every non-Gemini run.

- [ ] **Step 7: Run the full gate**

Run: `cargo fmt --check && cargo check --workspace --all-targets && cargo test -q --workspace`
Expected: all pass

- [ ] **Step 8: Commit**

```bash
git add src/orchestrator.rs src/llm.rs
git commit -m "fix: retry a malformed Mason edit proposal instead of failing the run"
```

---

## Task 2: Fenced-file envelope parser

**Files:**
- Create: `src/mason_transport.rs`
- Modify: `src/lib.rs` (add `mod mason_transport;`)
- Test: `src/mason_transport.rs` `mod tests`

**Interfaces:**
- Produces: `pub fn parse_fenced_edits(raw: &str) -> Result<FencedEnvelope>` where `pub struct FencedEnvelope { pub summary: String, pub rationale: Vec<String>, pub files: Vec<FencedFile> }` and `pub struct FencedFile { pub path: String, pub content: String }`.
- Produces: `pub const FENCED_FORMAT_INSTRUCTION: &str` — the exact wording given to the model, kept beside the parser so the two cannot drift.
- Consumed by: Task 3.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn fenced_envelope_passes_source_code_through_verbatim() {
    let raw = r#"SUMMARY: Add the bonus room
RATIONALE:
- followed the existing G.rooms shape
- registered the room in main.js

### FILE: js/bonus.js
G.rooms.bonus = {
  id: 'bonus',
  verbs: { lookat: "A dusty attic", open: "It creaks" },
  note: "quotes \" and backslashes \\ survive"
};
### END FILE

### FILE: README.md
The bonus room ("Dad's Workshop") is optional.
### END FILE
"#;

    let envelope = parse_fenced_edits(raw).expect("must parse");

    assert_eq!(envelope.summary, "Add the bonus room");
    assert_eq!(envelope.rationale.len(), 2);
    assert_eq!(envelope.files.len(), 2);
    assert_eq!(envelope.files[0].path, "js/bonus.js");
    assert!(envelope.files[0].content.contains(r#"lookat: "A dusty attic""#));
    assert!(envelope.files[0].content.contains(r#"backslashes \\ survive"#));
    assert!(envelope.files[1].content.contains(r#"("Dad's Workshop")"#));
    assert!(
        !envelope.files[0].content.contains("### END FILE"),
        "the terminator must not leak into content"
    );
}

#[test]
fn fenced_envelope_reports_an_unterminated_file_as_truncation() {
    let raw = "SUMMARY: x\n\n### FILE: js/a.js\nG.rooms.a = {};\n";
    let error = parse_fenced_edits(raw).expect_err("unterminated block must fail");
    assert!(
        format!("{error:#}").contains("never closed"),
        "an unterminated block means a cut-off response, got: {error:#}"
    );
}

#[test]
fn fenced_envelope_rejects_a_path_that_is_not_a_path() {
    let raw = "SUMMARY: x\n\n### FILE: \n### END FILE\n";
    assert!(parse_fenced_edits(raw).is_err(), "empty path must be refused");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -q --lib -- fenced_envelope`
Expected: FAIL — module does not exist

> ⚠️ **KNOWN-BAD reference implementation.** The parser below shipped a
> Critical silent defect: content lines were compared without symmetric
> trimming and a `### FILE:` marker encountered inside an open block was
> swallowed rather than refused, so a block could be truncated or a whole file
> dropped with `Ok` returned. The final review found two more defects the fixed
> version still carried — near-miss marker spellings (`#### FILE:`, `### File:`)
> silently dropping whole blocks, and every file losing its trailing newline.
> See `collect_fenced_edits`, `unreadable_edit_marker` and
> `near_miss_marker_word` in `src/mason_transport.rs` for what is actually
> correct.

- [ ] **Step 3: Implement the parser**

```rust
use anyhow::{bail, Result};

/// Wording handed to the model. Lives next to the parser deliberately: a prompt
/// that asks for one format while the parser expects another is precisely the
/// class of failure this transport exists to eliminate.
pub const FENCED_FORMAT_INSTRUCTION: &str = "\
Respond in this exact plain-text format. Do NOT use JSON. Do NOT use markdown code fences.

SUMMARY: <one line describing the change>
RATIONALE:
- <one reason per line>

Then, for every file you are writing, a block of exactly this shape:

### FILE: <relative/path/from/the/workspace/root>
<the complete contents of the file, written literally>
### END FILE

Write file contents exactly as they should appear on disk. Do not escape \
quotes, backslashes or newlines. Do not wrap contents in backticks. Emit one \
block per file, and nothing after the final ### END FILE.";

const FILE_MARKER: &str = "### FILE:";
const END_MARKER: &str = "### END FILE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedEnvelope {
    pub summary: String,
    pub rationale: Vec<String>,
    pub files: Vec<FencedFile>,
}

pub fn parse_fenced_edits(raw: &str) -> Result<FencedEnvelope> {
    let mut summary = String::new();
    let mut rationale = Vec::new();
    let mut files = Vec::new();

    let mut current: Option<(String, Vec<String>)> = None;
    let mut in_rationale = false;

    for line in raw.lines() {
        if let Some((path, body)) = current.as_mut() {
            if line.trim_end() == END_MARKER {
                let path = path.clone();
                let content = body.join("\n");
                files.push(FencedFile { path, content });
                current = None;
            } else {
                body.push(line.to_string());
            }
            continue;
        }

        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(FILE_MARKER) {
            let path = rest.trim().to_string();
            if path.is_empty() {
                bail!("a {FILE_MARKER} block declared an empty path");
            }
            if path.contains('"') || path.len() > 200 {
                bail!("a {FILE_MARKER} block declared an implausible path: {path:?}");
            }
            in_rationale = false;
            current = Some((path, Vec::new()));
        } else if let Some(rest) = trimmed.strip_prefix("SUMMARY:") {
            summary = rest.trim().to_string();
            in_rationale = false;
        } else if trimmed == "RATIONALE:" {
            in_rationale = true;
        } else if in_rationale {
            if let Some(item) = trimmed.strip_prefix("- ") {
                rationale.push(item.trim().to_string());
            }
        }
    }

    // An unterminated block is a truncated response, not a formatting mistake.
    // Reporting it as such points the operator at the token budget rather than
    // at the model's punctuation.
    if let Some((path, _)) = current {
        bail!(
            "the {FILE_MARKER} block for {path:?} was never closed with {END_MARKER} — the \
             response was cut off. Raise MASON_EDIT_MAX_TOKENS or narrow the editable surface."
        );
    }

    if files.is_empty() {
        bail!("no {FILE_MARKER} blocks were found in the response");
    }

    Ok(FencedEnvelope { summary, rationale, files })
}
```

Register in `src/lib.rs`:

```rust
mod mason_transport;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -q --lib -- fenced_envelope`
Expected: PASS — 3 passed

- [ ] **Step 5: Run the full gate and commit**

```bash
cargo fmt --check && cargo check --workspace --all-targets && cargo test -q --workspace
git add src/mason_transport.rs src/lib.rs
git commit -m "feat: add a fenced-file envelope so source code is never string-escaped"
```

---

## Task 3: Switch the edit lane to the fenced envelope

**Files:**
- Modify: `src/orchestrator.rs` — system instruction (~`:7990`), and the parse path in all three call sites
- Modify: `src/mason_transport.rs` — add the conversion to `MasonEdit`
- Test: `src/orchestrator.rs` `mod tests`

**Interfaces:**
- Consumes: `parse_fenced_edits`, `FENCED_FORMAT_INSTRUCTION` from Task 2; `validate_mason_edits` from the existing code.
- Produces: `fn parse_mason_edit_response(raw: &str) -> Result<MasonEditProposal>` — tries the fenced envelope first, falls back to the existing JSON path, so a model that ignores the new instruction still works.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn edit_response_prefers_the_fenced_envelope_and_falls_back_to_json() {
    let fenced = "SUMMARY: add room\nRATIONALE:\n- because\n\n### FILE: js/bonus.js\nG.rooms.bonus = { a: \"b\", };\n### END FILE\n";
    let from_fenced = parse_mason_edit_response(fenced).expect("fenced must parse");
    assert_eq!(from_fenced.edits.len(), 1);
    assert_eq!(from_fenced.edits[0].path, "js/bonus.js");
    assert_eq!(from_fenced.edits[0].action, "write");
    assert!(from_fenced.edits[0].content.contains(r#"a: "b""#));

    let json = r#"{"summary":"s","rationale":[],"edits":[{"path":"js/a.js","action":"write","summary":"s","content":"x"}]}"#;
    let from_json = parse_mason_edit_response(json).expect("json must still parse");
    assert_eq!(from_json.edits[0].path, "js/a.js");
}

#[test]
fn edit_response_routes_fenced_output_through_edit_validation() {
    // A path that is really source code must be refused on the fenced path too.
    let bad = "SUMMARY: x\n\n### FILE: G.rooms = {\n### END FILE\n";
    assert!(parse_mason_edit_response(bad).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -q --lib -- edit_response_prefers`
Expected: FAIL — `cannot find function parse_mason_edit_response`

- [ ] **Step 3: Implement the dispatcher**

In `src/mason_transport.rs`:

```rust
impl FencedEnvelope {
    /// `action` is always `write` — the fenced transport only expresses whole
    /// files, which is exactly what the existing apply path already handles.
    pub fn into_edits(self) -> (String, Vec<String>, Vec<(String, String)>) {
        let files = self
            .files
            .into_iter()
            .map(|file| (file.path, file.content))
            .collect();
        (self.summary, self.rationale, files)
    }
}
```

In `src/orchestrator.rs`:

```rust
/// Try the fenced envelope first, then the legacy JSON object. Keeping both
/// means a model that ignores the new instruction — or a cached prompt from a
/// previous run — still produces a usable proposal.
fn parse_mason_edit_response(raw: &str) -> Result<MasonEditProposal> {
    match crate::mason_transport::parse_fenced_edits(raw) {
        Ok(envelope) => {
            let (summary, rationale, files) = envelope.into_edits();
            let edits = files
                .into_iter()
                .map(|(path, content)| MasonEdit {
                    path,
                    action: "write".to_string(),
                    summary: String::new(),
                    content,
                })
                .collect::<Vec<_>>();
            validate_mason_edits(&edits)?;
            Ok(MasonEditProposal { summary, rationale, edits })
        }
        Err(fenced_error) => parse_mason_edit_proposal(raw).map_err(|json_error| {
            anyhow::anyhow!(
                "the response matched neither transport. Fenced: {fenced_error:#}. JSON: {json_error:#}"
            )
        }),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -q --lib -- edit_response`
Expected: PASS

- [ ] **Step 5: Change the instruction the model receives**

In `mason_generate_and_apply_edits` (~`:7990`), replace both the `prompt_support` branch's task contract and the `unwrap_or_else` default with `crate::mason_transport::FENCED_FORMAT_INSTRUCTION`. Replace the closing line of the user message — currently "Respond with a single JSON object only…" — with:

```rust
"Respond using the FILE block format described above. Nothing outside the blocks."
```

Do the same in `mason_fix_from_build_failure` and `mason_fix_from_validation_failure`.

- [ ] **Step 6: Point the retry helper at the new dispatcher**

In `complete_edit_proposal_with_retry` from Task 1, change `parse_mason_edit_proposal(body)` to `parse_mason_edit_response(body)`, and change the retry instruction text to restate the fenced format rather than the JSON rules:

```rust
messages.push(crate::llm::Message::user(format!(
    "Your previous response could not be used: {error:#}\n\n{}",
    crate::mason_transport::FENCED_FORMAT_INSTRUCTION
)));
```

- [ ] **Step 7: Run the full gate and commit**

```bash
cargo fmt --check && cargo check --workspace --all-targets && cargo test -q --workspace
git add src/orchestrator.rs src/mason_transport.rs
git commit -m "feat: ask Mason for fenced file blocks instead of JSON-escaped content"
```

---

## Task 4: Report what each applied edit actually changed

**Files:**
- Modify: `src/orchestrator.rs` — the apply loop at `:8233`, and `MasonEditApplicationArtifact`
- Test: `src/orchestrator.rs` `mod tests`

**Interfaces:**
- Produces: `fn summarize_edit_delta(before: Option<&str>, after: &str) -> EditDelta` where `pub struct EditDelta { pub lines_before: usize, pub lines_after: usize, pub bytes_before: usize, pub bytes_after: usize, pub created: bool }`.
- Consumed by: the application artifact, so every run records the size of what it overwrote.

**Why this task exists:** whole-file rewrites can silently drop code, and nothing currently compares the proposal against the original. A run that deleted half a file looks identical to one that appended to it.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn edit_delta_flags_a_rewrite_that_loses_most_of_the_file() {
    let before = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let after = "line 0\nline 1";

    let delta = summarize_edit_delta(Some(&before), after);

    assert_eq!(delta.lines_before, 100);
    assert_eq!(delta.lines_after, 2);
    assert!(!delta.created);
    assert!(delta.shrank_sharply(), "a 98% reduction must be flagged");
}

#[test]
fn edit_delta_treats_a_new_file_as_creation_not_shrinkage() {
    let delta = summarize_edit_delta(None, "a\nb\nc");
    assert!(delta.created);
    assert!(!delta.shrank_sharply());
    assert_eq!(delta.lines_before, 0);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -q --lib -- edit_delta`
Expected: FAIL — `cannot find function summarize_edit_delta`

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EditDelta {
    lines_before: usize,
    lines_after: usize,
    bytes_before: usize,
    bytes_after: usize,
    created: bool,
}

impl EditDelta {
    /// A whole-file rewrite that keeps under a third of the original lines is
    /// far more likely to be a model losing context than a deliberate deletion.
    /// Worth surfacing to the operator; not worth blocking on, since a genuine
    /// rewrite is legitimate.
    fn shrank_sharply(&self) -> bool {
        !self.created && self.lines_before >= 20 && self.lines_after * 3 < self.lines_before
    }
}

fn summarize_edit_delta(before: Option<&str>, after: &str) -> EditDelta {
    let lines_before = before.map(|text| text.lines().count()).unwrap_or(0);
    let bytes_before = before.map(str::len).unwrap_or(0);
    EditDelta {
        lines_before,
        lines_after: after.lines().count(),
        bytes_before,
        bytes_after: after.len(),
        created: before.is_none(),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -q --lib -- edit_delta`
Expected: PASS

- [ ] **Step 5: Record deltas in the application artifact**

Add two fields to `MasonEditApplicationArtifact` (beside `changed_files`):

```rust
    deltas: Vec<EditDelta>,
    shrank_sharply: Vec<String>,
```

In the apply loop at `:8233`, capture the original before overwriting:

```rust
        let mut deltas = Vec::new();
        let mut shrank_sharply = Vec::new();
        for edit in &proposal.edits {
            let normalized = normalize_project_path(&edit.path);
            let full = staged_product.join(&normalized);
            let before = tokio::fs::read_to_string(&full).await.ok();
            let delta = summarize_edit_delta(before.as_deref(), &edit.content);
            if delta.shrank_sharply() {
                shrank_sharply.push(normalized.clone());
            }
            deltas.push(delta);

            // ... existing write of edit.content to `full` stays here ...
        }
```

Then, when building the artifact, surface it in `summary` so it reaches the run report rather than sitting only in JSON:

```rust
        let summary = if shrank_sharply.is_empty() {
            format!("Applied {} edit(s).", proposal.edits.len())
        } else {
            format!(
                "Applied {} edit(s). WARNING: {} file(s) lost more than two thirds of their \
                 lines — review before trusting this run: {}",
                proposal.edits.len(),
                shrank_sharply.len(),
                shrank_sharply.join(", ")
            )
        };
```

- [ ] **Step 6: Run the full gate and commit**

```bash
cargo fmt --check && cargo check --workspace --all-targets && cargo test -q --workspace
git add src/orchestrator.rs
git commit -m "feat: record what each Mason edit replaced, and flag sharp shrinkage"
```

---

## Task 5: Search/replace patch blocks

**Files:**
- Modify: `src/mason_transport.rs`
- Test: `src/mason_transport.rs` `mod tests`

**Interfaces:**
- Produces: `pub fn parse_patch_blocks(raw: &str) -> Result<Vec<PatchBlock>>` where `pub struct PatchBlock { pub path: String, pub search: String, pub replace: String }`.
- Produces: `pub fn apply_patch_block(original: &str, block: &PatchBlock) -> Result<String>`.
- Produces: `pub const PATCH_FORMAT_INSTRUCTION: &str`.
- Consumed by: Task 6.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn patch_block_replaces_an_exact_region() {
    let original = "line a\nline b\nline c\n";
    let block = PatchBlock {
        path: "js/a.js".to_string(),
        search: "line b".to_string(),
        replace: "line B1\nline B2".to_string(),
    };

    let patched = apply_patch_block(original, &block).expect("must apply");

    assert_eq!(patched, "line a\nline B1\nline B2\nline c\n");
}

#[test]
fn patch_block_refuses_when_the_search_text_is_absent() {
    let block = PatchBlock {
        path: "js/a.js".to_string(),
        search: "nowhere".to_string(),
        replace: "x".to_string(),
    };
    let error = apply_patch_block("line a\n", &block).expect_err("must refuse");
    assert!(format!("{error:#}").contains("did not match"));
}

#[test]
fn patch_block_refuses_an_ambiguous_match() {
    let block = PatchBlock {
        path: "js/a.js".to_string(),
        search: "dup".to_string(),
        replace: "x".to_string(),
    };
    let error = apply_patch_block("dup\ndup\n", &block).expect_err("must refuse");
    assert!(format!("{error:#}").contains("matched 2 times"));
}

#[test]
fn patch_blocks_parse_from_the_wire_format() {
    let raw = "### PATCH: js/a.js\n<<<<<<< SEARCH\nline b\n=======\nline B\n>>>>>>> REPLACE\n";
    let blocks = parse_patch_blocks(raw).expect("must parse");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].path, "js/a.js");
    assert_eq!(blocks[0].search, "line b");
    assert_eq!(blocks[0].replace, "line B");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -q --lib -- patch_block`
Expected: FAIL — types do not exist

> ⚠️ **KNOWN-BAD reference implementation.** `parse_patch_blocks` below shipped
> **five** defects, two of them Critical and silent — a `### PATCH:` header
> arriving while a block was open cleared the accumulated state and discarded
> the previous patch entirely (two patches in, one out, `Ok`, no signal), and a
> `### FILE:` block left open swallowed every later patch. It also has no
> section guards on `<<<<<<< SEARCH`, `=======` or `>>>>>>> REPLACE`, and
> `raw.lines()` strips the `\r` of a CRLF response so such a patch can never
> match a CRLF file. See `parse_patch_blocks` in `src/mason_transport.rs`.

- [ ] **Step 3: Implement**

```rust
pub const PATCH_FORMAT_INSTRUCTION: &str = "\
When changing an existing file, emit a patch rather than the whole file:

### PATCH: <relative/path>
<<<<<<< SEARCH
<text to find, copied exactly from the current file>
=======
<text to put in its place>
>>>>>>> REPLACE

The SEARCH text must appear exactly once in the file, copied character for \
character including indentation. Use a whole ### FILE: block instead when \
creating a new file.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchBlock {
    pub path: String,
    pub search: String,
    pub replace: String,
}

/// Exact match only, and it must be unique. A fuzzy matcher would apply more
/// patches, and would sometimes apply them in the wrong place — which is
/// unrecoverable once written. Refusing costs a retry; guessing costs the file.
pub fn apply_patch_block(original: &str, block: &PatchBlock) -> Result<String> {
    if block.search.is_empty() {
        bail!("patch for {} has an empty SEARCH section", block.path);
    }
    let occurrences = original.matches(block.search.as_str()).count();
    match occurrences {
        0 => bail!(
            "patch for {} did not match: the SEARCH text is not present in the file",
            block.path
        ),
        1 => Ok(original.replacen(block.search.as_str(), &block.replace, 1)),
        n => bail!(
            "patch for {} is ambiguous: the SEARCH text matched {n} times, so the target is \
             unclear. Include more surrounding context to make it unique.",
            block.path
        ),
    }
}

pub fn parse_patch_blocks(raw: &str) -> Result<Vec<PatchBlock>> {
    const PATCH_MARKER: &str = "### PATCH:";
    const SEARCH_START: &str = "<<<<<<< SEARCH";
    const DIVIDER: &str = "=======";
    const REPLACE_END: &str = ">>>>>>> REPLACE";

    let mut blocks = Vec::new();
    let mut path: Option<String> = None;
    let mut search: Vec<String> = Vec::new();
    let mut replace: Vec<String> = Vec::new();
    let mut section = 0u8; // 0 outside, 1 in SEARCH, 2 in REPLACE

    for line in raw.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.trim().strip_prefix(PATCH_MARKER) {
            path = Some(rest.trim().to_string());
            search.clear();
            replace.clear();
            section = 0;
        } else if trimmed.trim() == SEARCH_START {
            section = 1;
        } else if trimmed.trim() == DIVIDER && section == 1 {
            section = 2;
        } else if trimmed.trim() == REPLACE_END && section == 2 {
            let Some(current_path) = path.clone() else {
                bail!("a patch block closed without a preceding {PATCH_MARKER} line");
            };
            blocks.push(PatchBlock {
                path: current_path,
                search: search.join("\n"),
                replace: replace.join("\n"),
            });
            section = 0;
        } else if section == 1 {
            search.push(line.to_string());
        } else if section == 2 {
            replace.push(line.to_string());
        }
    }

    if section != 0 {
        bail!("a patch block was never closed with {REPLACE_END} — the response was cut off");
    }
    Ok(blocks)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -q --lib -- patch_block`
Expected: PASS — 4 passed

- [ ] **Step 5: Run the full gate and commit**

```bash
cargo fmt --check && cargo check --workspace --all-targets && cargo test -q --workspace
git add src/mason_transport.rs
git commit -m "feat: add search/replace patch blocks with exact, unique matching"
```

---

## Task 6: Accept patches in the edit lane, falling back to whole files

**Files:**
- Modify: `src/orchestrator.rs` — `parse_mason_edit_response`, the apply path, and the three instruction sites
- Test: `src/orchestrator.rs` `mod tests`

**Interfaces:**
- Consumes: `parse_patch_blocks`, `apply_patch_block`, `PATCH_FORMAT_INSTRUCTION` from Task 5.
- Produces: patches resolved against the staged workspace into ordinary `MasonEdit { action: "write" }` entries, so the existing apply, validation and delta machinery is reused unchanged.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn patches_resolve_against_staged_files_into_whole_file_edits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let staged = dir.path();
    std::fs::create_dir_all(staged.join("js")).expect("mkdir");
    std::fs::write(staged.join("js/hints.js"), "const HINTS = [\n  'a',\n];\n").expect("write");

    let raw = "SUMMARY: add a hint\n\n### PATCH: js/hints.js\n<<<<<<< SEARCH\n  'a',\n=======\n  'a',\n  'b',\n>>>>>>> REPLACE\n";

    let proposal = parse_mason_edit_response_with_staged(raw, staged).expect("must resolve");

    assert_eq!(proposal.edits.len(), 1);
    assert_eq!(proposal.edits[0].path, "js/hints.js");
    assert!(proposal.edits[0].content.contains("'b',"));
    assert!(
        proposal.edits[0].content.contains("const HINTS"),
        "unpatched regions must be preserved"
    );
}

#[test]
fn a_patch_against_a_missing_file_is_refused_clearly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw = "SUMMARY: x\n\n### PATCH: js/nope.js\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n";
    let error = parse_mason_edit_response_with_staged(raw, dir.path()).expect_err("must fail");
    assert!(format!("{error:#}").contains("js/nope.js"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -q --lib -- patches_resolve`
Expected: FAIL — `cannot find function parse_mason_edit_response_with_staged`

> ⚠️ **KNOWN-BAD reference implementation.** The `parse_patch_blocks(raw).unwrap_or_default()`
> on the first line of the function below is the Task 6 Critical, verbatim and
> unfixed: it swallows every patch-parse error, so a malformed patch hiding
> behind a valid `### FILE:` block vanished with no error anywhere. The same
> function later grew, and then lost, three more silent-skip defects across four
> review rounds. The final review added two more findings to this same
> function: a JSON edit proposal documenting the fenced format parsed as a
> fenced response naming a file `<path>`, and near-miss markers were never
> checked at all. See `parse_mason_edit_response_with_staged` and
> `parse_mason_text_edits` in `src/orchestrator.rs`.

- [ ] **Step 3: Implement**

```rust
/// Patches are resolved to whole-file contents here, at the boundary, so that
/// everything downstream — validation, the apply loop, delta reporting — keeps
/// working on exactly one representation.
fn parse_mason_edit_response_with_staged(raw: &str, staged_product: &Path) -> Result<MasonEditProposal> {
    let patches = crate::mason_transport::parse_patch_blocks(raw).unwrap_or_default();
    if patches.is_empty() {
        return parse_mason_edit_response(raw);
    }

    let mut edits = Vec::new();
    for patch in &patches {
        let normalized = normalize_project_path(&patch.path);
        let full = staged_product.join(&normalized);
        let original = std::fs::read_to_string(&full).with_context(|| {
            format!(
                "patch targets {} but that file does not exist in the staged workspace",
                patch.path
            )
        })?;
        let patched = crate::mason_transport::apply_patch_block(&original, patch)?;
        edits.push(MasonEdit {
            path: normalized,
            action: "write".to_string(),
            summary: format!("patch {}", patch.path),
            content: patched,
        });
    }

    // Whole-file blocks may accompany patches — new files cannot be patched.
    // Keep the envelope's summary and rationale: they are what the run report
    // and the decision log show the operator, and dropping them would make a
    // mixed patch/file response less legible than a pure one.
    let mut summary = String::new();
    let mut rationale = Vec::new();
    if let Ok(envelope) = crate::mason_transport::parse_fenced_edits(raw) {
        let (envelope_summary, envelope_rationale, files) = envelope.into_edits();
        summary = envelope_summary;
        rationale = envelope_rationale;
        for (path, content) in files {
            edits.push(MasonEdit {
                path,
                action: "write".to_string(),
                summary: String::new(),
                content,
            });
        }
    }

    validate_mason_edits(&edits)?;
    Ok(MasonEditProposal { summary, rationale, edits })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -q --lib -- patches_resolve a_patch_against`
Expected: PASS

- [ ] **Step 5: Offer the patch format to the model and route the call sites**

Append `PATCH_FORMAT_INSTRUCTION` to the system instruction after `FENCED_FORMAT_INSTRUCTION` at all three sites. Change the three call sites to use `parse_mason_edit_response_with_staged(body, &staged_product)`, and update `complete_edit_proposal_with_retry` to take the staged path so its retry parse matches.

- [ ] **Step 6: Run the full gate and commit**

```bash
cargo fmt --check && cargo check --workspace --all-targets && cargo test -q --workspace
git add src/orchestrator.rs src/mason_transport.rs
git commit -m "feat: let Mason send patches for existing files instead of whole rewrites"
```

---

## Task 7: Mason tool-call loop

**Files:**
- Create: `src/mason_tools.rs`
- Modify: `src/lib.rs`, `src/orchestrator.rs`
- Test: `src/mason_tools.rs` `mod tests`

**Interfaces:**
- Produces: `pub enum MasonTool { ReadFile { path: String }, ListDir { path: String }, WriteFile { path: String, content: String } }`, `pub fn parse_tool_call(raw: &str) -> Option<MasonTool>`, `pub fn render_tool_result(tool: &MasonTool, result: &str) -> String`.
- Produces: `pub async fn run_mason_tool_loop(provider: &dyn LlmProvider, req: LlmRequest, staged: &Path, max_turns: u32) -> Result<Vec<(String, String)>>` — returns `(path, content)` pairs rather than applying them, so the caller converts to `MasonEdit` and reuses the one existing apply path.
- Consumes: the existing invocation gateway so every write is recorded in `tool_invocations.json` exactly as host commands already are.

**Note:** this is the largest task and depends on nothing from Tasks 5-6. It can be deferred without blocking them. Tasks 1-4 should ship first; this is worth doing only once the earlier tiers show that the remaining failures are about *iteration*, not format.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn tool_calls_parse_from_the_wire_format() {
    assert_eq!(
        parse_tool_call("### TOOL: read_file\npath: js/data.js\n"),
        Some(MasonTool::ReadFile { path: "js/data.js".to_string() })
    );
    assert_eq!(
        parse_tool_call("### TOOL: list_dir\npath: js\n"),
        Some(MasonTool::ListDir { path: "js".to_string() })
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -q --lib -- tool_calls_parse`
Expected: FAIL — module does not exist

> ⚠️ **KNOWN-BAD reference implementation.** The tool loop below treats
> `parse_tool_calls` returning zero calls as "the model has finished", which is
> the Critical: a message carrying a write in a near-miss spelling
> (`#### TOOL:`, `### Tool:`) or a foreign transport parses to zero calls, and
> the loop signed off while discarding it. The shipped version adds
> `unreadable_write_marker` (and `near_miss_marker_word`, now shared with the
> single-shot lanes in `src/mason_transport.rs`) and a guard so the loop cannot
> discard writes it has already collected. Termination here is still not
> positive — see the note on `parse_tool_calls` in `src/mason_tools.rs`.

- [ ] **Step 3: Implement the tool vocabulary**

```rust
use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MasonTool {
    ReadFile { path: String },
    ListDir { path: String },
    WriteFile { path: String, content: String },
}

pub fn parse_tool_call(raw: &str) -> Option<MasonTool> {
    let mut kind: Option<String> = None;
    let mut path: Option<String> = None;
    let mut content: Vec<String> = Vec::new();
    let mut in_content = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if in_content {
            if trimmed == "### END CONTENT" {
                in_content = false;
            } else {
                content.push(line.to_string());
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### TOOL:") {
            kind = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("path:") {
            path = Some(rest.trim().to_string());
        } else if trimmed == "### CONTENT" {
            in_content = true;
        }
    }

    match (kind.as_deref(), path) {
        (Some("read_file"), Some(path)) => Some(MasonTool::ReadFile { path }),
        (Some("list_dir"), Some(path)) => Some(MasonTool::ListDir { path }),
        (Some("write_file"), Some(path)) => Some(MasonTool::WriteFile {
            path,
            content: content.join("\n"),
        }),
        _ => None,
    }
}

pub fn render_tool_result(tool: &MasonTool, result: &str) -> String {
    let name = match tool {
        MasonTool::ReadFile { .. } => "read_file",
        MasonTool::ListDir { .. } => "list_dir",
        MasonTool::WriteFile { .. } => "write_file",
    };
    format!("### TOOL RESULT: {name}\n{result}\n### END TOOL RESULT")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -q --lib -- tool_calls_parse write_file_tool`
Expected: PASS

- [ ] **Step 5: Implement the loop**

```rust
/// Confine every tool path to the staged workspace. A model that asks for
/// `../../etc/passwd` gets refused rather than served.
fn resolve_inside(staged: &Path, raw_path: &str) -> Result<std::path::PathBuf> {
    let candidate = staged.join(raw_path);
    let staged_canon = staged.canonicalize().unwrap_or_else(|_| staged.to_path_buf());
    let candidate_canon = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.clone());
    if !candidate_canon.starts_with(&staged_canon) {
        anyhow::bail!("tool path {raw_path:?} escapes the staged workspace");
    }
    Ok(candidate)
}

pub async fn run_mason_tool_loop(
    provider: &dyn crate::llm::LlmProvider,
    req: crate::llm::LlmRequest,
    staged: &Path,
    max_turns: u32,
) -> Result<Vec<(String, String)>> {
    let mut messages = req.messages.clone();
    let mut writes: Vec<(String, String)> = Vec::new();

    for _turn in 0..max_turns.max(1) {
        let response = provider
            .complete(crate::llm::LlmRequest {
                messages: messages.clone(),
                max_tokens: req.max_tokens,
                temperature: req.temperature,
            })
            .await?;

        let Some(tool) = parse_tool_call(&response.content) else {
            // No tool call means the model considers itself finished.
            return Ok(writes);
        };

        let result = match &tool {
            MasonTool::ReadFile { path } => {
                let full = resolve_inside(staged, path)?;
                std::fs::read_to_string(&full)
                    .unwrap_or_else(|error| format!("ERROR: {error}"))
            }
            MasonTool::ListDir { path } => {
                let full = resolve_inside(staged, path)?;
                match std::fs::read_dir(&full) {
                    Ok(entries) => entries
                        .flatten()
                        .map(|entry| entry.file_name().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Err(error) => format!("ERROR: {error}"),
                }
            }
            MasonTool::WriteFile { path, content } => {
                // Recorded, not yet written: the edits are returned to the
                // caller so they flow through validate_mason_edits and the
                // existing apply path like every other transport.
                writes.push((path.clone(), content.clone()));
                format!("recorded write of {} bytes to {path}", content.len())
            }
        };

        messages.push(crate::llm::Message::assistant(response.content.clone()));
        messages.push(crate::llm::Message::user(render_tool_result(&tool, &result)));
    }

    anyhow::bail!(
        "Mason used all {max_turns} tool turns without finishing. It read and listed but never \
         settled on a final set of writes."
    )
}
```

Returning writes rather than performing them keeps a single apply path: the caller converts each `(path, content)` pair into a `MasonEdit { action: "write" }`, runs `validate_mason_edits`, and reuses the delta reporting from Task 4.

- [ ] **Step 5b: Record tool writes in the invocation gateway**

Each `WriteFile` must be logged through the same gateway used for host commands at `:9318`, so tool writes land in `tool_invocations.json` and inherit the approval semantics already shipped for v1-E. Call the existing invocation-recording helper once per write, with the surface name `mason_tool_write` and the path as the command detail, before the caller applies the batch.

- [ ] **Step 6: Gate the loop behind a spec flag**

Add `tool_loop: bool` to `WorkerHarnessConfig` in `src/models.rs` (with `#[serde(default)]`). Only call `run_mason_tool_loop` when set, so the single-shot path stays the default until the loop has run against a real target.

- [ ] **Step 7: Run the full gate and commit**

```bash
cargo fmt --check && cargo check --workspace --all-targets && cargo test -q --workspace
git add src/mason_tools.rs src/lib.rs src/orchestrator.rs src/models.rs
git commit -m "feat: add an opt-in Mason tool-call loop routed through the invocation gateway"
```

---

## Verification against a real target

After Tasks 1-3, and again after Task 6, run the real end-to-end check rather than trusting the suite:

```bash
cd ~/Harkonnen-Labs/.claude/worktrees/phase6-typedb-adapter
cp ~/Harkonnen-Labs/.env .env
docker compose -f docker-compose.calvin.yml up -d typedb
HARKONNEN_SETUP=gemini-local HARKONNEN_HTTP_TIMEOUT_SECS=600 \
  cargo run -q -- run start factory/specs/drafts/dad-bonus-level-v2.yaml \
  --product-path <path-to-product-repo>
```

Then approve the checkpoint and confirm `mason_edit_application.json` reports `status: applied` with four changed files, and that the product repo has a `mason/*` branch whose diff is reviewable. The feature actually appearing in the product is the acceptance test for this whole plan.

Note the daily free-tier quota on `gemini-flash-latest`; `setups/gemini-local.toml` can be pointed at whichever model has quota that day.

---

## Sequencing

Tasks 1-4 are the high-value core and should ship together — they are roughly a day. Tasks 5-6 follow once whole-file writes are reliably landing. Task 7 is a separate piece of work and should not start until Tasks 1-6 have run against a real target, because it only pays off if the remaining failures turn out to be about iteration rather than format.
