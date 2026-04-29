import React, { useCallback, useEffect, useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

const STATUS_STYLE = {
  pending: { label: 'Pending', color: '#b8b2a7', bg: 'rgba(255,255,255,0.08)' },
  retry_pending: { label: 'Retry pending', color: '#e0b34f', bg: 'rgba(224,179,79,0.14)' },
  waiting_openbrain: { label: 'Waiting OB1', color: '#58b8e8', bg: 'rgba(88,184,232,0.14)' },
  held_for_review: { label: 'Review', color: '#d890d8', bg: 'rgba(216,144,216,0.14)' },
  captured_openbrain: { label: 'OB1 captured', color: '#64c27b', bg: 'rgba(100,194,123,0.14)' },
  promotion_pending: { label: 'Calvin review', color: '#df9f63', bg: 'rgba(223,159,99,0.14)' },
  duplicate_openbrain: { label: 'Duplicate', color: '#8f99a8', bg: 'rgba(143,153,168,0.12)' },
  ignored_ephemeral: { label: 'Ignored', color: '#8f99a8', bg: 'rgba(143,153,168,0.12)' },
};

const CHAIN_STATUS = {
  clear: { label: 'Clear', tone: 'good', detail: 'Memory chain has no pending operator action.' },
  processing: { label: 'Processing', tone: 'neutral', detail: 'Candidates are queued for processing.' },
  needs_review: { label: 'Needs review', tone: 'warn', detail: 'One or more candidates need operator action.' },
  retry_pending: { label: 'Retry pending', tone: 'warn', detail: 'A transient failure needs another processing attempt.' },
  waiting_openbrain: { label: 'Waiting OB1', tone: 'warn', detail: 'OB1 is not configured or reachable for shared recall capture.' },
  calvin_review: { label: 'Calvin review', tone: 'neutral', detail: 'Promotion contracts are waiting in the governed review flow.' },
};

function StatusChip({ status }) {
  const style = STATUS_STYLE[status] || { label: status || 'Unknown', color: '#b8b2a7', bg: 'rgba(255,255,255,0.08)' };
  return (
    <span style={{
      padding: '2px 8px',
      borderRadius: 99,
      fontSize: 11,
      fontWeight: 650,
      color: style.color,
      background: style.bg,
      whiteSpace: 'nowrap',
    }}>
      {style.label}
    </span>
  );
}

function CountTile({ label, value, tone = 'neutral' }) {
  const color = tone === 'warn' ? '#e0b34f' : tone === 'good' ? '#64c27b' : '#d8d3ca';
  return (
    <div style={{
      minWidth: 96,
      padding: '9px 10px',
      border: '1px solid rgba(255,255,255,0.08)',
      borderRadius: 6,
      background: '#191d1f',
    }}>
      <div style={{ fontSize: 11, color: '#8f99a8', marginBottom: 4 }}>{label}</div>
      <div style={{ fontSize: 18, fontWeight: 700, color }}>{value ?? 0}</div>
    </div>
  );
}

function ChainStatusBanner({ status, blockers }) {
  const entry = CHAIN_STATUS[status] || CHAIN_STATUS.clear;
  const color = entry.tone === 'warn' ? '#e0b34f' : entry.tone === 'good' ? '#64c27b' : '#d8d3ca';
  return (
    <div style={{
      border: `1px solid ${entry.tone === 'warn' ? 'rgba(224,179,79,0.28)' : 'rgba(255,255,255,0.08)'}`,
      background: entry.tone === 'warn' ? 'rgba(224,179,79,0.08)' : '#191d1f',
      borderRadius: 8,
      padding: '10px 12px',
      marginBottom: 12,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
        <span style={{ color, fontWeight: 750, fontSize: 13 }}>{entry.label}</span>
        <span style={{ color: '#8f99a8', fontSize: 12 }}>{entry.detail}</span>
      </div>
      {blockers?.length > 0 && (
        <div style={{ color: '#c9c1b4', fontSize: 12, lineHeight: 1.45 }}>
          {blockers.join(' · ')}
        </div>
      )}
    </div>
  );
}

function extractContent(candidate) {
  return candidate.distilled_content
    || candidate.raw_payload?.content
    || candidate.raw_payload?.content_preview
    || candidate.operation
    || candidate.candidate_id;
}

function CalvinContractPreview({ contract }) {
  if (!contract || contract.schema !== 'harkonnen.calvin.promotion.v1') return null;
  const chambers = Array.isArray(contract.chamber_targets) ? contract.chamber_targets.join(', ') : 'calvin';
  const outcome = contract.recommended_governance_outcome || 'review';
  const note = contract.preservation_note || 'Governed archive proposal.';
  return (
    <div style={{
      marginTop: 8,
      padding: '8px 10px',
      borderRadius: 6,
      border: '1px solid rgba(223,159,99,0.22)',
      background: 'rgba(223,159,99,0.08)',
      color: '#d8d3ca',
      fontSize: 12,
      lineHeight: 1.4,
    }}>
      <div style={{ color: '#df9f63', fontWeight: 700, marginBottom: 3 }}>
        Calvin proposal · {outcome} · {chambers}
      </div>
      <div>{note}</div>
    </div>
  );
}

export default function MemoryCandidatesPanel({ runId }) {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [busyCandidate, setBusyCandidate] = useState('');
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    if (!runId) return;
    setLoading(true);
    setError('');
    try {
      const response = await fetch(`${API_BASE}/runs/${runId}/memory/candidates`);
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      setData(await response.json());
    } catch (err) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }, [runId]);

  useEffect(() => { load(); }, [load]);

  async function retryProcessing() {
    setRetrying(true);
    setError('');
    try {
      const response = await fetch(`${API_BASE}/runs/${runId}/memory/candidates/retry`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ limit: 100 }),
      });
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      await load();
    } catch (err) {
      setError(err.message);
    } finally {
      setRetrying(false);
    }
  }

  async function reviewCandidate(candidateId, action) {
    setBusyCandidate(candidateId);
    setError('');
    try {
      const response = await fetch(`${API_BASE}/runs/${runId}/memory/candidates/${candidateId}/${action}`, {
        method: 'POST',
      });
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      await load();
    } catch (err) {
      setError(err.message);
    } finally {
      setBusyCandidate('');
    }
  }

  const candidates = data?.candidates || [];
  const recent = [...candidates].reverse().slice(0, 12);
  const retryable = data?.retryable || 0;

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, marginBottom: 12 }}>
        <div>
          <div className="drawer-eyebrow">Memory Candidates</div>
          <div style={{ color: '#d8d3ca', fontSize: 14 }}>
            {loading ? 'Loading' : `${data?.total || 0} captured from this run`}
          </div>
        </div>
        <button
          className="wb-action-btn wb-keep"
          onClick={retryProcessing}
          disabled={retrying || retryable === 0}
          title="Retry pending, retry-pending, and waiting-OB1 candidates"
        >
          {retrying ? 'Retrying...' : 'Retry processing'}
        </button>
      </div>

      {error && <div className="drawer-error">Error: {error}</div>}

      <ChainStatusBanner
        status={data?.memory_chain_status || 'clear'}
        blockers={data?.memory_chain_blockers || []}
      />

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginBottom: 14 }}>
        <CountTile label="Retryable" value={data?.retryable} tone={retryable > 0 ? 'warn' : 'neutral'} />
        <CountTile label="Needs review" value={data?.actionable} tone={(data?.actionable || 0) > 0 ? 'warn' : 'neutral'} />
        <CountTile label="OB1" value={data?.captured_openbrain} tone="good" />
        <CountTile label="Calvin review" value={data?.promotion_pending} />
      </div>

      {recent.length === 0 && !loading && (
        <div className="drawer-empty">No memory candidates recorded for this run.</div>
      )}

      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {recent.map((candidate) => (
          <div
            key={candidate.candidate_id}
            style={{
              background: '#1e2224',
              border: '1px solid rgba(255,255,255,0.07)',
              borderRadius: 8,
              padding: '10px 12px',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
              <StatusChip status={candidate.status} />
              <span style={{ color: '#8f99a8', fontSize: 11 }}>{candidate.retention_class}</span>
              <span style={{ color: '#8f99a8', fontSize: 11 }}>{candidate.agent || candidate.role}</span>
            </div>
            <div style={{ color: '#e5e1d8', fontSize: 13, lineHeight: 1.4 }}>
              {extractContent(candidate)}
            </div>
            <CalvinContractPreview contract={candidate.calvin_contract_json} />
            {(candidate.status === 'held_for_review'
              || candidate.status === 'retry_pending'
              || candidate.status === 'waiting_openbrain') && (
              <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
                <button
                  className="wb-action-btn wb-keep"
                  onClick={() => reviewCandidate(candidate.candidate_id, 'approve')}
                  disabled={busyCandidate === candidate.candidate_id}
                >
                  {busyCandidate === candidate.candidate_id ? '...' : 'Approve'}
                </button>
                <button
                  className="wb-action-btn wb-discard"
                  onClick={() => reviewCandidate(candidate.candidate_id, 'discard')}
                  disabled={busyCandidate === candidate.candidate_id}
                >
                  Discard
                </button>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
