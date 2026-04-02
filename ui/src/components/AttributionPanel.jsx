import LabradorIcon from './LabradorIcon';

const AGENT_META = {
  scout:  { color: '#c4922a', emblem: '🏅' },
  keeper: { color: '#8a7a3a', emblem: '📜' },
  mason:  { color: '#c4662a', emblem: '⛑'  },
  piper:  { color: '#5a7a5a', emblem: '🔧' },
  ash:    { color: '#2a7a7a', emblem: '🎒' },
  bramble:{ color: '#a89a2a', emblem: '📋' },
  sable:  { color: '#3a4a5a', emblem: '🥽' },
  flint:  { color: '#8a6a3a', emblem: '📦' },
  coobie: { color: '#e04060', emblem: '💡' },
};

const OUTCOME_STYLES = {
  success:  { color: '#8fae7c', bg: 'rgba(143,174,124,0.1)',  border: 'rgba(143,174,124,0.3)',  label: 'Success' },
  complete: { color: '#8fae7c', bg: 'rgba(143,174,124,0.1)',  border: 'rgba(143,174,124,0.3)',  label: 'Complete' },
  failed:   { color: '#c7684c', bg: 'rgba(199,104,76,0.1)',   border: 'rgba(199,104,76,0.3)',   label: 'Failed' },
  failure:  { color: '#c7684c', bg: 'rgba(199,104,76,0.1)',   border: 'rgba(199,104,76,0.3)',   label: 'Failed' },
  partial:  { color: '#c4922a', bg: 'rgba(196,146,42,0.1)',   border: 'rgba(196,146,42,0.3)',   label: 'Partial' },
  warning:  { color: '#c4922a', bg: 'rgba(196,146,42,0.1)',   border: 'rgba(196,146,42,0.3)',   label: 'Warning' },
  skipped:  { color: '#67727b', bg: 'rgba(103,114,123,0.08)', border: 'rgba(103,114,123,0.2)',  label: 'Skipped' },
};

function outcomeStyle(outcome) {
  return OUTCOME_STYLES[(outcome || '').toLowerCase()] || OUTCOME_STYLES.skipped;
}

function shortFingerprint(fp) {
  if (!fp) return null;
  return fp.length > 12 ? fp.slice(0, 12) : fp;
}

function shortSkillId(id) {
  // "anthropic/doc-coauthoring" → "doc-coauthoring"
  return id.includes('/') ? id.split('/').pop() : id;
}

function ProviderChip({ provider }) {
  if (!provider) return null;
  const isAnthropic = provider === 'anthropic' || provider === 'claude';
  const isOpenAI = provider === 'openai' || provider === 'codex';
  const color = isAnthropic ? '#c4922a' : isOpenAI ? '#5a8acc' : '#67727b';
  return (
    <span
      className="attrib-chip provider-chip"
      style={{ color, borderColor: `${color}55`, background: `${color}12` }}
    >
      {provider}
    </span>
  );
}

function ConfidenceBar({ value }) {
  if (value == null) return null;
  const pct = Math.round(value * 100);
  const color = value >= 0.7 ? '#8fae7c' : value >= 0.4 ? '#c4922a' : '#c7684c';
  return (
    <div className="confidence-bar-wrap" title={`Confidence: ${pct}%`}>
      <div className="confidence-bar-track">
        <div
          className="confidence-bar-fill"
          style={{ width: `${pct}%`, background: color }}
        />
      </div>
      <span className="confidence-label" style={{ color }}>{pct}%</span>
    </div>
  );
}

