export const ACTION_CARD_LIBRARY = {
  scout: {
    id: 'scout',
    name: 'Scout',
    title: 'Scout card incoming',
    subtitle: 'Spec intake and intent shaping',
    image: '/actioncards/scout.png',
    accent: '#c4922a',
    status: 'planned',
  },
  mason: {
    id: 'mason',
    name: 'Mason',
    title: 'Mason drafts and builds',
    subtitle: 'Implementation and code execution lane',
    image: '/actioncards/mason.png',
    accent: '#4f94ff',
    status: 'ready',
  },
  piper: {
    id: 'piper',
    name: 'Piper',
    title: 'Piper card incoming',
    subtitle: 'Tools, MCP, and docs routing',
    image: '/actioncards/piper.png',
    accent: '#43a67c',
    status: 'planned',
  },
  ash: {
    id: 'ash',
    name: 'Ash',
    title: 'Ash card incoming',
    subtitle: 'Twin provisioning and environment setup',
    image: '/actioncards/ash.png',
    accent: '#2f8ca8',
    status: 'planned',
  },
  bramble: {
    id: 'bramble',
    name: 'Bramble',
    title: 'Bramble card available',
    subtitle: 'Reasoning, consequence-checking, and test posture',
    image: '/actioncards/bramble.png',
    accent: '#88a930',
    status: 'ready',
  },
  sable: {
    id: 'sable',
    name: 'Sable',
    title: 'Sable pressure-tests outcomes',
    subtitle: 'Scenario evaluation and realism pressure',
    image: '/actioncards/sable.png',
    accent: '#6c8cff',
    status: 'ready',
  },
  flint: {
    id: 'flint',
    name: 'Flint',
    title: 'Flint card incoming',
    subtitle: 'Artifacts, packaging, and handoff polish',
    image: '/actioncards/flint.png',
    accent: '#c97842',
    status: 'planned',
  },
  keeper: {
    id: 'keeper',
    name: 'Keeper',
    title: 'Keeper carries the guardrails',
    subtitle: 'Authority, leases, and boundary control',
    image: '/actioncards/keeper.png',
    accent: '#69a8ff',
    status: 'ready',
  },
  coobie: {
    id: 'coobie',
    name: 'Coobie',
    title: 'Coobie leads the interview',
    subtitle: 'Memory, causal priors, and operator elicitation',
    image: '/actioncards/coobie.png',
    accent: '#ff5ad8',
    status: 'ready',
  },
  jerry: {
    id: 'jerry',
    name: 'Jerry',
    title: 'Jerry provides strategic oversight',
    subtitle: 'Human-in-the-loop supervisor and override lane',
    image: '/actioncards/jerry.png',
    accent: '#f0a23f',
    status: 'ready',
  },
};

export function getActionCard(id) {
  return ACTION_CARD_LIBRARY[id] || null;
}

export function getActionCards(ids) {
  return ids.map(getActionCard).filter(Boolean);
}

export const USER_SUPERVISOR_CARD_TEMPLATE_PATH = '/actioncards/user-supervisor-card-template.md';
export const DEFAULT_OPERATOR_SUPERVISOR_CARD_ID = 'jerry';

export const NEW_RUN_MODE_CARD_IDS = {
  draft: 'mason',
  interview: 'coobie',
};

export const OPERATOR_MODEL_CARD_GROUPS = {
  primary: ['coobie'],
  support: ['keeper', 'mason', 'sable'],
  supervisorFallback: ['jerry'],
  incoming: ['scout', 'piper', 'ash', 'flint'],
};
