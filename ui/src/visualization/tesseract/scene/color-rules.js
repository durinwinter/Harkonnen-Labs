/**
 * Visual language for the Coobie Causal Tesseract.
 *
 * Green   = accepted / successful path
 * Amber   = uncertain / needs intervention
 * Red     = failed / rejected
 * Blue    = remembered precedent / observed
 * Purple  = counterfactual / connector
 * Gray    = policy-constrained / blocked
 */

export const STATUS_COLOR = {
  accepted:    '#8fae7c',
  in_progress: '#c4922a',
  failed:      '#c7684c',
  rejected:    '#c7684c',
  default:     '#67727b',
};

export const CAUSE_COLOR = {
  spec_ambiguity:       '#c4922a',
  missing_failure_case: '#c7684c',
  low_twin_fidelity:    '#5a8acc',
  test_blind_spot:      '#8a7acc',
  policy_block:         '#7a7a8a',
  tool_misuse:          '#c4662a',
  context_gap:          '#a8a050',
  default:              '#c4922a',
};

/** Edge color and opacity per lens mode */
export const LENS_EDGE = {
  failure: {
    outer:     { color: '#5a8acc', opacity: 0.10 },
    inner:     { color: '#c2a372', opacity: 0.88 },
    connector: { color: '#8a6ab0', opacity: 0.55 },
  },
  memory: {
    outer:     { color: '#5a8acc', opacity: 0.52 },
    inner:     { color: '#c2a372', opacity: 0.52 },
    connector: { color: '#8a6ab0', opacity: 0.30 },
  },
  intervention: {
    outer:     { color: '#5a8acc', opacity: 0.14 },
    inner:     { color: '#c2a372', opacity: 0.32 },
    connector: { color: '#e0b060', opacity: 0.94 },
  },
};

/** Node emissive intensity per lens mode */
export const LENS_NODE_INTENSITY = {
  failure:      { episode: 0.6, cause: 0.8 },
  memory:       { episode: 0.4, cause: 0.6 },
  intervention: { episode: 0.3, cause: 0.5 },
};

/** Cluster hull colors by episode status group */
export const CLUSTER_COLOR = {
  accepted:    '#8fae7c',
  failed:      '#c7684c',
  rejected:    '#c7684c',
  in_progress: '#c4922a',
  default:     '#67727b',
};

/** Coobie flavor text */
export const COOBIE_FLAVOR = {
  trail_found:    'coobie found the trail',
  trail_lost:     'coobie lost the trail',
  confident:      'thassgrate',
  uncertain:      'thasnotgrate',
  policy_block:   'keeper says no',
  watching:       'coobie is watching',
  no_selection:   'click a node to begin the trail',
};
