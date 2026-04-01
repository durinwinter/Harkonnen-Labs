import { useRef } from 'react';
import { useFrame } from '@react-three/fiber';
import { CAUSE_COLOR } from '../scene/color-rules';

/**
 * Octahedral (diamond) node positioned in the inner (inferred) cube space.
 * Represents a causal concept inferred by Coobie.
 */
export default function CauseNode({ cause, selected, onClick }) {
  const meshRef = useRef();
  const isSelected = selected?.type === 'cause' && selected?.id === cause.id;
  const color = CAUSE_COLOR[cause.type] ?? CAUSE_COLOR.default;

  useFrame((_, delta) => {
    if (!meshRef.current) return;
    // Slow constant spin — causes are always "live" in Coobie's model
    meshRef.current.rotation.y += delta * (isSelected ? 1.8 : 0.5);
    meshRef.current.rotation.x += delta * 0.22;
  });

  return (
    <mesh
      ref={meshRef}
      position={cause.position3D}
      onClick={(e) => {
        e.stopPropagation();
        onClick?.({ type: 'cause', id: cause.id, data: cause });
      }}
      onPointerOver={(e) => {
        e.stopPropagation();
        document.body.style.cursor = 'pointer';
      }}
      onPointerOut={() => {
        document.body.style.cursor = 'default';
      }}
    >
      <octahedronGeometry args={[isSelected ? 0.092 : 0.068, 0]} />
      <meshStandardMaterial
        color={color}
        emissive={color}
        emissiveIntensity={isSelected ? 1.0 : 0.55}
        roughness={0.18}
        metalness={0.42}
        transparent
        opacity={0.88}
      />
    </mesh>
  );
}
