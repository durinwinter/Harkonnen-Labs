import React, { useEffect, useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

function pct(v) {
  return `${Math.round((v || 0) * 100)}%`;
}

function ScoreBar({ label, value }) {
  const width = `${Math.round((value || 0) * 100)}%`;
  const hue = Math.round((value || 0) * 120); // 0=red 120=green
  const color = `hsl(${hue}, 58%, 52%)`;
  return (
    <div className="score-row">
      <span className="score-label">{label}</span>
      <div className="score-track">
        <div className="score-fill" style={{ width, background: color }} />
      </div>
      <span className="score-num" style={{ color }}>{pct(value)}</span>
    </div>
  );
}

export default function CausalReportPanel({ runId }) {
  const [report, setReport] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!runId) return;
    let cancelled = false;
    setLoading(true);
    setError('');

    fetch(`${API_BASE}/runs/${runId}/causal-report`)
      .then((r) => {
        if (r.status === 404) return null;
        if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
        return r.json();
      })
      .then((data) => {
        if (!cancelled) {
          setReport(data);
          setLoading(false);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err.message);
          setLoading(false);
        }
      });

    return () => { cancelled = true; };
  }, [runId]);

  if (!runId) return null;

  return (
    <div className="causal-panel">
      <div className="causal-header">
        <span className="causal-eyebrow">Coobie</span>
        <h4>Causal Analysis</h4>
      </div>

      {loading && <div className="causal-empty">Loading causal report…</div>}
      {error && <div className="causal-error">Error: {error}</div>}
      {!loading && !error && !report && (
        <div className="causal-empty">No causal report for this run yet.</div>
      )}

      {report && (
        <div className="causal-body">
          <div className="causal-cause-block">
            <div className="block-label">Primary Cause</div>
            {report.primary_cause ? (
              <>
                <div className="cause-text">{report.primary_cause}</div>
                <div className="cause-confidence">confidence {pct(report.primary_confidence)}</div>
              </>
            ) : (
              <div className="cause-text muted">None identified</div>
            )}
          </div>

          {report.contributing_causes?.length > 0 && (
            <div className="causal-contributing">
              <div className="block-label">Contributing Causes</div>
              {report.contributing_causes.map((cause) => (
                <div key={cause} className="contrib-item">{cause}</div>
              ))}
            </div>
          )}

          <div className="causal-scores">
            <div className="block-label">Episode Scores</div>
            <ScoreBar label="Spec Clarity" value={report.episode_scores?.spec_clarity_score} />
            <ScoreBar label="Change Scope" value={report.episode_scores?.change_scope_score} />
            <ScoreBar label="Twin Fidelity" value={report.episode_scores?.twin_fidelity_score} />
            <ScoreBar label="Test Coverage" value={report.episode_scores?.test_coverage_score} />
            <ScoreBar label="Memory Retrieval" value={report.episode_scores?.memory_retrieval_score} />
          </div>

          {report.recommended_interventions?.length > 0 && (
            <div className="causal-interventions">
              <div className="block-label">Recommended Interventions</div>
              {report.recommended_interventions.map((iv, i) => (
                <div key={i} className="intervention-item">
                  <div className="iv-target">{iv.target}</div>
                  <div className="iv-action">{iv.action}</div>
                  <div className="iv-impact muted">{iv.expected_impact}</div>
                </div>
              ))}
            </div>
          )}

          {report.counterfactual_prediction && (
            <div className="causal-counterfactual">
              <div className="block-label">Counterfactual</div>
              <div className="cf-prediction">{report.counterfactual_prediction.prediction}</div>
              <div className="cf-gain muted">
                confidence gain {pct(report.counterfactual_prediction.confidence_gain)}
              </div>
            </div>
          )}

          <div className="causal-footer">
            Generated {new Date(report.generated_at).toLocaleString()}
          </div>
        </div>
      )}

      <style jsx>{`
        .causal-panel {
          background: rgba(22, 24, 26, 0.88);
          border: 1px solid rgba(229, 225, 216, 0.1);
          border-radius: 18px;
          padding: 1rem;
          box-shadow: 0 18px 36px rgba(0, 0, 0, 0.24);
        }
        .causal-header {
          display: flex;
          align-items: baseline;
          gap: 0.75rem;
          margin-bottom: 0.9rem;
        }
        .causal-eyebrow {
          text-transform: uppercase;
          letter-spacing: 0.16em;
          font-size: 0.68rem;
          font-weight: 800;
          color: #7a2a3a;
          background: rgba(122, 42, 58, 0.15);
          border: 1px solid rgba(122, 42, 58, 0.35);
          border-radius: 999px;
          padding: 0.22rem 0.6rem;
        }
        .causal-header h4 {
          font-size: 0.82rem;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: var(--accent-gold);
        }
        .causal-empty, .causal-error {
          color: var(--text-secondary);
          font-size: 0.82rem;
          padding: 0.5rem 0;
        }
        .causal-error { color: #d8876e; }
        .causal-body {
          display: flex;
          flex-direction: column;
          gap: 0.85rem;
        }
        .block-label {
          font-size: 0.64rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: var(--accent-gold);
          margin-bottom: 0.45rem;
        }
        .causal-cause-block, .causal-contributing, .causal-scores,
        .causal-interventions, .causal-counterfactual {
          border: 1px solid rgba(255, 255, 255, 0.05);
          background: rgba(0, 0, 0, 0.18);
          border-radius: 12px;
          padding: 0.75rem 0.85rem;
        }
        .cause-text {
          font-size: 0.9rem;
          font-weight: 600;
          line-height: 1.4;
        }
        .cause-confidence, .muted {
          color: var(--text-secondary);
          font-size: 0.74rem;
          margin-top: 0.3rem;
        }
        .contrib-item {
          font-size: 0.82rem;
          padding: 0.3rem 0;
          border-top: 1px solid rgba(255,255,255,0.05);
          color: var(--text-secondary);
        }
        .contrib-item:first-of-type { border-top: none; }
        .score-row {
          display: flex;
          align-items: center;
          gap: 0.65rem;
          margin-bottom: 0.5rem;
        }
        .score-label {
          width: 120px;
          font-size: 0.72rem;
          color: var(--text-secondary);
          flex-shrink: 0;
        }
        .score-track {
          flex: 1;
          height: 6px;
          background: rgba(255,255,255,0.07);
          border-radius: 999px;
          overflow: hidden;
        }
        .score-fill {
          height: 100%;
          border-radius: 999px;
          transition: width 0.4s ease;
        }
        .score-num {
          width: 36px;
          font-size: 0.72rem;
          font-family: var(--font-mono);
          text-align: right;
          flex-shrink: 0;
        }
        .intervention-item {
          padding: 0.45rem 0;
          border-top: 1px solid rgba(255,255,255,0.05);
        }
        .intervention-item:first-of-type { border-top: none; }
        .iv-target {
          font-size: 0.72rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          color: var(--accent-gold);
        }
        .iv-action {
          font-size: 0.86rem;
          font-weight: 600;
          margin-top: 0.2rem;
        }
        .iv-impact { margin-top: 0.2rem; }
        .cf-prediction {
          font-size: 0.88rem;
          font-weight: 600;
          line-height: 1.4;
        }
        .causal-footer {
          color: var(--text-secondary);
          font-size: 0.7rem;
          font-family: var(--font-mono);
          text-align: right;
        }
      `}</style>
    </div>
  );
}
