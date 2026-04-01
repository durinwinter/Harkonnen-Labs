import { useRef, useMemo, useEffect } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import {
  TESSERACT_VERTICES_4D,
  OUTER_EDGES,
  INNER_EDGES,
  CONNECTOR_EDGES,
  rotateXW,
  rotateYW,
  project4Dto3D,
} from '../scene/projection';
import { LENS_EDGE } from '../scene/color-rules';

/** Build a Float32Array of vertex pairs for a lineSegments geometry. */
function buildPositionBuffer(edges, projected) {
  const buf = new Float32Array(edges.length * 6);
  edges.forEach(([a, b], i) => {
    const pa = projected[a];
    const pb = projected[b];
    buf[i * 6]     = pa[0]; buf[i * 6 + 1] = pa[1]; buf[i * 6 + 2] = pa[2];
    buf[i * 6 + 3] = pb[0]; buf[i * 6 + 4] = pb[1]; buf[i * 6 + 5] = pb[2];
  });
  return buf;
}

/**
 * Animated 4D tesseract wireframe projected to 3D.
 *
 * Three separate lineSegments meshes for outer / inner / connector edges
 * let us colour and fade each group independently per lens mode.
 */
export default function TesseractFrame({ lensMode = 'memory' }) {
  const angleRef = useRef(0.52); // start at a visually interesting angle

  const outerGeo     = useMemo(() => new THREE.BufferGeometry(), []);
  const innerGeo     = useMemo(() => new THREE.BufferGeometry(), []);
  const connectorGeo = useMemo(() => new THREE.BufferGeometry(), []);

  // Dispose geometries on unmount
  useEffect(() => () => { outerGeo.dispose(); innerGeo.dispose(); connectorGeo.dispose(); }, []);

  // Animate in useFrame — update geometry positions every tick
  useFrame((_, delta) => {
    angleRef.current += delta * 0.16;
    const a  = angleRef.current;
    const a2 = a * 0.29; // second rotation plane — slower

    const projected = TESSERACT_VERTICES_4D.map((v) => {
      const r1 = rotateXW(v, a);
      const r2 = rotateYW(r1, a2);
      return project4Dto3D(r2, 2.4);
    });

    outerGeo.setAttribute(
      'position',
      new THREE.BufferAttribute(buildPositionBuffer(OUTER_EDGES, projected), 3),
    );
    innerGeo.setAttribute(
      'position',
      new THREE.BufferAttribute(buildPositionBuffer(INNER_EDGES, projected), 3),
    );
    connectorGeo.setAttribute(
      'position',
      new THREE.BufferAttribute(buildPositionBuffer(CONNECTOR_EDGES, projected), 3),
    );
  });

  const edgeStyle = LENS_EDGE[lensMode] ?? LENS_EDGE.memory;

  return (
    <group>
      <lineSegments geometry={outerGeo}>
        <lineBasicMaterial
          color={edgeStyle.outer.color}
          transparent
          opacity={edgeStyle.outer.opacity}
          depthWrite={false}
        />
      </lineSegments>

      <lineSegments geometry={innerGeo}>
        <lineBasicMaterial
          color={edgeStyle.inner.color}
          transparent
          opacity={edgeStyle.inner.opacity}
          depthWrite={false}
        />
      </lineSegments>

      <lineSegments geometry={connectorGeo}>
        <lineBasicMaterial
          color={edgeStyle.connector.color}
          transparent
          opacity={edgeStyle.connector.opacity}
          depthWrite={false}
        />
      </lineSegments>
    </group>
  );
}
