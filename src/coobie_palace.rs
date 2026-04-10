//! Coobie Palace — The Patch
//!
//! A spatially-organised projection layer over Coobie's causal memory.
//! Instead of a flat list of causaloids, the Patch groups them into Dens —
//! familiar territories that Coobie patrols before every run.
//!
//! ## Terminology
//!
//! | Term          | Meaning                                                   |
//! |---------------|-----------------------------------------------------------|
//! | The Patch     | The whole palace — all dens together                      |
//! | Den           | A named cluster of related causal failure patterns        |
//! | Scent         | A context bundle fetched from a den — what Coobie picks up|
//! | Patrol        | Pre-run traversal — Coobie walks each den and sniffs      |
//! | PatchPatrol   | The result of a full patrol — compound context + weight   |
//!
//! ## Design
//!
//! The palace is NOT a second memory store. It is a read-only projection that:
//! 1. Groups existing `SpecCauseSignal`s into dens by cause_id
//! 2. Computes a compound scent when multiple causes from the same den have fired
//! 3. Elevates den-level streak weight beyond individual cause streaks
//! 4. Feeds the result back into the briefing's required_checks / guardrails /
//!    open_questions — same injection points as the flat causal preflight guidance
//!
//! Coobie's causaloids remain the source of truth.  The palace only adds
//! compound recall — "the whole den smells, not just one corner."

use serde::Serialize;

use crate::models::LessonRecord;

// ── Den definitions ───────────────────────────────────────────────────────────

/// A named cluster of related causal failure patterns.
#[derive(Debug, Clone, Serialize)]
pub struct Den {
    /// Stable identifier, e.g. `"spec_den"`.
    pub id: &'static str,
    /// Human-readable name for Pack Board display.
    pub label: &'static str,
    /// Coobie's inner-monologue name for this den.
    pub nickname: &'static str,
    /// Cause IDs that belong here.
    pub residents: &'static [&'static str],
    /// Short description of what kind of failure lives in this den.
    pub description: &'static str,
}

/// All dens in the Patch.
pub const DENS: &[Den] = &[
    Den {
        id: "spec_den",
        label: "Spec Den",
        nickname: "da spec corner",
        residents: &["SPEC_AMBIGUITY", "BROAD_SCOPE"],
        description: "Failures rooted in unclear, missing, or over-scoped specifications.",
    },
    Den {
        id: "test_den",
        label: "Test Den",
        nickname: "da test room",
        residents: &["TEST_BLIND_SPOT"],
        description:
            "Failures where visible tests passed but hidden scenarios exposed blind spots.",
    },
    Den {
        id: "twin_den",
        label: "Twin Den",
        nickname: "da twin yard",
        residents: &["TWIN_GAP"],
        description:
            "Failures caused by the simulated environment not matching production conditions.",
    },
    Den {
        id: "pack_den",
        label: "Pack Den",
        nickname: "da pack corner",
        residents: &["PACK_BREAKDOWN"],
        description: "Failures caused by degraded or incomplete Labrador phase execution.",
    },
    Den {
        id: "memory_den",
        label: "Memory Den",
        nickname: "da memory spot",
        residents: &["NO_PRIOR_MEMORY"],
        description:
            "Failures where the factory ran cold — no relevant prior context was recalled.",
    },
];

// ── Den lesson — thin projection of a LessonRecord matched to a den ──────────

/// A lesson from Coobie's SQLite `lessons` table that matched this den's
/// resident cause IDs.  Carried as a thin struct so `DenScent` stays
/// serializable without pulling in `DateTime` from the full `LessonRecord`.
#[derive(Debug, Clone, Serialize)]
pub struct DenLesson {
    pub lesson_id: String,
    pub pattern: String,
    pub intervention: Option<String>,
    pub strength: f64,
}

// ── Cause signal input (mirrors SpecCauseSignal without owning it) ────────────

/// A minimal projection of a SpecCauseSignal that the palace consumes.
/// Created from orchestrator's `SpecCauseSignal` at patrol time.
#[derive(Debug, Clone)]
pub struct CauseSnapshot {
    pub cause_id: String,
    #[allow(dead_code)] // retained for future den-level narrative enrichment
    pub description: String,
    pub occurrences: usize,
    pub scenario_pass_rate: f32,
    pub streak_len: usize,
    pub escalate: bool,
}

