import { useEffect, useMemo, useRef, useState } from 'react';
import ActionCardTile from './ActionCardTile';
import {
  getActionCards,
  OPERATOR_MODEL_CARD_GROUPS,
  USER_SUPERVISOR_CARD_TEMPLATE_PATH,
} from './actionCards';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

function fallbackDisplayName(projectPath) {
  const trimmed = (projectPath || '').trim().replace(/\\/g, '/');
  if (!trimmed) return 'Project Operator Model';
  const segments = trimmed.split('/').filter(Boolean);
  return segments[segments.length - 1] || 'Project Operator Model';
}

function authorLabel(message) {
  if (message.role === 'operator') return 'You';
  if (message.role === 'system') return 'System';
  const agent = message.agent || 'coobie';
  return agent.charAt(0).toUpperCase() + agent.slice(1);
}

function formatTimestamp(value) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}

export default function OperatorModelFlow({ active, projectPath, product }) {
  const normalizedProjectPath = projectPath.trim();
  const [sessionData, setSessionData] = useState(null);
  const [messages, setMessages] = useState([]);
  const [sessionLoading, setSessionLoading] = useState(false);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState('');
  const [draft, setDraft] = useState('');
  const requestedPathRef = useRef('');
  const messageEndRef = useRef(null);

  const displayName = useMemo(() => {
    const trimmedProduct = product.trim();
    return trimmedProduct || fallbackDisplayName(normalizedProjectPath);
  }, [normalizedProjectPath, product]);

  async function fetchMessages(threadId, { silent = false } = {}) {
    if (!threadId) return;
    if (!silent) setMessagesLoading(true);
    try {
      const res = await fetch(`${API_BASE}/chat/threads/${threadId}/messages`);
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `${res.status} ${res.statusText}`);
      }
      const data = await res.json();
      setMessages(Array.isArray(data) ? data : []);
    } catch (err) {
      setError(err.message || String(err));
    } finally {
      if (!silent) setMessagesLoading(false);
    }
  }

  async function startOrResumeSession({ silent = false } = {}) {
    if (!normalizedProjectPath) return;
    if (!silent) setSessionLoading(true);
    setError('');
    try {
      const res = await fetch(`${API_BASE}/operator-model/sessions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          project_root: normalizedProjectPath,
          display_name: displayName,
          started_by: 'operator',
          resume_if_exists: true,
        }),
      });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `${res.status} ${res.statusText}`);
      }
      const data = await res.json();
      requestedPathRef.current = normalizedProjectPath;
      setSessionData(data);
      await fetchMessages(data.thread?.thread_id, { silent: true });
    } catch (err) {
      setError(err.message || String(err));
    } finally {
      if (!silent) setSessionLoading(false);
    }
  }

  async function sendMessage() {
    const threadId = sessionData?.thread?.thread_id;
    if (!threadId || !draft.trim() || sending) return;
    setSending(true);
    setError('');
    try {
      const res = await fetch(`${API_BASE}/chat/threads/${threadId}/messages`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content: draft.trim(), agent: 'coobie' }),
      });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `${res.status} ${res.statusText}`);
      }
      const data = await res.json();
      setDraft('');
      setMessages(prev => {
        const next = [...prev, data.operator_message];
        if (data.agent_reply) next.push(data.agent_reply);
        return next;
      });
    } catch (err) {
      setError(err.message || String(err));
    } finally {
      setSending(false);
    }
  }

  useEffect(() => {
    if (!active) return;
    if (!normalizedProjectPath) {
      requestedPathRef.current = '';
      setSessionData(null);
      setMessages([]);
      return;
    }
    if (requestedPathRef.current === normalizedProjectPath && sessionData) {
      return;
    }
    requestedPathRef.current = normalizedProjectPath;
    setSessionData(null);
    setMessages([]);
    startOrResumeSession({ silent: true });
  }, [active, normalizedProjectPath]);

  useEffect(() => {
    const threadId = sessionData?.thread?.thread_id;
    if (!active || !threadId) return;
    const intervalId = window.setInterval(() => {
      fetchMessages(threadId, { silent: true });
    }, 4000);
    return () => window.clearInterval(intervalId);
  }, [active, sessionData?.thread?.thread_id]);

  useEffect(() => {
    if (!messageEndRef.current) return;
    messageEndRef.current.scrollIntoView({ block: 'end', behavior: 'smooth' });
  }, [messages.length]);

  if (!active) return null;

  const currentLayer =
    sessionData?.session?.pending_layer ||
    sessionData?.thread?.metadata_json?.pending_layer ||
    'operating_rhythms';
  const primaryCards = getActionCards(OPERATOR_MODEL_CARD_GROUPS.primary);
  const supportCards = getActionCards(OPERATOR_MODEL_CARD_GROUPS.support);
  const supervisorFallbackCards = getActionCards(OPERATOR_MODEL_CARD_GROUPS.supervisorFallback);
  const incomingCards = getActionCards(OPERATOR_MODEL_CARD_GROUPS.incoming);

  return (
    <div className="omf-shell">
      <div className="omf-header">
        <div>
          <div className="omf-eyebrow">Operator Interview</div>
          <div className="omf-title">Project-scoped operator model</div>
        </div>
        <div className="omf-actions">
          <button
            className="omf-btn secondary"
            type="button"
            onClick={() => startOrResumeSession()}
            disabled={!normalizedProjectPath || sessionLoading}
          >
            {sessionData ? 'Refresh Session' : 'Start Interview'}
          </button>
        </div>
      </div>

      <p className="omf-copy">
        Coobie interviews against the target repo first, then Harkonnen stamps the confirmed output into
        <code> .harkonnen/operator-model/</code> for this project.
      </p>

      <div className="omf-card-stage">
        <div className="omf-primary-card">
          <ActionCardTile card={primaryCards[0]} variant="hero" />
        </div>
        <div className="omf-card-copy">
          <div className="omf-card-kicker">Who this interview serves</div>
          <div className="omf-card-headline">The operator model becomes a stamped working brief for the repo.</div>
          <div className="omf-card-text">
            Coobie elicits the workflow, Keeper preserves boundaries, Mason inherits the resulting brief, and Sable keeps the run honest under scenario pressure.
          </div>
          <div className="omf-support-grid">
            {supportCards.map(card => (
              <ActionCardTile key={card.id} card={card} variant="support" />
            ))}
          </div>
          <div className="omf-incoming-block">
            <div className="omf-incoming-title">Supervisor fallback and incoming slots</div>
            <div className="omf-incoming-copy">
              Jerry is the default supervisor representation whenever the operator does not want a personal card. The remaining planned lanes are already wired and will light up automatically when their PNGs land.
            </div>
            <div className="omf-user-card-callout">
              <div className="omf-user-card-title">Optional personal supervisor card</div>
              <div className="omf-user-card-copy">
                The initial interview now asks whether the operator wants a personal supervisor card. If not, Jerry stays in the system as the default human-in-the-loop fallback.
              </div>
              <a className="omf-user-card-link" href={USER_SUPERVISOR_CARD_TEMPLATE_PATH} target="_blank" rel="noreferrer">
                Open the user supervisor-card template
              </a>
            </div>
            <div className="omf-fallback-grid">
              {supervisorFallbackCards.map(card => (
                <ActionCardTile key={card.id} card={card} variant="support" />
              ))}
            </div>
            <div className="omf-incoming-grid">
              {incomingCards.map(card => (
                <ActionCardTile key={card.id} card={card} variant="support" />
              ))}
            </div>
          </div>
        </div>
      </div>

      {!normalizedProjectPath ? (
        <div className="omf-empty">
          Choose the target repo path above to start the interview. This flow is intentionally project-first.
        </div>
      ) : (
        <>
          <div className="omf-meta-grid">
            <div className="omf-chip"><strong>Profile</strong><span>{displayName}</span></div>
            <div className="omf-chip"><strong>Layer</strong><span>{currentLayer}</span></div>
            <div className="omf-chip"><strong>Scope</strong><span>{sessionData?.profile?.scope || 'project'}</span></div>
            <div className="omf-chip omf-chip-wide"><strong>Export root</strong><span>{sessionData?.export_root || 'Waiting for session...'}</span></div>
          </div>

          {error && <div className="omf-error">{error}</div>}

          <div className="omf-transcript">
            {sessionLoading && !sessionData ? (
              <div className="omf-empty">Starting the repo interview...</div>
            ) : messagesLoading && messages.length === 0 ? (
              <div className="omf-empty">Loading interview history...</div>
            ) : messages.length === 0 ? (
              <div className="omf-empty">Coobie will open with the first operator-model question as soon as the session is ready.</div>
            ) : (
              messages.map(message => (
                <div key={message.message_id} className={`omf-message ${message.role}`}>
                  <div className="omf-message-meta">
                    <span className="omf-author">{authorLabel(message)}</span>
                    <span className="omf-time">{formatTimestamp(message.created_at)}</span>
                  </div>
                  <div className="omf-message-body">{message.content}</div>
                </div>
              ))
            )}
            <div ref={messageEndRef} />
          </div>

          <div className="omf-compose">
            <textarea
              className="omf-input"
              rows={3}
              placeholder="Answer Coobie here. Focus on recurring triggers, decision rules, dependencies, and where work tends to get stuck."
              value={draft}
              onChange={event => setDraft(event.target.value)}
              onKeyDown={event => {
                if (event.key === 'Enter' && !event.shiftKey) {
                  event.preventDefault();
                  sendMessage();
                }
              }}
              disabled={!sessionData?.thread?.thread_id || sending}
            />
            <div className="omf-compose-actions">
              <div className="omf-footnote">
                Session {sessionData?.reused_existing_session ? 'resumed from this repo' : 'will stay attached to this repo'}.
              </div>
              <button
                className="omf-btn primary"
                type="button"
                onClick={sendMessage}
                disabled={!sessionData?.thread?.thread_id || !draft.trim() || sending}
              >
                {sending ? 'Sending...' : 'Send to Coobie'}
              </button>
            </div>
          </div>
        </>
      )}

      <style jsx>{`
        .omf-shell {
          display: flex;
          flex-direction: column;
          gap: 0.85rem;
          padding: 0.95rem;
          border-radius: 14px;
          border: 1px solid rgba(194, 163, 114, 0.18);
          background: linear-gradient(180deg, rgba(194, 163, 114, 0.08), rgba(255, 255, 255, 0.03));
        }
        .omf-header {
          display: flex;
          align-items: flex-start;
          justify-content: space-between;
          gap: 0.8rem;
        }
        .omf-eyebrow {
          text-transform: uppercase;
          letter-spacing: 0.14em;
          font-size: 0.68rem;
          font-weight: 800;
          color: var(--accent-gold, #c2a372);
        }
        .omf-title {
          margin-top: 0.15rem;
          font-size: 1rem;
          font-weight: 700;
          color: #fff;
        }
        .omf-copy {
          margin: 0;
          color: rgba(255, 255, 255, 0.8);
          line-height: 1.5;
        }
        .omf-card-stage {
          display: grid;
          grid-template-columns: minmax(180px, 220px) 1fr;
          gap: 0.9rem;
          align-items: stretch;
        }
        .omf-primary-card {
          min-height: 0;
        }
        .omf-primary-card :global(.act-card) {
          height: 100%;
        }
        .omf-card-copy {
          display: flex;
          flex-direction: column;
          gap: 0.65rem;
          padding: 0.8rem 0.9rem;
          border-radius: 14px;
          border: 1px solid rgba(255, 255, 255, 0.08);
          background: rgba(255, 255, 255, 0.04);
        }
        .omf-card-kicker {
          text-transform: uppercase;
          letter-spacing: 0.14em;
          font-size: 0.68rem;
          font-weight: 800;
          color: var(--accent-gold, #c2a372);
        }
        .omf-card-headline {
          font-size: 1rem;
          font-weight: 700;
          color: #fff;
          line-height: 1.3;
        }
        .omf-card-text {
          color: rgba(255, 255, 255, 0.76);
          line-height: 1.55;
        }
        .omf-support-grid,
        .omf-fallback-grid,
        .omf-incoming-grid {
          display: grid;
          grid-template-columns: repeat(3, minmax(0, 1fr));
          gap: 0.65rem;
        }
        .omf-incoming-block {
          display: flex;
          flex-direction: column;
          gap: 0.65rem;
          padding-top: 0.25rem;
          border-top: 1px solid rgba(255, 255, 255, 0.08);
        }
        .omf-incoming-title {
          font-size: 0.82rem;
          font-weight: 800;
          letter-spacing: 0.08em;
          text-transform: uppercase;
          color: var(--accent-gold, #c2a372);
        }
        .omf-incoming-copy {
          color: rgba(255, 255, 255, 0.68);
          line-height: 1.5;
          font-size: 0.82rem;
        }
        .omf-user-card-callout {
          display: flex;
          flex-direction: column;
          gap: 0.45rem;
          padding: 0.8rem 0.9rem;
          border-radius: 12px;
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid rgba(255, 255, 255, 0.08);
        }
        .omf-user-card-title {
          font-size: 0.82rem;
          font-weight: 800;
          letter-spacing: 0.08em;
          text-transform: uppercase;
          color: var(--accent-gold, #c2a372);
        }
        .omf-user-card-copy {
          color: rgba(255, 255, 255, 0.72);
          line-height: 1.5;
          font-size: 0.82rem;
        }
        .omf-user-card-link {
          color: #fff;
          font-weight: 700;
          text-decoration: none;
        }
        .omf-user-card-link:hover {
          text-decoration: underline;
        }
        .omf-copy code,
        .omf-chip span,
        .omf-message-body,
        .omf-footnote {
          font-family: var(--font-mono, monospace);
        }
        .omf-meta-grid {
          display: grid;
          grid-template-columns: repeat(3, minmax(0, 1fr));
          gap: 0.6rem;
        }
        .omf-chip {
          display: flex;
          flex-direction: column;
          gap: 0.22rem;
          padding: 0.65rem 0.75rem;
          border-radius: 12px;
          background: rgba(255, 255, 255, 0.05);
          border: 1px solid rgba(255, 255, 255, 0.08);
          min-width: 0;
        }
        .omf-chip strong {
          font-size: 0.68rem;
          letter-spacing: 0.1em;
          text-transform: uppercase;
          color: var(--accent-gold, #c2a372);
        }
        .omf-chip span {
          color: rgba(255, 255, 255, 0.86);
          font-size: 0.78rem;
          word-break: break-word;
        }
        .omf-chip-wide {
          grid-column: 1 / -1;
        }
        .omf-transcript {
          display: flex;
          flex-direction: column;
          gap: 0.7rem;
          max-height: 320px;
          overflow: auto;
          padding: 0.2rem;
        }
        .omf-message {
          padding: 0.75rem 0.85rem;
          border-radius: 14px;
          border: 1px solid rgba(255, 255, 255, 0.08);
          background: rgba(255, 255, 255, 0.04);
        }
        .omf-message.operator {
          background: rgba(194, 163, 114, 0.12);
          border-color: rgba(194, 163, 114, 0.22);
        }
        .omf-message.agent {
          background: rgba(224, 64, 96, 0.09);
          border-color: rgba(224, 64, 96, 0.2);
        }
        .omf-message.system {
          background: rgba(255, 255, 255, 0.03);
        }
        .omf-message-meta {
          display: flex;
          align-items: center;
          justify-content: space-between;
          gap: 0.8rem;
          margin-bottom: 0.38rem;
          font-size: 0.75rem;
          color: rgba(255, 255, 255, 0.62);
        }
        .omf-author {
          font-weight: 700;
          color: #fff;
        }
        .omf-message-body {
          white-space: pre-wrap;
          line-height: 1.5;
          color: rgba(255, 255, 255, 0.88);
        }
        .omf-compose {
          display: flex;
          flex-direction: column;
          gap: 0.6rem;
        }
        .omf-input {
          width: 100%;
          border-radius: 12px;
          border: 1px solid rgba(255, 255, 255, 0.12);
          background: rgba(255, 255, 255, 0.04);
          color: #fff;
          padding: 0.75rem 0.85rem;
          font: inherit;
          resize: vertical;
        }
        .omf-compose-actions {
          display: flex;
          align-items: center;
          justify-content: space-between;
          gap: 0.8rem;
        }
        .omf-footnote {
          color: rgba(255, 255, 255, 0.62);
          font-size: 0.76rem;
        }
        .omf-btn {
          border: none;
          cursor: pointer;
          border-radius: 12px;
          padding: 0.68rem 0.9rem;
          font: inherit;
        }
        .omf-btn.secondary {
          background: rgba(255, 255, 255, 0.08);
          color: #fff;
        }
        .omf-btn.primary {
          background: var(--accent-gold, #c2a372);
          color: #111;
          font-weight: 700;
        }
        .omf-btn:disabled {
          opacity: 0.6;
          cursor: not-allowed;
        }
        .omf-empty,
        .omf-error {
          border-radius: 12px;
          padding: 0.8rem 0.9rem;
          line-height: 1.5;
        }
        .omf-empty {
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid rgba(255, 255, 255, 0.08);
          color: rgba(255, 255, 255, 0.76);
        }
        .omf-error {
          background: rgba(120, 39, 30, 0.3);
          border: 1px solid rgba(199, 104, 76, 0.4);
          color: #f0c7bc;
        }
        @media (max-width: 720px) {
          .omf-meta-grid,
          .omf-support-grid,
          .omf-fallback-grid,
          .omf-incoming-grid,
          .omf-card-stage {
            grid-template-columns: 1fr;
          }
          .omf-compose-actions,
          .omf-header {
            flex-direction: column;
            align-items: stretch;
          }
        }
      `}</style>
    </div>
  );
}