function AttributionRow({ record }) {
  const agentId = (record.agent_name || '').toLowerCase();
  const meta = AGENT_META[agentId] || { color: '#67727b', emblem: '?' };
  const os = outcomeStyle(record.outcome);
  const fp = shortFingerprint(record.prompt_bundle_fingerprint);
  const skills = (record.pinned_skill_ids || []).map(shortSkillId);
  const memHits = (record.memory_hits || []).length;
  const lessonRefs = (record.relevant_lesson_ids || []).length;
  const requiredChecks = record.required_checks || [];
  const guardrails = record.guardrails || [];

  return (
    <div className="attrib-row">
      {/* Left: agent identity */}
      <div className="attrib-agent">
        <div className="attrib-icon">
          <LabradorIcon color={meta.color} size={28} status="idle" />
          <span className="attrib-emblem">{meta.emblem}</span>
        </div>
        <div className="attrib-agent-info">
          <span className="attrib-agent-name">{record.agent_name}</span>
          <span className="attrib-phase">{record.phase}</span>
        </div>
      </div>

      {/* Center: outcome + confidence */}
      <div className="attrib-outcome-col">
        <span
          className="attrib-outcome-chip"
          style={{ color: os.color, background: os.bg, borderColor: os.border }}
        >
          {os.label}
        </span>
        <ConfidenceBar value={record.confidence} />
      </div>

      {/* Right: bundle detail */}
      <div className="attrib-detail">
        <div className="attrib-chip-row">
          <ProviderChip provider={record.prompt_bundle_provider} />
          {fp && (
            <span className="attrib-chip bundle-chip" title={record.prompt_bundle_fingerprint}>
              bundle:{fp}
            </span>
          )}
        </div>

        {skills.length > 0 && (
          <div className="attrib-chip-row skills-row">
            {skills.map(s => (
              <span key={s} className="attrib-chip skill-chip">{s}</span>
            ))}
          </div>
        )}

        <div className="attrib-counts">
          {memHits > 0 && (
            <span className="attrib-count">
              <span className="count-dot mem" />
              {memHits} memory hit{memHits !== 1 ? 's' : ''}
            </span>
          )}
          {lessonRefs > 0 && (
            <span className="attrib-count">
              <span className="count-dot lesson" />
              {lessonRefs} lesson{lessonRefs !== 1 ? 's' : ''}
            </span>
          )}
          {requiredChecks.length > 0 && (
            <span className="attrib-count">
              <span className="count-dot check" />
              {requiredChecks.length} check{requiredChecks.length !== 1 ? 's' : ''}
            </span>
          )}
          {guardrails.length > 0 && (
            <span className="attrib-count">
              <span className="count-dot guard" />
              {guardrails.length} guardrail{guardrails.length !== 1 ? 's' : ''}
            </span>
          )}
        </div>
      </div>

      <style jsx>{`
        .attrib-row {
          display: grid;
          grid-template-columns: 140px 110px minmax(0, 1fr);
          gap: 0.75rem;
          align-items: start;
          padding: 0.75rem 0.85rem;
          border: 1px solid rgba(255,255,255,0.05);
          border-radius: 12px;
          background: rgba(22,24,26,0.7);
          transition: border-color 0.12s;
        }
        .attrib-row:hover {
          border-color: rgba(255,255,255,0.1);
        }

        /* Agent */
        .attrib-agent {
          display: flex;
          align-items: center;
          gap: 0.55rem;
          min-width: 0;
        }
        .attrib-icon {
          flex-shrink: 0;
          position: relative;
          width: 30px;
          height: 30px;
          display: flex;
          align-items: center;
          justify-content: center;
        }
        .attrib-emblem {
          position: absolute;
          bottom: -2px;
          right: -4px;
          font-size: 0.6rem;
        }
        .attrib-agent-info {
          display: flex;
          flex-direction: column;
          gap: 0.1rem;
          min-width: 0;
        }
        .attrib-agent-name {
          font-size: 0.78rem;
          font-weight: 800;
          text-transform: capitalize;
          white-space: nowrap;
          overflow: hidden;
          text-overflow: ellipsis;
        }
        .attrib-phase {
          font-size: 0.62rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          color: rgba(255,255,255,0.3);
          white-space: nowrap;
          overflow: hidden;
          text-overflow: ellipsis;
        }

        /* Outcome */
        .attrib-outcome-col {
          display: flex;
          flex-direction: column;
          gap: 0.4rem;
        }
        .attrib-outcome-chip {
          display: inline-block;
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          border: 1px solid;
          border-radius: 999px;
          padding: 0.15rem 0.5rem;
          white-space: nowrap;
        }
        .confidence-bar-wrap {
          display: flex;
          align-items: center;
          gap: 0.4rem;
        }
        .confidence-bar-track {
          flex: 1;
          height: 3px;
          border-radius: 2px;
          background: rgba(255,255,255,0.08);
          overflow: hidden;
        }
        .confidence-bar-fill {
          height: 100%;
          border-radius: 2px;
          transition: width 0.3s;
        }
        .confidence-label {
          font-size: 0.6rem;
          font-weight: 800;
          font-family: monospace;
          white-space: nowrap;
        }

        /* Detail */
        .attrib-detail {
          display: flex;
          flex-direction: column;
          gap: 0.35rem;
          min-width: 0;
        }
        .attrib-chip-row {
          display: flex;
          flex-wrap: wrap;
          gap: 0.3rem;
        }
        .skills-row {
          margin-top: 0.1rem;
        }
        .attrib-chip {
          font-size: 0.6rem;
          font-weight: 700;
          border: 1px solid rgba(255,255,255,0.1);
          border-radius: 5px;
          padding: 0.12rem 0.4rem;
          color: rgba(255,255,255,0.4);
          background: rgba(255,255,255,0.04);
          white-space: nowrap;
          font-family: monospace;
        }
        .provider-chip {
          font-family: inherit;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.06em;
        }
        .bundle-chip {
          color: rgba(255,255,255,0.25);
        }
        .skill-chip {
          color: rgba(194,163,114,0.7);
          border-color: rgba(194,163,114,0.15);
          background: rgba(194,163,114,0.05);
        }
        .attrib-counts {
          display: flex;
          flex-wrap: wrap;
          gap: 0.5rem;
          margin-top: 0.1rem;
        }
        .attrib-count {
          display: flex;
          align-items: center;
          gap: 0.25rem;
          font-size: 0.62rem;
          color: rgba(255,255,255,0.28);
        }
        .count-dot {
          width: 5px;
          height: 5px;
          border-radius: 50%;
          flex-shrink: 0;
        }
        .count-dot.mem    { background: #5a8acc; }
        .count-dot.lesson { background: #8fae7c; }
        .count-dot.check  { background: #c4922a; }
        .count-dot.guard  { background: #c7684c; }

        @media (max-width: 800px) {
          .attrib-row {
            grid-template-columns: 1fr;
          }
        }
      `}</style>
    </div>
  );
}

