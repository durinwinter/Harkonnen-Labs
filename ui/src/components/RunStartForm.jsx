import React, { useEffect, useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://localhost:3000/api';

export default function RunStartForm({ onRunStarted, onClose }) {
  const [spec, setSpec] = useState('');
  const [product, setProduct] = useState('');
  const [projectPath, setProjectPath] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerPath, setPickerPath] = useState('');
  const [pickerEntries, setPickerEntries] = useState([]);
  const [pickerParentPath, setPickerParentPath] = useState('');
  const [pickerLoading, setPickerLoading] = useState(false);
  const [pickerError, setPickerError] = useState('');

  useEffect(() => {
    const handler = (e) => { if (e.key === 'Escape') onClose?.(); };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [onClose]);

  const loadDirectories = async (nextPath = '') => {
    setPickerLoading(true);
    setPickerError('');
    try {
      const params = new URLSearchParams();
      if (nextPath.trim()) params.set('path', nextPath.trim());
      const suffix = params.toString() ? `?${params.toString()}` : '';
      const resp = await fetch(`${API_BASE}/fs/directories${suffix}`);
      if (!resp.ok) {
        const text = await resp.text();
        throw new Error(text || `${resp.status} ${resp.statusText}`);
      }
      const data = await resp.json();
      setPickerPath(data.current_path || '');
      setPickerEntries(Array.isArray(data.directories) ? data.directories : []);
      setPickerParentPath(data.parent_path || '');
    } catch (err) {
      setPickerError(err.message || String(err));
    } finally {
      setPickerLoading(false);
    }
  };

  const openPicker = async () => {
    setPickerOpen(true);
    await loadDirectories(projectPath.trim() || 'products');
  };

  const chooseProjectPath = (path) => {
    setProjectPath(path);
    setPickerOpen(false);
  };

  const handleSubmit = async (e) => {
    e.preventDefault();
    const trimmedSpec = spec.trim();
    const trimmedProduct = product.trim();
    const trimmedProjectPath = projectPath.trim();
    if (!trimmedSpec || (!trimmedProduct && !trimmedProjectPath)) {
      setError('Spec path and either a product name or project path are required.');
      return;
    }

    setSubmitting(true);
    setError('');
    try {
      const resp = await fetch(`${API_BASE}/runs/start`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          spec: trimmedSpec,
          ...(trimmedProjectPath ? { product_path: trimmedProjectPath } : {}),
          ...(!trimmedProjectPath && trimmedProduct ? { product: trimmedProduct } : {}),
        }),
      });
      if (!resp.ok) {
        const text = await resp.text();
        throw new Error(`${resp.status}: ${text}`);
      }
      const run = await resp.json();
      onRunStarted?.(run);
      onClose?.();
    } catch (err) {
      setError(err.message || String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="form-overlay">
      <div className="form-panel">
        <div className="form-header">
          <div>
            <div className="form-eyebrow">Harkonnen Labs</div>
            <h3>Start a Factory Run</h3>
          </div>
          <button className="form-close" onClick={onClose} type="button">✕</button>
        </div>

        <form onSubmit={handleSubmit} className="form-body">
          <label className="field">
            <span className="field-label">Spec file path</span>
            <input className="field-input" type="text" placeholder="factory/specs/examples/sample_feature.yaml" value={spec} onChange={(e) => setSpec(e.target.value)} disabled={submitting} autoFocus />
            <span className="field-hint">Path relative to the repo root, or absolute.</span>
          </label>

          <label className="field">
            <span className="field-label">Product name</span>
            <input className="field-input" type="text" placeholder="my-product" value={product} onChange={(e) => setProduct(e.target.value)} disabled={submitting} />
            <span className="field-hint">Name of a folder under <code>products/</code>. Leave blank if you are targeting an explicit project path.</span>
          </label>

          <label className="field">
            <span className="field-label">Project path (optional)</span>
            <div className="field-path-row">
              <input className="field-input" type="text" placeholder="Pick a folder from the browser or paste a path" value={projectPath} onChange={(e) => setProjectPath(e.target.value)} disabled={submitting} />
              <button className="btn-browse" type="button" onClick={openPicker} disabled={submitting}>Browse...</button>
            </div>
            <span className="field-hint">Use this for repos outside <code>products/</code>. If provided, it takes precedence over product name.</span>
          </label>

          {error && <div className="form-error">{error}</div>}

          <div className="form-actions">
            <button className="btn-cancel" type="button" onClick={onClose} disabled={submitting}>Cancel</button>
            <button className="btn-start" type="submit" disabled={submitting || !spec || (!product && !projectPath)}>{submitting ? 'Starting...' : 'Start Run'}</button>
          </div>
        </form>
      </div>

      {pickerOpen && (
        <div className="picker-overlay" onClick={(e) => { if (e.target === e.currentTarget) setPickerOpen(false); }}>
          <div className="picker-panel">
            <div className="picker-header">
              <div>
                <div className="form-eyebrow">Project Picker</div>
                <div className="picker-path">{pickerPath || 'Loading...'}</div>
              </div>
              <button className="form-close" type="button" onClick={() => setPickerOpen(false)}>✕</button>
            </div>
            <div className="picker-actions">
              <button className="btn-cancel" type="button" onClick={() => loadDirectories('products')} disabled={pickerLoading}>products/</button>
              <button className="btn-cancel" type="button" onClick={() => pickerParentPath && loadDirectories(pickerParentPath)} disabled={pickerLoading || !pickerParentPath}>Up One Level</button>
            </div>
            {pickerError && <div className="form-error">{pickerError}</div>}
            <div className="picker-list">
              {pickerLoading ? (
                <div className="picker-empty">Loading folders...</div>
              ) : pickerEntries.length === 0 ? (
                <div className="picker-empty">No subdirectories found.</div>
              ) : (
                pickerEntries.map((entry) => (
                  <div key={entry.path} className="picker-item">
                    <button className="picker-open" type="button" onClick={() => loadDirectories(entry.path)}>{entry.name}</button>
                    <button className="btn-start picker-select" type="button" onClick={() => chooseProjectPath(entry.path)}>Select</button>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      )}

      <style jsx>{`
        .form-overlay {
          position: fixed;
          inset: 0;
          background: rgba(0, 0, 0, 0.6);
          backdrop-filter: blur(6px);
          z-index: 1100;
          display: flex;
          align-items: center;
          justify-content: center;
          padding: 1rem;
        }
        .form-panel, .picker-panel {
          background: #1a1d1f;
          border: 1px solid rgba(194, 163, 114, 0.2);
          border-radius: 20px;
          box-shadow: 0 32px 80px rgba(0, 0, 0, 0.55);
          overflow: hidden;
        }
        .form-panel {
          width: min(480px, 100%);
        }
        .picker-panel {
          width: min(760px, 100%);
          max-height: min(80vh, 720px);
          display: flex;
          flex-direction: column;
        }
        .form-header, .picker-header {
          display: flex;
          justify-content: space-between;
          align-items: flex-start;
          padding: 1.2rem 1.3rem 0.9rem;
          border-bottom: 1px solid rgba(255, 255, 255, 0.07);
        }
        .form-eyebrow {
          text-transform: uppercase;
          letter-spacing: 0.16em;
          font-size: 0.68rem;
          font-weight: 800;
          color: var(--accent-gold, #c2a372);
          margin-bottom: 0.3rem;
        }
        .form-close {
          background: none;
          border: 1px solid rgba(255, 255, 255, 0.12);
          color: var(--text-secondary, rgba(255,255,255,0.72));
          border-radius: 50%;
          width: 30px;
          height: 30px;
          cursor: pointer;
          font-size: 0.85rem;
          display: flex;
          align-items: center;
          justify-content: center;
        }
        .form-body {
          padding: 1.2rem 1.3rem 1.3rem;
          display: flex;
          flex-direction: column;
          gap: 1rem;
        }
        .field {
          display: flex;
          flex-direction: column;
          gap: 0.4rem;
        }
        .field-label {
          font-size: 0.72rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: var(--accent-gold, #c2a372);
        }
        .field-path-row {
          display: flex;
          gap: 0.6rem;
        }
        .field-input {
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid rgba(255, 255, 255, 0.1);
          border-radius: 10px;
          color: var(--text-primary, #fff);
          font: inherit;
          font-size: 0.9rem;
          padding: 0.72rem 0.85rem;
          transition: border-color 0.15s;
          outline: none;
          width: 100%;
        }
        .field-hint, .picker-path {
          font-size: 0.72rem;
          color: var(--text-secondary, rgba(255,255,255,0.72));
          line-height: 1.4;
        }
        .btn-browse, .btn-cancel, .btn-start {
          cursor: pointer;
          border-radius: 10px;
          font: inherit;
          padding: 0.72rem 0.85rem;
        }
        .btn-browse, .btn-cancel {
          border: 1px solid rgba(255, 255, 255, 0.12);
          background: rgba(255, 255, 255, 0.05);
          color: #fff;
        }
        .btn-start {
          border: none;
          background: var(--accent-gold, #c2a372);
          color: #111;
          font-weight: 700;
        }
        .form-error {
          background: rgba(120, 39, 30, 0.3);
          border: 1px solid rgba(199, 104, 76, 0.4);
          color: #f0c7bc;
          border-radius: 10px;
          padding: 0.7rem 0.85rem;
          font-size: 0.82rem;
          line-height: 1.45;
        }
        .form-actions, .picker-actions {
          display: flex;
          justify-content: flex-end;
          gap: 0.65rem;
        }
        .picker-overlay {
          position: fixed;
          inset: 0;
          background: rgba(0, 0, 0, 0.72);
          z-index: 1150;
          display: flex;
          align-items: center;
          justify-content: center;
          padding: 1rem;
        }
        .picker-path {
          word-break: break-all;
          font-family: var(--font-mono, monospace);
        }
        .picker-actions {
          padding: 1rem 1.3rem 0;
        }
        .picker-list {
          padding: 1rem 1.3rem 1.3rem;
          overflow: auto;
          display: flex;
          flex-direction: column;
          gap: 0.65rem;
        }
        .picker-item {
          display: flex;
          align-items: center;
          gap: 0.7rem;
          justify-content: space-between;
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid rgba(255, 255, 255, 0.08);
          border-radius: 12px;
          padding: 0.7rem 0.8rem;
        }
        .picker-open {
          border: none;
          background: transparent;
          color: #fff;
          cursor: pointer;
          flex: 1 1 auto;
          text-align: left;
          font: inherit;
        }
        .picker-select {
          min-width: 88px;
        }
        .picker-empty {
          background: rgba(255, 255, 255, 0.04);
          border-radius: 12px;
          padding: 1rem;
          color: var(--text-secondary, rgba(255,255,255,0.72));
          text-align: center;
        }
      `}</style>
    </div>
  );
}
