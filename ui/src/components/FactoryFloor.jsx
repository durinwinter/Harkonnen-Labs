import { useState } from 'react';
import LabradorIcon from './LabradorIcon';

/**
 * FactoryFloor — full agent roster with labrador identities, live status,
 * collar glow when running, and a live activity feed per agent.
 *
 * Each agent maps to the pack art:
 *   Scout   — gold medal collar (top center)
 *   Keeper  — scroll holder (top right)
 *   Mason   — hard hat (right)
 *   Piper   — wrench vest (bottom right)
 *   Bramble — clipboard (bottom right inner)
 *   Coobie  — glowing LED collar (bottom center) ← memory active indicator
 *   Sable   — dive goggles (bottom left)
 *   Flint   — carrying packages (left)
 *   Ash     — saddle bags / backpack (upper left)
 */

// Agent meta — keep in sync with AGENTS.md roster
// pinned: true means trust-critical, always routed to Claude
const AGENT_META = [
  {
    id: 'scout',
    name: 'Scout',
    role: 'Spec Retriever',
    group: 'planning',
    color: '#c4922a',
    emblem: '🏅',
    desc: 'Parse specs, flag ambiguity, produce intent package.',
    collarGlow: '#c4922a',
    pinned: true,
  },
  {
    id: 'keeper',
    name: 'Keeper',
    role: 'Boundary Retriever',
    group: 'planning',
    color: '#8a7a3a',
    emblem: '📜',
    desc: 'Enforce policy, guard boundaries, and manage file-claim coordination.',
    collarGlow: '#8a7a3a',
    pinned: true,
  },
  {
    id: 'mason',
    name: 'Mason',
    role: 'Build Retriever',
    group: 'action',
    color: '#c4662a',
    emblem: '⛑',
    desc: 'Generate and modify code, multi-file changes.',
    collarGlow: '#c4662a',
    pinned: false,
  },
  {
    id: 'piper',
    name: 'Piper',
    role: 'Tool Retriever',
    group: 'action',
    color: '#5a7a5a',
    emblem: '🔧',
    desc: 'Run build tools, fetch docs, execute helpers.',
    collarGlow: '#5a7a5a',
    pinned: false,
  },
  {
    id: 'ash',
    name: 'Ash',
    role: 'Twin Retriever',
    group: 'action',
    color: '#2a7a7a',
    emblem: '🎒',
    desc: 'Provision digital twins, mock dependencies.',
    collarGlow: '#2a7a7a',
    pinned: false,
  },
  {
    id: 'bramble',
    name: 'Bramble',
    role: 'Test Retriever',
    group: 'verification',
    color: '#a89a2a',
    emblem: '📋',
    desc: 'Generate tests, run lint/build/visible tests.',
    collarGlow: '#a89a2a',
    pinned: false,
  },
  {
    id: 'sable',
    name: 'Sable',
    role: 'Scenario Retriever',
    group: 'verification',
    color: '#3a4a5a',
    emblem: '🥽',
    desc: 'Execute hidden scenarios, produce eval reports.',
    collarGlow: '#3a4a5a',
    pinned: true,
  },
  {
    id: 'flint',
    name: 'Flint',
    role: 'Artifact Retriever',
    group: 'verification',
    color: '#8a6a3a',
    emblem: '📦',
    desc: 'Collect outputs, package artifact bundles.',
    collarGlow: '#8a6a3a',
    pinned: false,
  },
  {
    id: 'coobie',
    name: 'Coobie',
    role: 'Memory Retriever',
    group: 'memory',
    color: '#7a2a3a',
    emblem: '💡',
    desc: 'Coordinate pack memory: working context, episodic capture, causal graph, consolidation, and cross-agent blackboard health.',
    collarGlow: '#e04060',
    pinned: false,
    isCoobie: true,
  },
];

// Blackboard slice labels — mirror AGENTS.md panel names exactly
const GROUP_LABELS = {
  planning:     'Intake & Planning',
  action:       'Action Board',
  verification: 'Verification',
  memory:       'Memory Board',
};

