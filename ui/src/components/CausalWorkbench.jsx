import { useState, useEffect, useRef, useCallback } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

const AGENTS = [
  { id: 'scout',   label: 'Scout',   group: 'planning',      color: '#c4922a' },
  { id: 'keeper',  label: 'Keeper',  group: 'planning',      color: '#8a7a3a' },
  { id: 'mason',   label: 'Mason',   group: 'action',        color: '#c4662a' },
  { id: 'piper',   label: 'Piper',   group: 'action',        color: '#5a7a5a' },
  { id: 'ash',     label: 'Ash',     group: 'action',        color: '#2a7a7a' },
  { id: 'bramble', label: 'Bramble', group: 'verification',  color: '#a89a2a' },
  { id: 'sable',   label: 'Sable',   group: 'verification',  color: '#3a4a5a' },
  { id: 'flint',   label: 'Flint',   group: 'verification',  color: '#8a6a3a' },
  { id: 'coobie',  label: 'Coobie',  group: 'memory',        color: '#7a2a3a' },
];

const STATUS_COLOR = {
  running:  '#c4922a',
  complete: '#5a8a5a',
  blocked:  '#c7684c',
  idle:     '#2a2d30',
  warning:  '#c4662a',
  failed:   '#8a2a2a',
};

const CAUSE_TYPES = [
  'spec_ambiguity',
  'low_twin_fidelity',
  'test_blind_spot',
  'context_gap',
  'missing_failure_case',
  'policy_block',
  'tool_misuse',
  'resource_contention',
  'timing_issue',
  'external_dependency',
  'other',
];

const ROW_H = 36;
const LABEL_W = 88;
const AXIS_H = 28;

// ── helpers ────────────────────────────────────────────────────────────────────

function tsOf(ev) { return new Date(ev.created_at).getTime(); }

function buildSegments(events) {
  // Group events by agent, sort by time, build [start, end, status] segments
  const byAgent = {};
  for (const ev of events) {
    const a = ev.agent?.toLowerCase();
    if (!a) continue;
    if (!byAgent[a]) byAgent[a] = [];
    byAgent[a].push(ev);
  }
  const result = {};
  for (const [agent, evs] of Object.entries(byAgent)) {
    evs.sort((a, b) => tsOf(a) - tsOf(b));
    const segs = [];
    for (let i = 0; i < evs.length; i++) {
      const ev = evs[i];
      const start = tsOf(ev);
      const end = i + 1 < evs.length ? tsOf(evs[i + 1]) : start + 30_000;
      segs.push({ start, end, status: ev.status || 'idle', message: ev.message, phase: ev.phase, ev });
    }
    result[agent] = segs;
  }
  return result;
}

function findCooбieMatch(causalReport, selStart, selEnd) {
  if (!causalReport?.episodes) return null;
  // Find episode whose timestamp falls within selection
  for (const ep of causalReport.episodes) {
    const t = ep.timestamp ? new Date(ep.timestamp).getTime() : null;
    if (!t) continue;
    if (t >= selStart && t <= selEnd) return ep;
  }
  return causalReport.episodes[0] ?? null; // fallback: most recent
}

// ── Axis tick labels ───────────────────────────────────────────────────────────

function TimeAxis({ minT, maxT, width }) {
  const span = maxT - minT;
  const ticks = [];
  const count = Math.min(8, Math.floor(width / 80));
  for (let i = 0; i <= count; i++) {
    const t = minT + (span * i) / count;
    const x = LABEL_W + ((t - minT) / span) * (width - LABEL_W);
    const label = new Date(t).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    ticks.push({ x, label });
  }
  return (
    <g>
      {ticks.map(({ x, label }) => (
        <g key={x}>
          <line x1={x} y1={0} x2={x} y2={AGENTS.length * ROW_H} stroke="rgba(255,255,255,0.06)" strokeWidth={1} />
          <text x={x} y={AXIS_H - 6} textAnchor="middle" fill="rgba(255,255,255,0.35)" fontSize={10} fontFamily="monospace">
            {label}
          </text>
        </g>
      ))}
    </g>
  );
}

// ── Selection brush ────────────────────────────────────────────────────────────

function SelectionBrush({ brush, minT, maxT, totalW }) {
  if (!brush) return null;
  const span = maxT - minT;
  const trackW = totalW - LABEL_W;
  const x1 = LABEL_W + ((Math.min(brush.start, brush.end) - minT) / span) * trackW;
  const x2 = LABEL_W + ((Math.max(brush.start, brush.end) - minT) / span) * trackW;
  const w = Math.max(2, x2 - x1);
  return (
    <>
      <rect x={x1} y={0} width={w} height={AGENTS.length * ROW_H}
        fill="rgba(194,163,114,0.12)" stroke="rgba(194,163,114,0.55)" strokeWidth={1.5} />
      <line x1={x1} y1={0} x2={x1} y2={AGENTS.length * ROW_H}
        stroke="#c2a372" strokeWidth={2} />
      <line x1={x1 + w} y1={0} x2={x1 + w} y2={AGENTS.length * ROW_H}
        stroke="#c2a372" strokeWidth={2} />
    </>
  );
}

// ── Single agent swimlane row ──────────────────────────────────────────────────

function AgentRow({ agent, segments, minT, maxT, rowY, totalW, onSegmentClick, selectedTs }) {
  const span = maxT - minT;
  const trackW = totalW - LABEL_W;

  return (
    <g>
      {/* Label */}
      <text x={LABEL_W - 8} y={rowY + ROW_H / 2 + 4} textAnchor="end"
        fill="rgba(255,255,255,0.6)" fontSize={11} fontWeight={700} fontFamily="sans-serif">
        {agent.label}
      </text>
      {/* Row bg */}
      <rect x={LABEL_W} y={rowY} width={trackW} height={ROW_H - 2}
        fill="rgba(255,255,255,0.02)" rx={2} />
      {/* Segments */}
      {(segments || []).map((seg, i) => {
        const x = LABEL_W + ((seg.start - minT) / span) * trackW;
        const w = Math.max(3, ((seg.end - seg.start) / span) * trackW);
        const color = STATUS_COLOR[seg.status] || STATUS_COLOR.idle;
        const isSelected = selectedTs && seg.start <= selectedTs && seg.end >= selectedTs;
        return (
          <g key={i} style={{ cursor: 'pointer' }} onClick={() => onSegmentClick(seg)}>
            <rect x={x} y={rowY + 3} width={w} height={ROW_H - 8}
              fill={color} rx={3} opacity={isSelected ? 1 : 0.72}
              stroke={isSelected ? '#fff' : 'none'} strokeWidth={1.5} />
            {w > 24 && (
              <text x={x + 4} y={rowY + ROW_H / 2 + 4}
                fill="rgba(255,255,255,0.7)" fontSize={9} fontFamily="monospace">
                {seg.phase || seg.status}
              </text>
            )}
          </g>
        );
      })}
    </g>
  );
}

// ── Annotation form ────────────────────────────────────────────────────────────

