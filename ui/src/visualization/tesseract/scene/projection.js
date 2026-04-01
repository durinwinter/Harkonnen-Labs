/**
 * 4D tesseract geometry and semantic projection utilities.
 *
 * The tesseract has 16 vertices: every combination of ±1 across four dimensions.
 * 32 edges connect vertices that differ by exactly one coordinate.
 *
 * Semantic classification (fixed, independent of rotation):
 *   w = +1 (index >= 8) → outer cube  (observed state space)
 *   w = -1 (index < 8)  → inner cube  (inferred causal space)
 *   mixed               → connectors  (inference bridges)
 */

// 16 vertices: (i&1)?1:-1, (i&2)?1:-1, (i&4)?1:-1, (i&8)?1:-1
export const TESSERACT_VERTICES_4D = Array.from({ length: 16 }, (_, i) => [
  (i & 1) ? 1 : -1,
  (i & 2) ? 1 : -1,
  (i & 4) ? 1 : -1,
  (i & 8) ? 1 : -1,
]);

// 32 edges: pairs of vertices differing in exactly one coordinate
export const TESSERACT_EDGES = (() => {
  const edges = [];
  for (let a = 0; a < 16; a++) {
    for (let b = a + 1; b < 16; b++) {
      let diff = 0;
      for (let k = 0; k < 4; k++) {
        if (TESSERACT_VERTICES_4D[a][k] !== TESSERACT_VERTICES_4D[b][k]) diff++;
      }
      if (diff === 1) edges.push([a, b]);
    }
  }
  return edges;
})();

// Pre-grouped by semantic type (stable regardless of rotation)
export const OUTER_EDGES    = TESSERACT_EDGES.filter(([a, b]) => a >= 8 && b >= 8);
export const INNER_EDGES    = TESSERACT_EDGES.filter(([a, b]) => a < 8  && b < 8);
export const CONNECTOR_EDGES = TESSERACT_EDGES.filter(([a, b]) => (a >= 8) !== (b >= 8));

/**
 * Rotate a 4D point in the XW plane by `angle` radians.
 */
export function rotateXW([x, y, z, w], angle) {
  const c = Math.cos(angle), s = Math.sin(angle);
  return [x * c - w * s, y, z, x * s + w * c];
}

/**
 * Rotate a 4D point in the YW plane by `angle` radians.
 */
export function rotateYW([x, y, z, w], angle) {
  const c = Math.cos(angle), s = Math.sin(angle);
  return [x, y * c - w * s, z, y * s + w * c];
}

/**
 * Project a 4D point to 3D via perspective along the W axis.
 * @param {number} distance - viewer distance in W (default 2.4)
 */
export function project4Dto3D([x, y, z, w], distance = 2.4) {
  const scale = distance / (distance - w);
  return [x * scale, y * scale, z * scale];
}

// ─── Semantic (observed/inferred) projection ────────────────────────────────

/**
 * Cause type → X axis position in the inferred (inner) cube space.
 */
export const CAUSE_X = {
  spec_ambiguity:       -0.80,
  missing_failure_case: -0.40,
  low_twin_fidelity:     0.00,
  test_blind_spot:       0.40,
  policy_block:          0.80,
  tool_misuse:           0.20,
  context_gap:          -0.20,
};

/**
 * Fixed inner-cube positions for cause nodes.
 */
export const CAUSE_POSITIONS = {
  spec_ambiguity:       [-0.65,  0.00, -0.30],
  missing_failure_case: [-0.30, -0.35,  0.20],
  low_twin_fidelity:    [ 0.00,  0.10,  0.40],
  test_blind_spot:      [ 0.35,  0.25, -0.10],
  policy_block:         [ 0.65, -0.10,  0.05],
  tool_misuse:          [ 0.15, -0.40,  0.30],
  context_gap:          [-0.15,  0.40, -0.25],
};

/**
 * Compute the observed (outer) and inferred (inner) 3D positions for an episode.
 *
 * Observed:  S × I × V axes — what actually happened
 * Inferred:  cause family × confidence × interventionPotential — what Coobie believes
 *
 * Output coordinates are in [-1, 1] range appropriate for the 3D scene.
 */
export function computeEpisodePositions(episode) {
  const {
    specQuality          = 0.5,
    implementationQuality = 0.5,
    validationOutcome    = 0.5,
    interventionPotential = 0.5,
    primaryCauseType,
    confidence           = 0.5,
  } = episode;

  const observed = [
    (specQuality           - 0.5) * 1.7,
    (implementationQuality - 0.5) * 1.7,
    (validationOutcome     - 0.5) * 1.7,
  ];

  const inferred = [
    (CAUSE_X[primaryCauseType] ?? 0) * 0.75,
    (confidence            - 0.5) * 1.2,
    (interventionPotential - 0.5) * 1.2,
  ];

  return { observed, inferred };
}
