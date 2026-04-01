import * as THREE from 'three';

/**
 * A translucent sphere hull surrounding a cluster of episode nodes.
 * Rendered with BackSide so it appears as an enclosing volume.
 */
export default function ClusterHull({ cluster }) {
  return (
    <mesh position={cluster.center}>
      <sphereGeometry args={[cluster.radius, 18, 18]} />
      <meshStandardMaterial
        color={cluster.color}
        transparent
        opacity={0.045}
        side={THREE.BackSide}
        depthWrite={false}
        roughness={1}
        metalness={0}
      />
    </mesh>
  );
}
