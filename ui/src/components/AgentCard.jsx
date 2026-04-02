import React from 'react';
import LabradorIcon from './LabradorIcon';

const STATUS_LABELS = {
  idle: 'idle',
  queued: 'queued',
  running: 'running',
  complete: 'complete',
  blocked: 'blocked',
  failed: 'failed',
  waiting: 'waiting',
};

const STATUS_COLORS = {
  idle: '#67727b',
  queued: '#67727b',
  waiting: '#67727b',
  running: 'var(--accent-gold)',
  complete: '#8fae7c',
  blocked: '#c7684c',
  failed: '#c7684c',
};

const AgentCard = ({ agent, variant = 'dark', isSingleton = false }) => {
  const {
    id,
    name,
    role,
    status,
    task,
    latestLog,
    accentColor,
    engine,
    ownership,
    latestPhase,
    bundleProvider,
    bundleFingerprint,
    pinnedSkillIds,
  } = agent;

  const skills = (pinnedSkillIds || []).map(sid =>
    sid.includes('/') ? sid.split('/').pop() : sid,
  );

  const cardStatus = STATUS_LABELS[status] || 'idle';
  const statusColor = STATUS_COLORS[cardStatus] || STATUS_COLORS.idle;
  return (
    <div
      className={`agent-card variant-${variant} status-${cardStatus} ${isSingleton ? 'singleton' : ''}`}
      style={{ '--agent-accent': accentColor || 'var(--accent-gold)' }}
    >
      <div className="agent-tile">
        <div className="agent-image-wrap">
          <LabradorIcon
            color={accentColor || 'var(--accent-gold, #c4922a)'}
            size={isSingleton ? 72 : 80}
            status={cardStatus}
          />
        </div>
        <div className="agent-nameplate">
          <div className="agent-name">{name}</div>
          <div className="agent-role-short">{role}</div>
        </div>
      </div>

      <div className="agent-body">
        <div className="agent-head">
          <div className="agent-phase">{latestPhase || 'awaiting signal'}</div>
          <div className="status-chip" style={{ borderColor: `${statusColor}55`, color: statusColor }}>
            <span className="status-dot" style={{ backgroundColor: statusColor }}></span>
            {cardStatus}
          </div>
        </div>

        <div className="agent-meta-grid">
          <div className="meta-cell">
            <span className="meta-label">ENGINE</span>
            <span className="meta-value">{engine || 'offline'}</span>
          </div>
          <div className="meta-cell">
            <span className="meta-label">CLAIM</span>
            <span className="meta-value">{ownership || 'unclaimed'}</span>
          </div>
        </div>

        <div className="info-block">
          <div className="info-label">ACTIVE TASK</div>
          <div className="info-value">{task}</div>
        </div>

        {skills.length > 0 && (
          <div className="info-block skills-block">
            <div className="info-label">SKILLS</div>
            <div className="skill-chips">
              {skills.map(s => (
                <span key={s} className="skill-chip">{s}</span>
              ))}
              {bundleProvider && (
                <span className={`provider-chip provider-${bundleProvider}`}>{bundleProvider}</span>
              )}
            </div>
          </div>
        )}

        <div className="info-block log-block">
          <div className="info-label">LATEST LOG</div>
          <div className="info-value mono">{latestLog}</div>
          {bundleFingerprint && (
            <div className="bundle-fp" title={`Bundle: ${bundleFingerprint}`}>
              bundle:{bundleFingerprint.slice(0, 10)}
            </div>
          )}
        </div>
      </div>

      <style jsx>{`
        .agent-card {
          display: grid;
          grid-template-columns: 128px minmax(0, 1fr);
          min-height: 220px;
          background: linear-gradient(180deg, rgba(36, 40, 43, 0.95), rgba(24, 27, 29, 0.98));
          border: 1px solid var(--border-glass);
          border-radius: 16px;
          overflow: hidden;
          box-shadow: 0 16px 40px rgba(0, 0, 0, 0.28);
          position: relative;
        }

        .agent-card::after {
          content: '';
          position: absolute;
          inset: 0;
          pointer-events: none;
          border-top: 1px solid rgba(255, 255, 255, 0.03);
        }

        .agent-card.status-running {
          box-shadow: 0 0 0 1px rgba(194, 163, 114, 0.25), 0 18px 48px rgba(0, 0, 0, 0.34);
        }

        .agent-card.singleton {
          grid-template-columns: 120px minmax(0, 1fr);
        }

        .variant-light {
          background: linear-gradient(180deg, #e5dece, #d3c8b2);
          color: #202224;
        }

        .agent-tile {
          display: flex;
          flex-direction: column;
          background: linear-gradient(180deg, rgba(255, 255, 255, 0.03), rgba(0, 0, 0, 0.1));
          border-right: 1px solid rgba(255, 255, 255, 0.05);
        }

        .agent-image-wrap {
          flex: 1;
          position: relative;
          display: grid;
          place-items: center;
          padding: 16px;
        }

        .agent-nameplate {
          background: var(--agent-accent);
          color: #17191a;
          padding: 10px 12px;
          display: flex;
          flex-direction: column;
          gap: 2px;
        }

        .agent-name {
          font-size: 0.92rem;
          font-weight: 800;
          letter-spacing: 0.08em;
          text-transform: uppercase;
        }

        .agent-role-short {
          font-size: 0.68rem;
          font-weight: 700;
          opacity: 0.86;
          line-height: 1.3;
        }

        .agent-body {
          padding: 1rem 1.1rem;
          display: flex;
          flex-direction: column;
          gap: 0.9rem;
          min-width: 0;
        }

        .agent-head {
          display: flex;
          justify-content: space-between;
          align-items: center;
          gap: 0.75rem;
        }

        .agent-phase {
          text-transform: uppercase;
          letter-spacing: 0.1em;
          font-size: 0.68rem;
          color: var(--text-secondary);
          font-weight: 800;
        }

        .status-chip {
          display: inline-flex;
          align-items: center;
          gap: 0.45rem;
          border: 1px solid;
          border-radius: 999px;
          padding: 0.28rem 0.6rem;
          font-size: 0.7rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          background: rgba(0, 0, 0, 0.14);
          white-space: nowrap;
        }

        .status-dot {
          width: 0.5rem;
          height: 0.5rem;
          border-radius: 999px;
          flex-shrink: 0;
        }

        .agent-meta-grid {
          display: grid;
          grid-template-columns: repeat(2, minmax(0, 1fr));
          gap: 0.65rem;
        }

        .meta-cell,
        .info-block {
          border: 1px solid rgba(255, 255, 255, 0.06);
          background: rgba(0, 0, 0, 0.18);
          border-radius: 10px;
          padding: 0.72rem 0.8rem;
          min-width: 0;
        }

        .meta-label,
        .info-label {
          display: block;
          font-size: 0.62rem;
          font-weight: 800;
          letter-spacing: 0.12em;
          color: var(--accent-gold);
          text-transform: uppercase;
          margin-bottom: 0.3rem;
        }

        .meta-value,
        .info-value {
          display: block;
          font-size: 0.84rem;
          font-weight: 600;
          color: inherit;
          line-height: 1.45;
          word-break: break-word;
        }

        .log-block {
          flex: 1;
        }

        .skills-block {
          padding-top: 0.55rem;
          padding-bottom: 0.55rem;
        }

        .skill-chips {
          display: flex;
          flex-wrap: wrap;
          gap: 0.3rem;
          margin-top: 0.2rem;
        }

        .skill-chip {
          font-size: 0.6rem;
          font-weight: 700;
          color: rgba(194, 163, 114, 0.75);
          background: rgba(194, 163, 114, 0.07);
          border: 1px solid rgba(194, 163, 114, 0.15);
          border-radius: 5px;
          padding: 0.1rem 0.4rem;
          font-family: var(--font-mono);
          white-space: nowrap;
        }

        .provider-chip {
          font-size: 0.58rem;
          font-weight: 900;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          border-radius: 5px;
          padding: 0.1rem 0.4rem;
          border: 1px solid;
        }

        .provider-chip.provider-anthropic,
        .provider-chip.provider-claude {
          color: #c4922a;
          background: rgba(196, 146, 42, 0.1);
          border-color: rgba(196, 146, 42, 0.25);
        }

        .provider-chip.provider-openai,
        .provider-chip.provider-codex {
          color: #5a8acc;
          background: rgba(90, 138, 204, 0.1);
          border-color: rgba(90, 138, 204, 0.25);
        }

        .bundle-fp {
          margin-top: 0.3rem;
          font-family: var(--font-mono);
          font-size: 0.6rem;
          color: rgba(255, 255, 255, 0.18);
          letter-spacing: 0.04em;
        }

        .mono {
          font-family: var(--font-mono);
          color: var(--text-secondary);
          font-size: 0.76rem;
        }

        @media (max-width: 760px) {
          .agent-card,
          .agent-card.singleton {
            grid-template-columns: 1fr;
          }

          .agent-tile {
            border-right: none;
            border-bottom: 1px solid rgba(255, 255, 255, 0.05);
          }

          .agent-image-wrap {
            min-height: 140px;
          }
        }
      `}</style>
    </div>
  );
};

export default AgentCard;
