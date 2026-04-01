import React, { useEffect, useRef, useState } from 'react';
import CausalReportPanel from './CausalReportPanel';
import ValidationPanel from './ValidationPanel';
import CoobieSignalPanel from './CoobieSignalPanel';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://localhost:3000/api';

async function fetchJson(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  return r.json();
}

function titleCase(value) {
  if (!value) return 'idle';
  return value
    .replaceAll('_', ' ')
    .split(' ')
    .filter(Boolean)
    .map((p) => p[0].toUpperCase() + p.slice(1))
    .join(' ');
}

export default function RunDetailDrawer({ runId, onClose }) {
  const [state, setState] = useState(null);
  const [tab, setTab] = useState('overview');
  const [error, setError] = useState('');
  const drawerRef = useRef(null);

  useEffect(() => {
    if (!runId) return;
    let cancelled = false;

    const load = async () => {
      try {
        const data = await fetchJson(`${API_BASE}/runs/${runId}/state`);
        if (!cancelled) { setState(data); setError(''); }
      } catch (err) {
        if (!cancelled) setError(err.message);
      }
    };

    load();
    const interval = setInterval(load, 3000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [runId]);

  // Close on outside click
  useEffect(() => {
    const handler = (e) => {
      if (drawerRef.current && !drawerRef.current.contains(e.target)) {
        onClose?.();
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  // Close on Escape
  useEffect(() => {
    const handler = (e) => { if (e.key === 'Escape') onClose?.(); };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [onClose]);

  if (!runId) return null;

  const run = state?.run;
  const events = state?.events || [];
  const blackboard = state?.blackboard;
  const lessons = state?.lessons || [];
  const executions = state?.agent_executions || [];
  const coobieTranslations = state?.coobie_translations || [];

  const validation = blackboard?.artifact_refs?.includes('validation.json') ? null : null;
  const TABS = ['overview', 'timeline', 'agents', 'causal', 'lessons'];

  return (
    <div className="drawer-overlay">
      <div className="drawer-panel" ref={drawerRef}>
        <div className="drawer-header">
          <div>
            <div className="drawer-eyebrow">Run Detail</div>
            <div className="drawer-title">
              {run ? `${run.product} · ${run.spec_id}` : runId.slice(0, 8)}
            </div>
            <div className="drawer-meta">
              <span>{runId.slice(0, 8)}</span>
              <span>{titleCase(run?.status || 'loading')}</span>
              {run && <span>{new Date(run.created_at).toLocaleString()}</span>}
            </div>
          </div>
          <button className="drawer-close" onClick={onClose}>✕</button>
        </div>

        {error && <div className="drawer-error">Error: {error}</div>}

        <div className="drawer-tabs">
          {TABS.map((t) => (
            <button
              key={t}
              className={`drawer-tab ${tab === t ? 'active' : ''}`}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </div>

        <div className="drawer-body">
          {tab === 'overview' && (
            <div className="overview-grid">
              <div className="ov-stat">
                <span className="ov-label">Status</span>
                <span className="ov-value">{titleCase(run?.status || '—')}</span>
              </div>
              <div className="ov-stat">
                <span className="ov-label">Phase</span>
                <span className="ov-value">{titleCase(blackboard?.current_phase || '—')}</span>
              </div>
              <div className="ov-stat">
                <span className="ov-label">Events</span>
                <span className="ov-value">{events.length}</span>
              </div>
              <div className="ov-stat">
                <span className="ov-label">Lessons</span>
                <span className="ov-value">{lessons.length}</span>
              </div>
              <div className="ov-stat">
                <span className="ov-label">Artifacts</span>
                <span className="ov-value">{blackboard?.artifact_refs?.length || 0}</span>
              </div>
              <div className="ov-stat">
                <span className="ov-label">Agents run</span>
                <span className="ov-value">{executions.length}</span>
              </div>

              {blackboard?.open_blockers?.length > 0 && (
                <div className="ov-blockers">
                  {blackboard.open_blockers.map((b) => (
                    <span key={b} className="blocker-chip">{b}</span>
                  ))}
                </div>
              )}

              <div className="ov-artifacts">
                <div className="ov-label">Artifact refs</div>
                {(blackboard?.artifact_refs || []).length === 0 ? (
                  <span className="ov-empty">None yet</span>
                ) : (
                  <div className="artifact-list">
                    {blackboard.artifact_refs.map((a) => (
                      <span key={a} className="artifact-chip">{a}</span>
                    ))}
                  </div>
                )}
              </div>
            </div>
          )}

          {tab === 'timeline' && (
            <div className="timeline-scroll">
              {events.length === 0 ? (
                <div className="drawer-empty">No events recorded.</div>
              ) : (
                [...events].reverse().map((ev) => (
                  <div key={ev.event_id} className="tl-item">
                    <div className="tl-meta">
                      <span>{titleCase(ev.phase)}</span>
                      <span>{ev.agent}</span>
                      <span className={`tl-status status-${ev.status}`}>{ev.status}</span>
                    </div>
                    <div className="tl-message">{ev.message}</div>
                    <div className="tl-time">{new Date(ev.created_at).toLocaleString()}</div>
                  </div>
                ))
              )}
            </div>
          )}

          {tab === 'agents' && (
            <div className="agents-list">
              {executions.length === 0 ? (
                <div className="drawer-empty">No agent executions recorded.</div>
              ) : (
                executions.map((ex) => (
                  <div key={ex.agent_name} className="agent-row">
                    <div className="ar-header">
                      <span className="ar-name">{ex.display_name || ex.agent_name}</span>
                      <span className="ar-engine">{ex.provider}/{ex.model}</span>
                    </div>
                    <div className="ar-summary">{ex.summary}</div>
                    <details className="ar-output">
                      <summary>Output</summary>
                      <pre>{ex.output}</pre>
                    </details>
                  </div>
                ))
              )}
            </div>
          )}

          {tab === 'causal' && (
            <div className="agents-list">
              <CoobieSignalPanel translations={coobieTranslations} />
              <CausalReportPanel runId={runId} />
            </div>
          )}

          {tab === 'lessons' && (
            <div className="lessons-list">
              {lessons.length === 0 ? (
                <div className="drawer-empty">No lessons promoted for this run.</div>
              ) : (
                lessons.map((l) => (
                  <div key={l.lesson_id} className="lesson-row">
                    <div className="lesson-pattern">{l.pattern}</div>
                    <div className="lesson-tags">
                      {(l.tags || []).map((t) => (
                        <span key={t} className="lesson-tag">{t}</span>
                      ))}
                    </div>
                    {l.intervention && (
                      <div className="lesson-intervention">→ {l.intervention}</div>
                    )}
                    <div className="lesson-meta">
                      strength {l.strength?.toFixed(2)} · recalled {l.recall_count}×
                    </div>
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      </div>

      <style jsx>{`
        .drawer-overlay {
          position: fixed;
          inset: 0;
          background: rgba(0, 0, 0, 0.55);
          backdrop-filter: blur(4px);
          z-index: 1000;
          display: flex;
          justify-content: flex-end;
        }
        .drawer-panel {
          width: min(680px, 96vw);
          height: 100vh;
          background: #1a1d1f;
          border-left: 1px solid rgba(229, 225, 216, 0.1);
          box-shadow: -24px 0 64px rgba(0, 0, 0, 0.5);
          display: flex;
          flex-direction: column;
          overflow: hidden;
        }
        .drawer-header {
          padding: 1.2rem 1.3rem 0.85rem;
          border-bottom: 1px solid rgba(255,255,255,0.07);
          display: flex;
          justify-content: space-between;
          align-items: flex-start;
          flex-shrink: 0;
        }
        .drawer-eyebrow {
          text-transform: uppercase;
          letter-spacing: 0.16em;
          font-size: 0.68rem;
          font-weight: 800;
          color: var(--accent-gold);
          margin-bottom: 0.35rem;
        }
        .drawer-title {
          font-size: 1.25rem;
          font-weight: 700;
          margin-bottom: 0.45rem;
        }
        .drawer-meta {
          display: flex;
          flex-wrap: wrap;
          gap: 0.5rem;
          font-size: 0.78rem;
          color: var(--text-secondary);
        }
        .drawer-meta span {
          background: rgba(255,255,255,0.04);
          border: 1px solid rgba(255,255,255,0.07);
          border-radius: 999px;
          padding: 0.2rem 0.55rem;
        }
        .drawer-close {
          background: none;
          border: 1px solid rgba(255,255,255,0.12);
          color: var(--text-secondary);
          border-radius: 50%;
          width: 32px;
          height: 32px;
          cursor: pointer;
          font-size: 0.9rem;
          display: flex;
          align-items: center;
          justify-content: center;
          flex-shrink: 0;
        }
        .drawer-close:hover { color: var(--text-primary); border-color: rgba(255,255,255,0.3); }
        .drawer-error {
          padding: 0.65rem 1.3rem;
          background: rgba(120,39,30,0.25);
          color: #f0c7bc;
          font-size: 0.82rem;
          flex-shrink: 0;
        }
        .drawer-tabs {
          display: flex;
          gap: 0;
          border-bottom: 1px solid rgba(255,255,255,0.07);
          flex-shrink: 0;
          overflow-x: auto;
        }
        .drawer-tab {
          padding: 0.65rem 1rem;
          background: none;
          border: none;
          color: var(--text-secondary);
          font-size: 0.76rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          cursor: pointer;
          border-bottom: 2px solid transparent;
          transition: color 0.15s, border-color 0.15s;
          white-space: nowrap;
        }
        .drawer-tab:hover { color: var(--text-primary); }
        .drawer-tab.active {
          color: var(--accent-gold);
          border-bottom-color: var(--accent-gold);
        }
        .drawer-body {
          flex: 1;
          overflow-y: auto;
          padding: 1.1rem 1.3rem;
        }
        .drawer-empty {
          color: var(--text-secondary);
          font-size: 0.84rem;
          padding: 0.5rem 0;
        }
        .overview-grid {
          display: grid;
          grid-template-columns: repeat(3, 1fr);
          gap: 0.65rem;
        }
        .ov-stat {
          background: rgba(255,255,255,0.03);
          border: 1px solid rgba(255,255,255,0.06);
          border-radius: 12px;
          padding: 0.75rem 0.85rem;
        }
        .ov-label {
          display: block;
          font-size: 0.65rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: var(--accent-gold);
          margin-bottom: 0.3rem;
        }
        .ov-value {
          font-size: 1rem;
          font-weight: 700;
        }
        .ov-blockers, .ov-artifacts {
          grid-column: 1 / -1;
          padding: 0.65rem;
          background: rgba(255,255,255,0.02);
          border: 1px solid rgba(255,255,255,0.05);
          border-radius: 12px;
        }
        .ov-empty { color: var(--text-secondary); font-size: 0.78rem; }
        .blocker-chip, .artifact-chip, .lesson-tag {
          display: inline-block;
          padding: 0.22rem 0.6rem;
          border-radius: 999px;
          font-size: 0.72rem;
          font-weight: 700;
          margin: 0.2rem 0.2rem 0 0;
        }
        .blocker-chip {
          background: rgba(199,104,76,0.15);
          border: 1px solid rgba(199,104,76,0.4);
          color: #c7684c;
        }
        .artifact-chip {
          background: rgba(255,255,255,0.05);
          border: 1px solid rgba(255,255,255,0.08);
          color: var(--text-secondary);
          font-family: var(--font-mono);
          font-size: 0.68rem;
        }
        .artifact-list { margin-top: 0.35rem; }
        .timeline-scroll { display: flex; flex-direction: column; gap: 0; }
        .tl-item {
          border-left: 2px solid rgba(194, 163, 114, 0.4);
          padding: 0.4rem 0 0.4rem 0.9rem;
          margin-bottom: 0.5rem;
        }
        .tl-meta {
          display: flex;
          flex-wrap: wrap;
          gap: 0.45rem;
          font-size: 0.65rem;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          color: var(--accent-gold);
          margin-bottom: 0.2rem;
        }
        .tl-status { font-weight: 800; }
        .tl-message { font-size: 0.86rem; line-height: 1.45; }
        .tl-time { color: var(--text-secondary); font-size: 0.7rem; font-family: var(--font-mono); margin-top: 0.2rem; }
        .agents-list { display: flex; flex-direction: column; gap: 0.75rem; }
        .agent-row {
          border: 1px solid rgba(255,255,255,0.06);
          background: rgba(0,0,0,0.18);
          border-radius: 12px;
          padding: 0.85rem;
        }
        .ar-header {
          display: flex;
          justify-content: space-between;
          align-items: baseline;
          margin-bottom: 0.35rem;
        }
        .ar-name { font-weight: 800; font-size: 0.9rem; }
        .ar-engine { font-size: 0.72rem; color: var(--text-secondary); font-family: var(--font-mono); }
        .ar-summary { font-size: 0.82rem; color: var(--text-secondary); line-height: 1.45; margin-bottom: 0.45rem; }
        .ar-output summary { cursor: pointer; font-size: 0.72rem; color: var(--accent-gold); text-transform: uppercase; letter-spacing: 0.08em; }
        .ar-output pre {
          margin-top: 0.45rem;
          background: rgba(0,0,0,0.3);
          border-radius: 8px;
          padding: 0.65rem;
          font-size: 0.72rem;
          font-family: var(--font-mono);
          overflow-x: auto;
          white-space: pre-wrap;
          color: var(--text-secondary);
          max-height: 260px;
        }
        .lessons-list { display: flex; flex-direction: column; gap: 0.75rem; }
        .lesson-row {
          border: 1px solid rgba(255,255,255,0.06);
          background: rgba(0,0,0,0.18);
          border-radius: 12px;
          padding: 0.85rem;
        }
        .lesson-pattern { font-size: 0.9rem; font-weight: 600; line-height: 1.45; margin-bottom: 0.4rem; }
        .lesson-intervention { font-size: 0.8rem; color: #8fae7c; margin-top: 0.3rem; }
        .lesson-meta { font-size: 0.7rem; color: var(--text-secondary); font-family: var(--font-mono); margin-top: 0.35rem; }
        .lesson-tag {
          background: rgba(194, 163, 114, 0.1);
          border: 1px solid rgba(194, 163, 114, 0.25);
          color: var(--accent-gold);
        }
      `}</style>
    </div>
  );
}
