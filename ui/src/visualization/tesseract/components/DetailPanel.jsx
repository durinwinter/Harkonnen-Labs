import { COOBIE_FLAVOR, STATUS_COLOR, CAUSE_COLOR } from '../scene/color-rules';

function ScoreBar({ label, value, invert = false }) {
  const pct = Math.round((value ?? 0) * 100);
  const displayValue = invert ? 1 - (value ?? 0) : (value ?? 0);
  const barColor = displayValue > 0.62 ? '#8fae7c' : displayValue > 0.36 ? '#c4922a' : '#c7684c';
  return (
    <div className="score-row">
      <span className="score-label">{label}</span>
      <div className="score-track">
        <div className="score-fill" style={{ width: `${Math.round(displayValue * 100)}%`, background: barColor }} />
      </div>
      <span className="score-pct">{pct}%</span>
    </div>
  );
}

function EpisodeDetail({ episode, scene, onClose }) {
  const relatedEdges = scene?.edges?.filter((e) => e.sourceId === episode.id) ?? [];
  const relatedCauses = relatedEdges
    .map((e) => scene?.causeNodes?.find((c) => c.id === e.targetId))
    .filter(Boolean);

  const statusColor = STATUS_COLOR[episode.status] ?? STATUS_COLOR.default;
  const flavor = episode.status === 'accepted'
    ? COOBIE_FLAVOR.confident
    : episode.confidence > 0.65
      ? COOBIE_FLAVOR.trail_found
      : COOBIE_FLAVOR.trail_lost;

  const s = episode.rawScores;

  return (
    <div className="dp-content">
      <button className="dp-close" onClick={onClose}>✕</button>
      <div className="dp-eyebrow">Episode</div>
      <div className="dp-title">{episode.label}</div>
      <div className="dp-status-badge" style={{ color: statusColor, borderColor: `${statusColor}55` }}>
        {episode.status}
        {s && (
          <>
            {s.scenario_passed  ? ' · scenario ✓' : ' · scenario ✗'}
            {s.validation_passed ? ' · valid ✓'   : ' · valid ✗'}
          </>
        )}
      </div>

      <div className="dp-section-label">Episode scores</div>
      <div className="score-stack">
        {s ? (
          <>
            <ScoreBar label="Spec clarity"    value={s.spec_clarity_score} />
            <ScoreBar label="Twin fidelity"   value={s.twin_fidelity_score} />
            <ScoreBar label="Test coverage"   value={s.test_coverage_score} />
            <ScoreBar label="Memory retrieval" value={s.memory_retrieval_score} />
            <ScoreBar label="Scope (risk↑)"   value={s.change_scope_score} invert />
          </>
        ) : (
          <>
            <ScoreBar label="Spec"           value={episode.specQuality} />
            <ScoreBar label="Implementation" value={episode.implementationQuality} />
            <ScoreBar label="Validation"     value={episode.validationOutcome} />
            <ScoreBar label="Intervention ↑" value={episode.interventionPotential} />
          </>
        )}
      </div>

      {episode.primaryCauseText && (
        <>
          <div className="dp-section-label">Primary cause</div>
          <div className="dp-cause-text">{episode.primaryCauseText}</div>
          <div className="dp-conf-bar">
            <div className="dp-conf-fill" style={{ width: `${Math.round(episode.confidence * 100)}%`, background: '#c4922a' }} />
          </div>
          <div className="dp-conf-pct">{Math.round(episode.confidence * 100)}% confidence</div>
        </>
      )}

      {relatedCauses.length > 0 && (
        <>
          <div className="dp-section-label">Coobie's trail</div>
          {relatedCauses.map((cause) => {
            const causeColor = CAUSE_COLOR[cause.type] ?? CAUSE_COLOR.default;
            return (
              <div key={cause.id} className="dp-cause-row">
                <span className="dp-cause-chip" style={{ color: causeColor, borderColor: `${causeColor}55` }}>
                  {cause.type.replace(/_/g, ' ')}
                </span>
              </div>
            );
          })}
        </>
      )}

      {(episode.interventions ?? []).length > 0 && (
        <>
          <div className="dp-section-label">Interventions</div>
          {episode.interventions.slice(0, 2).map((iv, i) => (
            <div key={i} className="dp-intervention">
              <div className="dp-iv-target">{iv.target}</div>
              <div className="dp-iv-action">{iv.action}</div>
            </div>
          ))}
        </>
      )}

      <div className="dp-meta">{episode.runId?.slice(0, 8)} · {new Date(episode.timestamp).toLocaleDateString()}</div>
      <div className="dp-flavor">{flavor}</div>
    </div>
  );
}

