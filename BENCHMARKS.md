# Harkonnen Labs Benchmark Strategy

Harkonnen should publish benchmark results as a suite, not a single score.
Different Labrador roles do different work, so benchmark coverage needs to map
onto the actual system responsibilities.

## Benchmark Matrix

### Memory and retrieval — vs Mem0 / MindPalace / Zep

| Suite | Harkonnen subsystem | Why it matters | Primary metric | Baseline source |
| --- | --- | --- | --- | --- |
| LongMemEval | Coobie / PackChat memory | Long-term assistant memory, multi-session reasoning, knowledge updates, temporal reasoning, abstention | QA accuracy by category | Official benchmark repo + reproduced baselines |
| LongMemEval raw baseline | Underlying LLM only | Same dataset scored without PackChat orchestration so Harkonnen gains are visible | QA accuracy by category | Reproduced local baseline using the same provider routing |
| LoCoMo | Coobie / long-horizon dialogue memory | Very long conversations, event summarization, temporal/causal dialogue structure | QA first, then summarization metrics | Official repo + ACL 2024 paper |
| FRAMES | Coobie / multi-hop retrieval | Multi-hop factual recall across long documents — Mem0 publishes here, making it the primary competitive line | Factual accuracy (multi-hop) | Google DeepMind paper + reproduced baseline |
| StreamingQA | Coobie / memory invalidation | Tests correct belief *updates* when facts change — no vector-only competitor has explicit fact-update tracking | Belief-update accuracy | Reproduced baseline using same provider |
| HELMET | Coobie / retrieval precision | Precision/recall on long-context retrieval — validates whether Palace patrol reduces noise vs flat similarity | Retrieval precision / recall | Official benchmark + reproduced baseline |

### Coding loop — vs OpenCode / Aider / single-agent tools

| Suite | Harkonnen subsystem | Why it matters | Primary metric | Baseline source |
| --- | --- | --- | --- | --- |
| SWE-bench Verified | Mason / Piper / Bramble | Human-validated issue resolution benchmark for the code loop | % Resolved | Official SWE-bench leaderboard |
| SWE-bench Pro | Mason / Piper / Bramble | Harder frontier coding benchmark for stronger public claims | % Resolved | Official benchmark paper / leaderboard |
| LiveCodeBench | Mason / Piper | Recent competitive programming problems post-training-cutoff — no contamination | Pass rate | Official LiveCodeBench repo + reproduced baseline |
| Aider Polyglot | Mason / Piper | Multi-language coding benchmark with public leaderboard — direct open-source comparison | % Correct | Aider published leaderboard |
| DevBench | Scout / Mason / Piper / Bramble / Flint | Full software dev lifecycle (design → impl → test → docs) — measures the pipeline, not just one phase | Lifecycle completion score | Official DevBench paper |
| Local Regression Gate | Whole factory | Fast guardrail for every change before heavier benchmark runs | pass/fail | Internal 100% pass requirement |

### Multi-turn and tool-use — vs general agent frameworks

| Suite | Harkonnen subsystem | Why it matters | Primary metric | Baseline source |
| --- | --- | --- | --- | --- |
| tau2-bench | PackChat / tool-agent-user loop | Multi-turn user interaction, tools, domain rules, policy-following | Pass^1, Pass^4 | Official Sierra leaderboard |
| GAIA Level 3 | Full factory (Scout → Piper → Sable chain) | Multi-step tool use where single-agent tools fail because they cannot delegate | Task completion rate | Official GAIA leaderboard |
| AgentBench | Whole factory / Labrador role separation | Eight environments testing specialist coordination vs single generalist | Environment pass rate | Official AgentBench paper |

### Causal reasoning — unique to Harkonnen, no competitor runs these

| Suite | Harkonnen subsystem | Why it matters | Primary metric | Baseline source |
| --- | --- | --- | --- | --- |
| CLADDER | Coobie / causal memory (Layer D) | Pearl's causal hierarchy — associational, interventional, counterfactual — maps directly to Coobie's design | Accuracy by hierarchy level | Official CLADDER paper + reproduced baseline |
| E-CARE | Coobie / diagnose output | Explainable causal reasoning — whether generated explanations are natural-language coherent | Coherence score | Official E-CARE paper |

### Harkonnen-native — cannot be run by any competitor

