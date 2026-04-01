use serde::{Deserialize, Serialize};

use crate::{
    coobie::CausalReport,
    models::{CoobieBriefing, FactoryEvent},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidginSignal {
    pub line_index: usize,
    pub phrase: String,
    pub normalized: String,
    pub kind: String,
    pub severity: String,
    pub meaning: String,
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidginTranslation {
    pub source: String,
    pub raw: String,
    pub signals: Vec<PidginSignal>,
}

pub fn prepend_pidgin(pidgin: &str, detail: &str) -> String {
    let pidgin = pidgin.trim();
    let detail = detail.trim();
    match (pidgin.is_empty(), detail.is_empty()) {
        (true, true) => String::new(),
        (false, true) => pidgin.to_string(),
        (true, false) => detail.to_string(),
        (false, false) => format!("{}
{}", pidgin, detail),
    }
}

pub fn pidgin_summary(event: &FactoryEvent) -> String {
    let mut phrases = Vec::new();
    let lower = event.message.to_lowercase();

    match event.status.as_str() {
        "complete" => phrases.push(if event.agent == "coobie" {
            "thassrealgrate jerry"
        } else {
            "thassgrate jerry"
        }),
        "running" => phrases.push(if event.phase == "twin" {
            "field is weird"
        } else {
            "pack is workin"
        }),
        "warning" => phrases.push(if lower.contains("forced") || lower.contains("failed") {
            "thasrealnotgrate"
        } else {
            "thasnotgrate"
        }),
        "failed" | "error" => phrases.push("thasrealnotgrate"),
        _ => {}
    }

    match event.agent.as_str() {
        "coobie" => {
            if event.status == "running" {
                phrases.push("coobie smells somethin");
            } else if event.status == "complete" {
                if lower.contains("briefing") || lower.contains("causal") || lower.contains("memory") {
                    phrases.push("coobie found the trail");
                }
                if lower.contains("required check") || lower.contains("guardrail") {
                    phrases.push("coobie would try this jerry");
                }
            }
            if lower.contains("confus") || lower.contains("unclear") {
                phrases.push("coobie is confuzd");
            }
        }
        "scout" => {
            if lower.contains("ambigu") || lower.contains("missing") || lower.contains("unclear") {
                phrases.push("thasconfusin jerry");
            } else if event.status == "complete" {
                phrases.push("scout is a real geed dawg");
            }
        }
        "mason" => {
            if event.status == "warning" || lower.contains("fail") {
                phrases.push("mason is not a geed dawg");
            } else if event.status == "complete" {
                phrases.push("mason is a real geed dawg");
            }
        }
        "bramble" => {
            if event.status == "warning" || lower.contains("fail") {
                phrases.push("still not grrrate");
            } else if event.status == "running" {
                phrases.push("gonna try agin");
            }
        }
        "sable" => {
            if event.status == "warning" || lower.contains("hidden") {
                phrases.push("thasrealnotgrate");
            }
        }
        "ash" => {
            if event.status == "running" || lower.contains("twin") {
                phrases.push("field is weird");
            }
            if event.status == "complete" {
                phrases.push("field is clean");
            }
        }
        "flint" => {
            if event.status == "complete" {
                phrases.push("brought it back jerry");
            }
        }
        "keeper" => {
            if event.status == "warning" || event.status == "failed" || lower.contains("policy") {
                phrases.push("keeper says no");
            }
        }
        "piper" => {
            if event.status == "complete" {
                phrases.push("this stick better");
            } else if event.status == "running" {
                phrases.push("need different stick");
            }
        }
        _ => {}
    }

    if lower.contains("complex") || lower.contains("many") {
        phrases.push("too many smells");
    }
    if lower.contains("coordination") || lower.contains("conflict") {
        phrases.push("pack is messy");
    }

    dedupe_join(&phrases)
}

pub fn pidgin_for_agent_result(agent_name: &str, summary: &str, output: &str) -> String {
    let mut phrases = Vec::new();
    let lower = format!("{}
{}", summary, output).to_lowercase();

    if contains_any(&lower, &["failed", "failure", "missing", "warning", "blocked"]) {
        phrases.push("thasnotgrate");
    } else if contains_any(
        &lower,
        &["prepared", "captured", "ready", "provisioned", "stored", "packaged", "passed"],
    ) {
        phrases.push("thassgrate");
    }

    match agent_name {
        "coobie" => {
            if contains_any(&lower, &["causal", "briefing", "trail", "memory"]) {
                phrases.push("coobie found the trail");
            } else {
                phrases.push("coobie smells somethin");
            }
            if contains_any(&lower, &["guardrail", "required check", "intervention", "recommend"]) {
                phrases.push("coobie would try this jerry");
            }
        }
        "scout" => {
            if contains_any(&lower, &["ambigu", "missing", "unclear"]) {
                phrases.push("thasconfusin jerry");
            } else {
                phrases.push("scout is a real geed dawg");
            }
        }
        "mason" => phrases.push(if contains_any(&lower, &["failed", "blocked", "warning"]) {
            "mason is not a geed dawg"
        } else {
            "mason is a real geed dawg"
        }),
        "bramble" => phrases.push(if contains_any(&lower, &["failed", "warning"]) {
            "still not grrrate"
        } else {
            "gonna try agin"
        }),
        "sable" => phrases.push("thasrealnotgrate"),
        "ash" => phrases.push(if contains_any(&lower, &["gap", "missing", "stub"]) {
            "field is weird"
        } else {
            "field is clean"
        }),
        "flint" => phrases.push("brought it back jerry"),
        "piper" => phrases.push(if contains_any(&lower, &["gap", "missing", "unsupported"]) {
            "need different stick"
        } else {
            "this stick better"
        }),
        "keeper" => phrases.push(if contains_any(&lower, &["policy", "risk", "deny", "violation"]) {
            "keeper says no"
        } else {
            "keeper is a real geed dawg"
        }),
        _ => {}
    }

    dedupe_join(&phrases)
}

pub fn coobie_briefing_pidgin(briefing: &CoobieBriefing) -> String {
    let mut phrases = vec!["coobie smells somethin"];
    if !briefing.prior_causes.is_empty() || !briefing.relevant_lessons.is_empty() {
        phrases.push("coobie found the trail");
    }
    if !briefing.recommended_guardrails.is_empty() || !briefing.required_checks.is_empty() {
        phrases.push("coobie would try this jerry");
    }
    if !briefing.open_questions.is_empty() {
        phrases.push("thasconfusin jerry");
    } else {
        phrases.push("thassrealgrate jerry");
    }
    dedupe_join(&phrases)
}

pub fn coobie_report_pidgin(report: &CausalReport) -> String {
    let mut phrases = vec!["coobie found the trail"];
    if report.primary_cause.is_some() {
        phrases.push("coobie thinks this is why");
    } else {
        phrases.push("coobie lost the trail");
    }
    if !report.recommended_interventions.is_empty() {
        phrases.push("coobie would try this jerry");
    }
    if report.primary_confidence >= 0.75 {
        phrases.push("thassrealgrate jerry");
    } else {
        phrases.push("thasskinda-grate");
    }
    dedupe_join(&phrases)
}

pub fn translate_pidgin_text(source: &str, raw: &str) -> PidginTranslation {
    let signals = raw
        .lines()
        .enumerate()
        .filter_map(|(index, line)| translate_pidgin_line(line, index))
        .collect::<Vec<_>>();

    PidginTranslation {
        source: source.to_string(),
        raw: raw.to_string(),
        signals,
    }
}

fn translate_pidgin_line(line: &str, line_index: usize) -> Option<PidginSignal> {
    let phrase = line.trim();
    if phrase.is_empty() {
        return None;
    }

    let normalized = normalize_phrase(phrase);
    let mut signal = match normalized.as_str() {
        "thassgrate" | "thassgrate jerry" => build_signal(
            line_index,
            phrase,
            &normalized,
            "success",
            "ok",
            "Operation succeeded or the current state looks healthy.",
            None,
        ),
        "thassrealgrate" | "thassrealgrate jerry" => build_signal(
            line_index,
            phrase,
            &normalized,
            "success",
            "high",
            "Operation succeeded with high confidence.",
            None,
        ),
        "thasskinda-grate" => build_signal(
            line_index,
            phrase,
            &normalized,
            "success",
            "medium",
            "Partial success or a mixed-good outcome.",
            None,
        ),
        "thasnotgrate" => build_signal(
            line_index,
            phrase,
            &normalized,
            "failure",
            "medium",
            "The operation failed or the current state is not acceptable.",
            None,
        ),
        "thasrealnotgrate" => build_signal(
            line_index,
            phrase,
            &normalized,
            "failure",
            "high",
            "A major failure or severe mismatch was detected.",
            None,
        ),
        "thasconfusin jerry" => build_signal(
            line_index,
            phrase,
            &normalized,
            "ambiguity",
            "medium",
            "The request, spec, or evidence is unclear and needs clarification.",
            None,
        ),
        "coobie is confuzd" => build_signal(
            line_index,
            phrase,
            &normalized,
            "ambiguity",
            "medium",
            "Coobie cannot reason cleanly from the current context.",
            Some("coobie"),
        ),
        "keeper says no" => build_signal(
            line_index,
            phrase,
            &normalized,
            "policy",
            "high",
            "Keeper flagged a safety or policy violation.",
            Some("keeper"),
        ),
        "coobie smells somethin" => build_signal(
            line_index,
            phrase,
            &normalized,
            "reasoning",
            "medium",
            "Coobie detected a causal clue worth investigating.",
            Some("coobie"),
        ),
        "coobie found the trail" => build_signal(
            line_index,
            phrase,
            &normalized,
            "reasoning",
            "high",
            "Coobie found a plausible causal chain.",
            Some("coobie"),
        ),
        "coobie lost the trail" => build_signal(
            line_index,
            phrase,
            &normalized,
            "reasoning",
            "medium",
            "Coobie could not sustain the causal chain to a clean explanation.",
            Some("coobie"),
        ),
        "coobie thinks this is why" => build_signal(
            line_index,
            phrase,
            &normalized,
            "reasoning",
            "high",
            "Coobie is stating a causal hypothesis.",
            Some("coobie"),
        ),
        "coobie would try this jerry" => build_signal(
            line_index,
            phrase,
            &normalized,
            "intervention",
            "medium",
            "Coobie is recommending an intervention or next action.",
            Some("coobie"),
        ),
        "gonna try agin" => build_signal(
            line_index,
            phrase,
            &normalized,
            "iteration",
            "low",
            "The system is retrying or attempting another pass.",
            None,
        ),
        "need different stick" => build_signal(
            line_index,
            phrase,
            &normalized,
            "iteration",
            "medium",
            "The current approach is wrong and needs to change.",
            None,
        ),
        "this stick better" => build_signal(
            line_index,
            phrase,
            &normalized,
            "iteration",
            "low",
            "A revised approach looks better than the previous one.",
            None,
        ),
        "still not grrrate" => build_signal(
            line_index,
            phrase,
            &normalized,
            "failure",
            "medium",
            "The problem persists after another attempt.",
            None,
        ),
        "pack is workin" => build_signal(
            line_index,
            phrase,
            &normalized,
            "state",
            "low",
            "Agents appear aligned and coordination looks healthy.",
            None,
        ),
        "pack is messy" => build_signal(
            line_index,
            phrase,
            &normalized,
            "state",
            "medium",
            "Coordination is degraded or conflicting.",
            None,
        ),
        "too many smells" => build_signal(
            line_index,
            phrase,
            &normalized,
            "state",
            "medium",
            "The system is dealing with too much complexity or too many simultaneous concerns.",
            None,
        ),
        "field is clean" => build_signal(
            line_index,
            phrase,
            &normalized,
            "environment",
            "low",
            "The simulated or operational environment looks ready and coherent.",
            None,
        ),
        "field is weird" => build_signal(
            line_index,
            phrase,
            &normalized,
            "environment",
            "medium",
            "The environment looks inconsistent, incomplete, or risky.",
            None,
        ),
        _ => {
            if let Some(agent) = normalized.strip_suffix(" is not a geed dawg") {
                build_signal(
                    line_index,
                    phrase,
                    &normalized,
                    "behavior",
                    "high",
                    "The agent violated expectations or failed its role.",
                    Some(agent),
                )
            } else if let Some(agent) = normalized.strip_suffix(" tryin to be a geed dawg") {
                build_signal(
                    line_index,
                    phrase,
                    &normalized,
                    "behavior",
                    "low",
                    "The agent is partially complying but has not fully met expectations yet.",
                    Some(agent),
                )
            } else if let Some(agent) = normalized.strip_suffix(" is a real geed dawg") {
                build_signal(
                    line_index,
                    phrase,
                    &normalized,
                    "behavior",
                    "low",
                    "The agent is behaving correctly and fulfilling its role well.",
                    Some(agent),
                )
            } else if normalized.starts_with("thass") && normalized.contains("grate") {
                build_signal(
                    line_index,
                    phrase,
                    &normalized,
                    "success",
                    "medium",
                    "A positive success signal was emitted.",
                    None,
                )
            } else if normalized.starts_with("thas") && normalized.contains("notgrate") {
                build_signal(
                    line_index,
                    phrase,
                    &normalized,
                    "failure",
                    "medium",
                    "A negative failure signal was emitted.",
                    None,
                )
            } else {
                return None;
            }
        }
    };

    if signal.agent.is_none() && signal.kind == "behavior" {
        signal.agent = Some(phrase.split_whitespace().next().unwrap_or_default().to_lowercase());
    }

    Some(signal)
}

fn build_signal(
    line_index: usize,
    phrase: &str,
    normalized: &str,
    kind: &str,
    severity: &str,
    meaning: &str,
    agent: Option<&str>,
) -> PidginSignal {
    PidginSignal {
        line_index,
        phrase: phrase.to_string(),
        normalized: normalized.to_string(),
        kind: kind.to_string(),
        severity: severity.to_string(),
        meaning: meaning.to_string(),
        agent: agent.map(|value| value.to_string()),
    }
}

fn normalize_phrase(phrase: &str) -> String {
    phrase
        .trim()
        .trim_matches(|ch: char| matches!(ch, '!' | '.' | ',' | ':' | ';'))
        .to_lowercase()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn dedupe_join(phrases: &[&str]) -> String {
    let mut out = Vec::new();
    for phrase in phrases {
        if !out.iter().any(|existing: &String| existing == phrase) {
            out.push((*phrase).to_string());
        }
    }
    out.join("
")
}
