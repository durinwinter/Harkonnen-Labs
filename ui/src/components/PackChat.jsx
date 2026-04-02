import { useState, useRef, useEffect } from 'react';
import LabradorIcon from './LabradorIcon';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

const COOBIE_AVATAR = '/agents/coobie-oracle.svg';

// Agent roster for chat — role strings match AGENTS.md key responsibilities
// pinned: true = trust-critical, always routed to Claude
const AGENTS = {
  scout:  { name: 'Scout',   color: '#c4922a', emblem: '🏅', role: 'Parse specs, flag ambiguity, produce intent package', pinned: true },
  keeper: { name: 'Keeper',  color: '#8a7a3a', emblem: '📜', role: 'Enforce policy, guard boundaries, manage file-claim coordination', pinned: true },
  mason:  { name: 'Mason',   color: '#c4662a', emblem: '⛑',  role: 'Generate and modify code, multi-file changes', pinned: false },
  piper:  { name: 'Piper',   color: '#5a7a5a', emblem: '🔧', role: 'Run build tools, fetch docs, execute helpers', pinned: false },
  ash:    { name: 'Ash',     color: '#2a7a7a', emblem: '🎒', role: 'Provision digital twins, mock dependencies', pinned: false },
  bramble:{ name: 'Bramble', color: '#a89a2a', emblem: '📋', role: 'Generate tests, run lint/build/visible tests', pinned: false },
  sable:  { name: 'Sable',   color: '#3a4a5a', emblem: '🥽', role: 'Execute hidden scenarios, produce eval reports', pinned: true },
  flint:  { name: 'Flint',   color: '#8a6a3a', emblem: '📦', role: 'Collect outputs, package artifact bundles', pinned: false },
  coobie: { name: 'Coobie',  color: '#e04060', emblem: '💡', role: 'Coordinate pack memory: working context, episodic capture, causal graph, consolidation', pinned: false, avatar: COOBIE_AVATAR },
};

const COMMISSION_TRIGGERS = ['build', 'create', 'add', 'implement', 'make', 'write', 'draft', 'generate', 'commission', 'develop', 'design'];

function detectAddress(text) {
  const m = text.match(/^@(\w+)\s+([\s\S]+)/);
  if (m) {
    const id = m[1].toLowerCase();
    if (AGENTS[id]) return { agentId: id, body: m[2].trim() };
  }
  return null;
}

function detectCommission(text) {
  const lower = text.toLowerCase();
  return COMMISSION_TRIGGERS.some(t => lower.startsWith(t) || new RegExp(`\\b${t}\\b`).test(lower));
}

function slugify(text) {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, '')
    .trim()
    .replace(/\s+/g, '-')
    .slice(0, 40);
}

function deriveProduct(text) {
  const m = text.match(
    /(?:build|create|add|implement|make|write|develop|design)\s+(?:a\s+|an\s+|the\s+)?(.+?)(?:\s+for\b|\s+to\b|\s+that\b|\s+which\b|\s+with\b|$)/i,
  );
  return m ? slugify(m[1]) : 'new-feature';
}

let _msgId = 0;
function mkId() { return `msg-${++_msgId}-${Date.now()}`; }

// ─── Sub-components ──────────────────────────────────────────────────────────

function AgentAvatar({ id, size = 32 }) {
  const a = AGENTS[id];
  const [imageFailed, setImageFailed] = useState(false);
  if (!a) return null;
  const avatarSrc = a.avatar && !imageFailed ? a.avatar : null;
  return (
    <div className="msg-avatar">
      {avatarSrc ? (
        <img
          src={avatarSrc}
          alt={a.name}
          className="msg-avatar-image"
          width={size}
          height={size}
          onError={() => setImageFailed(true)}
        />
      ) : (
        <LabradorIcon color={a.color} size={size} status="idle" />
      )}
      <span className="msg-emblem" title={a.name}>{a.emblem}</span>
    </div>
  );
}

function SystemBubble({ text }) {
  return (
    <div className="chat-msg system-msg">
      <div className="system-text">{text}</div>
    </div>
  );
}

function AgentBubble({ from, text }) {
  const a = AGENTS[from] || AGENTS.coobie;
  return (
    <div className="chat-msg agent-msg">
      <AgentAvatar id={from} />
      <div className="msg-bubble">
        <div className="msg-name">
          {a.name}
          {a.pinned && <span className="msg-pinned-badge" title="Pinned to Claude">Claude</span>}
          <span className="msg-role">{a.role}</span>
        </div>
        <div className="msg-text">{text}</div>
      </div>
    </div>
  );
}

