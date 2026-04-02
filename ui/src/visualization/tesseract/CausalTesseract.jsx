import { useState, useEffect, Suspense } from 'react';
import { Canvas } from '@react-three/fiber';
import { OrbitControls, Stars } from '@react-three/drei';

import TesseractFrame    from './components/TesseractFrame';
import EpisodeNode       from './components/EpisodeNode';
import CauseNode         from './components/CauseNode';
import InterventionArc   from './components/InterventionArc';
import ClusterHull       from './components/ClusterHull';
import DetailPanel       from './components/DetailPanel';
import { useLensMode, LENS_MODES } from './hooks/useLensMode';
import { useSceneSelection } from './hooks/useSceneSelection';
import { EXAMPLE_SCENE } from './scene/scene-builder';

const API_BASE = import.meta.env.VITE_API_BASE || 'http://127.0.0.1:3057/api';

/**
 * The Rust handler serialises with serde rename_all = "camelCase" and
 * explicit #[serde(rename = "observedPosition3D")] / "inferredPosition3D" /
 * "position3D".  Map any remaining snake_case fields that the Three.js
 * components reference by their JS-side names.
 */
function normalizeScene(raw) {
  const episodeNodes = (raw.episodeNodes ?? []).map((ep) => ({
    ...ep,
    // positions arrive as arrays [x,y,z] — keep as-is
    observedPosition3D: ep.observedPosition3D ?? ep.observed_position_3d ?? [0, 0, 0],
    inferredPosition3D: ep.inferredPosition3D ?? ep.inferred_position_3d ?? [0, 0, 0],
    primaryCauseType:   ep.primaryCauseType   ?? ep.primary_cause_type   ?? 'context_gap',
    primaryCauseText:   ep.primaryCauseText   ?? ep.primary_cause_text   ?? null,
    rawScores:          ep.rawScores          ?? ep.raw_scores           ?? null,
    interventionPotential: ep.interventionPotential ?? ep.intervention_potential ?? 0.5,
    interventions:      ep.interventions      ?? [],
    contributingCauses: ep.contributingCauses ?? ep.contributing_causes  ?? [],
  }));

  const causeNodes = (raw.causeNodes ?? []).map((c) => ({
    ...c,
    position3D:        c.position3D  ?? c.position_3d  ?? [0, 0, 0],
    causeType:         c.causeType   ?? c.cause_type   ?? c.type ?? 'context_gap',
    type:              c.causeType   ?? c.cause_type   ?? c.type ?? 'context_gap',
    supportingRunIds:  c.supportingRunIds ?? c.supporting_run_ids ?? [],
    interventions:     c.interventions   ?? [],
  }));

  return {
    episodeNodes,
    causeNodes,
    interventionNodes: raw.interventionNodes ?? [],
    edges:    raw.edges    ?? [],
    clusters: raw.clusters ?? [],
  };
}

/** Ambient + fill lighting for the scene. */
function SceneLights() {
  return (
    <>
      <ambientLight intensity={0.12} />
      <pointLight position={[3.5, 3.5, 3.5]}  intensity={0.9} color="#ffffff" />
      <pointLight position={[-3, -2.5, -2]}    intensity={0.5} color="#5a8acc" />
      <pointLight position={[0, -3, 2]}        intensity={0.3} color="#8a6ab0" />
    </>
  );
}

