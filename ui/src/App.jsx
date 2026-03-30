import React, { useEffect, useState } from 'react';
import AgentCard from './components/AgentCard';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://localhost:3000/api';

const AGENT_DEFS = [
  { id: 'scout', name: 'Scout', role: 'Spec Retriever', group: 'planning', accentColor: '#c4922a' },
  { id: 'keeper', name: 'Keeper', role: 'Boundary Retriever', group: 'planning', accentColor: '#8a7a3a' },
  { id: 'mason', name: 'Mason', role: 'Build Retriever', group: 'action', accentColor: '#c4662a' },
  { id: 'piper', name: 'Piper', role: 'Tool Retriever', group: 'action', accentColor: '#5a7a5a' },
  { id: 'ash', name: 'Ash', role: 'Twin Retriever', group: 'action', accentColor: '#2a7a7a' },
  { id: 'bramble', name: 'Bramble', role: 'Test Retriever', group: 'verification', accentColor: '#a89a2a' },
  { id: 'sable', name: 'Sable', role: 'Scenario Retriever', group: 'verification', accentColor: '#3a4a5a' },
  { id: 'flint', name: 'Flint', role: 'Artifact Retriever', group: 'verification', accentColor: '#8a6a3a' },
  { id: 'coobie', name: 'Coobie', role: 'Memory Retriever', group: 'memory', accentColor: '#7a2a3a' },
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

  const run = runState?.run || null;
  const events = runState?.events || [];
  const blackboard = runState?.blackboard || null;
  const lessons = runState?.lessons || [];
  const agentExecutions = runState?.agent_executions || [];
  const agents = deriveAgents(events, blackboard, agentExecutions);
  const planningAgents = agents.filter((agent) => agent.group === 'planning');
  const actionAgents = agents.filter((agent) => agent.group === 'action');
  const verificationAgents = agents.filter((agent) => agent.group === 'verification');
  const memoryAgent = agents.find((agent) => agent.group === 'memory');
  const recentEvents = [...events].slice(-14).reverse();
  const activeThreads = agents.filter((agent) => agent.status === 'running');
  const claims = Object.entries(blackboard?.agent_claims || {});

  return (
    <div className="pack-board-shell">
      <header className="run-header glass-panel">
        <div>
          <div className="eyebrow">Harkonnen Labs / Pack Board</div>
          <h1>{run ? `${run.product} · ${run.spec_id}` : 'Factory offline'}</h1>
          <div className="header-meta">
            <span>Run: {run?.run_id?.slice(0, 8) || 'none'}</span>
            <span>Phase: {titleCase(blackboard?.current_phase || run?.status || 'idle')}</span>
            <span>Status: {(run?.status || 'idle').toUpperCase()}</span>
          </div>
        </div>

        <div className="header-controls">
          <label className="run-selector-label">
            Recent runs
            <select
              className="run-selector"
              value={activeRunId}
              onChange={(event) => setActiveRunId(event.target.value)}
            >
              {runs.length === 0 ? <option value="">No runs</option> : null}
              {runs.map((candidate) => (
                <option key={candidate.run_id} value={candidate.run_id}>
                  {candidate.run_id.slice(0, 8)} · {candidate.product} · {candidate.status}
                </option>
              ))}
            </select>
          </label>
          <div className={`status-pill status-${run?.status || 'idle'}`}>
            {run?.status || 'idle'}
          </div>
        </div>
      </header>

      {error ? <div className="error-banner">API error: {error}</div> : null}

      <main className="dashboard-grid">
        <section className="main-column">
          <Panel title="01 · Intake & Planning">
            <div className="agent-grid two-up">
              {planningAgents.map((agent) => (
                <AgentCard key={agent.id} agent={agent} />
              ))}
            </div>
          </Panel>

          <Panel title="02 · Implementation & Action">
            <div className="agent-grid three-up">
              {actionAgents.map((agent) => (
                <AgentCard key={agent.id} agent={agent} variant={agent.id === 'mason' ? 'light' : 'dark'} />
              ))}
            </div>
          </Panel>

          <Panel title="03 · Verification & Bundling">
            <div className="agent-grid three-up">
              {verificationAgents.map((agent) => (
                <AgentCard key={agent.id} agent={agent} />
              ))}
            </div>
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
          </Panel>

          <Panel title="Coobie Memory Vault" compact>
            {memoryAgent ? <AgentCard agent={memoryAgent} isSingleton /> : null}
            <div className="info-stack top-gap">
              <div className="info-row"><span>Lesson refs</span><strong>{blackboard?.lesson_refs?.length || 0}</strong></div>
              <div className="info-row"><span>Promoted lessons</span><strong>{lessons.length}</strong></div>
              <div className="info-row"><span>Recent recalls</span><strong>{agentExecutions.length}</strong></div>
            </div>
            <div className="list-block">
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

          <Panel title="Action Board" compact>
            <div className="list-block compact-list">
              {claims.length === 0 ? (
                <div className="empty-state">No active claims.</div>
              ) : (
                claims.map(([agent, claim]) => (
                  <div key={agent} className="list-item">
                    <div className="list-item-title">{agent}</div>
                    <div className="list-item-subtle">{claim}</div>
                  </div>
                ))
              )}
            </div>
          </Panel>
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
