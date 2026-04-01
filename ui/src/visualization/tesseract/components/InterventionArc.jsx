import { useMemo, useEffect } from 'react';
import * as THREE from 'three';

/**
 * A curved arc connecting an observed episode node (outer cube) to its
 * inferred cause node (inner cube).
 *
 * Uses a quadratic Bézier curve with a lift point above the midpoint,
 * rendered as a lineSegments geometry for performance.
 */
export default function InterventionArc({ from, to, confidence = 0.5, selected }) {
  const geo = useMemo(() => {
    const [fx, fy, fz] = from;
    const [tx, ty, tz] = to;

    // Lift the control point above the midpoint to create an arc
    const lift = 0.22;
    const mid = [
      (fx + tx) / 2 + (ty - fy) * 0.15,
      (fy + ty) / 2 + lift,
      (fz + tz) / 2,
    ];

    const STEPS = 16;
    const positions = new Float32Array((STEPS + 1) * 3);

    for (let i = 0; i <= STEPS; i++) {
      const t  = i / STEPS;
      const t1 = 1 - t;
      positions[i * 3]     = t1 * t1 * fx + 2 * t1 * t * mid[0] + t * t * tx;
      positions[i * 3 + 1] = t1 * t1 * fy + 2 * t1 * t * mid[1] + t * t * ty;
      positions[i * 3 + 2] = t1 * t1 * fz + 2 * t1 * t * mid[2] + t * t * tz;
    }

    // lineSegments needs pairs, so duplicate as sequential pairs
    const segBuf = new Float32Array(STEPS * 6);
    for (let i = 0; i < STEPS; i++) {
      segBuf[i * 6]     = positions[i * 3];
      segBuf[i * 6 + 1] = positions[i * 3 + 1];
      segBuf[i * 6 + 2] = positions[i * 3 + 2];
      segBuf[i * 6 + 3] = positions[(i + 1) * 3];
      segBuf[i * 6 + 4] = positions[(i + 1) * 3 + 1];
      segBuf[i * 6 + 5] = positions[(i + 1) * 3 + 2];
    }

    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(segBuf, 3));
    return g;
  }, [from, to]);

  useEffect(() => () => geo.dispose(), [geo]);

  const opacity = selected ? 0.85 : 0.18 + confidence * 0.50;
  const color   = selected ? '#ffffff' : '#8a6ab0';

  return (
    <lineSegments geometry={geo}>
      <lineBasicMaterial
        color={color}
        transparent
        opacity={opacity}
        depthWrite={false}
      />
    </lineSegments>
  );
}
