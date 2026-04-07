---
tags: [run, project-memory, wire-fallback-for, Harkonnen-Labs, completed-with-issues]
summary: Run 3bc96a3c-2dbd-43eb-9ac1-14ee29d7764c for Wire CapacityState::fallback_for into build_provider_with_capacity
source_label: Harkonnen-Labs
source_kind: path
source_path: /media/earthling/Caleb's Files1/Harkonnen-Labs
source_run_id: 3bc96a3c-2dbd-43eb-9ac1-14ee29d7764c
source_spec_id: wire-fallback-for
git_branch: main
git_commit: ea32a062bad3845c86d11921b81777e15f342270
git_remote: https://github.com/durinwinter/Harkonnen-Labs.git
evidence_run_ids: [3bc96a3c-2dbd-43eb-9ac1-14ee29d7764c]
stale_when: [git commit or branch changes for the target repo, hidden scenario oracle, dataset, or runtime assumptions change]
observed_paths: [src/llm.rs, src/capacity.rs]
code_under_test_paths: [src/llm.rs, src/capacity.rs]
---

Spec: wire-fallback-for
Product: Harkonnen-Labs
Visible validation passed: true
Hidden scenarios passed: false
Recommended steps: Stage an isolated workspace on a new git branch per worker harness requirements., Read src/capacity.rs in full — extract fallback_for's exact signature, return type, visibility (pub/pub(crate)/private), parameter types, and whether it carries #[allow(dead_code)] (Coobie guardrail #1, run cf62669a guidance)., Read src/llm.rs in full — map the manual fallback_chain walk inside build_provider_with_capacity line by line, noting every edge case handled (empty chain, exhausted providers, single-element chain) (Coobie guardrail #1)., Verify structural behavioral equivalence: confirm that fallback_for produces the same provider-selection sequence as the manual walk for all reachable code paths — document any divergence before editing (Coobie guardrail #5)., If fallback_for diverges from the manual walk on edge cases, adjust fallback_for in src/capacity.rs to cover the missing cases — do not change its public method signature, only its internal logic if needed., Replace the manual fallback_chain walk in build_provider_with_capacity with a call to fallback_for — ensure the replacement is a drop-in that preserves all control flow and return semantics., Remove any #[allow(dead_code)] annotation on fallback_for if present, since the call site now exists., Run cargo check --quiet in the staged workspace — confirm zero errors and zero dead_code warnings for fallback_for (Coobie required check, confirmed helpful command per forge-preference citations across 4 prior runs)., Grep src/llm.rs for 'fallback_for' to confirm the reference exists in the output artifact (Coobie required check)., Verify all existing callers of build_provider and build_provider_with_capacity still compile without modification (Coobie required check, spec constraint)., Document what changed since forge run cf62669a before claiming the same command path passes — the working tree has 30 dirty paths (Coobie required check)., Package evidence artifacts: retriever_execution_report, forge hooks, trail drift check, and the grep confirmation before Sable runs hidden scenarios., If Sable runs hidden scenarios, ensure the twin narrative explicitly names which external systems are simulated, stubbed, or missing — this is the recurring gap that has blocked hidden scenario passage across all 5 prior runs (Coobie required check, TWIN_GAP cause with 17 occurrences at 0% scenario pass rate).

Project memory root: /media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/project-memory

Top memory hits:
[project context] [/media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/project-context.md] # Project Context

- Project: Harkonnen-Labs
- Source path: /media/earthling/Caleb's Files1/Harkonnen-Labs
- Project memory root: /media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/project-memory
- Evidence root: /media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/evidence
- Project scan: /media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/project-scan.md
- Project manifest: /media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/project-manifest.json
- Resume packet: /media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/resume-packet.md
- Strategy register: /media/ear

[project scan] [/media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/project-scan.md] # Project Scan

- Generated at: 2026-04-06T17:48:03.488870464+00:00
- Project: Harkonnen-Labs
- Source kind: path
- Source path: /media/earthling/Caleb's Files1/Harkonnen-Labs
- Project memory root: /media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/project-memory
- Git: branch=main commit=b7317ee56bb58e89d028e7f18c649c6107461b9a remote=https://github.com/durinwinter/Harkonnen-Labs.git clean=true