// ── Scent — what a den emits ──────────────────────────────────────────────────

/// Context bundle fetched from a single den during patrol.
#[derive(Debug, Clone, Serialize)]
pub struct DenScent {
    /// Den identifier.
    pub den_id: &'static str,
    pub den_label: &'static str,
    /// How many distinct resident causes fired in this den.
    pub active_residents: usize,
    /// Total occurrences across all residents.
    pub total_occurrences: usize,
    /// Maximum consecutive streak across residents.
    pub peak_streak: usize,
    /// True when any resident is escalated OR the den has ≥ 2 active residents.
    pub den_escalated: bool,
    /// Compound context narrative — richer than single-cause instructions.
    pub compound_narrative: String,
    /// Concrete checks that apply at the den level (may span multiple causes).
    pub required_checks: Vec<String>,
    /// Guardrails derived from the den's collective pattern.
    pub guardrails: Vec<String>,
    /// Open questions when the den is escalated.
    pub open_questions: Vec<String>,
    /// Lessons from Coobie's SQLite store whose tags matched a resident cause ID.
    /// Up to 3 per den, ordered by strength descending.
    pub matched_lessons: Vec<DenLesson>,
}

// ── PatchPatrol — result of a full patrol ────────────────────────────────────

/// The result of Coobie walking the full patch before a run.
#[derive(Debug, Clone, Serialize)]
pub struct PatchPatrol {
    /// All dens that had at least one active resident.
    pub active_dens: Vec<DenScent>,
    /// Total active dens.
    pub active_den_count: usize,
    /// Combined weight — drives how prominently Coobie voices the patrol in the briefing.
    /// 0.0 = clean patch, 1.0 = every den is escalated.
    pub patch_weight: f32,
    /// Single-sentence summary for Pack Board display.
    pub summary: String,
}

impl PatchPatrol {
    pub fn is_clear(&self) -> bool {
        self.active_dens.is_empty()
    }
}

// ── Patrol logic ──────────────────────────────────────────────────────────────

/// Walk the patch: project `causes` into dens, compute scents, return patrol.
///
/// `lessons` is the slice of `LessonRecord`s already fetched for this run
/// (e.g. from `find_relevant_lessons`).  Lessons whose tags match a den's
/// resident cause IDs are attached to that den's scent and surfaced in the
/// briefing as guardrails.  Pass `&[]` when no lessons are available.
pub fn patrol(causes: &[CauseSnapshot], lessons: &[LessonRecord]) -> PatchPatrol {
    if causes.is_empty() {
        return PatchPatrol {
            active_dens: vec![],
            active_den_count: 0,
            patch_weight: 0.0,
            summary: "Patch is clear — no prior causal signals for this spec.".to_string(),
        };
    }

    let mut active_dens: Vec<DenScent> = Vec::new();

    for den in DENS {
        // Collect residents that are present in the causes slice.
        let residents: Vec<&CauseSnapshot> = causes
            .iter()
            .filter(|c| den.residents.contains(&c.cause_id.as_str()))
            .collect();

        if residents.is_empty() {
            continue;
        }

        let active_residents = residents.len();
        let total_occurrences: usize = residents.iter().map(|c| c.occurrences).sum();
        let peak_streak = residents.iter().map(|c| c.streak_len).max().unwrap_or(0);
        let any_escalated = residents.iter().any(|c| c.escalate);
        let den_escalated = any_escalated || active_residents >= 2;

        let matched_lessons = match_lessons_to_den(den, lessons);

        let (compound_narrative, required_checks, guardrails, open_questions) = build_den_scent(
            den,
            &residents,
            active_residents,
            peak_streak,
            den_escalated,
        );

        active_dens.push(DenScent {
            den_id: den.id,
            den_label: den.label,
            active_residents,
            total_occurrences,
            peak_streak,
            den_escalated,
            compound_narrative,
            required_checks,
            guardrails,
            open_questions,
            matched_lessons,
        });
    }

    let active_den_count = active_dens.len();

    // Patch weight: average of per-den weights.
    // Den weight = (peak_streak / 5.0).min(1.0) boosted by multi-resident compound.
    let patch_weight = if active_den_count == 0 {
        0.0
    } else {
        let sum: f32 = active_dens
            .iter()
            .map(|d| {
                let base = (d.peak_streak as f32 / 5.0_f32).min(1.0);
                if d.den_escalated {
                    (base + 0.2).min(1.0)
                } else {
                    base
                }
            })
            .sum();
        (sum / active_den_count as f32).min(1.0)
    };

    let summary = build_patrol_summary(&active_dens, patch_weight);

    PatchPatrol {
        active_dens,
        active_den_count,
        patch_weight,
        summary,
    }
}

