import React, { useEffect, useState } from 'react';
import AgentCard from './components/AgentCard';
import CoobieSignalPanel from './components/CoobieSignalPanel';
import CausalTesseract from './visualization/tesseract/CausalTesseract';
import CausalWorkbench from './components/CausalWorkbench';
import FactoryFloor from './components/FactoryFloor';
import AttributionPanel from './components/AttributionPanel';
import OperationsDeck from './components/OperationsDeck';
import PackChat from './components/PackChat';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

// Agent roster — source of truth, kept in sync with AGENTS.md
// pinned: 'claude' means trust-critical, always routed to Claude
const AGENT_DEFS = [
  {
    id: 'scout', name: 'Scout', role: 'Spec Retriever', group: 'planning', accentColor: '#c4922a',
    pinned: 'claude',
    desc: 'Parse specs, flag ambiguity, produce intent package',
  },
  {
    id: 'keeper', name: 'Keeper', role: 'Boundary Retriever', group: 'planning', accentColor: '#8a7a3a',
    pinned: 'claude',
    desc: 'Enforce policy, guard boundaries, and manage file-claim coordination',
  },
  {
    id: 'mason', name: 'Mason', role: 'Build Retriever', group: 'action', accentColor: '#c4662a',
    pinned: null,
    desc: 'Generate and modify code, multi-file changes',
  },
  {
    id: 'piper', name: 'Piper', role: 'Tool Retriever', group: 'action', accentColor: '#5a7a5a',
    pinned: null,
    desc: 'Run build tools, fetch docs, execute helpers',
  },
  {
    id: 'ash', name: 'Ash', role: 'Twin Retriever', group: 'action', accentColor: '#2a7a7a',
    pinned: null,
    desc: 'Provision digital twins, mock dependencies',
  },
  {
    id: 'bramble', name: 'Bramble', role: 'Test Retriever', group: 'verification', accentColor: '#a89a2a',
    pinned: null,
    desc: 'Generate tests, run lint/build/visible tests',
  },
  {
    id: 'sable', name: 'Sable', role: 'Scenario Retriever', group: 'verification', accentColor: '#3a4a5a',
    pinned: 'claude',
    desc: 'Execute hidden scenarios, produce eval reports',
  },
  {
    id: 'flint', name: 'Flint', role: 'Artifact Retriever', group: 'verification', accentColor: '#8a6a3a',
    pinned: null,
    desc: 'Collect outputs, package artifact bundles',
  },
  {
    id: 'coobie', name: 'Coobie', role: 'Memory Retriever', group: 'memory', accentColor: '#7a2a3a',
    pinned: null,
    desc: 'Coordinate pack memory: working context, episodic capture, causal graph, consolidation, and cross-agent blackboard health',
  },
];

function titleCase(value) {
  if (!value) {
    return 'idle';
  }
  return value
    .replaceAll('_', ' ')
    .split(' ')
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(' ');
}

function normalizeStatus(status, ownership) {
  const normalized = (status || '').toLowerCase();
  if (normalized === 'warning' || normalized === 'failed') {
    return 'blocked';
  }
  if (normalized === 'complete') {
    return 'complete';
  }
  if (normalized === 'running') {
    return 'running';
  }
  if (ownership) {
    return 'running';
  }
  return 'idle';
}

function deriveAgents(events, blackboard, executions) {
  const claims = blackboard?.agent_claims || {};
  const executionMap = Object.fromEntries(
    (executions || []).map((execution) => [execution.agent_name.toLowerCase(), execution]),
  );

  return AGENT_DEFS.map((definition) => {
    const agentEvents = (events || []).filter(
      (event) => event.agent.toLowerCase() === definition.id.toLowerCase(),
    );
    const latest = agentEvents.length > 0 ? agentEvents[agentEvents.length - 1] : null;
    const execution = executionMap[definition.id.toLowerCase()];
    const ownership = claims[definition.id] || '';

    return {
      ...definition,
      status: normalizeStatus(latest?.status, ownership),
      task: ownership || latest?.message || `Awaiting ${definition.role.toLowerCase()}`,
      latestLog: latest
        ? `${titleCase(latest.phase)} · ${latest.message}`
        : 'Ready for the next run.',
      latestPhase: latest ? titleCase(latest.phase) : 'Awaiting signal',
      ownership,
      engine: execution ? `${execution.provider}/${execution.model}` : 'unassigned',
      bundleProvider: execution?.prompt_bundle_provider || null,
      bundleFingerprint: execution?.prompt_bundle_fingerprint || null,
      pinnedSkillIds: execution?.pinned_skill_ids || [],
      desc: definition.desc,
      pinned: definition.pinned,
    };
  });
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json();
}