function CauseDetail({ cause, scene, onClose }) {
  const affected = scene?.episodeNodes?.filter((ep) =>
    cause.supportingRunIds?.includes(ep.runId ?? ep.id),
  ) ?? [];
  const causeColor = CAUSE_COLOR[cause.type] ?? CAUSE_COLOR.default;
  const flavor = cause.confidence > 0.7 ? COOBIE_FLAVOR.trail_found : COOBIE_FLAVOR.trail_lost;

  return (
    <div className="dp-content">
      <button className="dp-close" onClick={onClose}>✕</button>
      <div className="dp-eyebrow">Inferred Cause</div>
      <div className="dp-title">{cause.type.replace(/_/g, ' ')}</div>
      <div className="dp-cause-chip standalone" style={{ color: causeColor, borderColor: `${causeColor}55` }}>
        {cause.type.replace(/_/g, ' ')}
      </div>

      <div className="dp-desc">{cause.label}</div>

      <div className="dp-section-label">Confidence</div>
      <div className="dp-conf-bar">
        <div className="dp-conf-fill" style={{ width: `${Math.round(cause.confidence * 100)}%`, background: causeColor }} />
      </div>
      <div className="dp-conf-pct">{Math.round(cause.confidence * 100)}%</div>

      {affected.length > 0 && (
        <>
          <div className="dp-section-label">Affected runs ({affected.length})</div>
          {affected.slice(0, 6).map((ep) => {
            const sc = STATUS_COLOR[ep.status] ?? STATUS_COLOR.default;
            return (
              <div key={ep.id} className="dp-ep-row" style={{ borderColor: `${sc}44` }}>
                <span className="dp-ep-dot" style={{ background: sc }} />
                {ep.label}
              </div>
            );
          })}
        </>
      )}

      <div className="dp-flavor">{flavor}</div>
    </div>
  );
}

/**
 * Right-side detail panel. Driven by the current selection.
 * When nothing is selected, shows the observatory idle state.
 */
