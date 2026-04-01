import { computeEpisodePositions, CAUSE_POSITIONS } from './projection';
import { clusterEpisodes } from './clustering';

/**
 * Build a CausalTesseractScene from an array of { run, report } objects.
 *
 * run    → RunRecord  from /api/runs
 * report → CausalReport from /api/runs/:id/causal-report
 *
 * CausalReport shape (from src/coobie.rs):
 *   primary_cause:              Option<String>   — rule description string
 *   primary_confidence:         f32
 *   contributing_causes:        Vec<String>
 *   recommended_interventions:  Vec<InterventionPlan>
 *   episode_scores: {
 *     spec_clarity_score:      f32   → specQuality
 *     change_scope_score:      f32   → risk/breadth (inverted → implQuality)
 *     twin_fidelity_score:     f32   → implementationQuality
 *     test_coverage_score:     f32   → validationOutcome base
 *     memory_retrieval_score:  f32   → memory quality
 *     scenario_passed:         bool
 *     validation_passed:       bool
 *   }
 */
export function buildScene(runReports) {
  const episodeNodes = [];
  const causeMap     = new Map();
  const edges        = [];

  for (const { run, report } of runReports) {
    if (!run) continue;

    const s      = report?.episode_scores;
    const status = mapRunStatus(run.status);

    // ── Quality dimensions from real EpisodeScores ─────────────────────────
    const specQuality = s
      ? s.spec_clarity_score
      : status === 'accepted' ? 0.72 : status === 'in_progress' ? 0.55 : 0.38;

    // twin_fidelity_score tells us how faithful the implementation environment was
    const implementationQuality = s
      ? s.twin_fidelity_score
      : status === 'accepted' ? 0.78 : 0.34;

    // Blend visible test coverage with hidden scenario / validation results
    const validationOutcome = s
      ? clampUnit(
          s.test_coverage_score * 0.5 +
          (s.scenario_passed  ? 0.30 : 0.0) +
          (s.validation_passed ? 0.20 : 0.0),
        )
      : status === 'accepted' ? 0.82 : 0.18;

    // How confident Coobie is about the recommended intervention
    const confidence          = report?.primary_confidence ?? 0.5;
    const interventionPotential = report ? clampUnit(confidence * 0.8 + 0.15) : 0.40;

    // ── Cause type from primary_cause description string ───────────────────
    const primaryCauseType = inferCauseType(report?.primary_cause ?? '');

    const episode = {
      id:                    run.run_id,
      label:                 `${run.product} · ${run.spec_id}`,
      productId:             run.product,
      runId:                 run.run_id,
      specQuality,
      implementationQuality,
      validationOutcome,
      interventionPotential,
      primaryCauseType,
      confidence,
      status,
      timestamp:             run.created_at,

      // Raw scores for DetailPanel display
      rawScores: s ?? null,
      primaryCauseText:      report?.primary_cause ?? null,
      contributingCauses:    report?.contributing_causes ?? [],
      interventions:         report?.recommended_interventions ?? [],
      deepCausality:         report?.deep_causality ?? null,
    };

    const { observed, inferred } = computeEpisodePositions(episode);
    episode.observedPosition3D = observed;
    episode.inferredPosition3D = inferred;

    episodeNodes.push(episode);

    // ── Cause nodes ────────────────────────────────────────────────────────
    if (report?.primary_cause) {
      if (!causeMap.has(primaryCauseType)) {
        causeMap.set(primaryCauseType, {
          id:              primaryCauseType,
          label:           report.primary_cause,
          type:            primaryCauseType,
          confidence,
          supportingRunIds: [],
          position3D:      CAUSE_POSITIONS[primaryCauseType] ?? [0, 0, 0],
          interventions:   report.recommended_interventions ?? [],
        });
      }
      const cause = causeMap.get(primaryCauseType);
      cause.supportingRunIds.push(run.run_id);
      cause.confidence = Math.max(cause.confidence, confidence);
      // Keep the highest-confidence intervention description
      if (confidence > (cause._topConfidence ?? 0)) {
        cause.label           = report.primary_cause;
        cause.interventions   = report.recommended_interventions ?? [];
        cause._topConfidence  = confidence;
      }

      edges.push({
        id:         `${run.run_id}->${primaryCauseType}`,
        sourceId:   run.run_id,
        targetId:   primaryCauseType,
        kind:       'observed_to_inferred',
        confidence,
        weight:     confidence,
      });
    }

    // ── Contributing causes as secondary edges ─────────────────────────────
    for (const contrib of report?.contributing_causes ?? []) {
      const contribType = inferCauseType(contrib);
      if (contribType === primaryCauseType) continue;

      if (!causeMap.has(contribType)) {
        causeMap.set(contribType, {
          id:              contribType,
          label:           contrib,
          type:            contribType,
          confidence:      confidence * 0.6,
          supportingRunIds: [],
          position3D:      CAUSE_POSITIONS[contribType] ?? [0, 0, 0],
          interventions:   [],
        });
      }
      causeMap.get(contribType).supportingRunIds.push(run.run_id);

      edges.push({
        id:       `${run.run_id}->${contribType}-contrib`,
        sourceId: run.run_id,
        targetId: contribType,
        kind:     'cause_to_effect',
        confidence: confidence * 0.55,
        weight:     confidence * 0.55,
      });
    }
  }

  const causeNodes = [...causeMap.values()];
  const clusters   = clusterEpisodes(episodeNodes);

  return { episodeNodes, causeNodes, interventionNodes: [], edges, clusters };
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function clampUnit(v) {
  return Math.max(0, Math.min(1, v));
}

function mapRunStatus(status) {
  if (status === 'completed') return 'accepted';
  if (status === 'failed')    return 'failed';
  if (status === 'running')   return 'in_progress';
  return 'rejected';
}

/**
 * Map a Coobie CausalRule description string to a cause type key.
 * Matches against both rule IDs (SPEC_AMBIGUITY, TWIN_GAP, …) and
 * natural-language fragments in the description text.
 */
function inferCauseType(description) {
  if (!description) return 'context_gap';
  const d = description.toLowerCase();

  if (d.includes('spec_ambiguity') || d.includes('spec ambig') || d.includes('ambiguous') || d.includes('acceptance criteria'))
    return 'spec_ambiguity';
  if (d.includes('twin_gap') || d.includes('twin gap') || d.includes('twin fidelit') || d.includes('twin_fidelit'))
    return 'low_twin_fidelity';
  if (d.includes('test_blind') || d.includes('blind spot') || d.includes('happy path') || d.includes('false-negative'))
    return 'test_blind_spot';
  if (d.includes('no_prior_memory') || d.includes('no prior memory') || d.includes('prior memory') || d.includes('pattern-cold'))
    return 'context_gap';
  if (d.includes('broad_scope') || d.includes('broad scope') || d.includes('scope reduction') || d.includes('narrow the spec'))
    return 'missing_failure_case';
  if (d.includes('policy') || d.includes('block') || d.includes('keeper'))
    return 'policy_block';
  if (d.includes('tool') || d.includes('misu'))
    return 'tool_misuse';

  return 'context_gap';
}

// ── Demo scene (shown when API is unreachable) ────────────────────────────────

export const EXAMPLE_SCENE = {
  episodeNodes: [
    {
      id: 'ex-1', label: 'ceres-station · lamdet-corpus', productId: 'ceres-station',
      runId: 'ex-1', specQuality: 0.38, implementationQuality: 0.61, validationOutcome: 0.28,
      interventionPotential: 0.76, primaryCauseType: 'test_blind_spot', confidence: 0.80,
      status: 'failed', timestamp: new Date().toISOString(),
      observedPosition3D: [0.14, 0.38, -0.55], inferredPosition3D: [0.35, 0.30, -0.12],
      primaryCauseText: 'Visible tests all passed but hidden scenarios failed — tests were too aligned with the happy path.',
      contributingCauses: ['Ambiguous or incomplete spec likely caused scenario failure.'],
      rawScores: { spec_clarity_score: 0.38, change_scope_score: 0.55, twin_fidelity_score: 0.61, test_coverage_score: 0.95, memory_retrieval_score: 0.20, scenario_passed: false, validation_passed: true },
    },
    {
      id: 'ex-2', label: 'ceres-station · parity-v2', productId: 'ceres-station',
      runId: 'ex-2', specQuality: 0.82, implementationQuality: 0.86, validationOutcome: 0.91,
      interventionPotential: 0.28, primaryCauseType: 'context_gap', confidence: 0.42,
      status: 'accepted', timestamp: new Date().toISOString(),
      observedPosition3D: [0.55, 0.62, 0.70], inferredPosition3D: [-0.15, 0.02, -0.30],
      primaryCauseText: 'No relevant prior memory was retrieved before the run.',
      contributingCauses: [],
      rawScores: { spec_clarity_score: 0.82, change_scope_score: 0.30, twin_fidelity_score: 0.86, test_coverage_score: 0.90, memory_retrieval_score: 0.05, scenario_passed: true, validation_passed: true },
    },
    {
      id: 'ex-3', label: 'ceres-station · twin-align', productId: 'ceres-station',
      runId: 'ex-3', specQuality: 0.44, implementationQuality: 0.31, validationOutcome: 0.22,
      interventionPotential: 0.68, primaryCauseType: 'low_twin_fidelity', confidence: 0.65,
      status: 'rejected', timestamp: new Date().toISOString(),
      observedPosition3D: [-0.48, -0.62, -0.62], inferredPosition3D: [0.00, 0.22, 0.50],
      primaryCauseText: 'Low twin fidelity may have caused false-negative hidden scenario failure.',
      contributingCauses: ['Broad implementation scope increases probability of hidden scenario failures.'],
      rawScores: { spec_clarity_score: 0.44, change_scope_score: 0.72, twin_fidelity_score: 0.31, test_coverage_score: 0.65, memory_retrieval_score: 0.35, scenario_passed: false, validation_passed: false },
    },
  ],
  causeNodes: [
    { id: 'test_blind_spot',    type: 'test_blind_spot',    confidence: 0.80, label: 'Visible tests passed but hidden scenarios failed — happy path bias.', supportingRunIds: ['ex-1'], position3D: [0.35, 0.25, -0.10] },
    { id: 'context_gap',        type: 'context_gap',        confidence: 0.42, label: 'No prior memory retrieved — pattern-cold run.', supportingRunIds: ['ex-2'], position3D: [-0.15, 0.40, -0.25] },
    { id: 'low_twin_fidelity',  type: 'low_twin_fidelity',  confidence: 0.65, label: 'Twin environment did not replicate production conditions.', supportingRunIds: ['ex-3'], position3D: [0.00, 0.10, 0.40] },
    { id: 'spec_ambiguity',     type: 'spec_ambiguity',     confidence: 0.45, label: 'Ambiguous acceptance criteria (contributing cause from ex-1).', supportingRunIds: ['ex-1'], position3D: [-0.65, 0.00, -0.30] },
    { id: 'missing_failure_case', type: 'missing_failure_case', confidence: 0.40, label: 'Broad scope (contributing cause from ex-3).', supportingRunIds: ['ex-3'], position3D: [-0.30, -0.35, 0.20] },
  ],
  interventionNodes: [],
  edges: [
    { id: 'ex-1->tbs',  sourceId: 'ex-1', targetId: 'test_blind_spot',    kind: 'observed_to_inferred', confidence: 0.80, weight: 0.80 },
    { id: 'ex-1->sa',   sourceId: 'ex-1', targetId: 'spec_ambiguity',      kind: 'cause_to_effect',      confidence: 0.44, weight: 0.44 },
    { id: 'ex-2->cg',   sourceId: 'ex-2', targetId: 'context_gap',         kind: 'observed_to_inferred', confidence: 0.42, weight: 0.42 },
    { id: 'ex-3->ltf',  sourceId: 'ex-3', targetId: 'low_twin_fidelity',   kind: 'observed_to_inferred', confidence: 0.65, weight: 0.65 },
    { id: 'ex-3->mfc',  sourceId: 'ex-3', targetId: 'missing_failure_case', kind: 'cause_to_effect',     confidence: 0.36, weight: 0.36 },
  ],
  clusters: [
    { id: 'failed',   label: 'Failed',   center: [0.14,  0.38, -0.55], radius: 0.25, color: '#c7684c' },
    { id: 'accepted', label: 'Accepted', center: [0.55,  0.62,  0.70], radius: 0.20, color: '#8fae7c' },
    { id: 'rejected', label: 'Rejected', center: [-0.48, -0.62, -0.62], radius: 0.22, color: '#c7684c' },
  ],
};
