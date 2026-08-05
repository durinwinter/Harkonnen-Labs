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
block per file, and nothing after the final ### END FILE. Important: file content \
must not contain any line that starts with '### FILE:' — the parser cannot \
distinguish such a line from a real file header. Also, no line of content may \
consist solely of '### END FILE' — the parser uses that exact phrase to mark block ends. \
Indenting such a line does NOT make it safe: the parser trims each line before \
comparing it, so an indented '### END FILE' still ends your block and every line \
after it is read as ordinary prose and discarded.

IMPORTANT: The format cannot express a file that lacks a trailing newline. Every \
file written through this transport is given exactly one final newline, which is \
what nearly every tool expects. If a file genuinely must end without one, say so \
instead of writing it.

Markers are matched exactly: three hashes, upper case, spelled as shown. \
'#### FILE:', '### File:' and '## FILE:' are not headers, and a response \
containing one is rejected rather than guessed at.";

const FILE_MARKER: &str = "### FILE:";
const END_MARKER: &str = "### END FILE";
const PATCH_HEADER_MARKER: &str = "### PATCH:";
const REPLACE_END_MARKER: &str = ">>>>>>> REPLACE";

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

/// Tracks whether the line currently being scanned lies inside a `### FILE:`
/// … `### END FILE` or `### PATCH:` … `>>>>>>> REPLACE` block body, as opposed
/// to being a header/closer line or ordinary top-level text. Each block only
/// closes on the marker that actually opened it — a `>>>>>>> REPLACE` line
/// quoted inside a `### FILE:` block's documentation content does not
/// prematurely end that FILE block, and vice versa.
///
/// This is a best-effort scanner, not a structural validator: `parse_fenced_edits`
/// and `parse_patch_blocks` remain the source of truth for whether the blocks
/// themselves are well-formed. Shared by five consumers —
/// `parse_summary_and_rationale`, `has_top_level_patch_header`,
/// `parse_patch_blocks`, `parse_fenced_edits`, and `unreadable_edit_marker` —
/// one block-tracking
/// implementation, so none of them can disagree about what counts as "inside
/// a block."
///
/// The two structural parsers each track only the *other* transport's blocks,
/// because each already owns its own block internals and needs those lines to
/// reach its own state machine rather than be skipped here:
/// `parse_patch_blocks` uses `file_blocks_only()`, and `parse_fenced_edits`
/// uses `patch_blocks_only()`. Tracking a parser's own block type here would
/// silently eat the body lines it exists to read.
///
/// Skipping is only ever *deferral*, never *discard*: because a tracker that
/// is still open at end of input means every line after the opener was hidden
/// from the parser, both structural parsers check `open_block_label()` when
/// their scan finishes and fail loudly rather than return a partial result.
///
/// Coupling note: this tracker and `parse_fenced_edits` agree on the FILE
/// block closing rule only because both hardcode the literal `### END FILE`
/// (via the shared `END_MARKER` constant). Nothing besides that shared
/// constant enforces the agreement — if `parse_fenced_edits`'s own
/// file-block-building loop ever changes what it accepts as a closer, this
/// tracker must change with it.
struct BlockTracker {
    closing_marker: Option<&'static str>,
    open_label: Option<String>,
    recognize_file_headers: bool,
    recognize_patch_headers: bool,
}

impl BlockTracker {
    /// Tracks both `### FILE:` and `### PATCH:` blocks — for consumers that
    /// need to skip over either kind of block indiscriminately.
    fn new() -> Self {
        Self {
            closing_marker: None,
            open_label: None,
            recognize_file_headers: true,
            recognize_patch_headers: true,
        }
    }

    /// Tracks only `### FILE:` blocks, leaving `### PATCH:` headers and
    /// bodies untouched — for `parse_patch_blocks`, which must keep
    /// processing those lines itself rather than have them skipped here too.
    fn file_blocks_only() -> Self {
        Self {
            closing_marker: None,
            open_label: None,
            recognize_file_headers: true,
            recognize_patch_headers: false,
        }
    }

    /// The mirror image: tracks only `### PATCH:` blocks, leaving `### FILE:`
    /// headers and bodies untouched — for `parse_fenced_edits`, which owns
    /// FILE block internals and must not have them skipped, but must also not
    /// mistake a `### FILE:` line quoted inside a *patch's* SEARCH or REPLACE
    /// body for a real file block of its own.
    fn patch_blocks_only() -> Self {
        Self {
            closing_marker: None,
            open_label: None,
            recognize_file_headers: false,
            recognize_patch_headers: true,
        }
    }

    /// Feed the next trimmed line. Returns `true` if this line lies outside
    /// any block body (a header line, a closer line, or top-level text), and
    /// `false` if it lies inside one.
    fn consume(&mut self, trimmed: &str) -> bool {
        if let Some(closer) = self.closing_marker {
            if trimmed == closer {
                self.closing_marker = None;
                self.open_label = None;
            }
            return false;
        }
        if self.recognize_file_headers {
            if let Some(rest) = trimmed.strip_prefix(FILE_MARKER) {
                self.closing_marker = Some(END_MARKER);
                self.open_label = Some(rest.trim().to_string());
                return true;
            }
        }
        if self.recognize_patch_headers {
            if let Some(rest) = trimmed.strip_prefix(PATCH_HEADER_MARKER) {
                self.closing_marker = Some(REPLACE_END_MARKER);
                self.open_label = Some(rest.trim().to_string());
            }
        }
        true
    }

    /// `Some(path)` when a block is still open — i.e. the scan ended while
    /// this tracker was still swallowing lines as block content. Every line
    /// after that opener was hidden from the parser that owns this tracker,
    /// so a caller finding `Some` here must fail rather than return whatever
    /// it managed to collect before the opener.
    fn open_block_label(&self) -> Option<&str> {
        self.open_label.as_deref()
    }
}