// ── Lesson matching ───────────────────────────────────────────────────────────

/// Return the top-3 lessons (by strength) whose tags match any resident
/// cause ID of this den, or the den's own ID.
///
/// Matching is case-insensitive and exact on the tag value.  A lesson tagged
/// `"SPEC_AMBIGUITY"` matches the Spec Den; one tagged `"spec_den"` does too.
fn match_lessons_to_den(den: &Den, lessons: &[LessonRecord]) -> Vec<DenLesson> {
    let mut matched: Vec<DenLesson> = lessons
        .iter()
        .filter(|l| {
            l.tags.iter().any(|tag| {
                let t = tag.to_lowercase();
                den.residents.iter().any(|r| t == r.to_lowercase()) || t == den.id
            })
        })
        .map(|l| DenLesson {
            lesson_id: l.lesson_id.clone(),
            pattern: l.pattern.clone(),
            intervention: l.intervention.clone(),
            strength: l.strength,
        })
        .collect();

    // Strongest lessons first so the most reliable signal surfaces in the briefing.
    matched.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap_or(std::cmp::Ordering::Equal));
    matched.truncate(3);
    matched
}

// ── Den scent builder ─────────────────────────────────────────────────────────

fn build_den_scent(
    den: &Den,
    residents: &[&CauseSnapshot],
    active_residents: usize,
    peak_streak: usize,
    den_escalated: bool,
) -> (String, Vec<String>, Vec<String>, Vec<String>) {
    let mut narrative_parts: Vec<String> = Vec::new();
    let mut required_checks: Vec<String> = Vec::new();
    let mut guardrails: Vec<String> = Vec::new();
    let mut open_questions: Vec<String> = Vec::new();

    // Per-resident contributions.
    for cause in residents {
        let streak_tag = if cause.streak_len >= 2 {
            format!("[{}-run streak] ", cause.streak_len)
        } else {
            String::new()
        };

        narrative_parts.push(format!(
            "{}{} fired {} time(s) (pass rate {:.0}%)",
            streak_tag,
            cause.cause_id,
            cause.occurrences,
            cause.scenario_pass_rate * 100.0,
        ));

        // Per-cause check injection.
        match cause.cause_id.as_str() {
            "SPEC_AMBIGUITY" => {
                required_checks.push(format!(
                    "{}SPEC_AMBIGUITY: confirm every acceptance criterion has an explicit \
                     pass/fail condition and at least one failure-mode example before Mason begins.",
                    streak_tag,
                ));
                if cause.scenario_pass_rate < 0.5 {
                    guardrails.push(
                        "Spec clarity is historically low — require Scout to validate \
                         acceptance criteria completeness before implementation starts."
                            .to_string(),
                    );
                }
            }
            "BROAD_SCOPE" => {
                required_checks.push(format!(
                    "{}BROAD_SCOPE: confirm this run's deliverable is minimum scope; \
                     flag any out-of-scope agent activity for Mason to avoid.",
                    streak_tag,
                ));
            }
            "TEST_BLIND_SPOT" => {
                required_checks.push(format!(
                    "{}TEST_BLIND_SPOT: include at least one explicit failure-path test \
                     (expired credential, invalid input, permission boundary, or timeout) \
                     before this run proceeds.",
                    streak_tag,
                ));
                guardrails.push(
                    "Do not treat a green visible test suite as a proxy for scenario readiness \
                     on this spec — Sable has found blind spots here before."
                        .to_string(),
                );
            }
            "TWIN_GAP" => {
                required_checks.push(format!(
                    "{}TWIN_GAP: enumerate which production conditions are NOT simulated in \
                     the twin (auth expiry, third-party errors, network partitions) before \
                     Mason writes code that depends on them.",
                    streak_tag,
                ));
                guardrails.push(
                    "Twin fidelity has been a recurring gap — treat every external dependency \
                     as a stub risk and call it out explicitly in the twin narrative."
                        .to_string(),
                );
            }
            "NO_PRIOR_MEMORY" => {
                guardrails.push(format!(
                    "{}Memory retrieval was insufficient on {} prior run(s) — seed Coobie with \
                     domain context before re-attempting if semantic hit count is still low.",
                    streak_tag, cause.occurrences,
                ));
            }
            "PACK_BREAKDOWN" => {
                required_checks.push(format!(
                    "{}PACK_BREAKDOWN: identify which Labrador phase degraded last time and \
                     verify its prompt bundle and provider route before starting.",
                    streak_tag,
                ));
            }
            _ => {
                required_checks.push(format!(
                    "{}{} fired {} time(s) on this spec — review prior runs before proceeding.",
                    streak_tag, cause.cause_id, cause.occurrences,
                ));
            }
        }
    }

    // Compound narrative when multiple residents are active — this is the key
    // addition over flat per-cause guidance.
    if active_residents >= 2 {
        let cause_ids: Vec<&str> = residents.iter().map(|c| c.cause_id.as_str()).collect();
        let compound = build_compound_narrative(den, &cause_ids, peak_streak);
        required_checks.push(compound.clone());

        // Compound escalation open question.
        if den_escalated {
            open_questions.push(format!(
                "{} is showing a compound pattern ({}) — does the interaction between these \
                 failures suggest a structural problem rather than individual fixes?",
                den.label,
                cause_ids.join(" + "),
            ));
        }
    } else if den_escalated && peak_streak >= 3 {
        // Single-resident but escalated.
        open_questions.push(format!(
            "{} has been active for {} consecutive runs — is the current intervention \
             actually breaking the cycle, or should a different approach be tried?",
            den.label, peak_streak,
        ));
    }

    // Den-level narrative summary (always present).
    let narrative = if narrative_parts.is_empty() {
        format!("{} is active.", den.label)
    } else {
        format!("{}: {}", den.label, narrative_parts.join("; "))
    };

    (narrative, required_checks, guardrails, open_questions)
}