| Suite | Harkonnen subsystem | Why it matters | Primary metric | Baseline source |
| --- | --- | --- | --- | --- |
| Spec Adherence Rate | Scout / Mason | Completeness and precision of implementation vs stated spec — isolates the spec-first contribution | Completeness %, Precision % | Internal: with vs without Scout formalization |
| Hidden Scenario Delta | Bramble / Sable | Gap between visible test pass rate and hidden scenario pass rate — proves Sable catches what Bramble misses | Delta (hidden − visible pass rate) | Internal: per-run corpus |
| Causal Attribution Accuracy | Coobie / diagnose | Seeded failure corpus — does diagnose rank the true cause in top-1 or top-3? | Top-1 / Top-3 accuracy | Internal: labeled seeded-failure corpus |

## Publication Policy

Use two comparison layers in the GitHub repo:

1. Official or live benchmark comparisons.
   This includes tau2-bench and SWE-bench leaderboards whenever Harkonnen is run through the official harness or submission path.
2. Reproduced academic baseline comparisons.
   This includes LongMemEval and LoCoMo, where the honest comparison is usually against reproduced baselines using the official dataset and evaluation recipe.

Every published score should include:

- benchmark name and exact split or task variant
- Harkonnen commit hash
- benchmark revision or repo commit
- provider routing used during the run
- exact metric reported
- cost or token budget when available
- whether the baseline is official leaderboard data or a reproduced local baseline

## Current Toolchain

Harkonnen now includes a benchmark toolchain with these entrypoints:

```bash
cargo run -- benchmark list
cargo run -- benchmark run
cargo run -- benchmark run --suite local_regression --strict
cargo run -- benchmark run --all
cargo run -- benchmark report <results.json>
./scripts/run-benchmarks.sh
```

If `setups/lm-studio-local.toml` exists and `HARKONNEN_SETUP` is unset,
`./scripts/run-benchmarks.sh` now defaults benchmark runs to that local LM Studio setup
and seeds `LM_STUDIO_API_KEY=lm-studio` automatically when needed.

Machine-readable benchmark suites live at:

```text
factory/benchmarks/suites.yaml
```

Benchmark reports are written to:

```text
factory/artifacts/benchmarks/
```

The default `benchmark run` command executes the always-on local regression suite.
`benchmark run --all` also tries the broader benchmark suites and marks them as skipped
when their datasets or adapter commands are not configured yet.

## External Adapter Environment

The first automation pass uses small wrapper scripts so each external benchmark can be attached without changing Rust code.

| Benchmark | Required env to make it runnable |
| --- | --- |
| LongMemEval | `LONGMEMEVAL_DATASET`, optional `LONGMEMEVAL_MODE`, `LONGMEMEVAL_DIRECT_PROVIDER`, `LONGMEMEVAL_LIMIT`, `LONGMEMEVAL_OUTPUT_DIR`, `LONGMEMEVAL_OFFICIAL_EVAL_COMMAND`, `LONGMEMEVAL_OFFICIAL_EVAL_ROOT`, `LONGMEMEVAL_MIN_PROXY_EXACT`, `LONGMEMEVAL_MIN_PROXY_F1` |
| LoCoMo | `LOCOMO_DATASET`, optional `LOCOMO_MODE`, `LOCOMO_ROOT`, `LOCOMO_LIMIT`, `LOCOMO_OUTPUT_DIR`, `LOCOMO_DIRECT_PROVIDER`, `LOCOMO_MIN_PROXY_SCORE` |
| tau2-bench | `TAU2_BENCH_COMMAND`, optional `TAU2_BENCH_ROOT`, `TAU2_BENCH_AUTOSTART_HARKONNEN`, `TAU2_BENCH_API_PORT`, `TAU2_BENCH_WAIT_SECS`, `TAU2_BENCH_HEALTH_PATH`, `TAU2_BENCH_SERVER_LOG` |
| SWE-bench Verified | `SWEBENCH_COMMAND`, optional `SWEBENCH_ROOT` |
| SWE-bench Pro | `SWEBENCH_PRO_COMMAND`, optional `SWEBENCH_PRO_ROOT` |
| FRAMES | `FRAMES_DATASET`, optional `FRAMES_MODE`, `FRAMES_LIMIT`, `FRAMES_OUTPUT_DIR`, `FRAMES_DIRECT_PROVIDER`, `FRAMES_MIN_ACCURACY` |
| StreamingQA | `STREAMINGQA_DATASET`, optional `STREAMINGQA_LIMIT`, `STREAMINGQA_OUTPUT_DIR`, `STREAMINGQA_DIRECT_PROVIDER` |
| HELMET | `HELMET_COMMAND`, optional `HELMET_ROOT`, `HELMET_SPLIT` |
| LiveCodeBench | `LIVECODEBENCH_COMMAND`, optional `LIVECODEBENCH_ROOT`, `LIVECODEBENCH_LIMIT` |
| Aider Polyglot | `AIDER_POLYGLOT_COMMAND`, optional `AIDER_POLYGLOT_ROOT` |
| DevBench | `DEVBENCH_COMMAND`, optional `DEVBENCH_ROOT`, `DEVBENCH_CATEGORIES` |
| CLADDER | `CLADDER_DATASET`, optional `CLADDER_MODE`, `CLADDER_LIMIT`, `CLADDER_OUTPUT_DIR`, `CLADDER_DIRECT_PROVIDER` |
| E-CARE | `ECARE_DATASET`, optional `ECARE_LIMIT`, `ECARE_OUTPUT_DIR`, `ECARE_DIRECT_PROVIDER` |
| GAIA | `GAIA_COMMAND`, optional `GAIA_ROOT`, `GAIA_LEVEL` |
| AgentBench | `AGENTBENCH_COMMAND`, optional `AGENTBENCH_ROOT`, `AGENTBENCH_ENVS` |