export default function DetailPanel({ selected, scene, lensMode, onClose }) {
  return (
    <div className="detail-panel">
      {!selected ? (
        <div className="dp-content dp-idle">
          <div className="dp-eyebrow">Coobie Observatory</div>
          <div className="dp-idle-hint">Click an episode sphere or cause diamond to inspect the causal trail.</div>
          <div className="dp-idle-lens">Lens: <strong>{lensMode}</strong></div>
          <div className="dp-idle-counts">
            <div>{scene?.episodeNodes?.length ?? 0} episodes</div>
            <div>{scene?.causeNodes?.length ?? 0} causes</div>
            <div>{scene?.clusters?.length ?? 0} clusters</div>
          </div>
          <div className="dp-flavor">{COOBIE_FLAVOR.watching}</div>
        </div>
      ) : selected.type === 'episode' ? (
        <EpisodeDetail episode={selected.data} scene={scene} onClose={onClose} />
      ) : selected.type === 'cause' ? (
        <CauseDetail cause={selected.data} scene={scene} onClose={onClose} />
      ) : null}

      <style jsx>{`
        .detail-panel {
          width: 300px;
          min-width: 300px;
          background: rgba(18, 20, 22, 0.92);
          border-left: 1px solid rgba(255, 255, 255, 0.07);
          display: flex;
          flex-direction: column;
          overflow-y: auto;
          position: relative;
        }
        .dp-content {
          padding: 1.2rem 1.1rem;
          display: flex;
          flex-direction: column;
          gap: 0.75rem;
          flex: 1;
        }
        .dp-idle { justify-content: center; align-items: center; text-align: center; gap: 1rem; }
        .dp-idle-hint { color: var(--text-secondary); font-size: 0.82rem; line-height: 1.5; }
        .dp-idle-lens { font-size: 0.78rem; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.08em; }
        .dp-idle-counts { display: flex; flex-direction: column; gap: 0.25rem; font-size: 0.76rem; color: var(--text-secondary); font-family: var(--font-mono); }
        .dp-close {
          position: absolute; top: 0.8rem; right: 0.8rem;
          background: none; border: 1px solid rgba(255,255,255,0.1); color: var(--text-secondary);
          border-radius: 50%; width: 26px; height: 26px; cursor: pointer; font-size: 0.78rem;
          display: flex; align-items: center; justify-content: center;
        }
        .dp-close:hover { color: var(--text-primary); border-color: rgba(255,255,255,0.25); }
        .dp-eyebrow {
          text-transform: uppercase; letter-spacing: 0.16em; font-size: 0.62rem;
          font-weight: 800; color: var(--accent-gold);
        }
        .dp-title { font-size: 1.0rem; font-weight: 700; line-height: 1.35; padding-right: 1.5rem; }
        .dp-desc { font-size: 0.82rem; color: var(--text-secondary); line-height: 1.5; }
        .dp-status-badge {
          display: inline-block; border: 1px solid; border-radius: 999px;
          padding: 0.2rem 0.6rem; font-size: 0.68rem; font-weight: 800;
          text-transform: uppercase; letter-spacing: 0.08em;
        }
        .dp-section-label {
          font-size: 0.62rem; font-weight: 800; text-transform: uppercase;
          letter-spacing: 0.1em; color: var(--accent-gold); margin-top: 0.35rem;
        }
        .score-stack { display: flex; flex-direction: column; gap: 0.4rem; }
        .score-row { display: grid; grid-template-columns: 5.5rem 1fr 2.2rem; gap: 0.45rem; align-items: center; }
        .score-label { font-size: 0.7rem; color: var(--text-secondary); }
        .score-track { height: 4px; background: rgba(255,255,255,0.07); border-radius: 2px; overflow: hidden; }
        .score-fill  { height: 100%; border-radius: 2px; transition: width 0.4s ease; }
        .score-pct   { font-size: 0.65rem; font-family: var(--font-mono); color: var(--text-secondary); text-align: right; }
        .dp-cause-row { display: flex; flex-direction: column; gap: 0.3rem; }
        .dp-cause-chip {
          display: inline-block; border: 1px solid; border-radius: 999px;
          padding: 0.15rem 0.5rem; font-size: 0.64rem; font-weight: 800;
          text-transform: uppercase; letter-spacing: 0.07em;
        }
        .dp-cause-chip.standalone { margin-bottom: 0.2rem; }
        .dp-cause-label { font-size: 0.78rem; color: var(--text-secondary); line-height: 1.4; }
        .dp-conf-bar { height: 4px; background: rgba(255,255,255,0.07); border-radius: 2px; overflow: hidden; margin-top: 0.1rem; }
        .dp-conf-fill { height: 100%; border-radius: 2px; transition: width 0.4s ease; }
        .dp-conf-pct { font-size: 0.65rem; font-family: var(--font-mono); color: var(--text-secondary); }
        .dp-ep-row {
          display: flex; align-items: center; gap: 0.5rem;
          font-size: 0.76rem; padding: 0.4rem 0.55rem;
          border: 1px solid; border-radius: 8px; background: rgba(255,255,255,0.02);
        }
        .dp-ep-dot { width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0; }
        .dp-meta { font-size: 0.68rem; font-family: var(--font-mono); color: var(--text-secondary); margin-top: 0.25rem; }
        .dp-flavor { font-size: 0.72rem; font-style: italic; color: var(--accent-gold); margin-top: auto; padding-top: 0.5rem; }
        .dp-cause-text { font-size: 0.78rem; color: var(--text-secondary); line-height: 1.5; font-style: italic; }
        .dp-intervention {
          border: 1px solid rgba(255,255,255,0.06); background: rgba(255,255,255,0.02);
          border-radius: 8px; padding: 0.5rem 0.65rem; display: flex; flex-direction: column; gap: 0.2rem;
        }
        .dp-iv-target { font-size: 0.65rem; font-weight: 800; text-transform: uppercase; letter-spacing: 0.08em; color: var(--accent-gold); }
        .dp-iv-action { font-size: 0.76rem; color: var(--text-secondary); line-height: 1.4; }
      `}</style>
    </div>
  );
}
