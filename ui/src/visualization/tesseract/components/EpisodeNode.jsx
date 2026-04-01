import { useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import { STATUS_COLOR } from '../scene/color-rules';

/**
 * Spherical episode node positioned in the outer (observed) cube space.
 * Pulses gently when selected.
 */
export default function EpisodeNode({ episode, selected, onClick }) {
  const meshRef = useRef();
  const isSelected = selected?.type === 'episode' && selected?.id === episode.id;
  const color = STATUS_COLOR[episode.status] ?? STATUS_COLOR.default;

  useFrame((_, delta) => {
    if (!meshRef.current) return;
    if (isSelected) {
      meshRef.current.rotation.y += delta * 1.4;
    }
  });

  return (
    <mesh
      ref={meshRef}
      position={episode.observedPosition3D}
      onClick={(e) => {
        e.stopPropagation();
        onClick?.({ type: 'episode', id: episode.id, data: episode });
      }}
      onPointerOver={(e) => {
        e.stopPropagation();
        document.body.style.cursor = 'pointer';
      }}
      onPointerOut={() => {
        document.body.style.cursor = 'default';
      }}
    >
      <sphereGeometry args={[isSelected ? 0.085 : 0.060, 14, 14]} />
      <meshStandardMaterial
        color={color}
        emissive={color}
        emissiveIntensity={isSelected ? 0.9 : 0.38}
        roughness={0.28}
        metalness={0.25}
      />
    </mesh>
  );
}