/// Generate a compound narrative for dens with multiple active residents.
fn build_compound_narrative(den: &Den, cause_ids: &[&str], peak_streak: usize) -> String {
    match den.id {
        "spec_den"
            if cause_ids.contains(&"SPEC_AMBIGUITY") && cause_ids.contains(&"BROAD_SCOPE") =>
        {
            format!(
                "Compound spec failure pattern: SPEC_AMBIGUITY and BROAD_SCOPE have both fired \
                 (peak streak: {} run(s)) — the spec is simultaneously unclear AND too wide. \
                 Require Scout to tighten acceptance criteria AND narrow scope before Mason begins; \
                 do not treat these as independent fixes.",
                peak_streak,
            )
        }
        _ => {
            format!(
                "Compound {} pattern: {} have all fired (peak streak: {} run(s)) — \
                 address these together, not separately.",
                den.label,
                cause_ids.join(" + "),
                peak_streak,
            )
        }
    }
}

// ── Patrol summary ────────────────────────────────────────────────────────────

fn build_patrol_summary(active_dens: &[DenScent], patch_weight: f32) -> String {
    if active_dens.is_empty() {
        return "Patch is clear.".to_string();
    }

    let escalated: Vec<&str> = active_dens
        .iter()
        .filter(|d| d.den_escalated)
        .map(|d| d.den_label)
        .collect();

    let active_labels: Vec<&str> = active_dens.iter().map(|d| d.den_label).collect();

    if escalated.is_empty() {
        format!(
            "Patrol complete — {} den(s) active ({}), patch weight {:.2}. No escalations.",
            active_dens.len(),
            active_labels.join(", "),
            patch_weight,
        )
    } else {
        format!(
            "Patrol complete — {} den(s) active ({}), {} escalated ({}), patch weight {:.2}.",
            active_dens.len(),
            active_labels.join(", "),
            escalated.len(),
            escalated.join(", "),
            patch_weight,
        )
    }
}

// ── Briefing injection ────────────────────────────────────────────────────────

