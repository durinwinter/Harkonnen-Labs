import React, { useCallback, useEffect, useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

const AUDIT_STATUS = {
  fulfilled: { label: 'Fulfilled', color: '#64c27b', bg: 'rgba(100,194,123,0.12)' },
  partial: { label: 'Partial', color: '#e0b34f', bg: 'rgba(224,179,79,0.14)' },
  missing: { label: 'Missing', color: '#f07f62', bg: 'rgba(240,127,98,0.14)' },
};

const SEVERITY = {
  high: { color: '#f07f62', bg: 'rgba(240,127,98,0.14)' },
  medium: { color: '#e0b34f', bg: 'rgba(224,179,79,0.14)' },
  low: { color: '#64c27b', bg: 'rgba(100,194,123,0.12)' },
};

function Chip({ label, color = '#b8b2a7', bg = 'rgba(255,255,255,0.08)' }) {
  return (
    <span style={{
      padding: '3px 8px',
      borderRadius: 99,
      fontSize: 11,
      fontWeight: 700,
      color,
      background: bg,
      whiteSpace: 'nowrap',
    }}>
      {label}
    </span>
  );
}

function StatTile({ label, value, tone = 'neutral' }) {
  const color = tone === 'warn' ? '#e0b34f' : tone === 'good' ? '#64c27b' : '#d8d3ca';
  return (
    <div style={{
      minWidth: 112,
      padding: '10px 11px',
      border: '1px solid rgba(255,255,255,0.08)',
      borderRadius: 6,
      background: '#191d1f',
    }}>
      <div style={{ fontSize: 11, color: '#8f99a8', marginBottom: 5 }}>{label}</div>
      <div style={{ fontSize: 19, fontWeight: 750, color }}>{value ?? 0}</div>
    </div>
  );
}

function AuditItem({ item }) {
  const style = AUDIT_STATUS[item?.status] || AUDIT_STATUS.missing;
  return (
    <div style={{
      padding: '10px 11px',
      borderRadius: 7,
      border: '1px solid rgba(255,255,255,0.08)',
      background: '#191d1f',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, marginBottom: 7 }}>
        <div style={{ color: '#d8d3ca', fontSize: 13, fontWeight: 750 }}>{item?.item_id || 'audit item'}</div>
        <Chip label={style.label} color={style.color} bg={style.bg} />
      </div>
      <div style={{ color: '#c9c1b4', fontSize: 12, lineHeight: 1.45, marginBottom: 6 }}>
        {item?.requirement || 'No requirement text recorded.'}
      </div>
      <div style={{ color: '#8f99a8', fontSize: 11, lineHeight: 1.45 }}>
        {item?.note || 'No note recorded.'}
      </div>
      {item?.evidence_refs?.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5, marginTop: 8 }}>
          {item.evidence_refs.map((ref) => (
            <span
              key={ref}
              style={{
                color: '#b8b2a7',
                background: 'rgba(255,255,255,0.06)',
                borderRadius: 5,
                padding: '2px 6px',
                fontSize: 11,
              }}
            >
              {ref}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function LearningRecord({ record }) {
  const severity = SEVERITY[record?.severity] || SEVERITY.low;
  const files = record?.files || [];
  return (
    <div style={{
      padding: '10px 11px',
      borderRadius: 7,
      border: '1px solid rgba(255,255,255,0.08)',
      background: '#191d1f',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, marginBottom: 7 }}>
        <div style={{ color: '#d8d3ca', fontSize: 13, fontWeight: 750 }}>
          {record?.finding_fingerprint || record?.record_id || 'review learning'}
        </div>
        <Chip label={record?.severity || 'low'} color={severity.color} bg={severity.bg} />
      </div>
      <div style={{ color: '#c9c1b4', fontSize: 12, lineHeight: 1.45, marginBottom: 7 }}>
        {record?.lesson || 'No lesson extracted.'}
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
        <Chip label={record?.resolution || 'recorded'} />
        {record?.source_agent && <Chip label={record.source_agent} />}
        {record?.reviewer_agent && <Chip label={record.reviewer_agent} />}
      </div>
      {files.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5, marginTop: 8 }}>
          {files.slice(0, 6).map((file) => (
            <span
              key={file}
              style={{
                color: '#b8b2a7',
                background: 'rgba(255,255,255,0.06)',
                borderRadius: 5,
                padding: '2px 6px',
                fontSize: 11,
              }}
            >
              {file}
            </span>
          ))}
          {files.length > 6 && (
            <span style={{ color: '#8f99a8', fontSize: 11 }}>+{files.length - 6}</span>
          )}
        </div>
      )}
    </div>
  );
}

export default function RunReviewPanel({ runId }) {
  const [audit, setAudit] = useState(null);
  const [learning, setLearning] = useState(null);
  const [context, setContext] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    if (!runId) return;
    setLoading(true);
    setError('');
    try {
      const [auditResponse, learningResponse, contextResponse] = await Promise.all([
        fetch(`${API_BASE}/runs/${runId}/plan-completion-audit`),
        fetch(`${API_BASE}/runs/${runId}/code-review-learning`),
        fetch(`${API_BASE}/runs/${runId}/context-utilization`),
      ]);
      if (!auditResponse.ok) throw new Error(`audit ${auditResponse.status} ${auditResponse.statusText}`);
      if (!learningResponse.ok) throw new Error(`review ${learningResponse.status} ${learningResponse.statusText}`);
      if (!contextResponse.ok) throw new Error(`context ${contextResponse.status} ${contextResponse.statusText}`);
      const [auditJson, learningJson, contextJson] = await Promise.all([
        auditResponse.json(),
        learningResponse.json(),
        contextResponse.json(),
      ]);
      setAudit(auditJson.audit || null);
      setLearning(learningJson);
      setContext(contextJson);
    } catch (err) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }, [runId]);

  useEffect(() => { load(); }, [load]);

  const items = audit?.items || [];
  const unresolved = audit?.unresolved_count ?? items.filter((item) => item.status !== 'fulfilled').length;
  const records = learning?.records || [];
  const statusTone = unresolved > 0 ? 'warn' : audit ? 'good' : 'neutral';
  const pullRecords = context?.pull_records || [];
  const contextSummary = context?.summary || {};

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {error && (
        <div style={{
          padding: '10px 12px',
          borderRadius: 7,
          background: 'rgba(240,127,98,0.1)',
          border: '1px solid rgba(240,127,98,0.24)',
          color: '#f4b1a2',
          fontSize: 12,
        }}>
          Error: {error}
        </div>
      )}

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        <StatTile label="Audit items" value={items.length} tone={audit ? 'neutral' : 'warn'} />
        <StatTile label="Unresolved" value={unresolved} tone={statusTone} />
        <StatTile label="Review records" value={learning?.total ?? records.length} tone={records.length ? 'good' : 'neutral'} />
        <StatTile label="Memory pulls" value={contextSummary.mid_task_pull_count ?? pullRecords.length} tone={pullRecords.length ? 'good' : 'neutral'} />
        <StatTile
          label="Utilization"
          value={`${Math.round((contextSummary.utilization_rate ?? 0) * 100)}%`}
          tone={contextSummary.utilization_status === 'low' ? 'warn' : contextSummary.utilization_status === 'healthy' ? 'good' : 'neutral'}
        />
      </div>

      <section style={{
        border: '1px solid rgba(255,255,255,0.08)',
        background: '#151819',
        borderRadius: 8,
        padding: '11px',
      }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, marginBottom: 9 }}>
          <div>
            <div style={{ color: '#d8d3ca', fontSize: 14, fontWeight: 780 }}>Plan completion audit</div>
            <div style={{ color: '#8f99a8', fontSize: 12, marginTop: 3 }}>
              {audit?.summary || (loading ? 'Loading...' : 'No audit artifact recorded for this run.')}
            </div>
          </div>
          {audit?.final_status && <Chip label={audit.final_status} />}
        </div>
        {items.length > 0 && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {items.slice(0, 10).map((item) => (
              <AuditItem key={item.item_id} item={item} />
            ))}
            {items.length > 10 && (
              <div style={{ color: '#8f99a8', fontSize: 12 }}>+{items.length - 10} more audit items</div>
            )}
          </div>
        )}
      </section>

      <section style={{
        border: '1px solid rgba(255,255,255,0.08)',
        background: '#151819',
        borderRadius: 8,
        padding: '11px',
      }}>
        <div style={{ color: '#d8d3ca', fontSize: 14, fontWeight: 780, marginBottom: 9 }}>
          Context utilization
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginBottom: 10 }}>
          <Chip label={`briefing hits ${contextSummary.briefing_hits_provided ?? 0}`} />
          <Chip label={`briefing tokens ${contextSummary.briefing_tokens_used ?? 0}`} />
          <Chip label={`pull tokens ${contextSummary.mid_task_pull_tokens ?? 0}`} />
          <Chip label={contextSummary.utilization_status || 'no_briefing'} />
        </div>
        {pullRecords.length === 0 ? (
          <div style={{ color: '#8f99a8', fontSize: 12 }}>
            {loading ? 'Loading...' : 'No run-scoped memory_pull calls recorded yet.'}
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {pullRecords.slice(0, 8).map((record) => (
              <div
                key={record.pull_id}
                style={{
                  padding: '10px 11px',
                  borderRadius: 7,
                  border: '1px solid rgba(255,255,255,0.08)',
                  background: '#191d1f',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, marginBottom: 6 }}>
                  <div style={{ color: '#d8d3ca', fontSize: 13, fontWeight: 750 }}>{record.query}</div>
                  <Chip label={record.scope || 'general'} />
                </div>
                <div style={{ color: '#8f99a8', fontSize: 11, marginBottom: 7 }}>
                  {record.hits_returned} hit(s) · {record.tokens_returned}/{record.max_tokens} tokens
                </div>
                {record.hit_previews?.length > 0 && (
                  <div style={{ color: '#c9c1b4', fontSize: 12, lineHeight: 1.45 }}>
                    {record.hit_previews[0]}
                  </div>
                )}
              </div>
            ))}
            {pullRecords.length > 8 && (
              <div style={{ color: '#8f99a8', fontSize: 12 }}>+{pullRecords.length - 8} more memory pulls</div>
            )}
          </div>
        )}
      </section>

      <section style={{
        border: '1px solid rgba(255,255,255,0.08)',
        background: '#151819',
        borderRadius: 8,
        padding: '11px',
      }}>
        <div style={{ color: '#d8d3ca', fontSize: 14, fontWeight: 780, marginBottom: 9 }}>
          Code-review learning
        </div>
        {records.length === 0 ? (
          <div style={{ color: '#8f99a8', fontSize: 12 }}>
            {loading ? 'Loading...' : 'No review-learning records for this run.'}
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {records.slice(0, 8).map((record) => (
              <LearningRecord key={record.record_id} record={record} />
            ))}
            {records.length > 8 && (
              <div style={{ color: '#8f99a8', fontSize: 12 }}>+{records.length - 8} more review records</div>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