## Detected Files
- Cargo.toml
- README.md

## Detected Directories
- src
- ui
- scripts

## Likely Commands
- cargo check
- cargo test -q

## Runtime Hints
- Repo appears to contain both backend and UI surfaces.
- Repo already contains Harkonnen-local continuity files.

[strategy register] [/media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/strategy-register.md] # Strategy Register

- Project: Harkonnen-Labs
- Entries: 6

- [d63dd0b2-1179-4b03-b1a4-be371fce5b47:hidden_scenarios-281ce3b5-9e46-43cd-a9aa-8f1c3de193f8] phase=hidden_scenarios agent=sable strategy=sable - pack is workin
thasrealnotgrate
Evaluating hidden scenarios failure_constraint=thasnotgrate
thasrealnotgrate
Hidden scenario evaluation finished: 2 scenario(s) reformulation=hidden_scenarios phase failed; preserve surviving structure and change strategy on retry
- [1647810b-cc75-474a-a65a-4da98ada47da:hidden_scenarios-77951de8-3754-4beb-89d4-2f25f67edb2b] phase=hidden_scenarios agent=sable strategy=sable - pack is workin
thasrealnotgrate
Evaluating hidden scenarios failure_constraint=thasnotgrate
thasrealnotgrate
Hidden scenario evaluation finished: 1 scenario(s) reformulation=hidden_s

[resume packet] [/media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/resume-packet.md] # Resume Packet

- Generated at: 2026-04-07T01:38:50.753163688+00:00
- Project: Harkonnen-Labs
- Current git: branch=main commit=ea32a062bad3845c86d11921b81777e15f342270 remote=https://github.com/durinwinter/Harkonnen-Labs.git clean=false changed_paths=30

## Summary
- Current git: branch=main commit=ea32a062bad3845c86d11921b81777e15f342270 remote=https://github.com/durinwinter/Harkonnen-Labs.git clean=false changed_paths=30
- Project memory entries indexed: 24
- Entries currently at risk: 23
- Entries already marked challenged/superseded: 16
- Risk mix: critical=20 high=2 medium=1 low=0
- Working tree changed paths: harkonnen/memory-status.json, harkonnen/memory-status.md, harkonnen/project-manifest.json, harkonnen/project-memory/index.json, harkonnen/project-memory/lesson-dead-end-9d66fc

[memory status] [/media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/memory-status.md] # Memory Status

- lesson-dead-end-9d66fcca-f152-409c-8b17-1caa6df47abc-hidden-scenarios-sable-sable-pack-is-workin-thas status=challenged superseded_by=none challenged_by=lesson-hidden_scenarios-f488d905-029f-45ab-addb-5f77bb64727a, lesson-exploration-hidden_scenarios-f488d905-029f-45ab-addb-5f77bb64727a, lesson-dead-end-bd89b27f-45c1-4a8a-a893-bc067a05b459-hidden-scenarios-sable-sable-pack-is-workin-thas, lesson-hidden_scenarios-781bed0b-c9fd-490e-8a6f-6f91c98978ac, lesson-exploration-hidden_scenarios-781bed0b-c9fd-490e-8a6f-6f91c98978ac, lesson-dead-end-cf62669a-fa1c-4493-823a-eace5b5c53d6-hidden-scenarios-sable-sable-pack-is-workin-thas
- lesson-dead-end-bd89b27f-45c1-4a8a-a893-bc067a05b459-hidden-scenarios-sable-sable-pack-is-workin-thas status=challenged superseded_by=none challenged