function Panel({ title, children, compact = false }) {
  return (
    <section className={`ops-panel ${compact ? 'compact' : ''}`}>
      <div className="panel-title-row">
        <h3>{title}</h3>
        <div className="panel-line"></div>
      </div>
      {children}
    </section>
  );
}

function App() {
  const [runs, setRuns] = useState([]);
  const [activeRunId, setActiveRunId] = useState('');
  const [runState, setRunState] = useState(null);
  const [selectedRole, setSelectedRole] = useState('mason');
  const [roleBoard, setRoleBoard] = useState(null);
  const [coordination, setCoordination] = useState(null);
  const [policyEvents, setPolicyEvents] = useState([]);
  const [capacity, setCapacity] = useState(null);
  const [showTesseract, setShowTesseract] = useState(false);
  const [showWorkbench, setShowWorkbench] = useState(false);
  const [showSystem, setShowSystem] = useState(false);
  const [causalReport, setCausalReport] = useState(null);
  const [error, setError] = useState('');

  useEffect(() => {
    let cancelled = false;

    const loadRuns = async () => {
      try {
        const data = await fetchJson(`${API_BASE}/runs`);
        if (cancelled) {
          return;
        }
        setRuns(data);
        setActiveRunId((current) => {
          if (!data.length) {
            return '';
          }
          if (current && data.some((run) => run.run_id === current)) {
            return current;
          }
          return data[0].run_id;
        });
        setError('');
      } catch (fetchError) {
        if (!cancelled) {
          setError(fetchError.message);
        }
      }
    };

    loadRuns();
    const interval = setInterval(loadRuns, 5000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, []);

  useEffect(() => {
    if (!activeRunId) {
      setRunState(null);
      return undefined;
    }

    let cancelled = false;

    const loadState = async () => {
      try {
        const data = await fetchJson(`${API_BASE}/runs/${activeRunId}/state`);
        if (!cancelled) {
          setRunState(data);
          setError('');
        }
      } catch (fetchError) {
        if (!cancelled) {
          setError(fetchError.message);
        }
      }
    };

    loadState();
    const interval = setInterval(loadState, 1500);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [activeRunId]);

  useEffect(() => {
    if (!activeRunId || !selectedRole) {
      setRoleBoard(null);
      return undefined;
    }

    let cancelled = false;

    const loadRoleBoard = async () => {
      try {
        const data = await fetchJson(`${API_BASE}/runs/${activeRunId}/blackboard/${selectedRole}`);
        if (!cancelled) {
          setRoleBoard(data);
        }
      } catch (fetchError) {
        if (!cancelled) {
          setRoleBoard(null);
        }
      }
    };

    loadRoleBoard();
    const interval = setInterval(loadRoleBoard, 1500);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [activeRunId, selectedRole]);

  useEffect(() => {
    let cancelled = false;

    const loadCoordination = async () => {
      try {
        const [assignments, events] = await Promise.all([
          fetchJson(`${API_BASE}/coordination/assignments`),
          fetchJson(`${API_BASE}/coordination/policy-events`),
        ]);
        if (!cancelled) {
          setCoordination(assignments);
          setPolicyEvents(Array.isArray(events) ? events : []);
        }
      } catch (fetchError) {
        if (!cancelled) {
          setCoordination(null);
          setPolicyEvents([]);
        }
      }
    };

    loadCoordination();
    const interval = setInterval(loadCoordination, 2000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    const loadCapacity = async () => {
      try {
        const data = await fetchJson(`${API_BASE}/capacity`);
        if (!cancelled) setCapacity(data);
      } catch { /* capacity.json optional */ }
    };

    loadCapacity();
    const interval = setInterval(loadCapacity, 5000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, []);

  // Fetch causal report for active run (used by workbench)
  useEffect(() => {
    if (!activeRunId) { setCausalReport(null); return undefined; }
    let cancelled = false;
    const load = async () => {
      try {
        const data = await fetchJson(`${API_BASE}/runs/${activeRunId}/causal-report`);
        if (!cancelled) setCausalReport(data);
      } catch { if (!cancelled) setCausalReport(null); }
    };
    load();
    return () => { cancelled = true; };
  }, [activeRunId]);

  const run = runState?.run || null;
  const events = runState?.events || [];
  const blackboard = runState?.blackboard || null;
  const lessons = runState?.lessons || [];
  const agentExecutions = runState?.agent_executions || [];
  const phaseAttributions = runState?.phase_attributions || [];
  const coobieTranslations = runState?.coobie_translations || [];
  const agents = deriveAgents(events, blackboard, agentExecutions);
  const planningAgents = agents.filter((agent) => agent.group === 'planning');
  const actionAgents = agents.filter((agent) => agent.group === 'action');
  const verificationAgents = agents.filter((agent) => agent.group === 'verification');
  const memoryAgent = agents.find((agent) => agent.group === 'memory');
  const recentEvents = [...events].slice(-14).reverse();
  const activeThreads = agents.filter((agent) => agent.status === 'running');
  const roleClaims = Object.entries(roleBoard?.agent_claims || {});
  const coordinationClaims = Object.entries(coordination?.active || {});
  const staleClaims = coordinationClaims.filter(([, claim]) => claim.status === 'stale');
  const healthyClaims = coordinationClaims.filter(([, claim]) => claim.status !== 'stale');
  const recentPolicyEvents = [...policyEvents].slice(-6).reverse();

  return (
    <div className="pack-board-shell">
      <header className="run-header glass-panel">
        <div>
          <div className="eyebrow">Harkonnen Labs · Pack Board</div>
          <h1>{run ? `${run.product} · ${run.spec_id}` : 'The pack is ready.'}</h1>
          <div className="header-meta">
            {run && <span>Run: {run.run_id.slice(0, 8)}</span>}
            {run && <span>Phase: {titleCase(blackboard?.current_phase || run.status || 'idle')}</span>}
            <span className={`status-pill status-${run?.status || 'idle'}`}>{run?.status || 'idle'}</span>
            <div className="pack-pins">
              {AGENT_DEFS.filter(a => a.pinned).map(a => (
                <span key={a.id} className="pack-pin" title={`${a.name} — pinned to Claude`}>{a.name}</span>
              ))}
              <span className="pack-pin-label">pinned to Claude</span>
            </div>
          </div>
        </div>

        <div className="header-controls">
          <label className="run-selector-label">
            Active run
            <select
              className="run-selector"
              value={activeRunId}
              onChange={(event) => setActiveRunId(event.target.value)}
            >
              {runs.length === 0 ? <option value="">No runs yet</option> : null}
              {runs.map((candidate) => (
                <option key={candidate.run_id} value={candidate.run_id}>
                  {candidate.run_id.slice(0, 8)} · {candidate.product} · {candidate.status}
                </option>
              ))}
            </select>
          </label>
          <div className="header-btn-row">
            <button
              className="tesseract-toggle"
              onClick={() => setShowTesseract(true)}
              title="Open Coobie Causal Tesseract"
            >
              Tesseract
            </button>
            <button
              className="tesseract-toggle workbench-btn"
              onClick={() => setShowWorkbench(true)}
              title="Open Causal Workbench — annotate run timeline for Coobie"
            >
              Workbench
            </button>
          </div>
        </div>
      </header>

      {showTesseract && <CausalTesseract onClose={() => setShowTesseract(false)} />}
      {showWorkbench && (
        <CausalWorkbench
          runId={activeRunId}
          events={events}
          causalReport={causalReport}
          onClose={() => setShowWorkbench(false)}
        />
      )}

      {error ? <div className="error-banner">API error: {error}</div> : null}

      <main className="dashboard-grid">
        <section className="main-column">
          <Panel title="Pack Chat">
            <PackChat
              activeRunId={activeRunId}
              agents={agents}
              onRunStarted={(runId) => setActiveRunId(runId)}
            />
          </Panel>

          <Panel title="Action Board">
            <FactoryFloor
              agents={agents}
              onOpenWorkbench={() => setShowWorkbench(true)}
            />
          </Panel>

          <Panel title="Attribution Board">
            <AttributionPanel phaseAttributions={phaseAttributions} />
          </Panel>

          <Panel title="Run Timeline">
            <div className="timeline-list">
              {recentEvents.length === 0 ? (
                <div className="empty-state">No events recorded yet.</div>
              ) : (
                recentEvents.map((event) => (
                  <div key={event.event_id} className="timeline-item">
                    <div className="timeline-meta">
                      <span>{titleCase(event.phase)}</span>
                      <span>{event.agent}</span>
                      <span>{event.status}</span>
                    </div>
                    <div className="timeline-message">{event.message}</div>
                    <div className="timeline-time">{new Date(event.created_at).toLocaleString()}</div>
                  </div>
                ))
              )}
            </div>
          </Panel>
        </section>

        <aside className="side-column">
          <Panel title="Memory Board" compact>
            {memoryAgent ? <AgentCard agent={memoryAgent} isSingleton /> : null}
            <div className="info-stack top-gap">
              <div className="info-row"><span>Lesson refs</span><strong>{blackboard?.lesson_refs?.length || 0}</strong></div>
              <div className="info-row"><span>Promoted lessons</span><strong>{lessons.length}</strong></div>
              <div className="info-row"><span>Recent recalls</span><strong>{agentExecutions.length}</strong></div>
              <div className="info-row"><span>Live pidgin signals</span><strong>{coobieTranslations.reduce((sum, item) => sum + (item.signals?.length || 0), 0)}</strong></div>
            </div>
            <div className="top-gap">
              <CoobieSignalPanel translations={coobieTranslations} compact />
            </div>
            <div className="list-block top-gap">
              {(lessons || []).length === 0 ? (
                <div className="empty-state">No lessons promoted for this run yet.</div>
              ) : (
                lessons.map((lesson) => (
                  <div key={lesson.lesson_id} className="list-item">
                    <div className="list-item-title">{lesson.pattern}</div>
                    <div className="list-item-subtle">
                      intervention: {lesson.intervention || 'none recorded'}
                    </div>
                  </div>
                ))
              )}
            </div>
          </Panel>

          <Panel title="Mission Board" compact>
            <div className="info-stack">
              <div className="info-row"><span>Current phase</span><strong>{titleCase(blackboard?.current_phase || 'idle')}</strong></div>
              <div className="info-row"><span>Active goal</span><strong>{blackboard?.active_goal || 'Awaiting a run.'}</strong></div>
              <div className="info-row"><span>Resolved items</span><strong>{blackboard?.resolved_items?.length || 0}</strong></div>
              <div className="info-row"><span>Artifacts tracked</span><strong>{blackboard?.artifact_refs?.length || 0}</strong></div>
            </div>
            <div className="chip-row">
              {(blackboard?.open_blockers || []).length === 0 ? (
                <span className="soft-chip ok">No blockers</span>
              ) : (
                blackboard.open_blockers.map((blocker) => (
                  <span key={blocker} className="soft-chip danger">{blocker}</span>
                ))
              )}
            </div>
            <div className="role-lens top-gap">
              <div className="role-lens-header">
                <span>Role lens</span>
                <select value={selectedRole} onChange={(event) => setSelectedRole(event.target.value)}>
                  {AGENT_DEFS.map((agent) => (
                    <option key={agent.id} value={agent.id}>{agent.name}</option>
                  ))}
                </select>
              </div>
              <div className="info-stack">
                <div className="info-row"><span>Lens phase</span><strong>{titleCase(roleBoard?.current_phase || 'idle')}</strong></div>
                <div className="info-row"><span>Visible lessons</span><strong>{roleBoard?.lesson_refs?.length || 0}</strong></div>
                <div className="info-row"><span>Visible artifacts</span><strong>{roleBoard?.artifact_refs?.length || 0}</strong></div>
                <div className="info-row"><span>Visible claims</span><strong>{roleClaims.length}</strong></div>
              </div>
            </div>
          </Panel>

          <Panel title="Evidence Board" compact>
            <div className="list-block compact-list">
              {(blackboard?.artifact_refs || []).length === 0 ? (
                <div className="empty-state">No artifact refs yet.</div>
              ) : (
                blackboard.artifact_refs.map((artifact) => (
                  <div key={artifact} className="list-item tight">{artifact}</div>
                ))
              )}
            </div>
          </Panel>

          <div className="system-section">
            <button
              className="system-toggle"
              onClick={() => setShowSystem(s => !s)}
            >
              <span>⚙ System</span>
              <span className="system-toggle-chevron">{showSystem ? '▲' : '▼'}</span>
            </button>
            {showSystem && <>
          <Panel title="Provider Capacity" compact>
            {!capacity ? (
              <div className="empty-state">capacity.json not loaded</div>
            ) : (
              <>
                <div className="capacity-chain">
                  {(capacity.fallback_chain || []).map((name, idx) => {
                    const p = capacity.providers?.[name] || {};
                    const statusColor = p.available === false ? '#c7684c' : p.status === 'near_limit' ? '#c4922a' : '#8fae7c';
                    return (
                      <div key={name} className="capacity-row">
                        <div className="capacity-rank">#{idx + 1}</div>
                        <div className="capacity-name">{name}</div>
                        <div className="capacity-chip" style={{ color: statusColor, borderColor: `${statusColor}55` }}>
                          <span className="status-dot" style={{ backgroundColor: statusColor }}></span>
                          {p.status || 'ok'}
                        </div>
                        {p.note && <div className="capacity-note">{p.note}</div>}
                      </div>
                    );
                  })}
                </div>
                <div className="capacity-updated">
                  updated {capacity.updated_at ? new Date(capacity.updated_at).toLocaleString() : '—'}
                </div>
              </>
            )}
          </Panel>

          <Panel title="Operations Deck" compact>
                <OperationsDeck activeRunId={activeRunId} />
              </Panel>

              <Panel title="Keeper Policy Board" compact>
            <div className="info-stack">
              <div className="info-row"><span>Managed by</span><strong>{coordination?.managed_by || 'keeper'}</strong></div>
              <div className="info-row"><span>Policy mode</span><strong>{coordination?.policy_mode || 'exclusive_file_claims'}</strong></div>
              <div className="info-row"><span>Heartbeat timeout</span><strong>{coordination?.stale_after_seconds || 600}s</strong></div>
              <div className="info-row"><span>Healthy claims</span><strong>{healthyClaims.length}</strong></div>
              <div className="info-row"><span>Stale claims</span><strong>{staleClaims.length}</strong></div>
              <div className="info-row"><span>Policy events</span><strong>{policyEvents.length}</strong></div>
            </div>

            <div className="list-block">
              <div className="list-item">
                <div className="list-item-title">{selectedRole} role view</div>
                <div className="list-item-subtle">{roleBoard?.active_goal || 'No role-scoped board loaded.'}</div>
              </div>
            </div>

            <div className="list-block compact-list top-gap">
              {coordinationClaims.length === 0 ? (
                <div className="empty-state">No live Keeper claims.</div>
              ) : (
                coordinationClaims.map(([agent, claim]) => (
                  <div key={agent} className={`list-item claim-item ${claim.status === 'stale' ? 'stale-claim' : ''}`}>
                    <div className="list-item-title">{agent}</div>
                    <div className="list-item-subtle">{claim.task}</div>
                    <div className="list-item-subtle">status: {claim.status || 'active'}</div>
                    <div className="list-item-subtle">last heartbeat: {claim.last_heartbeat_at ? new Date(claim.last_heartbeat_at).toLocaleString() : 'none recorded'}</div>
                    <div className="list-item-subtle mono top-gap-small">
                      {(claim.files || []).join(', ') || 'no files declared'}
                    </div>
                  </div>
                ))
              )}
            </div>

            <div className="list-block compact-list top-gap">
              {recentPolicyEvents.length === 0 ? (
                <div className="empty-state">No Keeper policy events yet.</div>
              ) : (
                recentPolicyEvents.map((event) => (
                  <div key={event.event_id} className={`list-item policy-event ${event.status}`}>
                    <div className="list-item-title">{event.event_type.replaceAll('_', ' ')}</div>
                    <div className="list-item-subtle">{event.message}</div>
                    <div className="list-item-subtle mono top-gap-small">
                      {new Date(event.created_at).toLocaleString()}
                    </div>
                  </div>
                ))
              )}
            </div>

            <div className="role-lens top-gap">
              <div className="list-block compact-list">
                {roleClaims.length === 0 ? (
                  <div className="empty-state">No claims visible to this role.</div>
                ) : (
                  roleClaims.map(([agent, claim]) => (
                    <div key={agent} className="list-item">
                      <div className="list-item-title">{agent}</div>
                      <div className="list-item-subtle">{claim}</div>
                    </div>
                  ))
                )}
              </div>
            </div>
          </Panel>
            </>}
          </div>
        </aside>
      </main>

      <footer className="footer-bar glass-panel">
        <span>Active threads: {activeThreads.length ? activeThreads.map((agent) => agent.name).join(', ') : 'none'}</span>
        <span>Events: {events.length}</span>
        <span>Lessons: {lessons.length}</span>
      </footer>

      <style jsx>{`
        .pack-board-shell {
          min-height: 100vh;
          background:
            radial-gradient(circle at top left, rgba(194, 163, 114, 0.12), transparent 28%),
            radial-gradient(circle at top right, rgba(94, 125, 113, 0.14), transparent 32%),
            linear-gradient(180deg, #171a1c 0%, #121416 100%);
          color: var(--text-primary);
          padding: 1.5rem;
          display: flex;
          flex-direction: column;
          gap: 1.25rem;
        }

        .glass-panel {
          background: rgba(27, 30, 32, 0.84);
          border: 1px solid var(--border-glass);
          box-shadow: 0 18px 40px rgba(0, 0, 0, 0.28);
          backdrop-filter: blur(14px);
        }

        .run-header,
        .footer-bar {
          border-radius: 18px;
          padding: 1.1rem 1.3rem;
        }

        .run-header {
          display: flex;
          justify-content: space-between;
          gap: 1rem;
          align-items: start;
        }

        .eyebrow {
          text-transform: uppercase;
          letter-spacing: 0.16em;
          font-size: 0.72rem;
          color: var(--accent-gold);
          margin-bottom: 0.45rem;
          font-weight: 800;
        }

        h1 {
          font-size: clamp(1.7rem, 3vw, 2.5rem);
          margin-bottom: 0.55rem;
          font-family: 'IBM Plex Sans', 'Segoe UI', sans-serif;
        }

        .header-meta {
          display: flex;
          flex-wrap: wrap;
          gap: 0.65rem;
          color: var(--text-secondary);
          font-size: 0.88rem;
        }

        .header-meta span,
        .status-pill,
        .soft-chip {
          border-radius: 999px;
          padding: 0.28rem 0.65rem;
          border: 1px solid rgba(255, 255, 255, 0.08);
          background: rgba(255, 255, 255, 0.03);
        }

        .pack-pins {
          display: flex;
          align-items: center;
          gap: 0.35rem;
          flex-wrap: wrap;
        }

        .pack-pin {
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          color: #c4922a;
          background: rgba(196, 146, 42, 0.12);
          border: 1px solid rgba(196, 146, 42, 0.28);
          border-radius: 999px;
          padding: 0.15rem 0.5rem;
        }

        .pack-pin-label {
          font-size: 0.58rem;
          color: rgba(255, 255, 255, 0.22);
          letter-spacing: 0.06em;
          text-transform: uppercase;
          background: none !important;
          border: none !important;
          padding: 0 !important;
        }

        .header-controls {
          display: flex;
          flex-direction: column;
          align-items: end;
          gap: 0.75rem;
        }

        .run-selector-label {
          display: flex;
          flex-direction: column;
          gap: 0.3rem;
          font-size: 0.72rem;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: var(--text-secondary);
          font-weight: 700;
        }

        .run-selector {
          min-width: 320px;
          max-width: 100%;
          border-radius: 12px;
          border: 1px solid var(--border-glass);
          background: #15181a;
          color: var(--text-primary);
          padding: 0.72rem 0.85rem;
          font: inherit;
        }

        .status-pill {
          text-transform: uppercase;
          letter-spacing: 0.12em;
          font-weight: 800;
          color: var(--accent-gold);
        }

        .dashboard-grid {
          display: grid;
          grid-template-columns: minmax(0, 1.9fr) minmax(320px, 0.95fr);
          gap: 1.2rem;
          align-items: start;
        }

        .main-column,
        .side-column {
          display: flex;
          flex-direction: column;
          gap: 1.1rem;
        }

        .ops-panel {
          background: rgba(22, 24, 26, 0.88);
          border: 1px solid var(--border-glass);
          border-radius: 18px;
          padding: 1rem 1rem 1.05rem;
          box-shadow: 0 18px 36px rgba(0, 0, 0, 0.24);
        }

        .panel-title-row {
          display: flex;
          align-items: center;
          gap: 0.8rem;
          margin-bottom: 0.95rem;
        }

        .panel-title-row h3 {
          white-space: nowrap;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          font-size: 0.82rem;
          color: var(--accent-gold);
        }

        .panel-line {
          height: 1px;
          flex: 1;
          background: linear-gradient(90deg, rgba(194, 163, 114, 0.55), transparent);
        }

        .agent-grid {
          display: grid;
          gap: 0.95rem;
        }

        .two-up {
          grid-template-columns: repeat(2, minmax(0, 1fr));
        }

        .three-up {
          grid-template-columns: repeat(3, minmax(0, 1fr));
        }

        .info-stack {
          display: flex;
          flex-direction: column;
          gap: 0.6rem;
        }

        .top-gap {
          margin-top: 0.9rem;
        }

        .top-gap-small {
          margin-top: 0.35rem;
        }

        .role-lens-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          gap: 0.75rem;
          margin-bottom: 0.75rem;
          color: var(--text-secondary);
          text-transform: uppercase;
          letter-spacing: 0.1em;
          font-size: 0.72rem;
          font-weight: 700;
        }

        .role-lens-header select {
          min-width: 150px;
          border-radius: 10px;
          border: 1px solid var(--border-glass);
          background: rgba(255, 255, 255, 0.04);
          color: var(--text-primary);
          padding: 0.45rem 0.65rem;
          font: inherit;
        }

        .info-row {
          display: flex;
          justify-content: space-between;
          gap: 0.8rem;
          border: 1px solid rgba(255, 255, 255, 0.05);
          background: rgba(255, 255, 255, 0.03);
          padding: 0.7rem 0.8rem;
          border-radius: 12px;
        }

        .info-row span {
          color: var(--text-secondary);
          font-size: 0.78rem;
          text-transform: uppercase;
          letter-spacing: 0.08em;
        }

        .info-row strong {
          font-size: 0.86rem;
          text-align: right;
        }

        .chip-row {
          display: flex;
          flex-wrap: wrap;
          gap: 0.5rem;
          margin-top: 0.85rem;
        }

        .soft-chip.ok {
          color: #8fae7c;
        }

        .soft-chip.danger {
          color: #d8876e;
        }

        .list-block {
          display: flex;
          flex-direction: column;
          gap: 0.55rem;
          margin-top: 0.9rem;
        }

        .compact-list {
          margin-top: 0;
        }

        .list-item {
          border: 1px solid rgba(255, 255, 255, 0.05);
          background: rgba(255, 255, 255, 0.03);
          border-radius: 12px;
          padding: 0.72rem 0.8rem;
        }

        .list-item.tight {
          padding: 0.6rem 0.75rem;
          font-family: var(--font-mono);
          font-size: 0.8rem;
        }

        .list-item-title {
          font-size: 0.86rem;
          font-weight: 700;
          margin-bottom: 0.25rem;
        }

        .list-item-subtle {
          color: var(--text-secondary);
          font-size: 0.76rem;
          line-height: 1.45;
        }

        .policy-event.blocked,
        .policy-event.stale {
          border-color: rgba(199, 104, 76, 0.45);
          background: rgba(120, 39, 30, 0.18);
        }

        .policy-event.granted,
        .policy-event.released,
        .policy-event.revived {
          border-color: rgba(143, 174, 124, 0.32);
        }

        .claim-item.stale-claim {
          border-color: rgba(199, 104, 76, 0.45);
          background: rgba(120, 39, 30, 0.14);
        }

        .mono {
          font-family: var(--font-mono);
        }

        .status-dot {
          width: 0.45rem;
          height: 0.45rem;
          border-radius: 999px;
          flex-shrink: 0;
          display: inline-block;
        }

        .capacity-chain {
          display: flex;
          flex-direction: column;
          gap: 0.45rem;
        }

        .capacity-row {
          display: grid;
          grid-template-columns: 1.5rem 1fr auto;
          grid-template-rows: auto auto;
          gap: 0.2rem 0.6rem;
          align-items: center;
          border: 1px solid rgba(255, 255, 255, 0.05);
          background: rgba(255, 255, 255, 0.03);
          border-radius: 10px;
          padding: 0.55rem 0.7rem;
        }

        .capacity-rank {
          font-size: 0.65rem;
          font-weight: 800;
          color: var(--text-secondary);
          font-family: var(--font-mono);
        }

        .capacity-name {
          font-size: 0.84rem;
          font-weight: 700;
          text-transform: capitalize;
        }

        .capacity-chip {
          display: inline-flex;
          align-items: center;
          gap: 0.35rem;
          border: 1px solid;
          border-radius: 999px;
          padding: 0.18rem 0.5rem;
          font-size: 0.66rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          white-space: nowrap;
        }

        .capacity-note {
          grid-column: 2 / -1;
          font-size: 0.72rem;
          color: var(--text-secondary);
          line-height: 1.4;
        }

        .capacity-updated {
          margin-top: 0.55rem;
          font-size: 0.68rem;
          color: var(--text-secondary);
          font-family: var(--font-mono);
        }

        .tesseract-toggle {
          padding: 0.4rem 0.9rem;
          background: rgba(194, 163, 114, 0.08);
          border: 1px solid rgba(194, 163, 114, 0.35);
          border-radius: 999px;
          color: var(--accent-gold);
          font-size: 0.72rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          cursor: pointer;
          transition: background 0.15s, border-color 0.15s;
        }

        .tesseract-toggle:hover {
          background: rgba(194, 163, 114, 0.15);
          border-color: rgba(194, 163, 114, 0.6);
        }

        .header-btn-row {
          display: flex;
          gap: 0.5rem;
        }

        .workbench-btn {
          background: rgba(90, 138, 204, 0.08);
          border-color: rgba(90, 138, 204, 0.35);
          color: #5a8acc;
        }

        .workbench-btn:hover:not(:disabled) {
          background: rgba(90, 138, 204, 0.15);
          border-color: rgba(90, 138, 204, 0.6);
        }

        .workbench-btn:disabled {
          opacity: 0.35;
          cursor: default;
        }

        .system-section {
          display: flex;
          flex-direction: column;
          gap: 0.75rem;
        }

        .system-toggle {
          display: flex;
          justify-content: space-between;
          align-items: center;
          width: 100%;
          background: rgba(255, 255, 255, 0.03);
          border: 1px solid rgba(255, 255, 255, 0.07);
          border-radius: 10px;
          color: rgba(255, 255, 255, 0.35);
          font-size: 0.72rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          padding: 0.55rem 0.8rem;
          cursor: pointer;
          transition: background 0.12s, color 0.12s;
        }

        .system-toggle:hover {
          background: rgba(255, 255, 255, 0.06);
          color: rgba(255, 255, 255, 0.55);
        }

        .system-toggle-chevron {
          font-size: 0.6rem;
          opacity: 0.6;
        }

        .timeline-list {
          display: flex;
          flex-direction: column;
          gap: 0.7rem;
        }

        .timeline-item {
          border-left: 2px solid rgba(194, 163, 114, 0.55);
          padding: 0.3rem 0 0.3rem 0.85rem;
        }

        .timeline-meta {
          display: flex;
          flex-wrap: wrap;
          gap: 0.5rem;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          font-size: 0.68rem;
          color: var(--accent-gold);
          margin-bottom: 0.25rem;
        }

        .timeline-message {
          font-size: 0.9rem;
          line-height: 1.45;
        }

        .timeline-time {
          margin-top: 0.25rem;
          color: var(--text-secondary);
          font-size: 0.75rem;
          font-family: var(--font-mono);
        }

        .empty-state {
          color: var(--text-secondary);
          font-size: 0.82rem;
        }

        .error-banner {
          border: 1px solid rgba(199, 104, 76, 0.5);
          background: rgba(120, 39, 30, 0.35);
          color: #f0c7bc;
          border-radius: 14px;
          padding: 0.8rem 1rem;
          font-size: 0.88rem;
        }

        .footer-bar {
          display: flex;
          flex-wrap: wrap;
          gap: 0.9rem;
          justify-content: space-between;
          color: var(--text-secondary);
          font-size: 0.82rem;
        }

        @media (max-width: 1280px) {
          .dashboard-grid {
            grid-template-columns: 1fr;
          }
        }

        @media (max-width: 980px) {
          .two-up,
          .three-up {
            grid-template-columns: 1fr;
          }

          .run-header {
            flex-direction: column;
          }

          .header-controls {
            align-items: stretch;
            width: 100%;
          }

          .run-selector {
            min-width: 0;
            width: 100%;
          }
        }
      `}</style>
    </div>
  );
}

export default App;