export default function AttributionPanel({ phaseAttributions }) {
  const records = phaseAttributions || [];

  if (records.length === 0) {
    return (
      <div className="attrib-empty">
        No phase attributions recorded yet — they appear as each Labrador completes a phase.
        <style jsx>{`
          .attrib-empty {
            color: rgba(255,255,255,0.28);
            font-size: 0.82rem;
            font-style: italic;
            padding: 0.5rem 0;
            line-height: 1.5;
          }
        `}</style>
      </div>
    );
  }

  // Summary stats
  const total = records.length;
  const successes = records.filter(r => ['success','complete'].includes((r.outcome||'').toLowerCase())).length;
  const failures = records.filter(r => ['failed','failure'].includes((r.outcome||'').toLowerCase())).length;
  const uniqueProviders = [...new Set(records.map(r => r.prompt_bundle_provider).filter(Boolean))];
  const uniqueSkills = [...new Set(records.flatMap(r => r.pinned_skill_ids || []).map(shortSkillId))];
  const totalMemHits = records.reduce((s, r) => s + (r.memory_hits || []).length, 0);

  return (
    <div className="attrib-panel">
      {/* Summary bar */}
      <div className="attrib-summary">
        <div className="attrib-summary-stat">
          <span className="stat-num">{total}</span>
          <span className="stat-label">phases</span>
        </div>
        <div className="attrib-summary-stat success">
          <span className="stat-num">{successes}</span>
          <span className="stat-label">succeeded</span>
        </div>
        {failures > 0 && (
          <div className="attrib-summary-stat failure">
            <span className="stat-num">{failures}</span>
            <span className="stat-label">failed</span>
          </div>
        )}
        <div className="attrib-summary-stat">
          <span className="stat-num">{totalMemHits}</span>
          <span className="stat-label">memory hits</span>
        </div>
        <div className="attrib-summary-providers">
          {uniqueProviders.map(p => <ProviderChip key={p} provider={p} />)}
        </div>
      </div>

      {/* Active skills across run */}
      {uniqueSkills.length > 0 && (
        <div className="attrib-skills-row">
          <span className="attrib-skills-label">Skills active this run:</span>
          {uniqueSkills.map(s => (
            <span key={s} className="attrib-skill-pill">{s}</span>
          ))}
        </div>
      )}

      {/* Per-phase records */}
      <div className="attrib-list">
        {records.map(r => (
          <AttributionRow key={r.attribution_id} record={r} />
        ))}
      </div>

      <style jsx>{`
        .attrib-panel {
          display: flex;
          flex-direction: column;
          gap: 0.8rem;
        }

        /* Summary */
        .attrib-summary {
          display: flex;
          flex-wrap: wrap;
          align-items: center;
          gap: 1rem;
          padding: 0.65rem 0.85rem;
          background: rgba(255,255,255,0.03);
          border: 1px solid rgba(255,255,255,0.06);
          border-radius: 10px;
        }
        .attrib-summary-stat {
          display: flex;
          flex-direction: column;
          gap: 0.05rem;
        }
        .stat-num {
          font-size: 1.1rem;
          font-weight: 900;
          color: rgba(255,255,255,0.7);
          line-height: 1;
        }
        .stat-label {
          font-size: 0.6rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: rgba(255,255,255,0.28);
        }
        .attrib-summary-stat.success .stat-num { color: #8fae7c; }
        .attrib-summary-stat.failure .stat-num { color: #c7684c; }
        .attrib-summary-providers {
          display: flex;
          gap: 0.35rem;
          flex-wrap: wrap;
          margin-left: auto;
        }

        /* Skills row */
        .attrib-skills-row {
          display: flex;
          flex-wrap: wrap;
          gap: 0.35rem;
          align-items: center;
        }
        .attrib-skills-label {
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: rgba(194,163,114,0.5);
          white-space: nowrap;
        }
        .attrib-skill-pill {
          font-size: 0.62rem;
          font-weight: 700;
          color: rgba(194,163,114,0.7);
          background: rgba(194,163,114,0.07);
          border: 1px solid rgba(194,163,114,0.15);
          border-radius: 6px;
          padding: 0.12rem 0.45rem;
          font-family: monospace;
        }

        /* List */
        .attrib-list {
          display: flex;
          flex-direction: column;
          gap: 0.55rem;
        }
      `}</style>
    </div>
  );
}
