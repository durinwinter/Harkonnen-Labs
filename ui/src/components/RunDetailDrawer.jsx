import React, { useEffect, useRef, useState } from 'react';
import CausalReportPanel from './CausalReportPanel';
import ConsolidationWorkbench from './ConsolidationWorkbench';
import ValidationPanel from './ValidationPanel';
import CoobieSignalPanel from './CoobieSignalPanel';
import MemoryCandidatesPanel from './MemoryCandidatesPanel';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

async function fetchJson(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  return r.json();
}

// Parse Residue-style exploration_log.md into structured episode objects
function parseExplorationLog(text) {
  const episodes = [];
  const blocks = text.split(/^##\s+Episode/m).slice(1);
  for (const block of blocks) {
    const ep = {};
    const phaseMatch = block.match(/^[^\n]*/);
    ep.phase = phaseMatch ? phaseMatch[0].replace(/^\d+\s*[-–·]?\s*/, '').trim() : '';
    const fields = ['strategy', 'outcome', 'failure_constraint', 'surviving_structure', 'reformulation'];
    for (const field of fields) {
      const re = new RegExp(`\\*\\*${field.replace('_', '[_ ]')}\\*\\*[:\\s]+([^\\n]+)`, 'i');
      const m = block.match(re);
      if (m) ep[field] = m[1].trim();
    }
    const outcome = (ep.outcome || '').toLowerCase();
    ep.outcomeKind = outcome.includes('pass') || outcome.includes('success') ? 'pass'
      : outcome.includes('fail') || outcome.includes('error') ? 'fail' : 'neutral';
    episodes.push(ep);
  }
  return episodes;
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
  const [decisions, setDecisions] = useState([]);
  const [tab, setTab] = useState('overview');
  const [error, setError] = useState('');
  const [explorationLog, setExplorationLog] = useState(null);
  const [corpusResults, setCorpusResults] = useState(null);
  const [artifactError, setArtifactError] = useState('');
  const drawerRef = useRef(null);

  useEffect(() => {
    if (!runId) return;
    let cancelled = false;

    const load = async () => {
      try {
        const [data, decisionData] = await Promise.all([
          fetchJson(`${API_BASE}/runs/${runId}/state`),
          fetchJson(`${API_BASE}/runs/${runId}/decisions`),
        ]);
        if (!cancelled) {
          setState(data);
          setDecisions(decisionData);
          setError('');
        }
      } catch (err) {
        if (!cancelled) setError(err.message);
      }
    };

    load();
    const interval = setInterval(load, 3000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [runId]);

  useEffect(() => {
    if (!runId || (tab !== 'explore' && tab !== 'corpus')) return;
    let cancelled = false;
    setArtifactError('');

    const load = async () => {
      try {
        if (tab === 'explore') {
          const r = await fetch(`${API_BASE}/runs/${runId}/artifacts/exploration_log.md`);
          if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
          const text = await r.text();
          if (!cancelled) setExplorationLog(text);
        } else {
          const r = await fetch(`${API_BASE}/runs/${runId}/artifacts/corpus_results.json`);
          if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
          const json = await r.json();
          if (!cancelled) setCorpusResults(json);
        }
      } catch (err) {
        if (!cancelled) setArtifactError(err.message);
      }
    };

    load();
    return () => { cancelled = true; };
  }, [runId, tab]);

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

  const hasExploreLog = blackboard?.artifact_refs?.includes('exploration_log.md');
  const hasCorpusResults = blackboard?.artifact_refs?.includes('corpus_results.json');
  const TABS = ['overview', 'timeline', 'decisions', 'agents', 'causal', 'lessons', 'memory', 'workbench', 'explore', 'corpus'];

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
          {TABS.map((t) => {
            const hasData = (t === 'explore' && hasExploreLog) || (t === 'corpus' && hasCorpusResults);
            return (
              <button
                key={t}
                className={`drawer-tab ${tab === t ? 'active' : ''} ${hasData ? 'has-data' : ''}`}
                onClick={() => setTab(t)}
              >
                {t}{hasData ? ' ·' : ''}
              </button>
            );
          })}
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
                <span className="ov-label">Decisions</span>
                <span className="ov-value">{decisions.length}</span>
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

          {tab === 'decisions' && (
            <div className="timeline-scroll">
              {decisions.length === 0 ? (
                <div className="drawer-empty">No decisions recorded.</div>
              ) : (
                [...decisions].reverse().map((decision) => (
                  <div key={decision.decision_id} className="tl-item">
                    <div className="tl-meta">
                      <span>{titleCase(decision.phase)}</span>
                      <span>{decision.agent}</span>
                      <span className="tl-status">{titleCase(decision.decision_kind)}</span>
                    </div>
                    <div className="tl-message">
                      <strong>{decision.chose}</strong>
                      {decision.justification ? ` — ${decision.justification}` : ''}
                    </div>
                    {decision.alternatives?.length > 0 && (
                      <div className="ar-summary">
                        Alternatives: {decision.alternatives.join(', ')}
                      </div>
                    )}
                    <div className="tl-time">{new Date(decision.created_at).toLocaleString()}</div>
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

          {tab === 'explore' && (
            <div className="explore-panel">
              {artifactError ? (
                <div className="drawer-error">Could not load exploration_log.md: {artifactError}</div>
              ) : !explorationLog ? (
                <div className="drawer-empty">
                  {hasExploreLog ? 'Loading...' : 'exploration_log.md not in artifact refs yet.'}
                </div>
              ) : (
                <div className="explore-episodes">
                  {parseExplorationLog(explorationLog).map((ep, i) => (
                    <div key={i} className="episode-card">
                      <div className="ep-header">
                        <span className="ep-phase">{ep.phase || `Episode ${i + 1}`}</span>
                        <span className={`ep-outcome-chip outcome-${ep.outcomeKind}`}>{ep.outcome || '—'}</span>
                      </div>
                      {ep.strategy && (
                        <div className="ep-field"><span className="ep-field-label">Strategy</span><span>{ep.strategy}</span></div>
                      )}
                      {ep.failure_constraint && (
                        <div className="ep-field"><span className="ep-field-label">Failure constraint</span><span className="ep-failure">{ep.failure_constraint}</span></div>
                      )}
                      {ep.surviving_structure && (
                        <div className="ep-field"><span className="ep-field-label">Surviving structure</span><span>{ep.surviving_structure}</span></div>
                      )}
                      {ep.reformulation && (
                        <div className="ep-field"><span className="ep-field-label">Reformulation</span><span className="ep-reform">{ep.reformulation}</span></div>
                      )}
                    </div>
                  ))}
                  {parseExplorationLog(explorationLog).length === 0 && (
                    <pre className="explore-raw">{explorationLog}</pre>
                  )}
                </div>
              )}
            </div>
          )}

          {tab === 'corpus' && (
            <div className="corpus-panel">
              {artifactError ? (
                <div className="drawer-error">Could not load corpus_results.json: {artifactError}</div>
              ) : !corpusResults ? (
                <div className="drawer-empty">
                  {hasCorpusResults ? 'Loading...' : 'corpus_results.json not in artifact refs yet.'}
                </div>
              ) : (
                <>
                  <div className={`corpus-summary ${corpusResults.all_passed ? 'passed' : 'failed'}`}>
                    {corpusResults.all_passed ? 'All corpus tests passed' : 'One or more corpus tests failed'}
                  </div>
                  <div className="corpus-commands">
                    {(corpusResults.commands || []).map((cmd, i) => (
                      <div key={i} className={`corpus-cmd ${cmd.passed ? 'passed' : 'failed'}`}>
                        <div className="cmd-header">
                          <span className="cmd-label">{cmd.label || `Command ${i + 1}`}</span>
                          <span className={`cmd-badge ${cmd.passed ? 'passed' : 'failed'}`}>
                            {cmd.passed ? 'PASS' : `FAIL (exit ${cmd.exit_code})`}
                          </span>
                        </div>
                      </div>
                    ))}
                  </div>
                </>
              )}
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

          {tab === 'memory' && (
            <MemoryCandidatesPanel runId={runId} />
          )}

          {tab === 'workbench' && (
            <ConsolidationWorkbench runId={runId} />
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
        .drawer-tab.has-data { color: var(--text-primary); }
        .explore-panel, .corpus-panel { display: flex; flex-direction: column; gap: 0.75rem; }
        .explore-episodes { display: flex; flex-direction: column; gap: 0.75rem; }
        .episode-card {
          border: 1px solid rgba(255,255,255,0.07);
          background: rgba(0,0,0,0.18);
          border-radius: 12px;
          padding: 0.85rem;
          display: flex;
          flex-direction: column;
          gap: 0.5rem;
        }
        .ep-header { display: flex; justify-content: space-between; align-items: center; gap: 0.5rem; margin-bottom: 0.2rem; }
        .ep-phase { font-size: 0.72rem; font-weight: 800; text-transform: uppercase; letter-spacing: 0.08em; color: var(--accent-gold); }
        .ep-outcome-chip {
          font-size: 0.68rem; font-weight: 800; text-transform: uppercase; letter-spacing: 0.08em;
          border: 1px solid; border-radius: 999px; padding: 0.18rem 0.5rem;
        }
        .ep-outcome-chip.outcome-pass { color: #8fae7c; border-color: rgba(143,174,124,0.4); }
        .ep-outcome-chip.outcome-fail { color: #c7684c; border-color: rgba(199,104,76,0.4); }
        .ep-outcome-chip.outcome-neutral { color: var(--text-secondary); border-color: rgba(255,255,255,0.12); }
        .ep-field { display: flex; flex-direction: column; gap: 0.15rem; }
        .ep-field-label { font-size: 0.62rem; font-weight: 800; text-transform: uppercase; letter-spacing: 0.1em; color: var(--accent-gold); }
        .ep-failure { color: #c7684c; font-size: 0.82rem; }
        .ep-reform { color: #8fae7c; font-size: 0.82rem; }
        .explore-raw { font-family: var(--font-mono); font-size: 0.74rem; color: var(--text-secondary); white-space: pre-wrap; overflow-x: auto; }
        .corpus-summary {
          border-radius: 12px; padding: 0.75rem 1rem;
          font-size: 0.88rem; font-weight: 700; text-align: center;
        }
        .corpus-summary.passed { background: rgba(143,174,124,0.12); border: 1px solid rgba(143,174,124,0.35); color: #8fae7c; }
        .corpus-summary.failed { background: rgba(120,39,30,0.18); border: 1px solid rgba(199,104,76,0.4); color: #c7684c; }
        .corpus-commands { display: flex; flex-direction: column; gap: 0.5rem; }
        .corpus-cmd {
          border: 1px solid rgba(255,255,255,0.06);
          background: rgba(0,0,0,0.18);
          border-radius: 10px;
          padding: 0.65rem 0.85rem;
        }
        .corpus-cmd.passed { border-color: rgba(143,174,124,0.25); }
        .corpus-cmd.failed { border-color: rgba(199,104,76,0.35); background: rgba(120,39,30,0.1); }
        .cmd-header { display: flex; justify-content: space-between; align-items: center; gap: 0.5rem; }
        .cmd-label { font-family: var(--font-mono); font-size: 0.78rem; }
        .cmd-badge { font-size: 0.66rem; font-weight: 800; text-transform: uppercase; letter-spacing: 0.08em; border-radius: 999px; padding: 0.18rem 0.55rem; border: 1px solid; }
        .cmd-badge.passed { color: #8fae7c; border-color: rgba(143,174,124,0.4); }
        .cmd-badge.failed { color: #c7684c; border-color: rgba(199,104,76,0.4); }
      `}</style>
    </div>
  );
}
