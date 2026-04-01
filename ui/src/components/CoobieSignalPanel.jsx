import React from 'react';

function severityColor(severity) {
  switch (severity) {
    case 'high':
      return '#d87a5c';
    case 'medium':
      return '#d0ae4d';
    default:
      return '#7db18f';
  }
}

function sourceLabel(source) {
  if (source === 'preflight') return 'Preflight';
  if (source === 'report') return 'Run Report';
  return source || 'Signal';
}

export default function CoobieSignalPanel({ translations = [], compact = false }) {
  if (!translations.length) {
    return <div className="coobie-signal-empty">No Coobie pidgin signals yet.</div>;
  }

  return (
    <div className={`coobie-signal-panel ${compact ? 'compact' : ''}`}>
      {translations.map((translation) => (
        <section key={translation.source} className="coobie-signal-group">
          <div className="coobie-signal-header">
            <span className="coobie-signal-source">{sourceLabel(translation.source)}</span>
            <span className="coobie-signal-count">
              {translation.signals.length} signal{translation.signals.length === 1 ? '' : 's'}
            </span>
          </div>

          <div className="coobie-signal-list">
            {translation.signals.map((signal) => (
              <div
                key={`${translation.source}-${signal.line_index}-${signal.normalized}`}
                className="coobie-signal-item"
              >
                <div className="coobie-signal-topline">
                  <span
                    className="coobie-signal-badge"
                    style={{ borderColor: severityColor(signal.severity), color: severityColor(signal.severity) }}
                  >
                    {signal.kind}
                  </span>
                  <span className="coobie-signal-phrase">{signal.phrase}</span>
                </div>
                <div className="coobie-signal-meta">
                  <span>{signal.meaning}</span>
                  {signal.agent ? <span>agent: {signal.agent}</span> : null}
                </div>
              </div>
            ))}
          </div>

          <details className="coobie-raw-response">
            <summary>raw coobie response</summary>
            <pre style={{ maxHeight: compact ? 180 : 240 }}>{translation.raw}</pre>
          </details>
        </section>
      ))}

      <style jsx>{`
        .coobie-signal-panel {
          display: flex;
          flex-direction: column;
          gap: 0.9rem;
        }
        .coobie-signal-empty {
          color: var(--text-secondary);
          font-size: 0.8rem;
        }
        .coobie-signal-group {
          border: 1px solid rgba(255, 255, 255, 0.06);
          background: rgba(0, 0, 0, 0.18);
          border-radius: 12px;
          padding: 0.75rem 0.85rem;
        }
        .coobie-signal-header {
          display: flex;
          justify-content: space-between;
          gap: 0.8rem;
          align-items: center;
          margin-bottom: 0.65rem;
        }
        .coobie-signal-source {
          font-size: 0.68rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: var(--accent-gold);
        }
        .coobie-signal-count {
          font-size: 0.68rem;
          color: var(--text-secondary);
          font-family: var(--font-mono);
        }
        .coobie-signal-list {
          display: flex;
          flex-direction: column;
          gap: 0.55rem;
        }
        .coobie-signal-item {
          border: 1px solid rgba(255, 255, 255, 0.05);
          border-radius: 10px;
          background: rgba(255, 255, 255, 0.02);
          padding: 0.6rem 0.7rem;
        }
        .coobie-signal-topline {
          display: flex;
          gap: 0.5rem;
          align-items: center;
          flex-wrap: wrap;
          margin-bottom: 0.28rem;
        }
        .coobie-signal-badge {
          border: 1px solid;
          border-radius: 999px;
          padding: 0.14rem 0.5rem;
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.08em;
        }
        .coobie-signal-phrase {
          font-size: 0.88rem;
          font-weight: 700;
          line-height: 1.35;
        }
        .coobie-signal-meta {
          display: flex;
          flex-wrap: wrap;
          gap: 0.6rem;
          font-size: 0.74rem;
          color: var(--text-secondary);
          line-height: 1.4;
        }
        .coobie-raw-response {
          margin-top: 0.65rem;
        }
        .coobie-raw-response summary {
          cursor: pointer;
          font-size: 0.68rem;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          color: var(--accent-gold);
        }
        .coobie-raw-response pre {
          margin-top: 0.45rem;
          padding: 0.75rem;
          border-radius: 10px;
          background: rgba(0, 0, 0, 0.28);
          color: var(--text-secondary);
          font-size: 0.72rem;
          font-family: var(--font-mono);
          white-space: pre-wrap;
          overflow-x: auto;
          overflow-y: auto;
        }
      `}</style>
    </div>
  );
}