function statusLabel(status) {
  const map = { running: 'Active', complete: 'Done', blocked: 'Blocked', idle: 'Idle', failed: 'Failed' };
  return map[status] || status || 'Idle';
}

function AgentFloorCard({ agent, onOpenWorkbench }) {
  const [expanded, setExpanded] = useState(false);
  const isRunning = agent.status === 'running';
  const isBlocked = agent.status === 'blocked' || agent.status === 'failed';
  const isDone    = agent.status === 'complete';

  return (
    <div
      className={`floor-card ${agent.status || 'idle'} ${expanded ? 'expanded' : ''} ${agent.isCoobie ? 'coobie-card' : ''}`}
      onClick={() => setExpanded(e => !e)}
    >
      {/* Collar glow ring when running */}
      {isRunning && (
        <div className="collar-glow" style={{ '--glow-color': agent.collarGlow }} />
      )}

      {/* Icon */}
      <div className="floor-icon">
        <LabradorIcon
          color={agent.color}
          size={agent.isCoobie ? 52 : 44}
          status={agent.status || 'idle'}
        />
        {agent.isCoobie && isRunning && (
          <div className="coobie-pulse" style={{ '--glow': agent.collarGlow }} />
        )}
      </div>

      {/* Info */}
      <div className="floor-info">
        <div className="floor-name-row">
          <span className="floor-name">{agent.name}</span>
          <span className="floor-emblem">{agent.emblem}</span>
          {agent.pinned && (
            <span className="floor-pinned-badge" title="Pinned to Claude — trust-critical role">Claude</span>
          )}
          <span className={`floor-status-chip ${agent.status || 'idle'}`}>
            {statusLabel(agent.status)}
          </span>
        </div>
        <div className="floor-role">{agent.role}</div>
        <div className="floor-task">{agent.task || agent.desc}</div>
        {agent.engine && agent.engine !== 'unassigned' && (
          <div className="floor-engine">{agent.engine}</div>
        )}
      </div>

      {/* Expanded detail */}
      {expanded && (
        <div className="floor-detail" onClick={e => e.stopPropagation()}>
          <div className="floor-detail-desc">{agent.desc}</div>
          {agent.latestLog && (
            <div className="floor-log">{agent.latestLog}</div>
          )}
          {agent.ownership && (
            <div className="floor-claim">
              Claiming: <span className="floor-claim-val">{agent.ownership}</span>
            </div>
          )}
          {agent.isCoobie && (
            <button
              className="floor-workbench-btn"
              onClick={() => { onOpenWorkbench?.(); setExpanded(false); }}
            >
              Open Causal Workbench
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export default function FactoryFloor({ agents, onOpenWorkbench }) {
  const grouped = {};
  for (const group of Object.keys(GROUP_LABELS)) {
    grouped[group] = (agents || []).filter(a => {
      const meta = AGENT_META.find(m => m.id === a.id);
      return meta?.group === group;
    }).map(a => ({ ...a, ...AGENT_META.find(m => m.id === a.id) }));
  }

  // Active count
  const activeCount = (agents || []).filter(a => a.status === 'running').length;
  const blockedCount = (agents || []).filter(a => a.status === 'blocked' || a.status === 'failed').length;

  return (
    <div className="factory-floor">

      {/* Status bar */}
      <div className="floor-status-bar">
        <span className="floor-bar-title">The Pack</span>
        <div className="floor-bar-stats">
          <span className="floor-bar-stat">
            <span className="floor-bar-dot running" />
            {activeCount} active
          </span>
          {blockedCount > 0 && (
            <span className="floor-bar-stat">
              <span className="floor-bar-dot blocked" />
              {blockedCount} blocked
            </span>
          )}
          <span className="floor-bar-stat muted">{(agents || []).length} total</span>
        </div>
      </div>

      {/* Groups */}
      {Object.entries(GROUP_LABELS).map(([group, label]) => (
        <div key={group} className="floor-group">
          <div className="floor-group-header">
            <span className="floor-group-label">{label}</span>
            <div className="floor-group-line" />
          </div>
          <div className={`floor-group-cards ${group === 'memory' ? 'memory-row' : ''}`}>
            {grouped[group]?.map(agent => (
              <AgentFloorCard
                key={agent.id}
                agent={agent}
                onOpenWorkbench={onOpenWorkbench}
              />
            ))}
          </div>
        </div>
      ))}

      <style jsx>{`
        .factory-floor {
          display: flex;
          flex-direction: column;
          gap: 1.1rem;
        }

        /* ── Status bar ── */
        .floor-status-bar {
          display: flex;
          align-items: center;
          justify-content: space-between;
          padding: 0.5rem 0.2rem;
          border-bottom: 1px solid rgba(255,255,255,0.06);
          margin-bottom: 0.2rem;
        }
        .floor-bar-title {
          font-size: 0.7rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.16em;
          color: #c2a372;
        }
        .floor-bar-stats {
          display: flex;
          gap: 0.9rem;
          align-items: center;
        }
        .floor-bar-stat {
          display: flex;
          align-items: center;
          gap: 0.3rem;
          font-size: 0.72rem;
          font-weight: 700;
          color: rgba(255,255,255,0.5);
        }
        .floor-bar-stat.muted { color: rgba(255,255,255,0.25); }
        .floor-bar-dot {
          width: 7px;
          height: 7px;
          border-radius: 50%;
          flex-shrink: 0;
        }
        .floor-bar-dot.running { background: #c4922a; box-shadow: 0 0 5px #c4922a; }
        .floor-bar-dot.blocked { background: #c7684c; }

        /* ── Group ── */
        .floor-group-header {
          display: flex;
          align-items: center;
          gap: 0.75rem;
          margin-bottom: 0.65rem;
        }
        .floor-group-label {
          font-size: 0.7rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: #c2a372;
          white-space: nowrap;
        }
        .floor-group-line {
          flex: 1;
          height: 1px;
          background: linear-gradient(90deg, rgba(194,163,114,0.4), transparent);
        }
        .floor-group-cards {
          display: grid;
          grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
          gap: 0.75rem;
        }
        .floor-group-cards.memory-row {
          grid-template-columns: 1fr;
        }

        /* ── Card ── */
        .floor-card {
          position: relative;
          background: rgba(22,24,26,0.88);
          border: 1px solid rgba(255,255,255,0.06);
          border-radius: 16px;
          padding: 0.85rem;
          cursor: pointer;
          transition: border-color 0.15s, background 0.15s;
          display: flex;
          gap: 0.75rem;
          align-items: flex-start;
          overflow: hidden;
        }
        .floor-card:hover {
          border-color: rgba(255,255,255,0.12);
          background: rgba(28,31,34,0.9);
        }
        .floor-card.running {
          border-color: rgba(196,146,42,0.35);
        }
        .floor-card.blocked, .floor-card.failed {
          border-color: rgba(199,104,76,0.35);
          background: rgba(120,39,30,0.12);
        }
        .floor-card.complete {
          border-color: rgba(90,138,90,0.22);
        }
        .floor-card.coobie-card {
          background: rgba(30,16,20,0.92);
          border-color: rgba(122,42,58,0.4);
        }
        .floor-card.coobie-card.running {
          border-color: rgba(224,64,96,0.5);
        }
        .floor-card.expanded {
          flex-direction: column;
        }

        /* ── Collar glow ── */
        .collar-glow {
          position: absolute;
          inset: 0;
          border-radius: 16px;
          pointer-events: none;
          box-shadow: inset 0 0 18px rgba(var(--glow-color), 0.08);
          animation: collar-pulse 2.2s ease-in-out infinite;
        }
        @keyframes collar-pulse {
          0%, 100% { opacity: 0.6; }
          50% { opacity: 1; }
        }

        /* ── Icon ── */
        .floor-icon {
          flex-shrink: 0;
          position: relative;
          display: flex;
          align-items: center;
          justify-content: center;
        }
        .coobie-pulse {
          position: absolute;
          inset: -6px;
          border-radius: 50%;
          border: 2px solid var(--glow);
          opacity: 0.5;
          animation: coobie-ring 1.8s ease-in-out infinite;
        }
        @keyframes coobie-ring {
          0%, 100% { transform: scale(0.9); opacity: 0.5; }
          50% { transform: scale(1.1); opacity: 0.15; }
        }

        /* ── Info ── */
        .floor-info {
          flex: 1;
          min-width: 0;
        }
        .floor-name-row {
          display: flex;
          align-items: center;
          gap: 0.4rem;
          margin-bottom: 0.2rem;
          flex-wrap: wrap;
        }
        .floor-name {
          font-size: 0.92rem;
          font-weight: 800;
          letter-spacing: 0.02em;
        }
        .floor-emblem {
          font-size: 0.85rem;
        }
        .floor-pinned-badge {
          font-size: 0.54rem;
          font-weight: 900;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: #c4922a;
          background: rgba(196, 146, 42, 0.12);
          border: 1px solid rgba(196, 146, 42, 0.3);
          border-radius: 999px;
          padding: 0.1rem 0.38rem;
          white-space: nowrap;
        }
        .floor-status-chip {
          font-size: 0.58rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          border-radius: 999px;
          padding: 0.1rem 0.4rem;
          border: 1px solid;
          margin-left: auto;
        }
        .floor-status-chip.running {
          color: #c4922a;
          border-color: rgba(196,146,42,0.4);
          background: rgba(196,146,42,0.1);
        }
        .floor-status-chip.complete {
          color: #8fae7c;
          border-color: rgba(143,174,124,0.35);
          background: rgba(143,174,124,0.08);
        }
        .floor-status-chip.blocked, .floor-status-chip.failed {
          color: #c7684c;
          border-color: rgba(199,104,76,0.4);
          background: rgba(199,104,76,0.1);
        }
        .floor-status-chip.idle {
          color: rgba(255,255,255,0.25);
          border-color: rgba(255,255,255,0.08);
        }
        .floor-role {
          font-size: 0.68rem;
          color: rgba(255,255,255,0.35);
          text-transform: uppercase;
          letter-spacing: 0.1em;
          font-weight: 700;
          margin-bottom: 0.3rem;
        }
        .floor-task {
          font-size: 0.78rem;
          color: rgba(255,255,255,0.65);
          line-height: 1.4;
          overflow: hidden;
          text-overflow: ellipsis;
          display: -webkit-box;
          -webkit-line-clamp: 2;
          -webkit-box-orient: vertical;
        }
        .floor-engine {
          margin-top: 0.3rem;
          font-size: 0.65rem;
          font-family: monospace;
          color: rgba(255,255,255,0.22);
        }

        /* ── Expanded detail ── */
        .floor-detail {
          width: 100%;
          border-top: 1px solid rgba(255,255,255,0.06);
          padding-top: 0.7rem;
          display: flex;
          flex-direction: column;
          gap: 0.4rem;
        }
        .floor-detail-desc {
          font-size: 0.78rem;
          color: rgba(255,255,255,0.5);
          font-style: italic;
          line-height: 1.45;
        }
        .floor-log {
          font-size: 0.75rem;
          color: rgba(255,255,255,0.65);
          background: rgba(255,255,255,0.03);
          border: 1px solid rgba(255,255,255,0.05);
          border-radius: 8px;
          padding: 0.4rem 0.6rem;
          font-family: monospace;
        }
        .floor-claim {
          font-size: 0.7rem;
          color: rgba(255,255,255,0.35);
        }
        .floor-claim-val {
          color: #c4922a;
          font-family: monospace;
        }
        .floor-workbench-btn {
          align-self: flex-start;
          padding: 0.4rem 0.8rem;
          background: rgba(122,42,58,0.15);
          border: 1px solid rgba(122,42,58,0.45);
          border-radius: 8px;
          color: #e04060;
          font-size: 0.7rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          cursor: pointer;
          transition: all 0.12s;
          margin-top: 0.25rem;
        }
        .floor-workbench-btn:hover {
          background: rgba(122,42,58,0.28);
          border-color: rgba(224,64,96,0.6);
        }
      `}</style>
    </div>
  );
}