/// Apply a completed `PatchPatrol` into the Coobie briefing fields.
///
/// This replaces / augments the flat `apply_causal_preflight_guidance` call with
/// compound, den-aware context.  Both can coexist — the palace adds the
/// compound layer on top.
pub fn apply_patrol_to_briefing(
    patrol: &PatchPatrol,
    required_checks: &mut Vec<String>,
    recommended_guardrails: &mut Vec<String>,
    open_questions: &mut Vec<String>,
) {
    if patrol.is_clear() {
        return;
    }

    // Prefix with the patrol summary so the LLM briefing response has context.
    let summary_check = format!("[Patch Patrol] {}", patrol.summary);
    if !required_checks.contains(&summary_check) {
        required_checks.push(summary_check);
    }

    for den in &patrol.active_dens {
        for check in &den.required_checks {
            if !required_checks.contains(check) {
                required_checks.push(check.clone());
            }
        }
        for guardrail in &den.guardrails {
            if !recommended_guardrails.contains(guardrail) {
                recommended_guardrails.push(guardrail.clone());
            }
        }
        for question in &den.open_questions {
            if !open_questions.contains(question) {
                open_questions.push(question.clone());
            }
        }
        // Surface lesson interventions as guardrails so prior pack learning
        // reaches the briefing alongside the den's structural signals.
        for lesson in &den.matched_lessons {
            if let Some(ref intervention) = lesson.intervention {
                let guardrail = format!(
                    "[{} lesson] {} → {}",
                    den.den_label, lesson.pattern, intervention
                );
                if !recommended_guardrails.contains(&guardrail) {
                    recommended_guardrails.push(guardrail);
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(cause_id: &str, occurrences: usize, streak_len: usize) -> CauseSnapshot {
        CauseSnapshot {
            cause_id: cause_id.to_string(),
            description: cause_id.to_string(),
            occurrences,
            scenario_pass_rate: 0.0,
            streak_len,
            escalate: streak_len >= 3,
        }
    }

    #[test]
    fn clear_patch_when_no_causes() {
        let patrol = patrol(&[], &[]);
        assert!(patrol.is_clear());
        assert_eq!(patrol.active_den_count, 0);
        assert_eq!(patrol.patch_weight, 0.0);
    }

    #[test]
    fn single_resident_activates_den() {
        let causes = vec![snapshot("TEST_BLIND_SPOT", 2, 2)];
        let p = patrol(&causes, &[]);
        assert_eq!(p.active_den_count, 1);
        assert_eq!(p.active_dens[0].den_id, "test_den");
        assert_eq!(p.active_dens[0].active_residents, 1);
    }

    #[test]
    fn compound_spec_den_fires_when_both_residents_present() {
        let causes = vec![
            snapshot("SPEC_AMBIGUITY", 3, 3),
            snapshot("BROAD_SCOPE", 2, 2),
        ];
        let p = patrol(&causes, &[]);
        let spec = p
            .active_dens
            .iter()
            .find(|d| d.den_id == "spec_den")
            .unwrap();
        assert_eq!(spec.active_residents, 2);
        assert!(spec.den_escalated);
        // Compound check should be present.
        assert!(spec
            .required_checks
            .iter()
            .any(|c| c.contains("Compound spec")));
    }

    #[test]
    fn patch_weight_scales_with_streak() {
        let low = patrol(&[snapshot("TEST_BLIND_SPOT", 1, 1)], &[]);
        let high = patrol(&[snapshot("TEST_BLIND_SPOT", 5, 5)], &[]);
        assert!(high.patch_weight > low.patch_weight);
    }

    #[test]
    fn apply_patrol_deduplicates_checks() {
        let causes = vec![snapshot("TEST_BLIND_SPOT", 2, 2)];
        let p = patrol(&causes, &[]);

        let mut checks = vec![];
        let mut guardrails = vec![];
        let mut questions = vec![];

        apply_patrol_to_briefing(&p, &mut checks, &mut guardrails, &mut questions);
        let initial_len = checks.len();

        // Applying twice should not duplicate.
        apply_patrol_to_briefing(&p, &mut checks, &mut guardrails, &mut questions);
        assert_eq!(checks.len(), initial_len);
    }
}