function UserBubble({ text }) {
  return (
    <div className="chat-msg user-msg">
      <div className="user-bubble">{text}</div>
    </div>
  );
}

function SpecCard({ msg, onRunStarted }) {
  const [yaml, setYaml] = useState(msg.specYaml || '');
  const [launching, setLaunching] = useState(false);
  const [launched, setLaunched] = useState(false);
  const [error, setError] = useState('');

  async function launch() {
    setLaunching(true);
    setError('');
    try {
      const res = await fetch(`${API_BASE}/runs/start`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          spec: msg.specPath,
          spec_yaml: yaml,
          product: msg.product,
          run_hidden_scenarios: true,
        }),
      });
      if (!res.ok) throw new Error((await res.text()) || `${res.status}`);
      const data = await res.json();
      setLaunched(true);
      onRunStarted?.(data.run_id);
    } catch (err) {
      setError(err.message);
    } finally {
      setLaunching(false);
    }
  }

  const a = AGENTS.scout;

  return (
    <div className="chat-msg agent-msg">
      <AgentAvatar id="scout" />
      <div className="msg-bubble spec-bubble">
        <div className="msg-name">
          Scout <span className="msg-role">Spec Retriever</span>
        </div>
        <div className="msg-text">{msg.text}</div>
        <div className="spec-card">
          <div className="spec-card-meta">
            <span className="spec-id-chip">{msg.specId}</span>
            <span className="spec-path-chip">{msg.specPath}</span>
          </div>
          {launched ? (
            <div className="spec-launched">
              <span className="spec-launched-check">✓</span>
              Run commissioned — the pack is assembling.
            </div>
          ) : (
            <>
              <textarea
                className="spec-yaml-editor"
                rows={14}
                value={yaml}
                onChange={e => setYaml(e.target.value)}
                spellCheck={false}
              />
              {error && <div className="spec-error">{error}</div>}
              <div className="spec-actions">
                <button
                  className="spec-launch-btn"
                  onClick={launch}
                  disabled={launching}
                >
                  {launching ? 'Assembling the pack…' : '🚀 Commission the Pack'}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function TypingIndicator({ agentId }) {
  return (
    <div className="chat-msg agent-msg">
      <AgentAvatar id={agentId || 'scout'} />
      <div className="typing-dots">
        <span /><span /><span />
      </div>
    </div>
  );
}

// ─── BlockedPrompt: appears when an agent needs user input ───────────────────

function BlockedPrompt({ msg, onReply }) {
  const [reply, setReply] = useState('');
  const [sent, setSent] = useState(false);

  function send() {
    if (!reply.trim() || sent) return;
    setSent(true);
    onReply?.(msg.agentId, reply.trim());
  }

  return (
    <div className="chat-msg agent-msg blocked-prompt">
      <AgentAvatar id={msg.agentId} />
      <div className="msg-bubble blocked-bubble">
        <div className="msg-name blocked-name">
          {AGENTS[msg.agentId]?.name}
          <span className="blocked-chip">Needs your input</span>
        </div>
        <div className="msg-text">{msg.text}</div>
        {!sent ? (
          <div className="blocked-reply-row">
            <input
              className="blocked-input"
              placeholder="Your answer…"
              value={reply}
              onChange={e => setReply(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && send()}
              autoFocus
            />
            <button className="blocked-send-btn" onClick={send} disabled={!reply.trim()}>
              Send
            </button>
          </div>
        ) : (
          <div className="blocked-sent">✓ Sent — the pack is continuing.</div>
        )}
      </div>
    </div>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

export default function PackChat({ activeRunId, agents, onRunStarted }) {
  const [messages, setMessages] = useState([
    {
      id: mkId(),
      type: 'agent',
      from: 'coobie',
      text: "The pack is ready. Describe what you want to build and I'll get Scout started on a spec. You can also address a specific pup directly — try @scout, @keeper, or any name.",
    },
  ]);
  const [input, setInput] = useState('');
  const [product, setProduct] = useState('');
  const [typing, setTyping] = useState(null); // agentId while thinking
  const threadRef = useRef(null);
  const inputRef = useRef(null);

  // Scroll to bottom on new message
  useEffect(() => {
    if (threadRef.current) {
      threadRef.current.scrollTop = threadRef.current.scrollHeight;
    }
  }, [messages, typing]);

  function push(msg) {
    setMessages(prev => [...prev, { id: mkId(), ...msg }]);
  }

  async function handleSend() {
    const text = input.trim();
    if (!text) return;
    setInput('');

    push({ type: 'user', text });

    const addressed = detectAddress(text);
    if (addressed) {
      await routeToAgent(addressed.agentId, addressed.body);
      return;
    }

    if (detectCommission(text)) {
      await handleCommission(text);
      return;
    }

    await handleGeneralChat(text);
  }

  async function routeToAgent(agentId, body) {
    const agent = AGENTS[agentId];
    setTyping(agentId);
    try {
      const res = await fetch(`${API_BASE}/agents/${agentId}/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: body, run_id: activeRunId || null }),
      });
      if (!res.ok) throw new Error((await res.text()) || `${res.status}`);
      const data = await res.json();
      push({ type: 'agent', from: agentId, text: data.response || data.message });
    } catch {
      push({
        type: 'agent',
        from: agentId,
        text: `${agent.name} here — direct agent chat isn't wired up yet, but I'm watching. Describe what you need and I'll pick it up in the next run.`,
      });
    } finally {
      setTyping(null);
    }
  }

  async function handleCommission(text) {
    const productName = product.trim() || deriveProduct(text);
    setTyping('scout');
    push({
      type: 'agent',
      from: 'scout',
      text: `On it. Drafting a spec for "${productName}" — give me a moment.`,
    });

    try {
      const res = await fetch(`${API_BASE}/scout/draft`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          intent: text,
          product: productName,
          run_hidden_scenarios: true,
        }),
      });
      if (!res.ok) throw new Error((await res.text()) || `${res.status}`);
      const data = await res.json();
      push({
        type: 'spec-card',
        from: 'scout',
        text: "Here's the spec I drafted. Edit it if anything needs adjusting, then commission the pack.",
        specId: data.spec_id,
        specPath: data.spec_path,
        specYaml: data.spec_yaml,
        product: productName,
      });
    } catch (err) {
      push({
        type: 'agent',
        from: 'scout',
        text: `Couldn't draft spec: ${err.message}`,
      });
    } finally {
      setTyping(null);
    }
  }

  async function handleGeneralChat(text) {
    setTyping('coobie');
    try {
      const res = await fetch(`${API_BASE}/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: text, run_id: activeRunId || null }),
      });
      if (!res.ok) throw new Error((await res.text()) || `${res.status}`);
      const data = await res.json();
      push({ type: 'agent', from: data.agent || 'coobie', text: data.response });
    } catch {
      push({
        type: 'agent',
        from: 'coobie',
        text: "I heard you. The general chat endpoint isn't live yet — but describe what you want to build and I'll get Scout on it.",
      });
    } finally {
      setTyping(null);
    }
  }

  async function handleBlockedReply(agentId, reply) {
    push({ type: 'user', text: reply });
    setTyping(agentId);
    try {
      const res = await fetch(`${API_BASE}/agents/${agentId}/unblock`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ answer_text: reply, run_id: activeRunId, answered_by: 'operator' }),
      });
      if (!res.ok) throw new Error((await res.text()) || `${res.status}`);
      const data = await res.json();
      push({ type: 'agent', from: agentId, text: data.response || 'Thanks — continuing.' });
    } catch {
      push({ type: 'agent', from: agentId, text: 'Got it — continuing.' });
    } finally {
      setTyping(null);
    }
  }

  const hasProduct = product.trim().length > 0;

  return (
    <div className="pack-chat">
      {/* Thread */}
      <div className="chat-thread" ref={threadRef}>
        {messages.map(msg => {
          if (msg.type === 'user') return <UserBubble key={msg.id} text={msg.text} />;
          if (msg.type === 'system') return <SystemBubble key={msg.id} text={msg.text} />;
          if (msg.type === 'spec-card') return <SpecCard key={msg.id} msg={msg} onRunStarted={onRunStarted} />;
          if (msg.type === 'blocked') return <BlockedPrompt key={msg.id} msg={msg} onReply={handleBlockedReply} />;
          return <AgentBubble key={msg.id} from={msg.from} text={msg.text} />;
        })}
        {typing && <TypingIndicator agentId={typing} />}
      </div>

      {/* Footer */}
      <div className="chat-footer">
        <div className="chat-product-row">
          <label className="product-label">Product</label>
          <input
            className="product-input"
            placeholder="name-your-product  (optional — Scout will derive one if omitted)"
            value={product}
            onChange={e => setProduct(e.target.value)}
          />
          {hasProduct && (
            <span className="product-chip">{product.trim()}</span>
          )}
        </div>

        <div className="chat-input-row">
          <textarea
            ref={inputRef}
            className="chat-input"
            rows={2}
            placeholder="Commission a run, ask the pack anything, or @scout / @coobie / @keeper to address a pup directly…"
            value={input}
            disabled={!!typing}
            onChange={e => setInput(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
          />
          <button
            className="chat-send-btn"
            onClick={handleSend}
            disabled={!input.trim() || !!typing}
            title="Send (Enter)"
          >
            {typing ? '…' : '↑'}
          </button>
        </div>

        <div className="chat-hint">
          Enter to send · Shift+Enter for newline · @agentname to address a pup directly
        </div>
      </div>

      <style jsx>{`
        .pack-chat {
          display: flex;
          flex-direction: column;
          height: 520px;
          background: rgba(16, 18, 20, 0.92);
          border: 1px solid rgba(194, 163, 114, 0.14);
          border-radius: 18px;
          overflow: hidden;
        }

        /* ── Thread ── */
        .chat-thread {
          flex: 1;
          overflow-y: auto;
          display: flex;
          flex-direction: column;
          gap: 1rem;
          padding: 1.1rem 1.1rem 0.4rem;
          scroll-behavior: smooth;
        }

        .chat-thread::-webkit-scrollbar {
          width: 4px;
        }
        .chat-thread::-webkit-scrollbar-track {
          background: transparent;
        }
        .chat-thread::-webkit-scrollbar-thumb {
          background: rgba(255, 255, 255, 0.1);
          border-radius: 4px;
        }

        /* ── Message rows ── */
        .chat-msg {
          display: flex;
          gap: 0.65rem;
          align-items: flex-start;
          max-width: 92%;
        }

        .agent-msg { align-self: flex-start; }
        .user-msg  { align-self: flex-end; flex-direction: row-reverse; }
        .system-msg {
          align-self: center;
          max-width: 100%;
        }

        /* ── User bubble ── */
        .user-bubble {
          background: linear-gradient(135deg, rgba(194, 163, 114, 0.22), rgba(194, 163, 114, 0.14));
          border: 1px solid rgba(194, 163, 114, 0.28);
          border-radius: 16px 4px 16px 16px;
          padding: 0.7rem 1rem;
          font-size: 0.9rem;
          line-height: 1.5;
          color: #fff;
          max-width: 480px;
        }

        /* ── System text ── */
        .system-text {
          font-size: 0.72rem;
          color: rgba(255, 255, 255, 0.32);
          text-align: center;
          padding: 0.15rem 0.5rem;
          font-style: italic;
        }

        /* ── Avatar ── */
        .msg-avatar {
          flex-shrink: 0;
          position: relative;
          width: 36px;
          height: 36px;
          display: flex;
          align-items: center;
          justify-content: center;
        }

        .msg-avatar-image {
          width: 36px;
          height: 36px;
          object-fit: cover;
          border-radius: 50%;
          border: 1px solid rgba(224, 64, 96, 0.36);
          background: radial-gradient(circle at 30% 20%, rgba(22, 54, 102, 0.95), rgba(10, 18, 30, 0.98));
          box-shadow: 0 0 14px rgba(98, 178, 255, 0.18);
        }

        .msg-emblem {
          position: absolute;
          bottom: -2px;
          right: -4px;
          font-size: 0.7rem;
          line-height: 1;
        }

        /* ── Agent bubble ── */
        .msg-bubble {
          background: rgba(28, 32, 36, 0.9);
          border: 1px solid rgba(255, 255, 255, 0.07);
          border-radius: 4px 16px 16px 16px;
          padding: 0.75rem 0.95rem;
          display: flex;
          flex-direction: column;
          gap: 0.45rem;
          max-width: 520px;
        }

        .msg-name {
          font-size: 0.72rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: rgba(255, 255, 255, 0.9);
          display: flex;
          align-items: center;
          gap: 0.5rem;
        }

        .msg-pinned-badge {
          font-size: 0.54rem;
          font-weight: 900;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: #c4922a;
          background: rgba(196, 146, 42, 0.12);
          border: 1px solid rgba(196, 146, 42, 0.28);
          border-radius: 999px;
          padding: 0.08rem 0.38rem;
          white-space: nowrap;
        }

        .msg-role {
          font-size: 0.62rem;
          font-weight: 600;
          text-transform: none;
          letter-spacing: 0.04em;
          color: rgba(255, 255, 255, 0.35);
        }

        .msg-text {
          font-size: 0.88rem;
          line-height: 1.55;
          color: rgba(255, 255, 255, 0.85);
        }

        /* ── Spec card ── */
        .spec-bubble {
          max-width: 640px !important;
          width: 100%;
        }

        .spec-card {
          display: flex;
          flex-direction: column;
          gap: 0.65rem;
          margin-top: 0.3rem;
        }

        .spec-card-meta {
          display: flex;
          flex-wrap: wrap;
          gap: 0.45rem;
        }

        .spec-id-chip {
          font-family: var(--font-mono, monospace);
          font-size: 0.72rem;
          color: #c4922a;
          background: rgba(196, 146, 42, 0.12);
          border: 1px solid rgba(196, 146, 42, 0.25);
          border-radius: 6px;
          padding: 0.18rem 0.5rem;
        }

        .spec-path-chip {
          font-family: var(--font-mono, monospace);
          font-size: 0.66rem;
          color: rgba(255, 255, 255, 0.35);
          word-break: break-all;
        }

        .spec-yaml-editor {
          width: 100%;
          background: rgba(0, 0, 0, 0.35);
          border: 1px solid rgba(255, 255, 255, 0.08);
          border-radius: 10px;
          color: #d4e8c2;
          font-family: var(--font-mono, 'IBM Plex Mono', monospace);
          font-size: 0.76rem;
          line-height: 1.5;
          padding: 0.75rem 0.85rem;
          resize: vertical;
          outline: none;
          box-sizing: border-box;
        }

        .spec-yaml-editor:focus {
          border-color: rgba(194, 163, 114, 0.3);
        }

        .spec-error {
          background: rgba(120, 39, 30, 0.25);
          border: 1px solid rgba(199, 104, 76, 0.35);
          color: #f0c7bc;
          border-radius: 8px;
          padding: 0.55rem 0.75rem;
          font-size: 0.8rem;
        }

        .spec-actions {
          display: flex;
          justify-content: flex-end;
        }

        .spec-launch-btn {
          background: linear-gradient(135deg, #c4922a, #a87820);
          color: #111;
          font-weight: 800;
          font-size: 0.84rem;
          border: none;
          border-radius: 12px;
          padding: 0.7rem 1.2rem;
          cursor: pointer;
          letter-spacing: 0.03em;
          transition: opacity 0.12s;
        }

        .spec-launch-btn:disabled {
          opacity: 0.55;
          cursor: default;
        }

        .spec-launched {
          display: flex;
          align-items: center;
          gap: 0.5rem;
          color: #8fae7c;
          font-size: 0.86rem;
          font-weight: 600;
          padding: 0.5rem 0;
        }

        .spec-launched-check {
          font-size: 1rem;
        }

        /* ── Blocked prompt ── */
        .blocked-bubble {
          border-color: rgba(199, 104, 76, 0.3) !important;
          background: rgba(120, 39, 30, 0.1) !important;
        }

        .blocked-name {
          color: #c7684c !important;
          display: flex;
          align-items: center;
          gap: 0.5rem;
        }

        .blocked-chip {
          font-size: 0.58rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          background: rgba(199, 104, 76, 0.15);
          border: 1px solid rgba(199, 104, 76, 0.35);
          color: #c7684c;
          border-radius: 999px;
          padding: 0.12rem 0.5rem;
        }

        .blocked-reply-row {
          display: flex;
          gap: 0.5rem;
          margin-top: 0.2rem;
        }

        .blocked-input {
          flex: 1;
          background: rgba(0, 0, 0, 0.25);
          border: 1px solid rgba(255, 255, 255, 0.1);
          border-radius: 8px;
          color: #fff;
          font: inherit;
          font-size: 0.84rem;
          padding: 0.5rem 0.7rem;
          outline: none;
        }

        .blocked-send-btn {
          background: rgba(199, 104, 76, 0.2);
          border: 1px solid rgba(199, 104, 76, 0.4);
          color: #c7684c;
          border-radius: 8px;
          font: inherit;
          font-weight: 700;
          font-size: 0.82rem;
          padding: 0.5rem 0.9rem;
          cursor: pointer;
        }

        .blocked-sent {
          color: #8fae7c;
          font-size: 0.82rem;
          padding: 0.3rem 0;
        }

        /* ── Typing dots ── */
        .typing-dots {
          display: flex;
          gap: 0.3rem;
          align-items: center;
          padding: 0.6rem 0.8rem;
          background: rgba(28, 32, 36, 0.9);
          border: 1px solid rgba(255, 255, 255, 0.07);
          border-radius: 4px 16px 16px 16px;
          width: fit-content;
        }

        .typing-dots span {
          width: 6px;
          height: 6px;
          border-radius: 50%;
          background: rgba(255, 255, 255, 0.35);
          animation: typing-bounce 1.2s ease-in-out infinite;
        }

        .typing-dots span:nth-child(2) { animation-delay: 0.2s; }
        .typing-dots span:nth-child(3) { animation-delay: 0.4s; }

        @keyframes typing-bounce {
          0%, 80%, 100% { transform: translateY(0); opacity: 0.35; }
          40% { transform: translateY(-5px); opacity: 0.9; }
        }

        /* ── Footer ── */
        .chat-footer {
          flex-shrink: 0;
          display: flex;
          flex-direction: column;
          gap: 0.55rem;
          padding: 0.75rem 1rem 0.8rem;
          border-top: 1px solid rgba(255, 255, 255, 0.06);
          background: rgba(14, 16, 18, 0.8);
        }

        .chat-product-row {
          display: flex;
          align-items: center;
          gap: 0.55rem;
        }

        .product-label {
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: rgba(194, 163, 114, 0.7);
          white-space: nowrap;
          flex-shrink: 0;
        }

        .product-input {
          flex: 1;
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid rgba(255, 255, 255, 0.08);
          border-radius: 8px;
          color: rgba(255, 255, 255, 0.75);
          font: inherit;
          font-size: 0.78rem;
          padding: 0.4rem 0.65rem;
          outline: none;
          min-width: 0;
        }

        .product-input:focus {
          border-color: rgba(194, 163, 114, 0.25);
        }

        .product-chip {
          flex-shrink: 0;
          font-family: var(--font-mono, monospace);
          font-size: 0.7rem;
          font-weight: 700;
          color: #c4922a;
          background: rgba(196, 146, 42, 0.1);
          border: 1px solid rgba(196, 146, 42, 0.22);
          border-radius: 6px;
          padding: 0.18rem 0.5rem;
          white-space: nowrap;
        }

        .chat-input-row {
          display: flex;
          gap: 0.55rem;
          align-items: flex-end;
        }

        .chat-input {
          flex: 1;
          background: rgba(255, 255, 255, 0.05);
          border: 1px solid rgba(255, 255, 255, 0.1);
          border-radius: 14px;
          color: #fff;
          font: inherit;
          font-size: 0.9rem;
          line-height: 1.5;
          padding: 0.7rem 0.9rem;
          resize: none;
          outline: none;
          transition: border-color 0.12s;
        }

        .chat-input:focus {
          border-color: rgba(194, 163, 114, 0.32);
        }

        .chat-input:disabled {
          opacity: 0.5;
        }

        .chat-send-btn {
          flex-shrink: 0;
          width: 42px;
          height: 42px;
          border-radius: 12px;
          border: none;
          background: linear-gradient(135deg, #c4922a, #a87820);
          color: #111;
          font-size: 1.1rem;
          font-weight: 900;
          cursor: pointer;
          display: flex;
          align-items: center;
          justify-content: center;
          transition: opacity 0.12s;
          line-height: 1;
        }

        .chat-send-btn:disabled {
          opacity: 0.35;
          cursor: default;
        }

        .chat-hint {
          font-size: 0.64rem;
          color: rgba(255, 255, 255, 0.2);
          text-align: center;
          letter-spacing: 0.04em;
        }
      `}</style>
    </div>
  );
}