LongMemEval and LoCoMo now both have native Harkonnen adapters and raw-model direct baselines inside the Rust benchmark runner. Point `LONGMEMEVAL_DATASET` or `LOCOMO_DATASET` at local dataset files to run either mode and emit local prediction and summary artifacts. The remaining suites still use external wrapper commands.

For a quick native LongMemEval comparison run, use:

```bash
LONGMEMEVAL_DATASET=factory/benchmarks/fixtures/longmemeval-smoke.json \
LONGMEMEVAL_LIMIT=1 \
cargo run -- benchmark run --suite coobie_longmemeval --suite longmemeval_raw_llm

# then move to the real dataset for reportable runs
LONGMEMEVAL_DATASET=/path/to/longmemeval_s_cleaned.json \
LONGMEMEVAL_LIMIT=25 \
cargo run -- benchmark run --suite coobie_longmemeval --suite longmemeval_raw_llm
```

For a quick native LoCoMo comparison run, use:

```bash
LOCOMO_DATASET=factory/benchmarks/fixtures/locomo-smoke.json \
LOCOMO_LIMIT=1 \
cargo run -- benchmark run --suite coobie_locomo --suite locomo_raw_llm

# then move to the real dataset for reportable runs
LOCOMO_DATASET=/path/to/locomo10.json \
LOCOMO_LIMIT=25 \
cargo run -- benchmark run --suite coobie_locomo --suite locomo_raw_llm
```

For interactive local runs where you want visible progress and raw model output, including any `<think>` blocks the model emits, use:

```bash
scripts/run-visible-benchmark.sh longmemeval 10
scripts/run-visible-benchmark.sh locomo 10
```

This launcher enables `HARKONNEN_BENCH_VERBOSE=1` and `HARKONNEN_BENCH_SHOW_RAW=1` by default, prints heartbeat lines while a question is still running, and writes a full terminal log to `factory/artifacts/benchmarks/live-logs/`.

## Recommended Reporting Order

1. Local Regression Gate on every change.
2. LongMemEval for Coobie memory quality.
3. LoCoMo QA for long-horizon dialogue memory.
4. tau2-bench for PackChat and policy-aware tool interaction.
5. SWE-bench Verified plus SWE-bench Pro for the implementation loop.
6. Harkonnen-native hidden-scenario, twin-fidelity, and policy benchmarks for system-specific claims.

## Benchmark-Specific Guidance

### LongMemEval

Use for Coobie and PackChat memory reporting. It is the best current fit for:

- information extraction from long interaction history
- multi-session reasoning
- knowledge updates
- temporal reasoning
- abstention when memory is missing

Sources:

