# Harkonnen Labs Benchmark Strategy

Harkonnen should publish benchmark results as a suite, not a single score.
Different Labrador roles do different work, so benchmark coverage needs to map
onto the actual system responsibilities.

## Benchmark Matrix

| Suite | Harkonnen subsystem | Why it matters | Primary metric | Baseline source |
| --- | --- | --- | --- | --- |
| LongMemEval | Coobie / PackChat memory | Long-term assistant memory, multi-session reasoning, knowledge updates, temporal reasoning, abstention | QA accuracy by category | Official benchmark repo + reproduced baselines |
| LongMemEval raw baseline | Underlying LLM only | Same dataset scored without PackChat orchestration so Harkonnen gains are visible | QA accuracy by category | Reproduced local baseline using the same provider routing |
| LoCoMo | Coobie / long-horizon dialogue memory | Very long conversations, event summarization, temporal/causal dialogue structure | QA first, then summarization metrics | Official repo + ACL 2024 paper |
| tau2-bench | PackChat / tool-agent-user loop | Multi-turn user interaction, tools, domain rules, policy-following | Pass^1, Pass^4 | Official Sierra leaderboard |
| SWE-bench Verified | Mason / Piper / Bramble | Human-validated issue resolution benchmark for the code loop | % Resolved | Official SWE-bench leaderboard |
| SWE-bench Pro | Mason / Piper / Bramble | Harder frontier coding benchmark for stronger public claims | % Resolved | Official benchmark paper / leaderboard |
| Local Regression Gate | Whole factory | Fast guardrail for every change before heavier benchmark runs | pass/fail | Internal 100% pass requirement |

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
`benchmark run --all` also tries the external benchmark adapters and marks them as skipped
until their adapter commands are configured.

## External Adapter Environment

The first automation pass uses small wrapper scripts so each external benchmark can be attached without changing Rust code.

| Benchmark | Required env to make it runnable |
| --- | --- |
| LongMemEval | `LONGMEMEVAL_DATASET`, optional `LONGMEMEVAL_MODE`, `LONGMEMEVAL_DIRECT_PROVIDER`, `LONGMEMEVAL_LIMIT`, `LONGMEMEVAL_OUTPUT_DIR`, `LONGMEMEVAL_OFFICIAL_EVAL_COMMAND`, `LONGMEMEVAL_OFFICIAL_EVAL_ROOT`, `LONGMEMEVAL_MIN_PROXY_EXACT`, `LONGMEMEVAL_MIN_PROXY_F1` |
| LoCoMo | `LOCOMO_COMMAND`, optional `LOCOMO_ROOT` |
| tau2-bench | `TAU2_BENCH_COMMAND`, optional `TAU2_BENCH_ROOT` |
| SWE-bench Verified | `SWEBENCH_COMMAND`, optional `SWEBENCH_ROOT` |
| SWE-bench Pro | `SWEBENCH_PRO_COMMAND`, optional `SWEBENCH_PRO_ROOT` |

LongMemEval now has both a native Harkonnen adapter and a raw-model direct baseline inside the Rust benchmark runner. Point `LONGMEMEVAL_DATASET` at a local dataset file to run either mode and emit `jsonl` predictions plus local summary artifacts. The other suites still use external wrapper commands.

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

- Official repo: https://github.com/xiaowu0162/LongMemEval
- Project page: https://xiaowu0162.github.io/long-mem-eval/

### LoCoMo

Use after LongMemEval to test whether Coobie handles much longer, more narrative conversations and event structure.
Start with the QA task before adding event summarization or multimodal evaluation.

Sources:

- Official repo: https://github.com/snap-research/locomo
- Paper: https://aclanthology.org/2024.acl-long.747/

### tau2-bench

Use for PackChat when Harkonnen is acting as a tool-using conversational agent under domain rules and policies.
When reporting publicly, include trajectories or other run artifacts whenever possible.

Sources:

- Official repo: https://github.com/sierra-research/tau2-bench
- Leaderboard: https://sierra.ai/blog/t-bench-leaderboard

### SWE-bench Verified and SWE-bench Pro

Use for Mason, Piper, and Bramble as the public coding loop benchmark story.
Verified is still useful for continuity, but frontier claims should also cite SWE-bench Pro or an equivalent harder benchmark.

Sources:

- SWE-bench leaderboard: https://www.swebench.com/
- SWE-bench Verified: https://www.swebench.com/verified.html
- SWE-bench Pro paper page: https://labs.scale.com/papers/swe_bench_pro
- OpenAI note on why Verified alone is no longer enough: https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/

## Results Table Template

Use this template in the README or release notes once scores are available:

| Benchmark | Subsystem | Metric | Harkonnen | Baseline | Source | Date |
| --- | --- | --- | ---: | ---: | --- | --- |
| LongMemEval-S | Coobie | Accuracy | pending | pending | reproduced baseline | pending |
| LoCoMo QA | Coobie | QA score | pending | pending | reproduced baseline | pending |
| tau2-bench | PackChat | Pass^1 | pending | pending | official or reproduced | pending |
| SWE-bench Verified | Code loop | % Resolved | pending | pending | official leaderboard | pending |
| SWE-bench Pro | Code loop | % Resolved | pending | pending | official leaderboard or paper | pending |

## Near-Term Follow-up

The current toolchain is intentionally adapter-friendly. The next high-value follow-ups are:

- publish side-by-side LongMemEval PackChat versus raw-LLM results in the README
- add a first-class Harkonnen adapter for LoCoMo QA
- wire tau2-bench trajectories into PackChat artifacts
- add a first-class SWE-bench submission/export path for both Verified and Pro
- add a repo-native hidden-scenario and twin-fidelity benchmark suite for Sable and Ash