[stale memory history] [/media/earthling/Caleb's Files1/Harkonnen-Labs/.harkonnen/stale-memory-history.md] # Stale Memory History

- Project: Harkonnen-Labs
- Records retained: 10

## Recent Runs
- - run=cf62669a-fa1c-4493-823a-eace5b5c53d6 spec=wire-fallback-for generated=2026-04-07T01:30:08.819737621+00:00 entries=14 satisfied=0 partially_satisfied=14 unresolved=0 resolved_since_previous=0
- - run=bd89b27f-45c1-4a8a-a893-bc067a05b459 spec=wire-fallback-for generated=2026-04-06T18:19:00.216086686+00:00 entries=11 satisfied=0 partially_satisfied=11 unresolved=0 resolved_since_previous=0
- - run=9d66fcca-f152-409c-8b17-1caa6df47abc spec=wire-fallback-for generated=2026-04-06T18:05:32.765682381+00:00 entries=8 satisfied=0 partially_satisfied=8 unresolved=0 resolved_since_previous=0
- - run=c4ee92c5-2d93-40ae-bdfc-4cc98ee9e049 spec=wire-fallback-for generated=2026-04-06T18:02:25.307452951+00:00 en

[project memory] [run-cf62669a-fa1c-4493-823a-eace5b5c53d6] Run cf62669a-fa1c-4493-823a-eace5b5c53d6 for Wire CapacityState::fallback_for into build_provider_with_capacity
Spec: wire-fallback-for
Product: Harkonnen-Labs
Visible validation passed: true
Hidden scenarios passed: false
Recommended steps: Read src/capacity.rs in full — confirm fallback_for signature, return type, visibility, and any #[allow(dead_code)] annotation., Read src/llm.rs in full — identify the manual fallback_chain walk inside build_provider_with_capacity and map its exact logic., Grep the 

[project memory] [run-bd89b27f-45c1-4a8a-a893-bc067a05b459] Run bd89b27f-45c1-4a8a-a893-bc067a05b459 for Wire CapacityState::fallback_for into build_provider_with_capacity
Spec: wire-fallback-for
Product: Harkonnen-Labs
Visible validation passed: true
Hidden scenarios passed: false
Recommended steps: Read Coobie's preflight briefing and required guardrails, Retrieve prior patterns with Coobie, Stage an isolated product workspace, Run visible validation before scenario work, Evaluate hidden scenarios with Sable, Package evidence for human review

Project memory root:

[project memory] [run-9d66fcca-f152-409c-8b17-1caa6df47abc] Run 9d66fcca-f152-409c-8b17-1caa6df47abc for Wire CapacityState::fallback_for into build_provider_with_capacity
Spec: wire-fallback-for
Product: Harkonnen-Labs
Visible validation passed: true
Hidden scenarios passed: false
Recommended steps: Read Coobie's preflight briefing and required guardrails, Retrieve prior patterns with Coobie, Stage an isolated product workspace, Run visible validation before scenario work, Evaluate hidden scenarios with Sable, Package evidence for human review

Project memory root:

[project memory] [run-c4ee92c5-2d93-40ae-bdfc-4cc98ee9e049] Run c4ee92c5-2d93-40ae-bdfc-4cc98ee9e049 for Wire CapacityState::fallback_for into build_provider_with_capacity
Spec: wire-fallback-for
Product: Harkonnen-Labs
Visible validation passed: true
Hidden scenarios passed: false
Recommended steps: Read Coobie's preflight briefing and required guardrails, Retrieve prior patterns with Coobie, Stage an isolated product workspace, Run visible validation before scenario work, Evaluate hidden scenarios with Sable, Package evidence for human review

Project memory root:

[project memory] [run-1647810b-cc75-474a-a65a-4da98ada47da] Run 1647810b-cc75-474a-a65a-4da98ada47da for Wire CapacityState::fallback_for into build_provider_with_capacity
Spec: wire-fallback-for
Product: Harkonnen-Labs
Visible validation passed: true
Hidden scenarios passed: false
Recommended steps: Read Coobie's preflight briefing and required guardrails, Retrieve prior patterns with Coobie, Stage an isolated product workspace, Run visible validation before scenario work, Evaluate hidden scenarios with Sable, Package evidence for human review

Project memory root:

[project memory] [lesson-phase-pattern-hidden-scenarios-sable-failure-claude-anthropic] Repeatable failure pattern in hidden_scenarios / sable via claude with anthropic/webapp-testing
Occurrences: 2
Supporting runs: d63dd0b2-1179-4b03-b1a4-be371fce5b47
Provider route: claude
Prompt bundle fingerprint: e8b6470a2c051708
Prompt bundle artifact: agents/sable_prompt_bundle.json
Pinned skills: anthropic/webapp-testing
Required checks: Visible validation must prove the main project still builds or executes in the staged workspace. | The twin narrative must state which external systems