- [Official repo](https://github.com/xiaowu0162/LongMemEval)
- [Project page](https://xiaowu0162.github.io/long-mem-eval/)

### LoCoMo

Use after LongMemEval to test whether Coobie handles much longer, more narrative conversations and event structure.
Start with the QA task before adding event summarization or multimodal evaluation.

Sources:

- [Official repo](https://github.com/snap-research/locomo)
- [Paper](https://aclanthology.org/2024.acl-long.747/)

### tau2-bench

Use for PackChat when Harkonnen is acting as a tool-using conversational agent under domain rules and policies.
When reporting publicly, include trajectories or other run artifacts whenever possible.

The repo wrapper at `scripts/benchmark-tau2.sh` now supports autostarting Harkonnen's API server and exporting a stable PackChat base URL into the external tau2 command. Set `TAU2_BENCH_AUTOSTART_HARKONNEN=1` to launch `cargo run -- serve`, then read `TAU2_BENCH_HARKONNEN_BASE_URL` or `HARKONNEN_BENCH_BASE_URL` inside your tau2 adapter command.

Sources:

- [Official repo](https://github.com/sierra-research/tau2-bench)
- [Leaderboard](https://sierra.ai/blog/t-bench-leaderboard)

### SWE-bench Verified and SWE-bench Pro

Use for Mason, Piper, and Bramble as the public coding loop benchmark story.
Verified is still useful for continuity, but frontier claims should also cite SWE-bench Pro or an equivalent harder benchmark.

Sources:

- [SWE-bench leaderboard](https://www.swebench.com/)
- [SWE-bench Verified](https://www.swebench.com/verified.html)
- [SWE-bench Pro paper page](https://labs.scale.com/papers/swe_bench_pro)
- [OpenAI note on why Verified alone is no longer enough](https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/)

### FRAMES

Use for Coobie memory quality against Mem0 and other vector-store memory systems.
FRAMES specifically tests multi-hop factual recall — questions that require chaining
two or more retrieved facts. Single-pass vector similarity cannot resolve these;
Coobie's multi-hop retrieval chain (Phase 4) is the required feature.
Run in paired mode: Coobie vs raw-LLM baseline, same provider.

Sources:

- [Paper](https://arxiv.org/abs/2409.12941)
- [Dataset (HuggingFace)](https://huggingface.co/datasets/google/frames-benchmark)

### StreamingQA

Use for Coobie belief-update accuracy. StreamingQA streams fact changes over time and
tests whether the system correctly updates its beliefs — not just whether it recalls
stored facts. No competitor publishes StreamingQA scores because no other memory system
has explicit fact-invalidation tracking. Requires the memory invalidation feature
(Phase 4) to produce meaningful results.

Sources:

- [Paper](https://arxiv.org/abs/2205.11388)

### HELMET

Use for Coobie retrieval precision/recall on long-context tasks. HELMET separates
"retrieved the right passage" from "retrieved too much noise." Run after Phase 4 to
validate whether the Palace patrol compound scent reduces retrieval noise versus
flat vector similarity.

Sources:

- [Official repo](https://github.com/princeton-nlp/HELMET)
- [Paper](https://arxiv.org/abs/2410.02669)

### LiveCodeBench

Use for Mason/Piper against OpenCode and other single-agent coding tools.
Problems are pulled from recent competitive programming contests and postdate
training cutoffs, making contamination unlikely. Run Mason's iterative fix loop
plus online-judge feedback parsing against the same problems as competitors.

Sources:

- [Official repo](https://github.com/LiveCodeBench/LiveCodeBench)
- [Leaderboard](https://livecodebench.github.io/leaderboard.html)

### Aider Polyglot

Use for Mason/Piper as a direct open-source coding agent comparison.
Aider publishes its own benchmark and leaderboard, making it the most reproducible
apples-to-apples comparison against a credible open-source competitor.
The adapter is a thin script — no structural changes to Harkonnen required.

Sources:

- [Benchmark page](https://aider.chat/docs/leaderboards/)

### DevBench

Use for Scout/Mason/Piper/Bramble/Flint as the full software development lifecycle
claim. DevBench scores design, implementation, testing, and documentation separately —
each maps to a Labrador phase. This is the structural argument against single-agent
tools that only measure the implementation phase.
Requires Flint documentation artifacts (Phase 3).

Sources:

- [Official repo](https://github.com/open-compass/DevBench)
- [Paper](https://arxiv.org/abs/2403.08604)

### CLADDER

Use for Coobie's causal memory layer (Layer D). CLADDER tests Pearl's causal
hierarchy: associational, interventional, and counterfactual questions scored
separately. This maps directly to Coobie's `diagnose` output structure after
Phase 4 adds Pearl hierarchy labeling. No memory or agent competitor publishes
CLADDER scores — this is a unique differentiating claim.
Run in paired mode: Coobie diagnose vs raw-LLM direct answer.

Sources:

- [Official repo](https://github.com/causalNLP/cladder)
- [Paper](https://arxiv.org/abs/2312.04350)

### E-CARE

Use for Coobie's `diagnose` output quality after Phase 5 consolidation is live.
E-CARE scores whether causal explanations are natural-language coherent, not just
structurally correct. Run after consolidation so that approved lessons can inform
subsequent diagnose output and improvement over time is measurable.

Sources:

- [Official repo](https://github.com/Waste-Wood/E-CARE)
- [Paper](https://arxiv.org/abs/2205.07364)

### GAIA

Use for the full factory chain (Scout → Mason → Piper → Sable) as a multi-step
delegation claim. Level 3 tasks are the target — single-agent tools fail here
because they cannot delegate sub-tasks. Run after Phase 6 when TypeDB is live so
Coobie can answer cross-run context questions mid-task.

Sources:

- [Official repo](https://huggingface.co/datasets/gaia-benchmark/GAIA)
- [Paper](https://arxiv.org/abs/2311.12983)

### AgentBench

Use for Labrador role separation against single-generalist agent frameworks.
Target the OS, database, and web environments first — these map most cleanly to
Mason/Piper (OS), Ash (DB), and Flint (web). Run after Phase 6.

Sources:

- [Official repo](https://github.com/THUDM/AgentBench)
- [Paper](https://arxiv.org/abs/2308.03688)

### Harkonnen-Native Benchmarks

**Spec Adherence Rate** — no external repo. Build the grader in
`factory/benchmarks/spec-adherence/`. Uses an LLM-as-judge prompt that extracts
requirements from the spec and scores the implementation output on completeness and
precision. Run two variants: with Scout formalization and without (raw spec → Mason).

**Hidden Scenario Delta** — no external repo. Tracked automatically from run data
once Bramble real test execution (Phase 2) and Sable hidden scenario results are
both stored in comparable format. Report surfaces the gap per spec type and
aggregates across the corpus.

**Causal Attribution Accuracy** — seeded failure corpus lives in
`factory/benchmarks/causal-attribution/`. Each entry has a spec, a seeded failure,
a ground-truth cause label, and the Coobie diagnose output from that run.
Score top-1 and top-3 accuracy. Build the corpus incrementally — 10 entries is
enough for a first baseline; 30–50 is reportable.

## Results Table Template

Use this template in the README or release notes once scores are available:

| Benchmark | Subsystem | Metric | Harkonnen | Baseline | Comparison target | Source | Date |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| LongMemEval-S | Coobie | Accuracy | pending | pending | Mem0 / raw LLM | reproduced baseline | pending |
| FRAMES | Coobie | Multi-hop accuracy | pending | pending | Mem0 / raw LLM | reproduced baseline | pending |
| StreamingQA | Coobie | Belief-update accuracy | pending | pending | raw LLM | reproduced baseline | pending |
| LoCoMo QA | Coobie | Proxy QA score | pending | pending | raw LLM | reproduced baseline | pending |
| CLADDER | Coobie | Causal accuracy by level | pending | pending | raw LLM | reproduced baseline | pending |
| E-CARE | Coobie | Coherence score | pending | pending | raw LLM | reproduced baseline | pending |
| LiveCodeBench | Mason / Piper | Pass rate | pending | pending | OpenCode / Aider | official leaderboard | pending |
| Aider Polyglot | Mason / Piper | % Correct | pending | pending | Aider | official leaderboard | pending |
| DevBench | Full factory | Lifecycle score | pending | pending | Single-agent tools | reproduced baseline | pending |
| SWE-bench Verified | Code loop | % Resolved | pending | pending | SWE-agent / OpenCode | official leaderboard | pending |
| SWE-bench Pro | Code loop | % Resolved | pending | pending | SWE-agent | official leaderboard | pending |
| tau2-bench | PackChat | Pass^1 | pending | pending | raw LLM | official or reproduced | pending |
| GAIA Level 3 | Full factory | Task completion | pending | pending | General agents | official leaderboard | pending |
| AgentBench (OS/DB/web) | Labrador roles | Env pass rate | pending | pending | Single-agent frameworks | reproduced baseline | pending |
| Spec Adherence Rate | Scout / Mason | Completeness / Precision | pending | pending | No Scout baseline | internal | pending |
| Hidden Scenario Delta | Bramble / Sable | Pass rate gap | pending | pending | Visible tests only | internal | pending |
| Causal Attribution Accuracy | Coobie diagnose | Top-1 / Top-3 | pending | pending | Semantic recall only | internal | pending |

## Near-Term Follow-up

The current toolchain is intentionally adapter-friendly. Priority order for the next adapter work:

1. Publish side-by-side LongMemEval PackChat versus raw-LLM results in the README
2. Wire FRAMES adapter — the Mem0 comparison line, highest competitive value
3. Wire CLADDER adapter — unique claim, no competitor can respond to it
4. Wire LiveCodeBench adapter — the OpenCode/Aider comparison line
5. Wire Aider Polyglot adapter — direct open-source leaderboard comparison
6. Add a first-class SWE-bench submission/export path for both Verified and Pro
7. Wire DevBench adapter after Flint doc phase ships (Phase 3)
8. Add StreamingQA adapter after memory invalidation feature ships (Phase 4)
9. Begin building causal attribution seeded failure corpus (can start now, incrementally)
10. Wire GAIA and AgentBench after Phase 6
