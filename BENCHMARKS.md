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
| tau2-bench | `TAU2_BENCH_COMMAND`, optional `TAU2_BENCH_ROOT` |
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

## Results Table Template

Use this template in the README or release notes once scores are available:

| Benchmark | Subsystem | Metric | Harkonnen | Baseline | Source | Date |
| --- | --- | --- | ---: | ---: | --- | --- |
| LongMemEval-S | Coobie | Accuracy | pending | pending | reproduced baseline | pending |
| LoCoMo QA | Coobie | Proxy QA score | pending | pending | reproduced baseline | pending |
| tau2-bench | PackChat | Pass^1 | pending | pending | official or reproduced | pending |
| SWE-bench Verified | Code loop | % Resolved | pending | pending | official leaderboard | pending |
| SWE-bench Pro | Code loop | % Resolved | pending | pending | official leaderboard or paper | pending |

## Near-Term Follow-up

The current toolchain is intentionally adapter-friendly. The next high-value follow-ups are:

- publish side-by-side LongMemEval PackChat versus raw-LLM results in the README
- publish side-by-side LoCoMo PackChat versus raw-LLM results in the README
- wire tau2-bench trajectories into PackChat artifacts
- add a first-class SWE-bench submission/export path for both Verified and Pro
- add a repo-native hidden-scenario and twin-fidelity benchmark suite for Sable and Ash
