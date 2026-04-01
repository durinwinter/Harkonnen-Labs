import { CLUSTER_COLOR } from './color-rules';

/**
 * Group episodes into visual clusters by status family.
 * Returns an array of ClusterRegion objects with center, radius, and color.
 */
export function clusterEpisodes(episodes) {
  if (!episodes.length) return [];

  const groups = {};
  for (const ep of episodes) {
    const key = ep.status ?? 'default';
    if (!groups[key]) groups[key] = [];
    groups[key].push(ep);
  }

  return Object.entries(groups).map(([status, eps]) => {
    const n = eps.length;
    const center = eps.reduce(
      (acc, ep) => [
        acc[0] + ep.observedPosition3D[0] / n,
        acc[1] + ep.observedPosition3D[1] / n,
        acc[2] + ep.observedPosition3D[2] / n,
      ],
      [0, 0, 0],
    );

    const radius = Math.max(
      ...eps.map((ep) =>
        Math.sqrt(
          (ep.observedPosition3D[0] - center[0]) ** 2 +
          (ep.observedPosition3D[1] - center[1]) ** 2 +
          (ep.observedPosition3D[2] - center[2]) ** 2,
        ),
      ),
      0.18,
    ) + 0.18;

    return {
      id: status,
      label: status,
      center,
      radius,
      color: CLUSTER_COLOR[status] ?? CLUSTER_COLOR.default,
    };
  });
}