export default function CausalTesseract({ onClose }) {
  const [scene, setScene]       = useState(null);
  const [loading, setLoading]   = useState(true);
  const [isDemo, setIsDemo]     = useState(false);

  const { lensMode, setLensMode } = useLensMode('memory');
  const { selected, toggle, clear } = useSceneSelection();

  // ── Data pipeline ─────────────────────────────────────────────────────────
  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      try {
        const scene = await fetch(`${API_BASE}/tesseract/scene`).then((r) => {
          if (!r.ok) throw new Error(`${r.status}`);
          return r.json();
        });
        if (!cancelled) {
          // Normalize camelCase field names from Rust serde output
          const normalized = normalizeScene(scene);
          setScene(normalized.episodeNodes.length > 0 ? normalized : EXAMPLE_SCENE);
          setIsDemo(normalized.episodeNodes.length === 0);
          setLoading(false);
        }
      } catch {
        if (!cancelled) {
          setScene(EXAMPLE_SCENE);
          setIsDemo(true);
          setLoading(false);
        }
      }
    };

    load();
    return () => { cancelled = true; };
  }, []);

  // ── Keyboard shortcuts ────────────────────────────────────────────────────
  useEffect(() => {
    const handler = (e) => {
      if (e.key === 'Escape') onClose?.();
      if (e.key === 'Tab') {
        e.preventDefault();
        const next = LENS_MODES[(LENS_MODES.indexOf(lensMode) + 1) % LENS_MODES.length];
        setLensMode(next);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [lensMode, onClose, setLensMode]);

  return (
    <div className="tesseract-overlay">

      {/* ── Header ─────────────────────────────────────────────────────── */}
      <div className="tesseract-header">
        <div className="tesseract-title-block">
          <span className="tesseract-eyebrow">Coobie Observatory</span>
          <span className="tesseract-title">Causal Tesseract</span>
          {isDemo && <span className="demo-badge">demo data</span>}
        </div>

        <div className="lens-controls">
          {LENS_MODES.map((mode) => (
            <button
              key={mode}
              className={`lens-btn ${lensMode === mode ? 'active' : ''}`}
              onClick={() => setLensMode(mode)}
              title={`${mode} lens (Tab to cycle)`}
            >
              {mode}
            </button>
          ))}
        </div>

        <div className="tesseract-legend">
          <span className="legend-item outer">outer cube = observed</span>
          <span className="legend-item inner">inner cube = inferred</span>
          <span className="legend-item conn">arcs = causal trail</span>
        </div>

        <button className="tesseract-back" onClick={onClose} title="Back to Pack Board (Esc)">← Pack Board</button>
        <button className="tesseract-close" onClick={onClose} title="Close (Esc)">✕</button>
      </div>

      {/* ── Body ───────────────────────────────────────────────────────── */}
      <div className="tesseract-body">

        {/* Canvas */}
        <div className="tesseract-canvas-wrap">
          {loading ? (
            <div className="tesseract-loading">loading causal memory…</div>
          ) : (
            <Canvas
              camera={{ position: [0, 0, 5.5], fov: 52 }}
              onPointerMissed={clear}
              gl={{ antialias: true, alpha: true }}
            >
              <SceneLights />

              <Stars radius={22} depth={12} count={900} factor={0.75} saturation={0} fade />

              <Suspense fallback={null}>
                <TesseractFrame lensMode={lensMode} />

                {scene?.clusters?.map((cluster) => (
                  <ClusterHull key={cluster.id} cluster={cluster} />
                ))}

                {scene?.edges?.map((edge) => {
                  const from = scene.episodeNodes.find((e) => e.id === edge.sourceId);
                  const to   = scene.causeNodes?.find((c) => c.id === edge.targetId);
                  if (!from || !to) return null;
                  const isHighlighted =
                    selected?.id === from.id || selected?.id === to.id;
                  return (
                    <InterventionArc
                      key={edge.id}
                      from={from.observedPosition3D}
                      to={to.position3D}
                      confidence={edge.confidence}
                      selected={isHighlighted}
                    />
                  );
                })}

                {scene?.causeNodes?.map((cause) => (
                  <CauseNode
                    key={cause.id}
                    cause={cause}
                    selected={selected}
                    onClick={toggle}
                  />
                ))}

                {scene?.episodeNodes?.map((ep) => (
                  <EpisodeNode
                    key={ep.id}
                    episode={ep}
                    selected={selected}
                    onClick={toggle}
                  />
                ))}
              </Suspense>

              <OrbitControls
                enablePan={false}
                minDistance={2.8}
                maxDistance={11}
                autoRotate={!selected}
                autoRotateSpeed={0.28}
                dampingFactor={0.08}
                enableDamping
              />
            </Canvas>
          )}
        </div>

        {/* Detail panel */}
        <DetailPanel
          selected={selected}
          scene={scene}
          lensMode={lensMode}
          onClose={clear}
        />
      </div>

      {/* ── Footer hint ────────────────────────────────────────────────── */}
      <div className="tesseract-footer">
        <span>sphere = episode · diamond = cause · Tab cycles lens</span>
        {scene && (
          <span>
            {scene.episodeNodes.length} episodes · {scene.causeNodes.length} causes · {scene.clusters.length} clusters
          </span>
        )}
      </div>

      <style jsx>{`
        .tesseract-overlay {
          position: fixed;
          inset: 0;
          z-index: 2000;
          background: #0d0f11;
          display: flex;
          flex-direction: column;
          color: var(--text-primary);
        }

        .tesseract-header {
          display: flex;
          align-items: center;
          gap: 1.2rem;
          padding: 0.7rem 1.2rem;
          border-bottom: 1px solid rgba(255, 255, 255, 0.07);
          background: rgba(18, 20, 22, 0.9);
          flex-shrink: 0;
          flex-wrap: wrap;
        }

        .tesseract-title-block {
          display: flex;
          align-items: baseline;
          gap: 0.65rem;
        }

        .tesseract-eyebrow {
          font-size: 0.62rem;
          font-weight: 800;
          text-transform: uppercase;
          letter-spacing: 0.16em;
          color: var(--accent-gold);
        }

        .tesseract-title {
          font-size: 1.05rem;
          font-weight: 800;
          letter-spacing: 0.04em;
        }

        .demo-badge {
          font-size: 0.62rem;
          background: rgba(194, 163, 114, 0.15);
          border: 1px solid rgba(194, 163, 114, 0.35);
          color: var(--accent-gold);
          border-radius: 999px;
          padding: 0.15rem 0.5rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.08em;
        }

        .lens-controls {
          display: flex;
          gap: 0.3rem;
        }

        .lens-btn {
          padding: 0.35rem 0.8rem;
          background: rgba(255, 255, 255, 0.04);
          border: 1px solid rgba(255, 255, 255, 0.1);
          border-radius: 999px;
          color: var(--text-secondary);
          font-size: 0.7rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          cursor: pointer;
          transition: color 0.15s, border-color 0.15s, background 0.15s;
        }

        .lens-btn:hover {
          color: var(--text-primary);
          border-color: rgba(255, 255, 255, 0.2);
        }

        .lens-btn.active {
          color: var(--accent-gold);
          border-color: rgba(194, 163, 114, 0.5);
          background: rgba(194, 163, 114, 0.08);
        }

        .tesseract-legend {
          display: flex;
          gap: 0.85rem;
          margin-left: auto;
        }

        .legend-item {
          font-size: 0.64rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.07em;
        }

        .legend-item.outer { color: #5a8acc; }
        .legend-item.inner { color: #c2a372; }
        .legend-item.conn  { color: #8a6ab0; }

        .tesseract-back {
          padding: 0.35rem 0.85rem;
          background: rgba(255, 255, 255, 0.05);
          border: 1px solid rgba(255, 255, 255, 0.14);
          border-radius: 999px;
          color: var(--text-secondary);
          font-size: 0.7rem;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.08em;
          cursor: pointer;
          transition: color 0.15s, border-color 0.15s, background 0.15s;
          flex-shrink: 0;
        }

        .tesseract-back:hover {
          color: var(--text-primary);
          border-color: rgba(255, 255, 255, 0.3);
          background: rgba(255, 255, 255, 0.09);
        }

        .tesseract-close {
          background: none;
          border: 1px solid rgba(255, 255, 255, 0.12);
          color: var(--text-secondary);
          border-radius: 50%;
          width: 30px;
          height: 30px;
          cursor: pointer;
          font-size: 0.82rem;
          display: flex;
          align-items: center;
          justify-content: center;
          flex-shrink: 0;
          margin-left: 0.5rem;
        }

        .tesseract-close:hover {
          color: var(--text-primary);
          border-color: rgba(255, 255, 255, 0.3);
        }

        .tesseract-body {
          flex: 1;
          display: flex;
          min-height: 0;
          overflow: hidden;
        }

        .tesseract-canvas-wrap {
          flex: 1;
          min-width: 0;
          position: relative;
        }

        .tesseract-loading {
          position: absolute;
          inset: 0;
          display: flex;
          align-items: center;
          justify-content: center;
          color: var(--text-secondary);
          font-size: 0.84rem;
          font-style: italic;
          letter-spacing: 0.08em;
        }

        .tesseract-footer {
          display: flex;
          justify-content: space-between;
          padding: 0.45rem 1.2rem;
          font-size: 0.68rem;
          color: var(--text-secondary);
          border-top: 1px solid rgba(255, 255, 255, 0.06);
          background: rgba(18, 20, 22, 0.85);
          flex-shrink: 0;
          font-family: var(--font-mono);
        }
      `}</style>
    </div>
  );
}
