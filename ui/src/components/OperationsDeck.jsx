import { useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

async function fetchJson(url, options) {
  const response = await fetch(url, options);
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `${response.status} ${response.statusText}`);
  }
  return response.json();
}

export default function OperationsDeck({ activeRunId }) {
  const [busy, setBusy] = useState('');
  const [status, setStatus] = useState('');
  const [specPath, setSpecPath] = useState('factory/specs/examples/sample_feature.yaml');
  const [specResult, setSpecResult] = useState(null);
  const [setupSummary, setSetupSummary] = useState(null);
  const [reportText, setReportText] = useState('');
  const [packagePath, setPackagePath] = useState('');

  async function runOperation(key, fn) {
    setBusy(key);
    setStatus('');
    try {
      await fn();
    } catch (error) {
      setStatus(error.message || String(error));
    } finally {
      setBusy('');
    }
  }

  return (
    <div className="ops-deck">
      <div className="ops-group">
        <div className="ops-label">Factory</div>
        <div className="ops-actions">
          <button
            className="ops-btn"
            disabled={busy === 'setup'}
            onClick={() => runOperation('setup', async () => {
              const data = await fetchJson(`${API_BASE}/setup/check`);
              setSetupSummary(data);
              setStatus(`Setup ${data.setup_name} loaded. Default provider: ${data.default_provider}.`);
            })}
          >
            {busy === 'setup' ? 'Checking...' : 'Setup Check'}
          </button>
          <button
            className="ops-btn"
            disabled={busy === 'memory-init'}
            onClick={() => runOperation('memory-init', async () => {
              const data = await fetchJson(`${API_BASE}/memory/init`, { method: 'POST' });
              setStatus(data.message);
            })}
          >
            {busy === 'memory-init' ? 'Working...' : 'Memory Init'}
          </button>
          <button
            className="ops-btn"
            disabled={busy === 'memory-index'}
            onClick={() => runOperation('memory-index', async () => {
              const data = await fetchJson(`${API_BASE}/memory/index`, { method: 'POST' });
              setStatus(data.message);
            })}
          >
            {busy === 'memory-index' ? 'Working...' : 'Memory Index'}
          </button>
        </div>
      </div>

      <div className="ops-group">
        <div className="ops-label">Spec Validate</div>
        <div className="ops-spec-row">
          <input
            className="ops-input"
            value={specPath}
            onChange={(event) => setSpecPath(event.target.value)}
            placeholder="factory/specs/examples/sample_feature.yaml"
          />
          <button
            className="ops-btn"
            disabled={busy === 'spec-validate' || !specPath.trim()}
            onClick={() => runOperation('spec-validate', async () => {
              const data = await fetchJson(`${API_BASE}/spec/validate`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ path: specPath.trim() }),
              });
              setSpecResult(data);
              setStatus(`Spec ${data.spec_id} is valid.`);
            })}
          >
            {busy === 'spec-validate' ? 'Validating...' : 'Validate'}
          </button>
        </div>
        {specResult && (
          <div className="ops-note">
            {specResult.spec_id} · {specResult.title}
          </div>
        )}
      </div>

      <div className="ops-group">
        <div className="ops-label">Active Run</div>
        <div className="ops-actions">
          <button
            className="ops-btn"
            disabled={!activeRunId || busy === 'run-report'}
            onClick={() => runOperation('run-report', async () => {
              const data = await fetchJson(`${API_BASE}/runs/${activeRunId}/report`);
              setReportText(data.report || '');
              setStatus(`Loaded report for ${activeRunId.slice(0, 8)}.`);
            })}
          >
            {busy === 'run-report' ? 'Loading...' : 'Run Report'}
          </button>
          <button
            className="ops-btn"
            disabled={!activeRunId || busy === 'run-package'}
            onClick={() => runOperation('run-package', async () => {
              const data = await fetchJson(`${API_BASE}/runs/${activeRunId}/package`, { method: 'POST' });
              setPackagePath(data.path || '');
              setStatus(`Packaged artifacts for ${activeRunId.slice(0, 8)}.`);
            })}
          >
            {busy === 'run-package' ? 'Packaging...' : 'Package Artifacts'}
          </button>
        </div>
        {!activeRunId && <div className="ops-note">Select a run to enable report and packaging actions.</div>}
        {packagePath && <div className="ops-note ops-mono">{packagePath}</div>}
        {reportText && <textarea className="ops-report" readOnly value={reportText} rows={10} />}
      </div>

      {setupSummary && (
        <div className="ops-group">
          <div className="ops-label">Setup Snapshot</div>
          <div className="ops-note">
            {setupSummary.setup_name} · {setupSummary.platform} · default {setupSummary.default_provider}
          </div>
          <div className="ops-note">
            Providers: {(setupSummary.providers || []).map((provider) => `${provider.name}:${provider.configured ? 'configured' : 'missing-key'}`).join(', ') || 'none'}
          </div>
          <div className="ops-note">
            MCP: {(setupSummary.mcp_servers || []).map((server) => `${server.name}:${server.available ? 'ok' : 'missing'}`).join(', ') || 'none'}
          </div>
        </div>
      )}

      {status && <div className="ops-status">{status}</div>}

      <style jsx>{`
        .ops-deck {
          display: flex;
          flex-direction: column;
          gap: 0.9rem;
        }
        .ops-group {
          display: flex;
          flex-direction: column;
          gap: 0.55rem;
        }
        .ops-label {
          font-size: 0.72rem;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: var(--accent-gold, #c2a372);
          font-weight: 800;
        }
        .ops-actions, .ops-spec-row {
          display: flex;
          gap: 0.55rem;
          flex-wrap: wrap;
        }
        .ops-btn {
          background: rgba(255,255,255,0.05);
          color: var(--text-primary, #fff);
          border: 1px solid rgba(255,255,255,0.12);
          border-radius: 10px;
          padding: 0.55rem 0.8rem;
          cursor: pointer;
          font: inherit;
        }
        .ops-btn:disabled {
          opacity: 0.55;
          cursor: default;
        }
        .ops-input, .ops-report {
          width: 100%;
          background: rgba(255,255,255,0.04);
          color: var(--text-primary, #fff);
          border: 1px solid rgba(255,255,255,0.1);
          border-radius: 10px;
          padding: 0.65rem 0.75rem;
          font: inherit;
        }
        .ops-input {
          min-width: 0;
          flex: 1 1 260px;
        }
        .ops-report {
          resize: vertical;
          font-family: 'IBM Plex Mono', monospace;
          font-size: 0.78rem;
        }
        .ops-note {
          color: var(--text-secondary, rgba(255,255,255,0.72));
          font-size: 0.84rem;
          line-height: 1.4;
        }
        .ops-mono {
          font-family: 'IBM Plex Mono', monospace;
          word-break: break-all;
        }
        .ops-status {
          color: #f4d7a1;
          background: rgba(194, 163, 114, 0.1);
          border: 1px solid rgba(194, 163, 114, 0.2);
          border-radius: 10px;
          padding: 0.7rem 0.8rem;
          font-size: 0.84rem;
        }
      `}</style>
    </div>
  );
}
