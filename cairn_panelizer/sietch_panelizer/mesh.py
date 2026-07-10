"""Convert a dome vertex grid into a faceted trimesh.Trimesh shell."""

from __future__ import annotations

import numpy as np
import trimesh

from .config import DomeConfig
from .geometry import generate_dome_grid


def grid_to_mesh(grid: np.ndarray, config: DomeConfig) -> trimesh.Trimesh:
    """Triangulate the (rings+1, theta, 3) grid into a watertight-ish open shell.

    The angular dimension wraps around (no seam), the crown ring is left open
    (skylight), and the base ring is left open (the shell sits on a foundation).
    """
    n_rings_plus1, n_theta, _ = grid.shape
    n_rings = n_rings_plus1 - 1

    vertices = grid.reshape(-1, 3)

    def vid(ring: int, seg: int) -> int:
        return ring * n_theta + (seg % n_theta)

    faces = []
    for ring in range(n_rings):
        for seg in range(n_theta):
            v00 = vid(ring, seg)
            v01 = vid(ring, seg + 1)
            v10 = vid(ring + 1, seg)
            v11 = vid(ring + 1, seg + 1)

            if config.panel_mode == "quad":
                # trimesh triangulates quads on load; keep explicit triangles
                # for a consistent, predictable panel count either way.
                faces.append([v00, v01, v11])
                faces.append([v00, v11, v10])
            else:
                faces.append([v00, v01, v11])
                faces.append([v00, v11, v10])

    mesh = trimesh.Trimesh(vertices=vertices, faces=np.array(faces), process=False)
    return mesh


def build_dome_mesh(config: DomeConfig) -> trimesh.Trimesh:
    grid = generate_dome_grid(config)
    return grid_to_mesh(grid, config)
