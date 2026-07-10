"""Parametric trillium dome surface math.

The footprint is a polar curve with three (configurable) lobes:

    r(theta) = base_radius * (1 + lobe_amplitude * cos(lobe_count * theta))

The shell rises from this lobed footprint at the base to a circular crown
opening at the top. A simple quarter-sine profile drives both the radial
contraction toward the crown and the height rise, which keeps the surface
smooth and avoids fragile curve-fitting for a first prototype.
"""

from __future__ import annotations

import numpy as np

from .config import DomeConfig


def footprint_radius(theta: np.ndarray, config: DomeConfig) -> np.ndarray:
    """Radius of the lobed base footprint at angle(s) theta (radians)."""
    return config.base_radius_mm * (
        1.0 + config.lobe_amplitude * np.cos(config.lobe_count * theta)
    )


def ring_profile(t: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Quarter-sine dome profile.

    Returns (radial_scale, height_scale) for normalized ring height t in [0, 1].
    radial_scale goes 1 -> 0 (full footprint at base, crown circle at top).
    height_scale goes 0 -> 1 (base elevation to full dome height).
    """
    radial_scale = np.cos(t * np.pi / 2.0)
    height_scale = np.sin(t * np.pi / 2.0)
    return radial_scale, height_scale


def generate_dome_grid(config: DomeConfig) -> np.ndarray:
    """Generate the dome surface as a (rings+1, angular_segments, 3) vertex grid.

    Ring 0 is the base perimeter (full lobed footprint, z=0).
    The last ring is the crown opening (circular, radius = crown_radius_mm,
    z = height_mm). The crown ring is intentionally left uncapped so it forms
    the central skylight opening.
    """
    n_rings = config.ring_segments
    n_theta = config.angular_segments

    theta = np.linspace(0.0, 2.0 * np.pi, n_theta, endpoint=False)
    fr = footprint_radius(theta, config)

    grid = np.empty((n_rings + 1, n_theta, 3), dtype=float)

    for i in range(n_rings + 1):
        t = i / n_rings
        radial_scale, height_scale = ring_profile(np.array(t))
        ring_radius = config.crown_radius_mm + (fr - config.crown_radius_mm) * radial_scale
        z = config.height_mm * height_scale

        grid[i, :, 0] = ring_radius * np.cos(theta)
        grid[i, :, 1] = ring_radius * np.sin(theta)
        grid[i, :, 2] = z

    return grid


def angular_half_width_rad(arc_width_mm: float, radius_mm: float) -> float:
    """Approximate angular half-width (radians) subtended by a flat opening width."""
    return (arc_width_mm / 2.0) / max(radius_mm, 1.0)


def normalize_angle(theta: np.ndarray | float) -> np.ndarray | float:
    """Wrap angle(s) into [-pi, pi)."""
    return (np.asarray(theta) + np.pi) % (2.0 * np.pi) - np.pi