function AnnotationForm({ selection, causalReport, runId, onSaved, onClear }) {
  const [causeType, setCauseType] = useState('');
  const [agreesWithCoobie, setAgreesWithCoobie] = useState(null);
  const [description, setDescription] = useState('');
  const [likelyCause, setLikelyCause] = useState('');
  const [intervention, setIntervention] = useState('');
  const [severity, setSeverity] = useState('medium');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  const matchedEpisode = selection
    ? findCooбieMatch(causalReport, selection.startTs, selection.endTs)
    : null;

  const isSpan = selection && selection.type === 'span';
  const isPoint = selection && selection.type === 'point';

  async function save() {
    if (!runId || !selection) return;
    setSaving(true);

    const spanStr = isSpan
      ? `${new Date(selection.startTs).toISOString()} → ${new Date(selection.endTs).toISOString()}`
      : new Date(selection.startTs).toISOString();

    const agentsInvolved = selection.agents?.join(', ') || 'unknown';

    const noteMarkdown = `---
type: causal_annotation
run_id: ${runId}
selection_type: ${selection.type}
time_span: "${spanStr}"
agents: [${agentsInvolved}]
cause_type: ${causeType || 'unclassified'}
severity: ${severity}
agrees_with_coobie: ${agreesWithCoobie === null ? 'not_assessed' : agreesWithCoobie}
coobie_assessment: ${matchedEpisode?.primary_cause_type || 'none'}
created_at: ${new Date().toISOString()}
---

## What Happened

${description || '_No description provided._'}

## Likely Cause

${likelyCause || '_No cause specified._'}

## Suggested Intervention

${intervention || '_No intervention specified._'}

## Coobie Assessment

${matchedEpisode
  ? `- Cause type: \`${matchedEpisode.primary_cause_type || 'unknown'}\`
- Cause text: ${matchedEpisode.primary_cause_text || 'not available'}
- Human agrees: **${agreesWithCoobie === true ? 'Yes' : agreesWithCoobie === false ? 'No' : 'Partial / unset'}**`
  : '_No Coobie episode matched this time window._'}
`;

    try {
      await fetch(`${API_BASE}/runs/${runId}/memory-note`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          note: noteMarkdown,
          tags: ['causal-annotation', `cause:${causeType}`, `severity:${severity}`, `run:${runId}`],
        }),
      });
      setSaved(true);
      setTimeout(() => { setSaved(false); onSaved?.(); }, 1800);
    } catch {
      // silently fail — user can retry
    } finally {
      setSaving(false);
    }
  }

  if (!selection) {
    return (
      <div className="wb-form-empty">
        <div className="wb-form-hint">
          <div className="wb-hint-icon">⌗</div>
          <p>Drag the timeline to select a <strong>span</strong></p>
          <p>or click a segment to mark a <strong>point event</strong></p>
          <p className="wb-hint-sub">Coobie's assessment will appear here alongside your annotation</p>
        </div>
      </div>
    );
  }

  return (
    <div className="wb-form">
      {/* Selection info */}
      <div className="wb-sel-header">
        <span className={`wb-sel-type ${selection.type}`}>{selection.type}</span>
        <span className="wb-sel-time">
          {isPoint
            ? new Date(selection.startTs).toLocaleTimeString()
            : `${new Date(selection.startTs).toLocaleTimeString()} → ${new Date(selection.endTs).toLocaleTimeString()}`}
        </span>
        {isSpan && (
          <span className="wb-sel-duration">
            {Math.round((selection.endTs - selection.startTs) / 1000)}s
          </span>
        )}
        <button className="wb-clear-btn" onClick={onClear} title="Clear selection">✕</button>
      </div>

      {/* Coobie's assessment */}
      {matchedEpisode && (
        <div className="wb-coobie-block">
          <div className="wb-coobie-label">Coobie's read</div>
          <div className="wb-coobie-cause">{matchedEpisode.primary_cause_type?.replaceAll('_', ' ') || 'unknown'}</div>
          {matchedEpisode.primary_cause_text && (
            <div className="wb-coobie-text">"{matchedEpisode.primary_cause_text}"</div>
          )}
          <div className="wb-agree-row">
            <span className="wb-agree-label">Do you agree?</span>
            <button className={`wb-agree-btn ${agreesWithCoobie === true ? 'active-yes' : ''}`}
              onClick={() => setAgreesWithCoobie(true)}>Yes</button>
            <button className={`wb-agree-btn ${agreesWithCoobie === false ? 'active-no' : ''}`}
              onClick={() => setAgreesWithCoobie(false)}>No</button>
            <button className={`wb-agree-btn ${agreesWithCoobie === 'partial' ? 'active-partial' : ''}`}
              onClick={() => setAgreesWithCoobie('partial')}>Partial</button>
          </div>
        </div>
      )}

      {/* Human annotation */}
      <div className="wb-field-group">
        <label className="wb-label">What happened?</label>
        <textarea className="wb-textarea" rows={3} placeholder="Describe the observed behavior…"
          value={description} onChange={e => setDescription(e.target.value)} />
      </div>

      <div className="wb-field-row">
        <div className="wb-field-group half">
          <label className="wb-label">Cause type</label>
          <select className="wb-select" value={causeType} onChange={e => setCauseType(e.target.value)}>
            <option value="">— select —</option>
            {CAUSE_TYPES.map(c => (
              <option key={c} value={c}>{c.replaceAll('_', ' ')}</option>
            ))}
          </select>
        </div>
        <div className="wb-field-group half">
          <label className="wb-label">Severity</label>
          <select className="wb-select" value={severity} onChange={e => setSeverity(e.target.value)}>
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
            <option value="critical">Critical</option>
          </select>
        </div>
      </div>

      <div className="wb-field-group">
        <label className="wb-label">Likely cause</label>
        <textarea className="wb-textarea" rows={2} placeholder="Why did this happen?"
          value={likelyCause} onChange={e => setLikelyCause(e.target.value)} />
      </div>

      <div className="wb-field-group">
        <label className="wb-label">Suggested intervention</label>
        <textarea className="wb-textarea" rows={2} placeholder="What should be done differently?"
          value={intervention} onChange={e => setIntervention(e.target.value)} />
      </div>

      <button className={`wb-save-btn ${saved ? 'saved' : ''}`} onClick={save} disabled={saving || saved}>
        {saved ? '✓ Saved to Coobie memory' : saving ? 'Saving…' : 'Save annotation'}
      </button>
    </div>
  );
}

// ── Time series data tab ───────────────────────────────────────────────────────

function parseCSV(text) {
  const lines = text.trim().split('\n');
  if (lines.length < 2) return { headers: [], rows: [] };
  const headers = lines[0].split(',').map(h => h.trim().replace(/^"|"$/g, ''));
  const rows = lines.slice(1).map(line => {
    const vals = line.split(',').map(v => v.trim().replace(/^"|"$/g, ''));
    const obj = {};
    headers.forEach((h, i) => { obj[h] = isNaN(vals[i]) ? vals[i] : Number(vals[i]); });
    return obj;
  });
  return { headers, rows };
}

function DataSeriesChart({ rows, headers, xCol, yCols, brush, onBrush }) {
  const svgRef = useRef(null);
  const [w, setW] = useState(600);
  const PAD = { top: 16, right: 16, bottom: 32, left: 52 };
  const H = 200;

  useEffect(() => {
    if (!svgRef.current) return;
    const obs = new ResizeObserver(e => setW(e[0].contentRect.width));
    obs.observe(svgRef.current);
    return () => obs.disconnect();
  }, []);

  const [dragging, setDragging] = useState(null);

  if (!rows.length || !xCol || !yCols.length) return null;

  const chartW = w - PAD.left - PAD.right;
  const chartH = H - PAD.top - PAD.bottom;

  const xVals = rows.map(r => r[xCol]);
  const isDateX = typeof xVals[0] === 'string' && !isNaN(Date.parse(xVals[0]));
  const xNum = isDateX ? xVals.map(v => new Date(v).getTime()) : xVals.map(Number);
  const xMin = Math.min(...xNum), xMax = Math.max(...xNum);

  const allY = yCols.flatMap(c => rows.map(r => Number(r[c])).filter(isFinite));
  const yMin = Math.min(...allY), yMax = Math.max(...allY);
  const yRange = yMax - yMin || 1;

  const xScale = v => PAD.left + ((v - xMin) / (xMax - xMin || 1)) * chartW;
  const yScale = v => PAD.top + chartH - ((v - yMin) / yRange) * chartH;

  const COLORS = ['#c4922a','#5a8acc','#8fae7c','#c7684c','#8a6ab0','#2a7a7a'];

  function clientToX(clientX) {
    const rect = svgRef.current.getBoundingClientRect();
    const frac = Math.max(0, Math.min(1, (clientX - rect.left - PAD.left) / chartW));
    return xMin + frac * (xMax - xMin);
  }

  function mouseDown(e) {
    if (e.button !== 0) return;
    const v = clientToX(e.clientX);
    setDragging({ origin: v });
    onBrush({ start: v, end: v });
  }
  function mouseMove(e) {
    if (!dragging) return;
    onBrush({ start: Math.min(dragging.origin, clientToX(e.clientX)), end: Math.max(dragging.origin, clientToX(e.clientX)) });
  }
  function mouseUp(e) {
    if (!dragging) return;
    const v = clientToX(e.clientX);
    onBrush({ start: Math.min(dragging.origin, v), end: Math.max(dragging.origin, v) });
    setDragging(null);
  }

  // Build polyline points per series
  const lines = yCols.map(col => ({
    col,
    points: rows.map((r, i) => `${xScale(xNum[i])},${yScale(Number(r[col]))}`).join(' '),
  }));

  // Brush rect
  const bx1 = brush ? xScale(brush.start) : null;
  const bx2 = brush ? xScale(brush.end) : null;

  // Y axis ticks
  const yTicks = [yMin, yMin + yRange * 0.25, yMin + yRange * 0.5, yMin + yRange * 0.75, yMax];

  return (
    <svg ref={svgRef} width="100%" height={H}
      style={{ display: 'block', cursor: dragging ? 'ew-resize' : 'crosshair', userSelect: 'none' }}
      onMouseDown={mouseDown} onMouseMove={mouseMove} onMouseUp={mouseUp}
      onMouseLeave={() => { if (dragging) mouseUp({ clientX: 0 }); }}
    >
      {/* Y axis */}
      {yTicks.map(t => (
        <g key={t}>
          <line x1={PAD.left} y1={yScale(t)} x2={PAD.left + chartW} y2={yScale(t)}
            stroke="rgba(255,255,255,0.05)" strokeWidth={1} />
          <text x={PAD.left - 4} y={yScale(t) + 4} textAnchor="end"
            fill="rgba(255,255,255,0.28)" fontSize={9} fontFamily="monospace">
            {t.toPrecision(3)}
          </text>
        </g>
      ))}

      {/* X axis labels */}
      {[0, 0.25, 0.5, 0.75, 1].map(f => {
        const v = xMin + f * (xMax - xMin);
        const label = isDateX
          ? new Date(v).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
          : v.toPrecision(4);
        return (
          <text key={f} x={xScale(v)} y={H - 4} textAnchor="middle"
            fill="rgba(255,255,255,0.25)" fontSize={9} fontFamily="monospace">
            {label}
          </text>
        );
      })}

      {/* Series lines */}
      {lines.map(({ col, points }, i) => (
        <polyline key={col} points={points} fill="none"
          stroke={COLORS[i % COLORS.length]} strokeWidth={1.5} opacity={0.85} />
      ))}

      {/* Brush */}
      {brush && bx1 !== null && Math.abs(bx2 - bx1) > 1 && (
        <>
          <rect x={Math.min(bx1, bx2)} y={PAD.top} width={Math.abs(bx2 - bx1)} height={chartH}
            fill="rgba(194,163,114,0.1)" stroke="rgba(194,163,114,0.5)" strokeWidth={1} />
          <line x1={Math.min(bx1,bx2)} y1={PAD.top} x2={Math.min(bx1,bx2)} y2={PAD.top+chartH}
            stroke="#c2a372" strokeWidth={1.5} />
          <line x1={Math.max(bx1,bx2)} y1={PAD.top} x2={Math.max(bx1,bx2)} y2={PAD.top+chartH}
            stroke="#c2a372" strokeWidth={1.5} />
        </>
      )}
    </svg>
  );
}

const DATA_CAUSE_TYPES = [
  'sensor_anomaly',
  'process_failure',
  'data_quality',
  'threshold_breach',
  'correlation_break',
  'regime_change',
  'equipment_issue',
  'operator_action',
  'external_event',
  'other',
];

function DataAnnotationForm({ selection, xCol, yCols, runId, onSaved, onClear }) {
  const [what, setWhat] = useState('');
  const [cause, setCause] = useState('');
  const [causeType, setCauseType] = useState('');
  const [effect, setEffect] = useState('');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  if (!selection || Math.abs(selection.end - selection.start) < 0.001) {
    return (
      <div className="wb-form-empty">
        <div className="wb-form-hint">
          <div className="wb-hint-icon">〜</div>
          <p>Drag the chart to select a <strong>time span</strong></p>
          <p className="wb-hint-sub">Annotate cause and effect in the application data for Coobie</p>
        </div>
      </div>
    );
  }

  async function save() {
    setSaving(true);
    const note = `---
type: timeseries_annotation
run_id: ${runId || 'unknown'}
x_axis: ${xCol}
y_series: [${yCols.join(', ')}]
span_start: ${selection.start}
span_end: ${selection.end}
cause_type: ${causeType || 'unclassified'}
created_at: ${new Date().toISOString()}
---

## What Was Observed

${what || '_No description._'}

## Likely Cause

${cause || '_Not specified._'}

## Observed Effect

${effect || '_Not specified._'}
`;
    try {
      await fetch(`${API_BASE}/runs/${runId}/memory-note`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          note,
          tags: ['timeseries-annotation', `cause:${causeType}`, `series:${yCols[0]}`, `run:${runId}`],
        }),
      });
      setSaved(true);
      setTimeout(() => { setSaved(false); onSaved?.(); }, 1800);
    } catch { /* noop */ } finally { setSaving(false); }
  }

  return (
    <div className="wb-form">
      <div className="wb-sel-header">
        <span className="wb-sel-type span">span</span>
        <span className="wb-sel-time">
          {typeof selection.start === 'number' && selection.start > 1e12
            ? `${new Date(selection.start).toLocaleTimeString()} → ${new Date(selection.end).toLocaleTimeString()}`
            : `${Number(selection.start).toPrecision(5)} → ${Number(selection.end).toPrecision(5)}`}
        </span>
        <button className="wb-clear-btn" onClick={onClear}>✕</button>
      </div>

      <div className="wb-field-group">
        <label className="wb-label">What was observed?</label>
        <textarea className="wb-textarea" rows={3}
          placeholder="Describe the pattern, anomaly, or event visible in the data…"
          value={what} onChange={e => setWhat(e.target.value)} />
      </div>

      <div className="wb-field-group">
        <label className="wb-label">Cause type</label>
        <select className="wb-select" value={causeType} onChange={e => setCauseType(e.target.value)}>
          <option value="">— select —</option>
          {DATA_CAUSE_TYPES.map(c => (
            <option key={c} value={c}>{c.replaceAll('_', ' ')}</option>
          ))}
        </select>
      </div>

      <div className="wb-field-group">
        <label className="wb-label">Likely cause</label>
        <textarea className="wb-textarea" rows={2}
          placeholder="What caused this pattern in the data?"
          value={cause} onChange={e => setCause(e.target.value)} />
      </div>

      <div className="wb-field-group">
        <label className="wb-label">Observed effect</label>
        <textarea className="wb-textarea" rows={2}
          placeholder="What downstream effect did this cause?"
          value={effect} onChange={e => setEffect(e.target.value)} />
      </div>

      <button className={`wb-save-btn ${saved ? 'saved' : ''}`} onClick={save} disabled={saving || saved}>
        {saved ? '✓ Saved to Coobie memory' : saving ? 'Saving…' : 'Save annotation'}
      </button>
    </div>
  );
}

function DataTab({ runId }) {
  const [parsed, setParsed] = useState(null);
  const [loadedName, setLoadedName] = useState('');
  const [xCol, setXCol] = useState('');
  const [yCols, setYCols] = useState([]);
  const [brush, setBrush] = useState(null);
  const [savedCount, setSavedCount] = useState(0);
  const [artifacts, setArtifacts] = useState([]);
  const [artifactsLoading, setArtifactsLoading] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const fileRef = useRef(null);

  // Fetch artifact list from backend when runId changes
  useEffect(() => {
    if (!runId) { setArtifacts([]); return; }
    setArtifactsLoading(true);
    fetch(`${API_BASE}/runs/${runId}/artifacts`)
      .then(r => r.ok ? r.json() : [])
      .then(list => setArtifacts(Array.isArray(list) ? list : []))
      .catch(() => setArtifacts([]))
      .finally(() => setArtifactsLoading(false));
  }, [runId]);

  const csvArtifacts = artifacts.filter(a => ['csv', 'tsv', 'txt'].includes(a.ext?.toLowerCase()));

  function applyText(text, name) {
    const p = parseCSV(text);
    setParsed(p);
    setLoadedName(name);
    setBrush(null);
    if (p.headers.length) {
      setXCol(p.headers[0]);
      const numeric = p.headers.filter((h, i) => i > 0 && p.rows.slice(0, 5).every(r => isFinite(Number(r[h]))));
      setYCols(numeric.slice(0, 4));
    }
  }

  async function loadArtifact(name) {
    setLoadingFile(true);
    try {
      const res = await fetch(`${API_BASE}/runs/${runId}/artifacts/${name}`);
      if (!res.ok) throw new Error(`${res.status}`);
      const text = await res.text();
      applyText(text, name);
    } catch (e) {
      console.error('Failed to load artifact', e);
    } finally {
      setLoadingFile(false);
    }
  }

  function loadLocalFile(e) {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = ev => applyText(ev.target.result, file.name);
    reader.readAsText(file);
  }

  function toggleYCol(col) {
    setYCols(prev => prev.includes(col) ? prev.filter(c => c !== col) : [...prev, col]);
  }

  const numericCols = parsed?.headers.filter(h =>
    parsed.rows.slice(0, 5).every(r => isFinite(Number(r[h])))
  ) || [];

  return (
    <div className="dt-root">
      {/* Toolbar */}
      <div className="dt-toolbar">
        {/* Run artifact picker */}
        {runId && (
          <div className="dt-artifact-row">
            <span className="dt-col-label">Run artifacts</span>
            {artifactsLoading && <span className="dt-meta">loading…</span>}
            {!artifactsLoading && csvArtifacts.length === 0 && (
              <span className="dt-meta">no CSV/TSV in run dir</span>
            )}
            {csvArtifacts.map(a => (
              <button key={a.name}
                className={`dt-artifact-chip ${loadedName === a.name ? 'active' : ''}`}
                onClick={() => loadArtifact(a.name)}
                disabled={loadingFile}
                title={`${a.size} bytes`}
              >
                {a.name}
              </button>
            ))}
          </div>
        )}

        {/* Local file fallback */}
        <input ref={fileRef} type="file" accept=".csv,.tsv,.txt" style={{ display: 'none' }} onChange={loadLocalFile} />
        <button className="dt-load-btn" onClick={() => fileRef.current?.click()} title="Load a local CSV file">
          {loadingFile ? '…' : '+ Local file'}
        </button>

        {parsed && (
          <span className="dt-meta">{parsed.rows.length} rows · {parsed.headers.length} cols · <strong style={{color:'rgba(255,255,255,0.5)'}}>{loadedName}</strong></span>
        )}
        {savedCount > 0 && (
          <span className="wb-saved-badge">{savedCount} annotation{savedCount > 1 ? 's' : ''} saved</span>
        )}
      </div>

      {!parsed && (
        <div className="dt-empty">
          <div className="dt-empty-icon">📈</div>
          {runId
            ? <p>Select a CSV artifact from the run above, or load a local file</p>
            : <p>Load a CSV to visualize and annotate application time series data</p>
          }
          <p className="dt-empty-sub">LamDet outputs, sensor logs, benchmark results — anything with columns</p>
        </div>
      )}

      {parsed && (
        <div className="dt-body">
          {/* Column selectors */}
          <div className="dt-col-bar">
            <div className="dt-col-group">
              <span className="dt-col-label">X axis</span>
              <select className="dt-select" value={xCol} onChange={e => { setXCol(e.target.value); setBrush(null); }}>
                {parsed.headers.map(h => <option key={h} value={h}>{h}</option>)}
              </select>
            </div>
            <div className="dt-col-group">
              <span className="dt-col-label">Series (Y)</span>
              <div className="dt-y-chips">
                {parsed.headers.filter(h => h !== xCol).map((h, i) => {
                  const isNumeric = numericCols.includes(h);
                  const active = yCols.includes(h);
                  const COLORS = ['#c4922a','#5a8acc','#8fae7c','#c7684c','#8a6ab0','#2a7a7a'];
                  const color = COLORS[numericCols.indexOf(h) % COLORS.length];
                  return (
                    <button key={h}
                      className={`dt-y-chip ${active ? 'active' : ''} ${!isNumeric ? 'non-numeric' : ''}`}
                      style={active ? { borderColor: `${color}88`, color } : {}}
                      onClick={() => isNumeric && toggleYCol(h)}
                      title={!isNumeric ? 'Non-numeric column' : h}
                    >
                      {h}
                    </button>
                  );
                })}
              </div>
            </div>
          </div>

          {/* Charts */}
          <div className="dt-charts">
            {yCols.length === 0 && (
              <div className="dt-no-series">Select at least one Y series above</div>
            )}
            {yCols.length > 0 && (
              <>
                <DataSeriesChart
                  rows={parsed.rows}
                  headers={parsed.headers}
                  xCol={xCol}
                  yCols={yCols}
                  brush={brush}
                  onBrush={setBrush}
                />
                <div className="dt-series-legend">
                  {yCols.map((col, i) => {
                    const COLORS = ['#c4922a','#5a8acc','#8fae7c','#c7684c','#8a6ab0','#2a7a7a'];
                    return (
                      <span key={col} className="dt-legend-item">
                        <span className="dt-legend-dot" style={{ background: COLORS[i % COLORS.length] }} />
                        {col}
                      </span>
                    );
                  })}
                </div>
              </>
            )}
          </div>

          {/* Annotation panel */}
          <div className="dt-anno-panel">
            <div className="wb-anno-header">
              <span className="wb-anno-title">Data Annotation</span>
            </div>
            <DataAnnotationForm
              selection={brush}
              xCol={xCol}
              yCols={yCols}
              runId={runId}
              onSaved={() => setSavedCount(c => c + 1)}
              onClear={() => setBrush(null)}
            />
          </div>
        </div>
      )}
    </div>
  );
}

// ── Main workbench ─────────────────────────────────────────────────────────────

export default function CausalWorkbench({ runId, events, causalReport, onClose }) {
  const svgRef = useRef(null);
  const [tab, setTab] = useState('dev');            // 'dev' | 'data'
  const [svgWidth, setSvgWidth] = useState(800);
  const [brush, setBrush] = useState(null);       // { start, end } in timestamp ms
  const [dragging, setDragging] = useState(null); // { originX, originTs }
  const [selection, setSelection] = useState(null);
  const [hoveredSeg, setHoveredSeg] = useState(null);
  const [savedCount, setSavedCount] = useState(0);

  const segments = buildSegments(events || []);

  // Time range
  const allTs = (events || []).map(tsOf).filter(Boolean);
  const minT = allTs.length ? Math.min(...allTs) : Date.now() - 3_600_000;
  const maxT = allTs.length ? Math.max(...allTs) + 60_000 : Date.now();

  // Observe SVG width
  useEffect(() => {
    if (!svgRef.current) return;
    const obs = new ResizeObserver(entries => {
      setSvgWidth(entries[0].contentRect.width);
    });
    obs.observe(svgRef.current);
    return () => obs.disconnect();
  }, []);

  // ── Brush logic ──────────────────────────────────────────────────────────────

  function xToTs(clientX) {
    const rect = svgRef.current.getBoundingClientRect();
    const x = clientX - rect.left;
    const trackW = svgWidth - LABEL_W;
    const frac = Math.max(0, Math.min(1, (x - LABEL_W) / trackW));
    return minT + frac * (maxT - minT);
  }

  function handleMouseDown(e) {
    if (e.button !== 0) return;
    const ts = xToTs(e.clientX);
    setDragging({ originX: e.clientX, originTs: ts });
    setBrush({ start: ts, end: ts });
  }

  function handleMouseMove(e) {
    if (!dragging) return;
    const ts = xToTs(e.clientX);
    setBrush({ start: dragging.originTs, end: ts });
  }

  function handleMouseUp(e) {
    if (!dragging) return;
    const ts = xToTs(e.clientX);
    const delta = Math.abs(ts - dragging.originTs);
    if (delta < 2000) {
      // point event — find agents active at this ts
      const agents = AGENTS
        .filter(a => (segments[a.id] || []).some(s => s.start <= ts && s.end >= ts))
        .map(a => a.id);
      setSelection({ type: 'point', startTs: ts, endTs: ts, agents });
    } else {
      const startTs = Math.min(dragging.originTs, ts);
      const endTs   = Math.max(dragging.originTs, ts);
      const agents = AGENTS
        .filter(a => (segments[a.id] || []).some(s => s.start < endTs && s.end > startTs))
        .map(a => a.id);
      setSelection({ type: 'span', startTs, endTs, agents });
      setBrush({ start: startTs, end: endTs });
    }
    setDragging(null);
  }

  function handleSegmentClick(seg) {
    const agents = AGENTS
      .filter(a => (segments[a.id] || []).includes(seg))
      .map(a => a.id);
    setSelection({
      type: 'point',
      startTs: seg.start,
      endTs: seg.end,
      agents: agents.length ? agents : [seg.ev?.agent?.toLowerCase()].filter(Boolean),
    });
    setBrush({ start: seg.start, end: seg.end });
  }

  const svgH = AGENTS.length * ROW_H;

  return (
    <div className="wb-overlay">

      {/* Header */}
      <div className="wb-header">
        <div className="wb-title-block">
          <span className="wb-eyebrow">Coobie Causal Workbench</span>
          <span className="wb-title">Run · {runId?.slice(0, 8) || 'no run'}</span>
          {savedCount > 0 && (
            <span className="wb-saved-badge">{savedCount} annotation{savedCount > 1 ? 's' : ''} saved</span>
          )}
        </div>

        {/* Tab switcher */}
        <div className="wb-tabs">
          <button
            className={`wb-tab ${tab === 'dev' ? 'active' : ''}`}
            onClick={() => setTab('dev')}
          >
            Development
          </button>
          <button
            className={`wb-tab ${tab === 'data' ? 'active' : ''}`}
            onClick={() => setTab('data')}
          >
            Time Series
          </button>
        </div>

        {tab === 'dev' && (
          <div className="wb-legend">
            {Object.entries(STATUS_COLOR).filter(([k]) => k !== 'idle').map(([k, v]) => (
              <span key={k} className="wb-legend-item">
                <span className="wb-legend-dot" style={{ background: v }} />
                {k}
              </span>
            ))}
          </div>
        )}

        <button className="wb-close" onClick={onClose} title="Close (Esc)">✕</button>
      </div>

      {/* Data tab */}
      {tab === 'data' && <DataTab runId={runId} />}

      {/* Dev tab body */}
      {tab === 'dev' && <div className="wb-body">

        {/* ── Swimlane panel ── */}
        <div className="wb-lanes-panel">
          <div className="wb-lanes-scroll">

            {/* Axis row */}
            <svg width="100%" height={AXIS_H} style={{ display: 'block', flexShrink: 0 }}>
              <TimeAxis minT={minT} maxT={maxT} width={svgWidth} />
            </svg>

            {/* Main swimlane SVG */}
            <svg
              ref={svgRef}
              width="100%"
              height={svgH}
              style={{ display: 'block', cursor: dragging ? 'ew-resize' : 'crosshair', userSelect: 'none' }}
              onMouseDown={handleMouseDown}
              onMouseMove={handleMouseMove}
              onMouseUp={handleMouseUp}
              onMouseLeave={() => { if (dragging) handleMouseUp({ clientX: dragging.originX }); }}
            >
              {/* Row backgrounds alternate */}
              {AGENTS.map((agent, i) => (
                <rect key={agent.id} x={LABEL_W} y={i * ROW_H} width={svgWidth - LABEL_W} height={ROW_H}
                  fill={i % 2 === 0 ? 'rgba(255,255,255,0.015)' : 'transparent'} />
              ))}

              {/* Grid lines */}
              <TimeAxis minT={minT} maxT={maxT} width={svgWidth} />

              {/* Agent rows */}
              {AGENTS.map((agent, i) => (
                <AgentRow
                  key={agent.id}
                  agent={agent}
                  segments={segments[agent.id]}
                  minT={minT}
                  maxT={maxT}
                  rowY={i * ROW_H}
                  totalW={svgWidth}
                  onSegmentClick={handleSegmentClick}
                  selectedTs={selection?.type === 'point' ? selection.startTs : null}
                />
              ))}

              {/* Selection brush */}
              <SelectionBrush brush={brush} minT={minT} maxT={maxT} totalW={svgWidth} />
            </svg>

          </div>

          {/* Active agents in selection */}
          {selection && (
            <div className="wb-active-agents">
              <span className="wb-active-label">agents in window:</span>
              {(selection.agents || []).map(id => {
                const a = AGENTS.find(x => x.id === id);
                return (
                  <span key={id} className="wb-agent-chip" style={{ borderColor: `${a?.color}88`, color: a?.color }}>
                    {a?.label || id}
                  </span>
                );
              })}
            </div>
          )}

          {/* Event stream for selected window */}
          {selection && (
            <div className="wb-event-stream">
              <div className="wb-stream-header">Events in window</div>
              <div className="wb-stream-list">
                {(events || [])
                  .filter(ev => {
                    const t = tsOf(ev);
                    return t >= selection.startTs - 1000 && t <= selection.endTs + 1000;
                  })
                  .sort((a, b) => tsOf(a) - tsOf(b))
                  .map((ev, i) => (
                    <div key={i} className="wb-stream-item">
                      <span className="wb-stream-agent" style={{ color: AGENTS.find(a => a.id === ev.agent?.toLowerCase())?.color || '#888' }}>
                        {ev.agent}
                      </span>
                      <span className="wb-stream-phase">{ev.phase}</span>
                      <span className="wb-stream-msg">{ev.message}</span>
                      <span className="wb-stream-time">{new Date(ev.created_at).toLocaleTimeString()}</span>
                    </div>
                  ))}
                {(events || []).filter(ev => {
                  const t = tsOf(ev);
                  return t >= selection.startTs - 1000 && t <= selection.endTs + 1000;
                }).length === 0 && (
                  <div className="wb-stream-empty">No events in this window.</div>
                )}
              </div>
            </div>
          )}
        </div>

        {/* ── Annotation panel ── */}
        <div className="wb-annotation-panel">
          <div className="wb-anno-header">
            <span className="wb-anno-title">Annotation</span>
            {causalReport && (
              <span className="wb-coobie-indicator" title="Coobie has a causal report for this run">
                <span className="wb-coobie-dot" />
                Coobie report loaded
              </span>
            )}
          </div>
          <AnnotationForm
            selection={selection}
            causalReport={causalReport}
            runId={runId}
            onSaved={() => setSavedCount(c => c + 1)}
            onClear={() => { setSelection(null); setBrush(null); }}
          />
        </div>

      </div>}

      <style jsx>{`
        .wb-overlay {
          position: fixed;
          inset: 0;
          z-index: 2100;
          background: #0d0f11;
          display: flex;
          flex-direction: column;
          color: var(--text-primary, #eaeaea);
          font-family: 'IBM Plex Sans', 'Segoe UI', sans-serif;
        }

        /* ── Header ── */
        .wb-header {
          display: flex;
          align-items: center;
          gap: 1.2rem;
          padding: 0.6rem 1.2rem;
          border-bottom: 1px solid rgba(255,255,255,0.07);
          background: rgba(18,20,22,0.92);
          flex-shrink: 0;
          flex-wrap: wrap;
        }
        .wb-title-block {
          display: flex;
          align-items: baseline;
          gap: 0.65rem;
        }
        .wb-eyebrow {
          font-size: 0.6rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.16em;
          color: #c2a372;
        }
        .wb-title {
          font-size: 1rem;
          font-weight: 800;
          letter-spacing: 0.04em;
        }
        .wb-saved-badge {
          font-size: 0.62rem;
          background: rgba(90,138,90,0.18);
          border: 1px solid rgba(90,138,90,0.4);
          color: #8fae7c;
          border-radius: 999px;
          padding: 0.15rem 0.5rem;
          font-weight: 700;
        }
        .wb-legend {
          display: flex;
          gap: 0.75rem;
          flex-wrap: wrap;
          margin-left: auto;
        }
        .wb-legend-item {
          display: flex;
          align-items: center;
          gap: 0.3rem;
          font-size: 0.65rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.07em;
          color: rgba(255,255,255,0.45);
        }
        .wb-legend-dot {
          width: 8px;
          height: 8px;
          border-radius: 2px;
          flex-shrink: 0;
        }
        .wb-close {
          background: none;
          border: 1px solid rgba(255,255,255,0.12);
          color: rgba(255,255,255,0.5);
          border-radius: 50%;
          width: 28px;
          height: 28px;
          cursor: pointer;
          font-size: 0.8rem;
          display: flex;
          align-items: center;
          justify-content: center;
          flex-shrink: 0;
        }
        .wb-close:hover {
          color: #fff;
          border-color: rgba(255,255,255,0.3);
        }

        /* ── Body ── */
        .wb-body {
          flex: 1;
          display: grid;
          grid-template-columns: 1fr 340px;
          min-height: 0;
          overflow: hidden;
        }

        /* ── Swimlane panel ── */
        .wb-lanes-panel {
          display: flex;
          flex-direction: column;
          min-height: 0;
          overflow: hidden;
          border-right: 1px solid rgba(255,255,255,0.06);
        }
        .wb-lanes-scroll {
          flex-shrink: 0;
          padding: 0.5rem 0.8rem 0;
          overflow-x: auto;
        }
        .wb-active-agents {
          display: flex;
          align-items: center;
          gap: 0.4rem;
          flex-wrap: wrap;
          padding: 0.5rem 1rem;
          border-top: 1px solid rgba(255,255,255,0.05);
          border-bottom: 1px solid rgba(255,255,255,0.05);
          background: rgba(255,255,255,0.02);
          flex-shrink: 0;
        }
        .wb-active-label {
          font-size: 0.65rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: rgba(255,255,255,0.35);
        }
        .wb-agent-chip {
          font-size: 0.68rem;
          font-weight: 700;
          border: 1px solid;
          border-radius: 999px;
          padding: 0.12rem 0.45rem;
        }

        /* ── Event stream ── */
        .wb-event-stream {
          flex: 1;
          min-height: 0;
          overflow-y: auto;
          padding: 0.5rem 1rem;
        }
        .wb-stream-header {
          font-size: 0.65rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: #c2a372;
          margin-bottom: 0.5rem;
        }
        .wb-stream-list {
          display: flex;
          flex-direction: column;
          gap: 0.3rem;
        }
        .wb-stream-item {
          display: grid;
          grid-template-columns: 60px 72px 1fr auto;
          gap: 0.5rem;
          align-items: baseline;
          padding: 0.4rem 0.6rem;
          background: rgba(255,255,255,0.03);
          border: 1px solid rgba(255,255,255,0.04);
          border-radius: 8px;
          font-size: 0.78rem;
        }
        .wb-stream-agent {
          font-weight: 800;
          font-size: 0.72rem;
        }
        .wb-stream-phase {
          color: rgba(255,255,255,0.4);
          font-size: 0.7rem;
          font-family: monospace;
        }
        .wb-stream-msg {
          color: rgba(255,255,255,0.8);
          overflow: hidden;
          text-overflow: ellipsis;
          white-space: nowrap;
        }
        .wb-stream-time {
          color: rgba(255,255,255,0.25);
          font-size: 0.68rem;
          font-family: monospace;
          white-space: nowrap;
        }
        .wb-stream-empty {
          color: rgba(255,255,255,0.3);
          font-size: 0.8rem;
          font-style: italic;
          padding: 0.5rem;
        }

        /* ── Annotation panel ── */
        .wb-annotation-panel {
          display: flex;
          flex-direction: column;
          overflow-y: auto;
          background: rgba(16,18,20,0.95);
        }
        .wb-anno-header {
          display: flex;
          align-items: center;
          justify-content: space-between;
          padding: 0.7rem 1rem 0.5rem;
          border-bottom: 1px solid rgba(255,255,255,0.06);
          flex-shrink: 0;
        }
        .wb-anno-title {
          font-size: 0.7rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.12em;
          color: #c2a372;
        }
        .wb-coobie-indicator {
          display: flex;
          align-items: center;
          gap: 0.3rem;
          font-size: 0.62rem;
          color: rgba(255,255,255,0.35);
        }
        .wb-coobie-dot {
          width: 6px;
          height: 6px;
          border-radius: 50%;
          background: #7a2a3a;
          box-shadow: 0 0 4px #7a2a3a;
        }

        /* ── Form empty state ── */
        .wb-form-empty {
          flex: 1;
          display: flex;
          align-items: center;
          justify-content: center;
          padding: 2rem;
        }
        .wb-form-hint {
          text-align: center;
          color: rgba(255,255,255,0.3);
        }
        .wb-hint-icon {
          font-size: 2rem;
          margin-bottom: 0.8rem;
          opacity: 0.4;
        }
        .wb-form-hint p {
          font-size: 0.82rem;
          margin-bottom: 0.4rem;
        }
        .wb-form-hint strong {
          color: rgba(255,255,255,0.55);
        }
        .wb-hint-sub {
          font-size: 0.72rem !important;
          margin-top: 0.8rem !important;
          color: rgba(255,255,255,0.2) !important;
        }

        /* ── Form ── */
        .wb-form {
          padding: 0.8rem 1rem;
          display: flex;
          flex-direction: column;
          gap: 0.75rem;
        }
        .wb-sel-header {
          display: flex;
          align-items: center;
          gap: 0.5rem;
          flex-wrap: wrap;
          padding: 0.4rem 0.6rem;
          background: rgba(255,255,255,0.04);
          border: 1px solid rgba(255,255,255,0.07);
          border-radius: 10px;
        }
        .wb-sel-type {
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          border-radius: 999px;
          padding: 0.12rem 0.45rem;
          border: 1px solid;
        }
        .wb-sel-type.span {
          color: #c2a372;
          border-color: rgba(194,163,114,0.4);
          background: rgba(194,163,114,0.08);
        }
        .wb-sel-type.point {
          color: #5a8acc;
          border-color: rgba(90,138,204,0.4);
          background: rgba(90,138,204,0.08);
        }
        .wb-sel-time {
          font-size: 0.72rem;
          font-family: monospace;
          color: rgba(255,255,255,0.6);
        }
        .wb-sel-duration {
          font-size: 0.68rem;
          color: rgba(255,255,255,0.35);
          font-family: monospace;
        }
        .wb-clear-btn {
          margin-left: auto;
          background: none;
          border: none;
          color: rgba(255,255,255,0.3);
          cursor: pointer;
          font-size: 0.75rem;
          padding: 0.15rem 0.3rem;
        }
        .wb-clear-btn:hover { color: rgba(255,255,255,0.7); }

        /* ── Coobie block ── */
        .wb-coobie-block {
          padding: 0.65rem 0.8rem;
          background: rgba(122,42,58,0.15);
          border: 1px solid rgba(122,42,58,0.4);
          border-radius: 10px;
        }
        .wb-coobie-label {
          font-size: 0.6rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: rgba(122,42,58,0.9);
          margin-bottom: 0.3rem;
        }
        .wb-coobie-cause {
          font-size: 0.88rem;
          font-weight: 700;
          text-transform: capitalize;
          margin-bottom: 0.25rem;
        }
        .wb-coobie-text {
          font-size: 0.75rem;
          color: rgba(255,255,255,0.5);
          font-style: italic;
          margin-bottom: 0.5rem;
          line-height: 1.45;
        }
        .wb-agree-row {
          display: flex;
          align-items: center;
          gap: 0.4rem;
          flex-wrap: wrap;
        }
        .wb-agree-label {
          font-size: 0.65rem;
          color: rgba(255,255,255,0.4);
          text-transform: uppercase;
          letter-spacing: 0.08em;
          font-weight: 700;
          margin-right: 0.2rem;
        }
        .wb-agree-btn {
          padding: 0.2rem 0.55rem;
          border: 1px solid rgba(255,255,255,0.12);
          background: rgba(255,255,255,0.04);
          border-radius: 999px;
          color: rgba(255,255,255,0.45);
          font-size: 0.68rem;
          font-weight: 700;
          cursor: pointer;
          transition: all 0.12s;
        }
        .wb-agree-btn.active-yes {
          color: #8fae7c;
          border-color: rgba(143,174,124,0.5);
          background: rgba(143,174,124,0.1);
        }
        .wb-agree-btn.active-no {
          color: #c7684c;
          border-color: rgba(199,104,76,0.5);
          background: rgba(199,104,76,0.1);
        }
        .wb-agree-btn.active-partial {
          color: #c4922a;
          border-color: rgba(196,146,42,0.5);
          background: rgba(196,146,42,0.1);
        }

        /* ── Fields ── */
        .wb-field-group {
          display: flex;
          flex-direction: column;
          gap: 0.3rem;
        }
        .wb-field-row {
          display: flex;
          gap: 0.5rem;
        }
        .wb-field-group.half { flex: 1; }
        .wb-label {
          font-size: 0.65rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: rgba(255,255,255,0.4);
        }
        .wb-textarea {
          background: rgba(255,255,255,0.04);
          border: 1px solid rgba(255,255,255,0.1);
          border-radius: 8px;
          color: #eaeaea;
          padding: 0.5rem 0.65rem;
          font-size: 0.82rem;
          font-family: inherit;
          resize: vertical;
          line-height: 1.45;
          transition: border-color 0.12s;
        }
        .wb-textarea:focus {
          outline: none;
          border-color: rgba(194,163,114,0.45);
        }
        .wb-select {
          background: rgba(255,255,255,0.04);
          border: 1px solid rgba(255,255,255,0.1);
          border-radius: 8px;
          color: #eaeaea;
          padding: 0.45rem 0.65rem;
          font-size: 0.82rem;
          font-family: inherit;
          width: 100%;
        }
        .wb-select:focus {
          outline: none;
          border-color: rgba(194,163,114,0.45);
        }

        /* ── Save button ── */
        .wb-save-btn {
          padding: 0.6rem 1rem;
          background: rgba(194,163,114,0.1);
          border: 1px solid rgba(194,163,114,0.4);
          border-radius: 10px;
          color: #c2a372;
          font-size: 0.78rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          cursor: pointer;
          transition: all 0.15s;
          margin-top: 0.25rem;
        }
        .wb-save-btn:hover:not(:disabled) {
          background: rgba(194,163,114,0.18);
          border-color: rgba(194,163,114,0.65);
        }
        .wb-save-btn.saved {
          background: rgba(143,174,124,0.12);
          border-color: rgba(143,174,124,0.5);
          color: #8fae7c;
        }
        .wb-save-btn:disabled { opacity: 0.6; cursor: default; }

        @media (max-width: 900px) {
          .wb-body {
            grid-template-columns: 1fr;
            grid-template-rows: 1fr auto;
          }
          .wb-annotation-panel {
            border-top: 1px solid rgba(255,255,255,0.06);
            max-height: 45vh;
          }
        }

        /* ── Tab switcher ── */
        .wb-tabs {
          display: flex;
          gap: 0.2rem;
          background: rgba(255,255,255,0.04);
          border: 1px solid rgba(255,255,255,0.08);
          border-radius: 999px;
          padding: 0.2rem;
        }
        .wb-tab {
          padding: 0.28rem 0.9rem;
          border-radius: 999px;
          border: none;
          background: transparent;
          color: rgba(255,255,255,0.4);
          font-size: 0.7rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          cursor: pointer;
          transition: all 0.12s;
          font-family: inherit;
        }
        .wb-tab:hover { color: rgba(255,255,255,0.75); }
        .wb-tab.active {
          background: rgba(194,163,114,0.14);
          color: #c2a372;
          border: 1px solid rgba(194,163,114,0.35);
        }

        /* ── Data tab ── */
        .dt-root {
          flex: 1;
          display: flex;
          flex-direction: column;
          min-height: 0;
          overflow: hidden;
        }
        .dt-toolbar {
          display: flex;
          align-items: center;
          gap: 0.85rem;
          padding: 0.6rem 1.2rem;
          border-bottom: 1px solid rgba(255,255,255,0.06);
          background: rgba(18,20,22,0.7);
          flex-shrink: 0;
        }
        .dt-load-btn {
          padding: 0.38rem 0.9rem;
          background: rgba(90,138,204,0.1);
          border: 1px solid rgba(90,138,204,0.35);
          border-radius: 8px;
          color: #5a8acc;
          font-size: 0.72rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.09em;
          cursor: pointer;
          transition: all 0.12s;
          font-family: inherit;
        }
        .dt-load-btn:hover {
          background: rgba(90,138,204,0.2);
          border-color: rgba(90,138,204,0.6);
        }
        .dt-meta {
          font-size: 0.7rem;
          font-family: monospace;
          color: rgba(255,255,255,0.3);
        }
        .dt-empty {
          flex: 1;
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          gap: 0.5rem;
          color: rgba(255,255,255,0.3);
          padding: 2rem;
          text-align: center;
        }
        .dt-empty-icon { font-size: 2.5rem; opacity: 0.4; }
        .dt-empty p { font-size: 0.84rem; margin: 0; }
        .dt-empty-sub { font-size: 0.72rem !important; color: rgba(255,255,255,0.18) !important; margin-top: 0.3rem !important; }

        .dt-body {
          flex: 1;
          display: grid;
          grid-template-columns: 1fr 340px;
          grid-template-rows: auto 1fr;
          min-height: 0;
          overflow: hidden;
        }
        .dt-col-bar {
          grid-column: 1;
          display: flex;
          align-items: flex-start;
          gap: 1.2rem;
          padding: 0.65rem 1rem;
          border-bottom: 1px solid rgba(255,255,255,0.05);
          flex-wrap: wrap;
          background: rgba(255,255,255,0.02);
        }
        .dt-col-group {
          display: flex;
          align-items: center;
          gap: 0.5rem;
          flex-wrap: wrap;
        }
        .dt-col-label {
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.1em;
          color: rgba(255,255,255,0.35);
          white-space: nowrap;
        }
        .dt-select {
          background: rgba(255,255,255,0.05);
          border: 1px solid rgba(255,255,255,0.1);
          border-radius: 8px;
          color: #eaeaea;
          padding: 0.3rem 0.55rem;
          font-size: 0.78rem;
          font-family: monospace;
        }
        .dt-y-chips {
          display: flex;
          flex-wrap: wrap;
          gap: 0.3rem;
        }
        .dt-y-chip {
          padding: 0.18rem 0.5rem;
          border: 1px solid rgba(255,255,255,0.1);
          border-radius: 999px;
          background: rgba(255,255,255,0.03);
          color: rgba(255,255,255,0.35);
          font-size: 0.66rem;
          font-weight: 700;
          font-family: monospace;
          cursor: pointer;
          transition: all 0.12s;
        }
        .dt-y-chip.active { background: rgba(255,255,255,0.07); }
        .dt-y-chip.non-numeric { opacity: 0.3; cursor: default; }
        .dt-y-chip:hover:not(.non-numeric) { color: rgba(255,255,255,0.7); }

        .dt-charts {
          grid-column: 1;
          padding: 0.5rem 1rem;
          overflow-y: auto;
          display: flex;
          flex-direction: column;
          gap: 0.4rem;
        }
        .dt-no-series {
          color: rgba(255,255,255,0.25);
          font-size: 0.8rem;
          padding: 1rem;
          font-style: italic;
        }
        .dt-series-legend {
          display: flex;
          gap: 0.75rem;
          flex-wrap: wrap;
          padding: 0.2rem 0;
        }
        .dt-legend-item {
          display: flex;
          align-items: center;
          gap: 0.3rem;
          font-size: 0.66rem;
          font-weight: 700;
          font-family: monospace;
          color: rgba(255,255,255,0.5);
        }
        .dt-legend-dot {
          width: 10px;
          height: 3px;
          border-radius: 2px;
          flex-shrink: 0;
        }

        .dt-artifact-row {
          display: flex;
          align-items: center;
          gap: 0.4rem;
          flex-wrap: wrap;
          flex: 1;
        }
        .dt-artifact-chip {
          padding: 0.2rem 0.55rem;
          border: 1px solid rgba(255,255,255,0.1);
          border-radius: 6px;
          background: rgba(255,255,255,0.03);
          color: rgba(255,255,255,0.5);
          font-size: 0.68rem;
          font-weight: 700;
          font-family: monospace;
          cursor: pointer;
          transition: all 0.12s;
        }
        .dt-artifact-chip:hover:not(:disabled) {
          color: rgba(255,255,255,0.85);
          border-color: rgba(255,255,255,0.25);
        }
        .dt-artifact-chip.active {
          color: #c2a372;
          border-color: rgba(194,163,114,0.45);
          background: rgba(194,163,114,0.08);
        }
        .dt-artifact-chip:disabled { opacity: 0.4; cursor: default; }

        .dt-anno-panel {
          grid-column: 2;
          grid-row: 1 / -1;
          display: flex;
          flex-direction: column;
          border-left: 1px solid rgba(255,255,255,0.06);
          background: rgba(16,18,20,0.95);
          overflow-y: auto;
        }
      `}</style>
    </div>
  );
}
