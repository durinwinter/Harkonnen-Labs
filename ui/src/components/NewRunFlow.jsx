import { useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://localhost:3000/api';
const STEP_LABELS = ['Describe', 'Review Spec', 'Launch'];

export default function NewRunFlow({ onClose, onRunStarted }) {
  const [step, setStep] = useState(0);
  const [intent, setIntent] = useState('');
  const [product, setProduct] = useState('');
  const [projectPath, setProjectPath] = useState('');
  const [specYaml, setSpecYaml] = useState('');
  const [specPath, setSpecPath] = useState('');
  const [specId, setSpecId] = useState('');
  const [drafting, setDrafting] = useState(false);
  const [draftError, setDraftError] = useState('');
  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState('');
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerPath, setPickerPath] = useState('');
  const [pickerEntries, setPickerEntries] = useState([]);
  const [pickerParentPath, setPickerParentPath] = useState('');
  const [pickerLoading, setPickerLoading] = useState(false);
  const [pickerError, setPickerError] = useState('');

  async function loadDirectories(nextPath = '') {
    setPickerLoading(true);
    setPickerError('');
    try {
      const params = new URLSearchParams();
      if (nextPath.trim()) params.set('path', nextPath.trim());
      const suffix = params.toString() ? `?${params.toString()}` : '';
      const res = await fetch(`${API_BASE}/fs/directories${suffix}`);
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `${res.status} ${res.statusText}`);
      }
      const data = await res.json();
      setPickerPath(data.current_path || '');
      setPickerEntries(Array.isArray(data.directories) ? data.directories : []);
      setPickerParentPath(data.parent_path || '');
    } catch (err) {
      setPickerError(err.message || String(err));
    } finally {
      setPickerLoading(false);
    }
  }

  async function openPicker() {
    setPickerOpen(true);
    await loadDirectories(projectPath.trim() || 'products');
  }

  function chooseProjectPath(path) {
    setProjectPath(path);
    setPickerOpen(false);
  }

  async function draftSpec() {
    if (!intent.trim() || !product.trim()) return;
    const trimmedProjectPath = projectPath.trim();
    setDrafting(true);
    setDraftError('');
    try {
      const res = await fetch(`${API_BASE}/scout/draft`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          intent: intent.trim(),
          product: product.trim(),
          ...(trimmedProjectPath ? { product_path: trimmedProjectPath } : {}),
        }),
      });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `${res.status} ${res.statusText}`);
      }
      const data = await res.json();
      setSpecYaml(data.spec_yaml);
      setSpecPath(data.spec_path);
      setSpecId(data.spec_id);
      setStep(1);
    } catch (err) {
      setDraftError(err.message || String(err));
    } finally {
      setDrafting(false);
    }
  }

  async function startRun() {
    const trimmedProduct = product.trim();
    const trimmedProjectPath = projectPath.trim();
    setLaunching(true);
    setLaunchError('');
    try {
      const res = await fetch(`${API_BASE}/runs/start`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          spec: specPath,
          spec_yaml: specYaml,
          ...(trimmedProjectPath ? { product_path: trimmedProjectPath } : {}),
          ...(!trimmedProjectPath && trimmedProduct ? { product: trimmedProduct } : {}),
        }),
      });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `${res.status} ${res.statusText}`);
      }
      const data = await res.json();
      onRunStarted?.(data.run_id);
      onClose?.();
    } catch (err) {
      setLaunchError(err.message || String(err));
    } finally {
      setLaunching(false);
    }
  }

  return (
    <div className="nrf-backdrop" onClick={e => { if (e.target === e.currentTarget) onClose?.(); }}>
      <div className="nrf-modal">
        <div className="nrf-header">
          <div className="nrf-eyebrow">New Run</div>
          <div className="nrf-steps">
            {STEP_LABELS.map((label, i) => (
              <div key={i} className={`nrf-step ${i === step ? 'active' : ''} ${i < step ? 'done' : ''}`}>
                <span className="nrf-step-num">{i < step ? '✓' : i + 1}</span>
                <span className="nrf-step-label">{label}</span>
                {i < STEP_LABELS.length - 1 && <span className="nrf-step-sep">›</span>}
              </div>
            ))}
          </div>
          <button className="nrf-close" type="button" onClick={onClose}>✕</button>
        </div>

        {step === 0 && (
          <div className="nrf-body">
            <p className="nrf-lead">
              Tell Scout what you want to build. Be as specific as possible - it will draft a structured YAML spec for you to review.
            </p>

            <div className="nrf-field">
              <label className="nrf-label">Product name</label>
              <input
                className="nrf-input"
                type="text"
                placeholder="e.g. ceres-station, lamdet, my-service"
                value={product}
                onChange={e => setProduct(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && intent.trim() && draftSpec()}
              />
            </div>

            <div className="nrf-field">
              <label className="nrf-label">Project path (optional)</label>
              <div className="nrf-path-row">
                <input
                  className="nrf-input nrf-mono"
                  type="text"
                  placeholder="Pick a folder from the browser or paste a path"
                  value={projectPath}
                  onChange={e => setProjectPath(e.target.value)}
                />
                <button className="nrf-btn secondary nrf-browse-btn" type="button" onClick={openPicker}>
                  Browse...
                </button>
              </div>
            </div>

            <div className="nrf-field">
              <label className="nrf-label">Describe your intent</label>
              <textarea
                className="nrf-textarea"
                rows={5}
                placeholder={`What should this run accomplish?

Examples:
- Validate the LamDet corpus pipeline end-to-end using the Python example dataset
- Add rate-limiting middleware to the API gateway and test under load
- Verify that the causal annotation endpoint writes correct markdown to factory/memory`}
                value={intent}
                onChange={e => setIntent(e.target.value)}
              />
            </div>

            {draftError && <div className="nrf-error">{draftError}</div>}

            <div className="nrf-actions">
              <button className="nrf-btn secondary" type="button" onClick={onClose}>Cancel</button>
              <button className="nrf-btn primary" type="button" onClick={draftSpec} disabled={!intent.trim() || !product.trim() || drafting}>
                {drafting ? 'Scout is drafting...' : 'Draft Spec ->'}
              </button>
            </div>
          </div>
        )}

        {step === 1 && (
          <div className="nrf-body">
            <div className="nrf-spec-meta">
              <div className="nrf-spec-id">{specId}</div>
              <div className="nrf-spec-path">{specPath}</div>
            </div>
            <p className="nrf-lead">
              Scout drafted this spec. Edit it directly before launching, or go back and rephrase your intent.
            </p>
            <div className="nrf-field nrf-field-grow">
              <label className="nrf-label">Spec YAML</label>
              <textarea className="nrf-textarea nrf-mono" rows={18} value={specYaml} onChange={e => setSpecYaml(e.target.value)} />
            </div>
            <div className="nrf-actions">
              <button className="nrf-btn secondary" type="button" onClick={() => setStep(0)}>&larr; Back</button>
              <button className="nrf-btn primary" type="button" onClick={() => setStep(2)}>Looks good -&gt;</button>
            </div>
          </div>
        )}

        {step === 2 && (
          <div className="nrf-body">
            <div className="nrf-confirm-block">
              <div className="nrf-confirm-icon">🚀</div>
              <div className="nrf-confirm-text">
                <p className="nrf-confirm-title">Ready to launch</p>
                <p className="nrf-confirm-sub">Product: <strong>{product}</strong></p>
                {projectPath.trim() && <p className="nrf-confirm-sub">Project path: <code>{projectPath.trim()}</code></p>}
                <p className="nrf-confirm-sub">Spec: <code>{specId}</code></p>
                <p className="nrf-confirm-desc">
                  The full 9-agent pipeline will run - Scout through Coobie. You can monitor progress on the Factory Floor and annotate causes in the Workbench when it completes.
                </p>
              </div>
            </div>
            {launchError && <div className="nrf-error">{launchError}</div>}
            <div className="nrf-actions">
              <button className="nrf-btn secondary" type="button" onClick={() => setStep(1)}>&larr; Edit Spec</button>
              <button className="nrf-btn launch" type="button" onClick={startRun} disabled={launching}>
                {launching ? 'Launching...' : 'Launch Run'}
              </button>
            </div>
          </div>
        )}
      </div>

      {pickerOpen && (
        <div className="nrf-picker-backdrop" onClick={e => { if (e.target === e.currentTarget) setPickerOpen(false); }}>
          <div className="nrf-picker">
            <div className="nrf-picker-header">
              <div>
                <div className="nrf-eyebrow">Project Picker</div>
                <div className="nrf-picker-path">{pickerPath || 'Loading...'}</div>
              </div>
              <button className="nrf-close" type="button" onClick={() => setPickerOpen(false)}>✕</button>
            </div>
            <div className="nrf-picker-actions">
              <button className="nrf-btn secondary" type="button" onClick={() => loadDirectories('products')} disabled={pickerLoading}>products/</button>
              <button className="nrf-btn secondary" type="button" onClick={() => pickerParentPath && loadDirectories(pickerParentPath)} disabled={pickerLoading || !pickerParentPath}>Up One Level</button>
            </div>
            {pickerError && <div className="nrf-error">{pickerError}</div>}
            <div className="nrf-picker-list">
              {pickerLoading ? (
                <div className="nrf-picker-empty">Loading folders...</div>
              ) : pickerEntries.length === 0 ? (
                <div className="nrf-picker-empty">No subdirectories found.</div>
              ) : (
                pickerEntries.map((entry) => (
                  <div key={entry.path} className="nrf-picker-item">
                    <button className="nrf-picker-open" type="button" onClick={() => loadDirectories(entry.path)}>{entry.name}</button>
                    <button className="nrf-picker-select" type="button" onClick={() => chooseProjectPath(entry.path)}>Select</button>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      )}

      <style jsx>{`
        .nrf-backdrop {
          position: fixed;
          inset: 0;
          z-index: 3000;
          background: rgba(0, 0, 0, 0.72);
          display: flex;
          align-items: center;
          justify-content: center;
          backdrop-filter: blur(4px);
        }
        .nrf-modal {
          width: min(620px, calc(100vw - 2rem));
          max-height: calc(100vh - 2rem);
          overflow: auto;
          background: #13161a;
          border: 1px solid rgba(194, 163, 114, 0.22);
          border-radius: 18px;
          box-shadow: 0 30px 90px rgba(0, 0, 0, 0.55);
        }
        .nrf-header {
          display: flex;
          align-items: center;
          justify-content: space-between;
          gap: 1rem;
          padding: 1rem 1.1rem 0.8rem;
          border-bottom: 1px solid rgba(255, 255, 255, 0.08);
        }
        .nrf-eyebrow {
          text-transform: uppercase;
          letter-spacing: 0.16em;
          font-size: 0.68rem;
          font-weight: 800;
          color: var(--accent-gold, #c2a372);
        }
        .nrf-steps {
          display: flex;
          gap: 0.5rem;
          align-items: center;
          flex-wrap: wrap;
          justify-content: center;
        }
        .nrf-step {
          display: flex;
          gap: 0.35rem;
          align-items: center;
          color: rgba(255, 255, 255, 0.65);
          font-size: 0.82rem;
        }
        .nrf-step.active, .nrf-step.done {
          color: #fff;
        }
        .nrf-step-num {
          width: 1.4rem;
          height: 1.4rem;
          border-radius: 999px;
          display: inline-flex;
          align-items: center;
          justify-content: center;
          background: rgba(255, 255, 255, 0.1);
        }
        .nrf-close {
          background: none;
          border: 1px solid rgba(255, 255, 255, 0.12);
          color: rgba(255, 255, 255, 0.8);
          border-radius: 999px;
          width: 30px;
          height: 30px;
          cursor: pointer;
        }
        .nrf-body {
          padding: 1rem 1.1rem 1.2rem;
          display: flex;
          flex-direction: column;
          gap: 0.95rem;
        }
        .nrf-lead {
          color: rgba(255, 255, 255, 0.78);
          line-height: 1.45;
        }
        .nrf-field {
          display: flex;
          flex-direction: column;
          gap: 0.4rem;
        }
        .nrf-field-grow {
          flex: 1 1 auto;
        }
        .nrf-label {
          font-size: 0.72rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: var(--accent-gold, #c2a372);
        }
        .nrf-path-row {
          display: flex;
          gap: 0.55rem;
        }
        .nrf-input, .nrf-textarea {
          width: 100%;
          border-radius: 12px;
          border: 1px solid rgba(255, 255, 255, 0.12);
          background: rgba(255, 255, 255, 0.04);
          color: #fff;
          padding: 0.75rem 0.85rem;
          font: inherit;
          outline: none;
        }
        .nrf-mono {
          font-family: var(--font-mono, monospace);
        }
        .nrf-browse-btn {
          flex: 0 0 auto;
          white-space: nowrap;
        }
        .nrf-spec-meta, .nrf-confirm-block {
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid rgba(255, 255, 255, 0.08);
          border-radius: 12px;
          padding: 0.85rem;
        }
        .nrf-spec-id, .nrf-spec-path, .nrf-confirm-sub code {
          font-family: var(--font-mono, monospace);
        }
        .nrf-confirm-block {
          display: flex;
          gap: 0.85rem;
        }
        .nrf-confirm-icon {
          font-size: 1.6rem;
        }
        .nrf-confirm-title {
          font-weight: 700;
          margin-bottom: 0.35rem;
        }
        .nrf-confirm-sub, .nrf-confirm-desc {
          margin: 0.2rem 0;
          color: rgba(255, 255, 255, 0.82);
        }
        .nrf-error {
          background: rgba(120, 39, 30, 0.3);
          border: 1px solid rgba(199, 104, 76, 0.4);
          color: #f0c7bc;
          border-radius: 10px;
          padding: 0.7rem 0.85rem;
          font-size: 0.82rem;
          line-height: 1.45;
        }
        .nrf-actions {
          display: flex;
          justify-content: flex-end;
          gap: 0.65rem;
        }
        .nrf-btn {
          border: none;
          cursor: pointer;
          border-radius: 12px;
          padding: 0.72rem 0.95rem;
          font: inherit;
        }
        .nrf-btn.secondary {
          background: rgba(255, 255, 255, 0.08);
          color: #fff;
        }
        .nrf-btn.primary, .nrf-btn.launch, .nrf-picker-select {
          background: var(--accent-gold, #c2a372);
          color: #111;
          font-weight: 700;
        }
        .nrf-picker-backdrop {
          position: fixed;
          inset: 0;
          z-index: 3100;
          background: rgba(0, 0, 0, 0.68);
          display: flex;
          align-items: center;
          justify-content: center;
          padding: 1.2rem;
        }
        .nrf-picker {
          width: min(760px, 100%);
          max-height: min(80vh, 720px);
          display: flex;
          flex-direction: column;
          background: #13161a;
          border: 1px solid rgba(194, 163, 114, 0.22);
          border-radius: 18px;
          box-shadow: 0 30px 90px rgba(0, 0, 0, 0.55);
          overflow: hidden;
        }
        .nrf-picker-header {
          display: flex;
          align-items: flex-start;
          justify-content: space-between;
          gap: 1rem;
          padding: 1rem 1.1rem 0.8rem;
          border-bottom: 1px solid rgba(255, 255, 255, 0.08);
        }
        .nrf-picker-path {
          margin-top: 0.35rem;
          font-family: var(--font-mono, monospace);
          font-size: 0.78rem;
          color: rgba(255, 255, 255, 0.72);
          word-break: break-all;
        }
        .nrf-picker-actions {
          display: flex;
          gap: 0.65rem;
          padding: 0.9rem 1.1rem 0;
        }
        .nrf-picker-list {
          padding: 1rem 1.1rem 1.1rem;
          overflow: auto;
          display: flex;
          flex-direction: column;
          gap: 0.55rem;
        }
        .nrf-picker-item {
          display: flex;
          gap: 0.7rem;
          align-items: center;
          justify-content: space-between;
          background: rgba(255, 255, 255, 0.03);
          border: 1px solid rgba(255, 255, 255, 0.08);
          border-radius: 12px;
          padding: 0.6rem 0.7rem;
        }
        .nrf-picker-open {
          border: none;
          cursor: pointer;
          border-radius: 10px;
          font: inherit;
          flex: 1 1 auto;
          text-align: left;
          background: transparent;
          color: #fff;
          padding: 0.2rem 0.25rem;
        }
        .nrf-picker-empty {
          padding: 1.1rem;
          border-radius: 12px;
          background: rgba(255, 255, 255, 0.03);
          color: rgba(255, 255, 255, 0.72);
          text-align: center;
        }
      `}</style>
    </div>
  );
}
