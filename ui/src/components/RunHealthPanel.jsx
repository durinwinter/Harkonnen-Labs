import React, { useCallback, useEffect, useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

const STATUS = {
  ready: { label: 'Ready', color: '#64c27b', bg: 'rgba(100,194,123,0.12)' },
  running: { label: 'Running', color: '#58b8e8', bg: 'rgba(88,184,232,0.12)' },
  needs_review: { label: 'Needs review', color: '#e0b34f', bg: 'rgba(224,179,79,0.14)' },
  blocked: { label: 'Blocked', color: '#f07f62', bg: 'rgba(240,127,98,0.14)' },
};

function titleCase(value) {
  return String(value || 'unknown')
    .replaceAll('_', ' ')
    .split(' ')
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(' ');
}

function HealthChip({ label, tone = 'neutral' }) {
  const color = tone === 'good' ? '#64c27b' : tone === 'warn' ? '#e0b34f' : tone === 'bad' ? '#f07f62' : '#b8b2a7';
  const bg = tone === 'good' ? 'rgba(100,194,123,0.12)' : tone === 'warn' ? 'rgba(224,179,79,0.14)' : tone === 'bad' ? 'rgba(240,127,98,0.14)' : 'rgba(255,255,255,0.07)';
  return (
    <span style={{
      color,
      background: bg,
      borderRadius: 99,
      padding: '3px 8px',
      fontSize: 11,
      fontWeight: 750,
      whiteSpace: 'nowrap',
    }}>
      {label}
    </span>
  );
}

export default function RunHealthPanel({ runId }) {
  const [health, setHealth] = useState(null);
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    if (!runId) return;
    try {
      const response = await fetch(`${API_BASE}/runs/${runId}/health`);
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      setHealth(await response.json());
      setError('');
    } catch (err) {
      setError(err.message);
    }
  }, [runId]);

  useEffect(() => {
    load();
    const interval = setInterval(load, 5000);
    return () => clearInterval(interval);
  }, [load]);

  if (error) {
    return (
      <div className="run-health error">Health unavailable: {error}</div>
    );
  }
  if (!health) return null;

  const status = STATUS[health.status] || STATUS.running;
  const checks = health.checks || {};
  const validationTone = checks.validation?.passed === true ? 'good' : checks.validation?.passed === false ? 'bad' : 'neutral';
  const hiddenTone = checks.hidden_scenarios?.passed === true ? 'good' : checks.hidden_scenarios?.passed === false ? 'bad' : 'neutral';
  const memoryTone = checks.memory_chain?.status === 'clear' ? 'good' : checks.memory_chain?.status === 'blocked' ? 'bad' : 'warn';
  const contextTone = checks.context_utilization?.status === 'healthy' ? 'good' : checks.context_utilization?.status === 'low' ? 'warn' : 'neutral';

  return (
    <div className="run-health">
      <div className="health-top">
        <div>
          <div className="health-label">Run Health</div>
          <div className="health-title" style={{ color: status.color }}>{status.label}</div>
        </div>
        <span className="health-status" style={{ color: status.color, background: status.bg }}>
          {titleCase(health.run_status)}
        </span>
      </div>

      <div className="health-chips">
        <HealthChip label={`validation ${checks.validation?.passed === true ? 'pass' : checks.validation?.passed === false ? 'fail' : 'pending'}`} tone={validationTone} />
        <HealthChip label={`hidden ${checks.hidden_scenarios?.passed === true ? 'pass' : checks.hidden_scenarios?.passed === false ? 'fail' : 'pending'}`} tone={hiddenTone} />
        <HealthChip label={`memory ${checks.memory_chain?.status || 'clear'}`} tone={memoryTone} />
        <HealthChip label={`context ${Math.round((checks.context_utilization?.rate || 0) * 100)}%`} tone={contextTone} />
        <HealthChip label={`audit ${checks.plan_audit?.unresolved_count || 0}`} tone={(checks.plan_audit?.unresolved_count || 0) > 0 ? 'warn' : 'good'} />
      </div>

      {(health.blockers?.length > 0 || health.review_items?.length > 0) && (
        <div className="health-notes">
          {(health.blockers || []).slice(0, 4).map((item) => (
            <span key={`b-${item}`} className="health-note blocker">{item}</span>
          ))}
          {(health.review_items || []).slice(0, 4).map((item) => (
            <span key={`r-${item}`} className="health-note review">{item}</span>
          ))}
        </div>
      )}

      <style jsx>{`
        .run-health {
          grid-column: 1 / -1;
          border: 1px solid rgba(255,255,255,0.08);
          border-radius: 8px;
          background: #151819;
          padding: 12px;
        }
        .run-health.error {
          color: #f4b1a2;
          background: rgba(240,127,98,0.1);
          border-color: rgba(240,127,98,0.24);
          font-size: 12px;
        }
        .health-top {
          display: flex;
          justify-content: space-between;
          gap: 12px;
          align-items: flex-start;
          margin-bottom: 10px;
        }
        .health-label {
          color: var(--text-secondary);
          font-size: 11px;
          font-weight: 750;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          margin-bottom: 3px;
        }
        .health-title {
          font-size: 1.2rem;
          font-weight: 800;
        }
        .health-status {
          border-radius: 99px;
          padding: 4px 9px;
          font-size: 11px;
          font-weight: 800;
        }
        .health-chips {
          display: flex;
          flex-wrap: wrap;
          gap: 6px;
        }
        .health-notes {
          display: flex;
          flex-wrap: wrap;
          gap: 6px;
          margin-top: 10px;
        }
        .health-note {
          color: #c9c1b4;
          background: rgba(255,255,255,0.06);
          border-radius: 5px;
          padding: 3px 7px;
          font-size: 11px;
        }
        .health-note.blocker {
          color: #f07f62;
          background: rgba(240,127,98,0.12);
        }
        .health-note.review {
          color: #e0b34f;
          background: rgba(224,179,79,0.12);
        }
      `}</style>
    </div>
  );
}