/// Extracts `SUMMARY:` and `RATIONALE:` header lines independent of any
/// `### FILE:` or `### PATCH:` block. Shared by `parse_fenced_edits` and the
/// patch transport so summary/rationale extraction never diverges between
/// them — in particular, a patch-only response (the common case once patches
/// exist) is not silently treated as carrying no summary or rationale just
/// because it has no `### FILE:` block for the old, file-block-gated logic
/// to key off of.
pub fn parse_summary_and_rationale(raw: &str) -> (String, Vec<String>) {
    let mut summary = String::new();
    let mut rationale = Vec::new();
    let mut in_rationale = false;
    let mut tracker = BlockTracker::new();

    for line in raw.split('\n') {
        let line_for_markers = line.trim_end_matches('\r');
        let trimmed = line_for_markers.trim();

        if !tracker.consume(trimmed) {
            continue;
        }

        if trimmed.strip_prefix(FILE_MARKER).is_some()
            || trimmed.strip_prefix(PATCH_HEADER_MARKER).is_some()
        {
            in_rationale = false;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("SUMMARY:") {
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

    (summary, rationale)
}

/// True only when a `### PATCH:` header appears outside of any `### FILE:` …
/// `### END FILE` block — i.e. as a real patch, not as text quoted inside a
/// whole file's documentation content. `PATCH_FORMAT_INSTRUCTION` is itself a
/// complete example patch block and now appears in every Mason system
/// prompt, so a model writing documentation that quotes it back is not a
/// contrived case: a naive `raw.contains("### PATCH:")` check would treat
/// that quoted example as a real patch and reject (or misparse) an otherwise
/// valid whole-file response.
pub fn has_top_level_patch_header(raw: &str) -> bool {
    let mut tracker = BlockTracker::new();

    for line in raw.split('\n') {
        let line_for_markers = line.trim_end_matches('\r');
        let trimmed = line_for_markers.trim();

        let is_top_level = tracker.consume(trimmed);
        if is_top_level && trimmed.strip_prefix(PATCH_HEADER_MARKER).is_some() {
            return true;
        }
    }

    false
}

/// A hash-prefixed line shaped like one of this codebase's edit markers but
/// spelled wrong — `#### FILE:`, `### File:`, `## PATCH:`, `#### TOOL:`.
///
/// **Shared home on purpose.** This matcher was written, and tuned across three
/// rounds of false-positive work, for the opt-in tool loop
/// (`mason_tools::unreadable_write_marker`). The single-shot lanes had exactly
/// the same defect — a near-miss header is top-level prose to
/// `collect_fenced_edits`, so the whole block behind it is dropped without a
/// word — and were fixed by reusing this function rather than writing a second
/// one. Both lanes now call it, so neither can drift into a different idea of
/// what "nearly a marker" means. Keep it that way: any change here must be
/// weighed against both `unreadable_write_marker` and `unreadable_edit_marker`.
///
/// **The asymmetry here runs the opposite way to a parser's, and getting it
/// backwards is what made the first version of this function a Critical.** A
/// missed near-miss costs one file. A *false* near-miss on a terminal message
/// costs the entire run and every write in it: the rejection is deterministic,
/// so the model re-emits the same text, this function refuses it identically,
/// the budget drains, and the caller bails discarding writes it had already
/// collected. `## Patch notes` over a finished job was enough to do it. So this
/// matcher is deliberately strict, and anything it is unsure about is left
/// alone.
///
/// Strictness comes from requiring a *marker-shaped terminator*, not merely a
/// word boundary. A word boundary alone still matches `## Patch notes` and
/// `## File changes`, because the boundary is the space — and headings of
/// exactly that form are how a model naturally writes a summary:
///
/// - `TOOL` / `FILE` / `PATCH` / `END FILE` carry a path or a name, so they must
///   be followed by `:`. Nothing else counts, including end-of-line.
/// - `CONTENT` / `END CONTENT` standing alone are not matched here at all: see
///   the note at the end of the function for why a bare content marker cannot
///   be told from a heading, and why letting it through loses nothing.
pub(crate) fn near_miss_marker_word(trimmed: &str) -> Option<&'static str> {
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if !(2..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed[hashes..].trim_start().to_ascii_uppercase();

    // The colon is the whole tell, and it is required. Every genuine marker of
    // these four introduces a path or a name, so it always has one — while
    // `## File`, `## Patch` and `## Tool` are ordinary headings a model writes
    // when summarizing its work. An `after.is_empty()` clause here bought zero
    // coverage and cost three runs outright. Longest first, so `END FILE`
    // is never read as `FILE`.
    for word in ["END FILE", "TOOL", "FILE", "PATCH"] {
        let Some(after) = rest.strip_prefix(word) else {
            continue;
        };
        if after.starts_with(':') {
            return Some(word);
        }
    }

    // Standalone `CONTENT` / `END CONTENT` / `END FILE` are deliberately NOT
    // sufficient on their own. `## Content` is shape-identical to `#### CONTENT`
    // — the line alone cannot be told apart from a heading, and guessing wrong
    // costs the whole run. They are only meaningful beside a header, which the
    // callers find first: a block terminator with no header names no path, so
    // no write can be reconstructed from it and none is lost by letting it
    // through.
    None
}

/// Block-aware pre-scan for the *default* (non-tool-loop) edit transports.
/// `Some(reason)` means the response carries a line that is trying to open or
/// close an edit block in a spelling neither `collect_fenced_edits` nor
/// `parse_patch_blocks` reads.
///
/// This is the single-shot mirror of `mason_tools::unreadable_write_marker`,
/// and it exists because those two parsers recognise a header only on exact
/// `### FILE:` / `### PATCH:` and a closer only on exact `### END FILE`.
/// Anything that misses — `#### FILE:`, `### File:`, `###FILE:`, `## FILE:` —
/// is not a parse error. It is *top-level prose*, and so is every line of the
/// block behind it. A response mixing one correct block with one misspelled one
/// therefore returned `Ok` with the misspelled file simply absent: the run
/// reported success and the operator lost a file they were told was written.
/// It failed loudly only when *every* block was misspelled.
///
/// Three properties make this safe to run on the live path:
///
/// - **Block-aware.** Body lines of a well-formed `### FILE:` or `### PATCH:`
///   block are skipped via `BlockTracker`, because this repo contains several
///   files that legitimately document these markers, and `#### FILE:` inside a
///   file's own content is that file's text. A naive substring scan would
///   reject such a response forever, on every retry — the mistake
///   `has_top_level_patch_header` was fixed for.
/// - **Exact markers pass through.** They are what the parsers read; only
///   near-misses and foreign transports reach the matcher.
/// - **The matcher is [`near_miss_marker_word`]**, already tuned so ordinary
///   markdown headings (`## Files changed`, `## Patch notes`) do not match.
///
/// The cost of a false positive here is bounded: `complete_edit_proposal_with_retry`
/// runs two attempts, and the second failure surfaces as a loud
/// `invalid_llm_edit_response`. That is strictly better than the silent loss it
/// replaces.
///
/// Note this deliberately does *not* look for JSON edit proposals. JSON is
/// still a live transport on this lane, so a JSON-shaped body must be *routed*
/// to `parse_mason_edit_proposal`, not rejected — which is why the caller makes
/// that routing decision before calling this.
pub fn unreadable_edit_marker(raw: &str) -> Option<String> {
    let mut tracker = BlockTracker::new();

    for (index, line) in raw.split('\n').enumerate() {
        let trimmed = line.trim_end_matches('\r').trim();

        // Inside a block body, or on the closer line of one: content, not
        // structure.
        if !tracker.consume(trimmed) {
            continue;
        }

        // The exact markers these lanes *do* read are fine — they are the
        // reason the response parses at all.
        if trimmed.starts_with(FILE_MARKER)
            || trimmed.starts_with(PATCH_HEADER_MARKER)
            || trimmed == END_MARKER
        {
            continue;
        }

        if let Some(word) = near_miss_marker_word(trimmed) {
            return Some(format!(
                "line {}: {trimmed:?} looks like a '{word}' marker but is not one — this lane \
                 reads only '{FILE_MARKER}', '{END_MARKER}' and '{PATCH_HEADER_MARKER}', spelled \
                 exactly, with three hashes and in upper case. Every line of the block behind a \
                 misspelled marker would be read as prose and dropped, so the response is \
                 rejected instead.",
                index + 1
            ));
        }
    }

    None
}

/// Whole-file blocks, or an error explaining why there are none. For the
/// fenced transport a response without a single `### FILE:` block is a
/// failure, so that stays an error here.
pub fn parse_fenced_edits(raw: &str) -> Result<FencedEnvelope> {
    match collect_fenced_edits(raw)? {
        Some(envelope) => Ok(envelope),
        None => bail!("no {FILE_MARKER} blocks were found in the response"),
    }
}

/// The same scan, for callers where *absence* of `### FILE:` blocks is
/// legitimate but *malformation* is not — the patch transport, where a
/// patch-only response is the common case and carries no file blocks at all.
///
/// This exists so those two outcomes cannot be conflated. Testing the error
/// message, or running `parse_fenced_edits` best-effort and ignoring its
/// `Err`, would silently discard a file block the model meant to write
/// whenever it failed to parse for any *other* reason — the operator loses a
/// file and the run still reports success.
pub fn parse_fenced_edits_optional(raw: &str) -> Result<Option<FencedEnvelope>> {
    collect_fenced_edits(raw)
}

/// `Ok(None)` means the response contained no file blocks. Any structural
/// problem is an `Err`; nothing is ever dropped quietly.
fn collect_fenced_edits(raw: &str) -> Result<Option<FencedEnvelope>> {
    let (summary, rationale) = parse_summary_and_rationale(raw);
    let mut files = Vec::new();

    // This loop implements its own FILE-block open/close tracking (`current`)
    // rather than going through `BlockTracker`, because it also needs to
    // accumulate the body content and raise the specific errors below —
    // `BlockTracker` only reports in/out. It agrees with `BlockTracker` on
    // what closes a FILE block only because both compare against the same
    // `END_MARKER` constant; that constant is the entire coupling. If this
    // loop's notion of "closed" ever changes, `BlockTracker` (and therefore
    // `parse_summary_and_rationale`, `has_top_level_patch_header`, and
    // `parse_patch_blocks`'s FILE-block skipping) must change with it.
    let mut current: Option<(String, Vec<String>)> = None;

    // Between file blocks, `### PATCH:` bodies are skipped. A patch that edits
    // a file documenting this very format legitimately carries `### FILE:` and
    // `### END FILE` lines inside its SEARCH and REPLACE text; reading those
    // as a file block of this parser's own invents a whole-file write nobody
    // asked for, or rejects the response over a stray terminator that was
    // really just patch content. This tracker is only consulted at top level —
    // a `### PATCH:` line inside an open file block is that file's content and
    // is handled by the `current` branch below, before we get here.
    let mut patch_tracker = BlockTracker::patch_blocks_only();

    // Whether a top-level `### PATCH:` header has been seen. A patch block has
    // no terminator of its own — it ends at `>>>>>>> REPLACE` — while
    // `FENCED_FORMAT_INSTRUCTION` tells the model there must be "nothing after
    // the final ### END FILE". A model that closes a response whose last block
    // is a patch therefore reaches for `### END FILE`, which is the only
    // closing convention it was given. Observed verbatim on run 82f6fb01: two
    // FILE blocks, two PATCH blocks, and a single trailing `### END FILE`.
    // Rejecting that discards a fully correct set of edits over punctuation.
    let mut saw_patch_header = false;

    // Split on \n but preserve original lines (including trailing \r for CRLF)
    let lines: Vec<&str> = raw.split('\n').collect();
    for (line_num, line) in lines.iter().enumerate() {
        // For marker detection, work with a version that has \r stripped from the end
        let line_for_markers = line.trim_end_matches('\r');
        let trimmed = line_for_markers.trim();

        // If we're inside a file block, accumulate content
        if let Some((path, body)) = current.as_mut() {
            // Check if this line is exactly the END_MARKER
            if trimmed == END_MARKER {
                let path = path.clone();
                // One trailing newline, always — matching what the old JSON
                // transport produced and what essentially every tool on the
                // other side of this expects. `body.join("\n")` alone gave
                // every file written through this transport `\ No newline at
                // end of file`, failing `cargo fmt --check` and lint gates on
                // otherwise-correct output and making a read-then-write-
                // verbatim register as a change. The format cannot express a
                // file *without* a trailing newline; `FENCED_FORMAT_INSTRUCTION`
                // says so, exactly as `PATCH_FORMAT_INSTRUCTION` documents the
                // mirror-image limitation in the patch lane.
                let mut content = body.join("\n");
                if !content.is_empty() {
                    content.push('\n');
                }
                files.push(FencedFile { path, content });
                current = None;
            } else if trimmed.strip_prefix(FILE_MARKER).is_some() {
                // A FILE_MARKER encountered inside an open block is an error
                bail!(
                    "encountered a {FILE_MARKER} marker at line {} while the {FILE_MARKER} \
                     block for {path:?} was still open — file content must not contain a line \
                     consisting solely of '### FILE:' or '### END FILE'",
                    line_num + 1
                );
            } else {
                // Add the original line (preserving \r if present)
                body.push(line.to_string());
            }
            continue;
        }

        // We're not in a file block; skip any top-level patch body, then
        // check for markers.
        if !patch_tracker.consume(trimmed) {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix(FILE_MARKER) {
            let path = rest.trim().to_string();
            if path.is_empty() {
                bail!("a {FILE_MARKER} block declared an empty path");
            }
            if path.contains('"') || path.len() > 200 {
                bail!("a {FILE_MARKER} block declared an implausible path: {path:?}");
            }
            current = Some((path, Vec::new()));
        } else if trimmed.starts_with(PATCH_HEADER_MARKER) {
            saw_patch_header = true;
        } else if trimmed == END_MARKER {
            if saw_patch_header {
                // Closes out a response whose last block was a patch. No file
                // block is open, so this cannot be file content — the intent is
                // unambiguous and there is nothing to gain by refusing it.
                continue;
            }
            // A stray END_MARKER outside any open block is an error
            bail!(
                "found a stray {END_MARKER} at line {} not associated with any open file block — \
                 file content must not contain a line consisting solely of '### FILE:' or \
                 '### END FILE'",
                line_num + 1
            );
        }
        // SUMMARY:/RATIONALE:/rationale-item lines are metadata already
        // captured by `parse_summary_and_rationale` above; nothing to do
        // with them here.
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

    // An unterminated patch swallowed every line after it, so any file block
    // beyond that point was skipped rather than parsed. Returning the blocks
    // collected before it would drop the rest without a word.
    if let Some(path) = patch_tracker.open_block_label() {
        bail!(
            "the {PATCH_HEADER_MARKER} block for {path:?} was never closed with \
             {REPLACE_END_MARKER} — every line after it, including any further {FILE_MARKER} \
             blocks, was read as that patch's content. The response was cut off."
        );
    }

    if files.is_empty() {
        return Ok(None);
    }

    Ok(Some(FencedEnvelope {
        summary,
        rationale,
        files,
    }))
}

pub const PATCH_FORMAT_INSTRUCTION: &str = "\
When changing an existing file, emit a patch rather than the whole file:

### PATCH: <relative/path>
<<<<<<< SEARCH
<text to find, copied exactly from the current file>
=======
<text to put in its place>
>>>>>>> REPLACE

A patch block ends at its '>>>>>>> REPLACE' line. It has no terminator of its \
own: do NOT write '### END FILE' after a patch — that marker closes ### FILE: \
blocks only.

The SEARCH text must appear exactly once in the file, copied character for \
character including indentation. Use a whole ### FILE: block instead when \
creating a new file.

IMPORTANT: The SEARCH and REPLACE sections must not contain lines that consist \
solely of '<<<<<<< SEARCH', '=======', or '>>>>>>> REPLACE', nor may they start \
with '### PATCH:' — the parser cannot distinguish such lines from real delimiters. \
If a file contains these lines, use a whole ### FILE: block instead of a patch.

IMPORTANT: The format cannot express a trailing newline in the REPLACE section. \
To delete a line, include an adjacent line as context in both SEARCH and REPLACE. \
For example, to delete 'line 2' from a three-line file, search for 'line 1\\nline 2' \
and replace with 'line 1' — do not search for just 'line 2' and replace with empty, \
which would leave a blank line.";

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
        0 => match apply_reindented_patch(original, block) {
            Some(patched) => Ok(patched),
            None => bail!(
                "patch for {} did not match: the SEARCH text is not present in the file",
                block.path
            ),
        },
        1 => Ok(original.replacen(block.search.as_str(), &block.replace, 1)),
        n => bail!(
            "patch for {} is ambiguous: the SEARCH text matched {n} times, so the target is \
             unclear. Include more surrounding context to make it unique.",
            block.path
        ),
    }
}

/// Second chance for a SEARCH block that is correct except for a uniform
/// indentation shift.
///
/// Models re-indent. Observed on run b19b7236: gemma proposed a one-line
/// insertion into an HTML file with its SEARCH line indented four spaces, while
/// the real file has that line at column zero. Everything else — the text, the
/// intent, the surrounding edits — was right, and the whole four-file proposal
/// was thrown away over the leading whitespace of one line.
///
/// The match is deliberately conservative. Lines must agree exactly once their
/// own indentation is removed, the whole block must shift by the *same* amount
/// (so relative nesting is real, not guessed), and the result must be unique in
/// the file. Anything less certain falls through to the ordinary error, because
/// a wrong patch applied silently is far worse than a rejected one.
fn apply_reindented_patch(original: &str, block: &PatchBlock) -> Option<String> {
    let search_lines: Vec<&str> = block.search.split('\n').collect();
    let file_lines: Vec<&str> = original.split('\n').collect();
    if search_lines.is_empty() || search_lines.len() > file_lines.len() {
        return None;
    }

    let indent_of = |line: &str| -> String {
        line.chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    };

    // Every candidate window whose lines match once indentation is stripped and
    // whose shift is consistent across the block.
    let mut hits: Vec<(usize, String)> = Vec::new();
    for start in 0..=(file_lines.len() - search_lines.len()) {
        let window = &file_lines[start..start + search_lines.len()];

        let mut shift: Option<String> = None;
        let matched = window.iter().zip(search_lines.iter()).all(|(have, want)| {
            if have.trim() != want.trim() {
                return false;
            }
            if have.trim().is_empty() {
                return true; // a blank line carries no indentation signal
            }
            let have_indent = indent_of(have);
            match &shift {
                None => {
                    shift = Some(have_indent);
                    true
                }
                // The shift must be the same everywhere, or the block's own
                // nesting differs from the file's and this is not a re-indent.
                Some(first) => {
                    let want_first = indent_of(search_lines[0]);
                    let want_here = indent_of(want);
                    have_indent.len() as i64 - want_here.len() as i64
                        == first.len() as i64 - want_first.len() as i64
                }
            }
        });

        if matched {
            hits.push((start, shift.unwrap_or_default()));
        }
    }

    let (start, file_indent) = match hits.as_slice() {
        [single] => single.clone(),
        _ => return None, // no match, or ambiguous — let the caller report it
    };

    // Re-indent REPLACE from the block's own indentation onto the file's.
    let search_indent = indent_of(search_lines.iter().find(|l| !l.trim().is_empty())?);
    let replace_lines: Vec<String> = block
        .replace
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return line.to_string();
            }
            let stripped = line.strip_prefix(search_indent.as_str()).unwrap_or(line);
            format!("{file_indent}{stripped}")
        })
        .collect();

    let mut out: Vec<String> = file_lines[..start].iter().map(|l| l.to_string()).collect();
    out.extend(replace_lines);
    out.extend(
        file_lines[start + search_lines.len()..]
            .iter()
            .map(|l| l.to_string()),
    );
    Some(out.join("\n"))
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

    // This state machine is a flat scan over the whole response with no
    // awareness of `### FILE:` boundaries on its own — patch markers inside
    // a file block's body are content, not structure, so they must never
    // reach the checks below. A `### FILE:` block that documents or quotes
    // PATCH_FORMAT_INSTRUCTION (syntactically valid patch grammar, sitting
    // right there as file content) would otherwise be parsed as a second,
    // bogus patch and reject the entire response, permanently, alongside a
    // real top-level patch in the same reply.
    let mut file_tracker = BlockTracker::file_blocks_only();

    // Split on '\n' and keep any trailing '\r', exactly as `collect_fenced_edits`
    // does. `raw.lines()` strips the '\r' of a CRLF response, so a patch against
    // a CRLF file produced SEARCH text with LF endings that could never match
    // the file's CRLF ones. `apply_patch_block` refused — correctly and loudly —
    // but the model has no way to express a '\r' in this format, so the retry
    // was deterministic and the file was permanently unpatchable.
    for (line_idx, line) in raw.split('\n').enumerate() {
        let line_num = line_idx + 1;
        let marker_trim = line.trim_end_matches('\r').trim();

        // Only consult the FILE tracker between patches. Inside an open patch
        // (`section != 0`) every line belongs to that patch's SEARCH or
        // REPLACE body — including a `### FILE:` line, which there is ordinary
        // text being searched for or written, not the start of a file block.
        // Letting the tracker open mid-body silently dropped the rest of the
        // SEARCH text, and a truncated SEARCH that still matched somewhere
        // applied a real edit in the wrong place with `Ok` returned.
        //
        // This also keeps the two states mutually exclusive: the tracker can
        // only open while `section == 0`, and while it is open every line is
        // skipped, so `section` cannot move. An open tracker therefore always
        // implies `section == 0` — which is exactly why `section`'s own
        // end-of-input guard below could not see this failure.
        if section == 0 && !file_tracker.consume(marker_trim) {
            continue;
        }

        if let Some(rest) = marker_trim.strip_prefix(PATCH_MARKER) {
            // Guard: no new patch while one is open (section != 0)
            if section != 0 {
                let prev_path = path.as_ref().map(|p| p.as_str()).unwrap_or("(unknown)");
                let new_path = rest.trim();
                bail!(
                    "line {line_num}: a new patch header appeared while the block for \
                     {prev_path:?} was still open — found {PATCH_MARKER} {new_path:?}"
                );
            }
            path = Some(rest.trim().to_string());
            search.clear();
            replace.clear();
            section = 0;
        } else if marker_trim == SEARCH_START {
            // Guard: SEARCH marker only valid outside a block (section == 0)
            if section != 0 {
                bail!(
                    "line {line_num}: found {SEARCH_START} inside an open block (section {section}), \
                     expected only at the start of a new block"
                );
            }
            section = 1;
        } else if marker_trim == DIVIDER {
            // Guard: divider only valid in SEARCH section (section == 1)
            if section == 1 {
                section = 2;
            } else if section == 2 {
                bail!(
                    "line {line_num}: found a second {DIVIDER} in one patch block, \
                     each block has exactly one divider"
                );
            } else {
                bail!(
                    "line {line_num}: found {DIVIDER} outside of SEARCH section (section {section}), \
                     expected only after {SEARCH_START}"
                );
            }
        } else if marker_trim == REPLACE_END {
            // Guard: terminator only valid in REPLACE section (section == 2)
            if section == 2 {
                let Some(current_path) = path.clone() else {
                    bail!("line {line_num}: found {REPLACE_END} without a preceding {PATCH_MARKER} line");
                };
                blocks.push(PatchBlock {
                    path: current_path,
                    search: search.join("\n"),
                    replace: replace.join("\n"),
                });
                path = None;
                search.clear();
                replace.clear();
                section = 0;
            } else if section == 1 {
                bail!(
                    "line {line_num}: found {REPLACE_END} while still in SEARCH section, \
                     expected {DIVIDER} before {REPLACE_END}"
                );
            } else {
                bail!(
                    "line {line_num}: found {REPLACE_END} outside of any patch block (section {section}), \
                     no open block to close"
                );
            }
        } else if section == 1 {
            search.push(line.to_string());
        } else if section == 2 {
            replace.push(line.to_string());
        }
    }

    // A `### FILE:` block left open swallowed every line after it — including
    // any further, perfectly well-formed `### PATCH:` blocks, which never
    // reached the state machine above at all. `section` is necessarily still 0
    // in that case (see the tracker note in the loop), so its guard below
    // cannot notice, and returning `Ok` here would hand back a partial edit
    // list with no signal that anything was dropped. Skipping content is only
    // safe while the block that justified the skip is known to close.
    if let Some(path) = file_tracker.open_block_label() {
        bail!(
            "the {FILE_MARKER} block for {path:?} was never closed with {END_MARKER} — every line \
             after it, including any further {PATCH_MARKER} blocks, was read as that block's \
             content. The response was cut off. Raise MASON_EDIT_MAX_TOKENS or narrow the \
             editable surface."
        );
    }
    if section != 0 {
        bail!("a patch block was never closed with {REPLACE_END} — the response was cut off");
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for run b19b7236: a real observed case. SEARCH was
    /// indented four spaces, the file has the tag at column zero.
    #[test]
    fn patch_tolerates_a_uniform_indentation_shift() {
        let original = "<script src=\"js/music.js\"></script>\n\
                        <script src=\"js/one.js\"></script>\n\
                        </body>\n";
        let block = PatchBlock {
            path: "index.html".into(),
            search: "    <script src=\"js/one.js\"></script>".into(),
            replace:
                "    <script src=\"js/one.js\"></script>\n    <script src=\"js/two.js\"></script>"
                    .into(),
        };

        let patched = apply_patch_block(original, &block).expect("a re-indented patch must apply");

        assert!(patched.contains("\n<script src=\"js/two.js\"></script>"));
        assert!(
            !patched.contains("    <script src=\"js/two.js\">"),
            "the inserted line must take the file's indentation, not the patch's"
        );
        assert!(patched.contains("<script src=\"js/music.js\">"));
    }

    /// The tolerance must not invent a match. Different *text* stays an error
    /// however the whitespace lines up.
    #[test]
    fn patch_reindent_does_not_match_different_text() {
        let original = "<script src=\"js/other.js\"></script>\n";
        let block = PatchBlock {
            path: "index.html".into(),
            search: "    <script src=\"js/one.js\"></script>".into(),
            replace: "    nope".into(),
        };

        assert!(apply_patch_block(original, &block).is_err());
    }

    /// Nor may it pick one of several equally plausible sites.
    #[test]
    fn patch_reindent_refuses_an_ambiguous_match() {
        let original = "  a\nb\n  a\n";
        let block = PatchBlock {
            path: "f".into(),
            search: "a".into(),
            replace: "c".into(),
        };

        let err = apply_patch_block(original, &block)
            .expect_err("two candidate sites must not be guessed between");
        assert!(err.to_string().contains("ambiguous") || err.to_string().contains("not present"));
    }

    /// Regression for run 82f6fb01. Mason emitted two FILE blocks, two PATCH
    /// blocks, and closed the response with a single `### END FILE` after the
    /// last patch — the only closing convention the prompt had given it. The
    /// parser rejected the whole proposal over that one line, discarding four
    /// correct edits.
    #[test]
    fn trailing_end_file_after_a_patch_block_is_tolerated() {
        let raw = "SUMMARY: add the module\n\
                   RATIONALE:\n\
                   - created the module\n\
                   \n\
                   ### FILE: js/two.js\n\
                   G.registry.item = { id: 'item' };\n\
                   ### END FILE\n\
                   \n\
                   ### PATCH: README.md\n\
                   <<<<<<< SEARCH\n\
                   - **Act 3**\n\
                   =======\n\
                   - **New Section**\n\
                   - **Act 3**\n\
                   >>>>>>> REPLACE\n\
                   ### END FILE\n";

        let envelope = parse_fenced_edits(raw).expect("a trailing END FILE must not reject");
        assert_eq!(envelope.files.len(), 1, "the FILE block must still parse");
        assert_eq!(envelope.files[0].path, "js/two.js");
    }

    /// The tolerance above must not extend to a response with no patch in it:
    /// there, a stray terminator really is malformed content.
    #[test]
    fn trailing_end_file_without_any_patch_is_still_rejected() {
        let raw = "SUMMARY: s\n\
                   \n\
                   ### FILE: js/a.js\n\
                   var a = 1;\n\
                   ### END FILE\n\
                   ### END FILE\n";

        assert!(
            parse_fenced_edits(raw).is_err(),
            "a stray terminator with no patch block is still an error"
        );
    }

    #[test]
    fn fenced_envelope_passes_source_code_through_verbatim() {
        let raw = r#"SUMMARY: Add the module
RATIONALE:
- followed the existing G.rooms shape
- registered the room in main.js

### FILE: js/two.js
G.registry.entry = {
  id: 'entry',
  verbs: { lookat: "A dusty shelf", open: "It creaks" },
  note: "quotes \" and backslashes \\ survive"
};
### END FILE

### FILE: README.md
The extra entry ("Workshop") is optional.
### END FILE
"#;

        let envelope = parse_fenced_edits(raw).expect("must parse");

        assert_eq!(envelope.summary, "Add the module");
        assert_eq!(envelope.rationale.len(), 2);
        assert_eq!(envelope.files.len(), 2);
        assert_eq!(envelope.files[0].path, "js/two.js");
        assert!(envelope.files[0]
            .content
            .contains(r#"lookat: "A dusty shelf""#));
        assert!(envelope.files[0]
            .content
            .contains(r#"backslashes \\ survive"#));
        assert!(envelope.files[1].content.contains(r#"("Workshop")"#));
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
        assert!(
            parse_fenced_edits(raw).is_err(),
            "empty path must be refused"
        );
    }

    #[test]
    fn fenced_envelope_rejects_content_containing_a_terminator_line() {
        // Finding 1: a ### END FILE on a line by itself inside content should error, not truncate
        let raw = "SUMMARY: Fix\n\n### FILE: a.txt\nbefore\n### END FILE\nafter\n### END FILE\n";
        let error = parse_fenced_edits(raw).expect_err("content with terminator must fail");
        let error_msg = format!("{error:#}");
        assert!(
            error_msg.contains("stray") && error_msg.contains("not associated"),
            "should report stray terminator, got: {error_msg}"
        );
    }

    #[test]
    fn fenced_envelope_rejects_nested_file_marker() {
        // Finding 2: a ### FILE: inside an open block should error, not be swallowed
        let raw = "SUMMARY: Fix\n\n### FILE: a.txt\nfirst\n### FILE: b.txt\nsecond\n### END FILE\n";
        let error = parse_fenced_edits(raw).expect_err("nested file marker must fail");
        let error_msg = format!("{error:#}");
        assert!(
            error_msg.contains("while the") && error_msg.contains("still open"),
            "should report nested marker, got: {error_msg}"
        );
    }

    #[test]
    fn fenced_envelope_accepts_indented_terminator() {
        // Finding 3: an indented ### END FILE should be recognized as a terminator (symmetric trim)
        let raw = "SUMMARY: Fix\n\n### FILE: a.txt\ncontent\n  ### END FILE\n";
        let envelope = parse_fenced_edits(raw).expect("indented terminator must parse");
        assert_eq!(envelope.files.len(), 1);
        assert_eq!(envelope.files[0].path, "a.txt");
        // Trailing newline is added by the transport — see
        // `fenced_envelope_gives_every_file_a_trailing_newline`.
        assert_eq!(envelope.files[0].content, "content\n");
    }

    #[test]
    fn fenced_envelope_loses_the_tail_after_an_indented_terminator() {
        // I4: the round-1 symmetric-trim fix correctly made an indented
        // `### END FILE` close the block — and thereby turned a loud truncation
        // into a silent one. Everything after it is top-level prose, which this
        // transport legitimately ignores (models write closing remarks), so
        // there is no way to distinguish lost content from a sign-off. It
        // cannot be made loud without rejecting well-formed responses, so it is
        // pinned here and stated in FENCED_FORMAT_INSTRUCTION instead.
        let raw = "SUMMARY: x\n\n### FILE: a.txt\nreal line\n  ### END FILE\nTAIL LINE ONE\n\
                   TAIL LINE TWO\n";
        let envelope = parse_fenced_edits(raw).expect("an indented terminator closes the block");
        assert_eq!(envelope.files.len(), 1);
        assert_eq!(
            envelope.files[0].content, "real line\n",
            "everything after the indented terminator is discarded — the model must be told"
        );
        assert!(
            FENCED_FORMAT_INSTRUCTION.contains("Indenting such a line does NOT make it safe"),
            "the instruction must warn about this, as TOOL_LOOP_INSTRUCTION does"
        );
    }

    #[test]
    fn fenced_envelope_gives_every_file_a_trailing_newline() {
        // I3: `body.join("\n")` alone produced content that never ended in a
        // newline, so every file in every Mason branch carried
        // `\ No newline at end of file`. The old JSON transport did not.
        let raw = "SUMMARY: x\n\n### FILE: a.txt\nline1\nline2\n### END FILE\n";
        let envelope = parse_fenced_edits(raw).expect("must parse");
        assert_eq!(envelope.files[0].content, "line1\nline2\n");

        // An empty file stays empty — a lone newline is not "no content".
        let empty = "SUMMARY: x\n\n### FILE: a.txt\n### END FILE\n";
        let envelope = parse_fenced_edits(empty).expect("must parse");
        assert_eq!(envelope.files[0].content, "");

        // Content that already ends in a blank line keeps exactly one more
        // newline, not two — the blank line is a body line of its own.
        let blank_tail = "SUMMARY: x\n\n### FILE: a.txt\nline1\n\n### END FILE\n";
        let envelope = parse_fenced_edits(blank_tail).expect("must parse");
        assert_eq!(envelope.files[0].content, "line1\n\n");
    }

    #[test]
    fn unreadable_edit_marker_rejects_near_miss_spellings() {
        // C1: each of these returned `Ok` with the misspelled block's file
        // simply absent — one correct block was enough to make the run
        // "succeed".
        let cases = [
            "SUMMARY: x\n\n### FILE: a.js\naaa\n### END FILE\n\n#### FILE: b.js\nbbb\n#### END FILE\n",
            "SUMMARY: x\n\n### FILE: a.js\naaa\n### END FILE\n\n### File: b.js\nbbb\n### End File\n",
            "SUMMARY: x\n\n### FILE: a.js\naaa\n### END FILE\n\n###FILE: b.js\nbbb\n###END FILE\n",
            "SUMMARY: x\n\n### FILE: a.js\naaa\n### END FILE\n\n## FILE: b.js\nbbb\n## END FILE\n",
            "SUMMARY: x\n\n### FILE: a.js\naaa\n### END FILE\n\n#### PATCH: b.js\n<<<<<<< SEARCH\nq\n=======\nr\n>>>>>>> REPLACE\n",
            "SUMMARY: x\n\n### PATCH: a.js\n<<<<<<< SEARCH\nq\n=======\nr\n>>>>>>> REPLACE\n\n#### FILE: b.js\nbbb\n#### END FILE\n",
        ];
        for raw in cases {
            assert!(
                unreadable_edit_marker(raw).is_some(),
                "a near-miss marker must be rejected, not read as prose: {raw:?}"
            );
            // The old behaviour: the parser is perfectly happy, and the
            // misspelled block is gone.
            if let Ok(Some(envelope)) = parse_fenced_edits_optional(raw) {
                assert!(
                    envelope.files.iter().all(|file| file.path != "b.js"),
                    "this test is only meaningful while the parser still drops b.js"
                );
            }
        }
    }

    #[test]
    fn unreadable_edit_marker_accepts_well_formed_responses() {
        // The false-positive side, which is the expensive one: a rejection here
        // is deterministic and costs the whole run.
        let allowed = [
            // Exact markers.
            "SUMMARY: x\n\n### FILE: a.js\naaa\n### END FILE\n",
            "SUMMARY: x\n\n### PATCH: a.js\n<<<<<<< SEARCH\nq\n=======\nr\n>>>>>>> REPLACE\n",
            // Ordinary markdown headings a model writes when summarizing.
            "SUMMARY: x\n\n## Files changed\n## Patch notes\n### Summary of the work\n\
             ### FILE: a.js\naaa\n### END FILE\n",
            // Near-miss spellings *inside* a file block are that file's own
            // content. This repo contains several files that document them.
            "SUMMARY: x\n\n### FILE: docs/format.md\n#### FILE: example\n#### END FILE\n### END FILE\n",
            // And inside a patch body.
            "SUMMARY: x\n\n### PATCH: docs/format.md\n<<<<<<< SEARCH\n#### FILE: example\n\
             =======\n#### PATCH: example\n>>>>>>> REPLACE\n",
        ];
        for raw in allowed {
            assert_eq!(
                unreadable_edit_marker(raw),
                None,
                "a well-formed response must not be rejected: {raw:?}"
            );
        }
    }

    #[test]
    fn patch_blocks_preserve_crlf_line_endings() {
        // I5: `raw.lines()` stripped the '\r' of a CRLF response, so the SEARCH
        // text could never match a CRLF file. `apply_patch_block` refused
        // correctly — but the format cannot express a '\r', so the retry was
        // deterministic and the file was permanently unpatchable.
        let raw = "### PATCH: a.js\r\n<<<<<<< SEARCH\r\nline b\r\n=======\r\nline B\r\n>>>>>>> REPLACE\r\n";
        let blocks = parse_patch_blocks(raw).expect("must parse");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].search, "line b\r");
        assert_eq!(blocks[0].replace, "line B\r");

        let original = "line a\r\nline b\r\nline c\r\n";
        let patched = apply_patch_block(original, &blocks[0]).expect("a CRLF patch must apply");
        assert_eq!(patched, "line a\r\nline B\r\nline c\r\n");
    }

    #[test]
    fn fenced_envelope_preserves_crlf_line_endings() {
        // Finding 4: CRLF line endings should round-trip unchanged (not be stripped to LF)
        // When split on '\n', CRLF becomes visible as \r in each line, verifying \r is preserved
        let raw = "SUMMARY: Fix\r\n\r\n### FILE: a.txt\r\nline1\r\nline2\r\n### END FILE\r\n";
        let envelope = parse_fenced_edits(raw).expect("CRLF must parse");
        assert_eq!(envelope.files.len(), 1);
        // The content preserves \r from CRLF line endings (not stripped by .lines())
        // line1\r\nline2\r represents two lines with CRLF on the first, and CR remaining on the
        // second from the split
        assert!(
            envelope.files[0].content.contains("\r"),
            "CRLF must be preserved, not stripped to LF"
        );
        // Plus the one trailing newline every file gets — the second line's
        // own CR is still there, so the file ends "\r\n" as a CRLF file should.
        assert_eq!(envelope.files[0].content, "line1\r\nline2\r\n");
    }

    #[test]
    fn fenced_envelope_rejects_prose_starting_with_file_marker() {
        // Residual finding: content line starting with "### FILE:" but containing more
        // (like "### FILE: this is prose about files") must error with nested marker message
        // even though it's not an exact marker match, because the parser cannot distinguish it
        let raw = "SUMMARY: Docs\n\n### FILE: README.md\n\
                   The envelope format is described in task-2-brief.md.\n\
                   ### FILE: this is prose about files, not a marker\n\
                   But it starts with the marker phrase.\n\
                   ### END FILE\n";
        let error = parse_fenced_edits(raw).expect_err("prose starting with marker must fail");
        let error_msg = format!("{error:#}");
        assert!(
            error_msg.contains("while the") && error_msg.contains("still open"),
            "should report nested marker, got: {error_msg}"
        );
    }

    #[test]
    fn fenced_envelope_ignores_file_markers_inside_a_patch_body() {
        // The mirror of the FILE-block skipping in parse_patch_blocks: a
        // patch that edits a file which itself documents the edit format has
        // `### FILE:` / `### END FILE` lines in its SEARCH and REPLACE bodies.
        // Those are that patch's content. Reading them as a file block of its
        // own invents a whole-file write the model never asked for.
        let raw = "SUMMARY: x\n\n### PATCH: docs/format.md\n<<<<<<< SEARCH\n### FILE: phantom\nbody\n### END FILE\n=======\nrewritten\n>>>>>>> REPLACE\n\n### FILE: real.txt\nreal content\n### END FILE\n";

        let envelope = parse_fenced_edits(raw).expect("the real file block must still parse");
        assert_eq!(
            envelope.files.len(),
            1,
            "only the top-level file block is a file, got: {:?}",
            envelope.files
        );
        assert_eq!(envelope.files[0].path, "real.txt");
    }

    #[test]
    fn fenced_envelope_rejects_a_patch_block_that_never_closes() {
        // Same rule as parse_patch_blocks': skipping is deferral, not
        // discard. An unterminated patch hides every later file block, so
        // returning what was collected before it would drop them silently.
        let raw = "SUMMARY: x\n\n### PATCH: a.js\n<<<<<<< SEARCH\nline a\n=======\nLINE A\n\n### FILE: real.txt\nreal content\n### END FILE\n";
        let error = parse_fenced_edits(raw).expect_err("an unterminated patch must fail loudly");
        let msg = format!("{error:#}");
        assert!(
            msg.contains("never closed") && msg.contains("a.js"),
            "the error must name the unclosed patch, got: {msg}"
        );
    }

    #[test]
    fn optional_fenced_edits_separate_absence_from_malformation() {
        // The distinction the patch transport needs: a patch-only response
        // legitimately has no file blocks, but a *malformed* file block must
        // never be mistaken for that case and dropped.
        let patch_only =
            "SUMMARY: x\n\n### PATCH: a.js\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n";
        assert!(
            parse_fenced_edits_optional(patch_only)
                .expect("absence of file blocks is not an error")
                .is_none(),
            "a patch-only response has no file blocks — that is absence, not malformation"
        );

        let malformed = "SUMMARY: x\n\n### PATCH: a.js\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n\n### FILE: outer.txt\nfirst\n### FILE: inner.txt\nsecond\n### END FILE\n";
        let error = parse_fenced_edits_optional(malformed)
            .expect_err("a malformed file block must be reported, not reported as absence");
        assert!(
            format!("{error:#}").contains("still open"),
            "expected the nested-marker error, got: {error:#}"
        );
    }

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

    #[test]
    fn has_top_level_patch_header_detects_a_real_patch() {
        let raw =
            "SUMMARY: x\n\n### PATCH: js/a.js\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n";
        assert!(has_top_level_patch_header(raw));
    }

    #[test]
    fn has_top_level_patch_header_ignores_a_patch_quoted_inside_a_file_block() {
        // The exact failure mode this function exists to prevent: PATCH_FORMAT_INSTRUCTION
        // is a complete example patch block, and it now appears in every Mason
        // system prompt, so a whole-file response documenting or quoting it
        // back must not be mistaken for a real patch.
        let raw = format!(
            "SUMMARY: docs\n\n### FILE: docs/PATCHES.md\n{}\n### END FILE\n",
            PATCH_FORMAT_INSTRUCTION
        );
        assert!(
            !has_top_level_patch_header(&raw),
            "a ### PATCH: line quoted inside a ### FILE: block must not count as top-level"
        );
    }

    #[test]
    fn has_top_level_patch_header_is_false_with_no_patch_marker_at_all() {
        let raw = "SUMMARY: x\n\n### FILE: a.txt\ncontent\n### END FILE\n";
        assert!(!has_top_level_patch_header(raw));
    }

    #[test]
    fn has_top_level_patch_header_still_finds_a_real_patch_after_a_file_block() {
        let raw = "SUMMARY: x\n\n### FILE: a.txt\ncontent\n### END FILE\n\n### PATCH: b.js\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n";
        assert!(has_top_level_patch_header(raw));
    }

    #[test]
    fn parse_patch_blocks_rejects_duplicate_divider_in_replace() {
        // Defect 1: a second ======= inside REPLACE section should error
        let raw = "### PATCH: js/a.js\n<<<<<<< SEARCH\nsearch\n=======\nreplace1\n=======\nmore\n>>>>>>> REPLACE\n";
        let error = parse_patch_blocks(raw).expect_err("must reject second divider");
        let msg = format!("{error:#}");
        assert!(msg.contains("=======") && msg.contains("second"));
    }

    #[test]
    fn parse_patch_blocks_rejects_stray_terminator() {
        // Defect 2: >>>>>>> REPLACE appearing after a block closes is a stray terminator
        let raw = "### PATCH: js/a.js\n<<<<<<< SEARCH\nsearch\n=======\nreplace\n>>>>>>> REPLACE\ntrailing\n>>>>>>> REPLACE\n";
        let error = parse_patch_blocks(raw).expect_err("must reject stray terminator");
        let msg = format!("{error:#}");
        assert!(msg.contains(">>>>>>> REPLACE") || msg.contains("no open"));
    }

    #[test]
    fn parse_patch_blocks_rejects_search_marker_in_replace_section() {
        // Defect 3: <<<<<<< SEARCH inside REPLACE section should error
        let raw = "### PATCH: js/a.js\n<<<<<<< SEARCH\nsearch\n=======\nline1\n<<<<<<< SEARCH\nline2\n>>>>>>> REPLACE\n";
        let error =
            parse_patch_blocks(raw).expect_err("must reject SEARCH marker in REPLACE section");
        let msg = format!("{error:#}");
        assert!(msg.contains("<<<<<<< SEARCH"));
    }

    #[test]
    fn parse_patch_blocks_rejects_overlapping_patch_headers() {
        // Defect 4: ### PATCH: appearing before previous block closes silently discards it
        let raw = "### PATCH: a.js\n<<<<<<< SEARCH\nsearch a\n=======\nreplace a\n### PATCH: b.js\n<<<<<<< SEARCH\nsearch b\n=======\nreplace b\n>>>>>>> REPLACE\n";
        let error = parse_patch_blocks(raw).expect_err("must reject overlapping blocks");
        let msg = format!("{error:#}");
        assert!(msg.contains("a.js") || msg.contains("while the block"));
    }

    #[test]
    fn parse_patch_blocks_rejects_a_file_block_that_never_closes() {
        // Reproduction: a real patch, then a ### FILE: block that is never
        // closed, then a second genuine patch. The FILE tracker opens on the
        // unclosed block and every later line — including the whole second
        // patch — is skipped as if it were block content, so the patch state
        // machine never sees it and `section` is still 0 at EOF. Before the
        // fix this returned Ok with only the first patch: a complete, valid
        // edit silently discarded with no error anywhere.
        let raw = "### PATCH: a.js\n<<<<<<< SEARCH\nline a\n=======\nLINE A\n>>>>>>> REPLACE\n\
                   \n### FILE: broken.js\nsome content that never terminates\n\
                   \n### PATCH: b.js\n<<<<<<< SEARCH\nline b\n=======\nLINE B\n>>>>>>> REPLACE\n";

        let error = parse_patch_blocks(raw).expect_err(
            "a ### FILE: block left open swallows every later patch, so it must fail loudly",
        );
        let msg = format!("{error:#}");
        assert!(
            msg.contains("never closed") && msg.contains("broken.js"),
            "the error must name the unclosed block, got: {msg}"
        );
    }

    #[test]
    fn parse_patch_blocks_keeps_file_markers_inside_a_patch_body_as_content() {
        // A patch to a file that itself documents the edit format: its SEARCH
        // text legitimately contains `### FILE:` / `### END FILE` lines. The
        // FILE tracker must not open mid-patch-body, or those lines are eaten
        // out of the SEARCH text and the truncated remainder is matched
        // against the file — which applies a real edit in the wrong place,
        // silently, with Ok returned.
        let raw = "### PATCH: docs/format.md\n<<<<<<< SEARCH\n### FILE: x\nbody\n### END FILE\n\
                   =======\nreplaced\n>>>>>>> REPLACE\n";

        let blocks = parse_patch_blocks(raw).expect("a real patch body is content, not structure");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "docs/format.md");
        assert_eq!(
            blocks[0].search, "### FILE: x\nbody\n### END FILE",
            "the whole SEARCH body must survive, not just its first line"
        );
        assert_eq!(blocks[0].replace, "replaced");
    }

    #[test]
    fn patch_format_cannot_express_trailing_newline() {
        // Defect 5: The format cannot express trailing newlines; deleting a line without context leaves a blank
        let block = PatchBlock {
            path: "test.txt".to_string(),
            search: "line 2".to_string(),
            replace: "".to_string(),
        };
        let original = "line 1\nline 2\nline 3\n";
        let result = apply_patch_block(original, &block).expect("must apply");
        // Deleting "line 2" leaves a blank line because the search doesn't include the newline
        assert_eq!(result, "line 1\n\nline 3\n");
    }
}
