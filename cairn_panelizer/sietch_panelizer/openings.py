"""Flag panels that fall inside door / window opening zones.

v1 uses simple angular-wedge x height-band tests against each panel's
centroid. This is intentionally approximate — real arched/curved trim
geometry is a stretch goal — but it is enough to carve a believable
door hole in one lobe and flag candidate window panels.
"""

from __future__ import annotations

import numpy as np

from .config import DomeConfig
from .geometry import angular_half_width_rad, footprint_radius, normalize_angle
from .panel import Panel


def _mark_zone(panels: list[Panel], center_angle_rad: float, half_width_rad: float,
               z_min: float, z_max: float, label: str) -> int:
    count = 0
    for panel in panels:
        cx, cy, cz = panel.centroid
        theta = float(np.arctan2(cy, cx))
        delta = float(normalize_angle(theta - center_angle_rad))
        if abs(delta) <= half_width_rad and z_min <= cz <= z_max:
            panel.is_opening = True
            panel.panel_type = "opening_adjacent"
            panel.mold_family_id = label
            count += 1
    return count


def apply_door_opening(panels: list[Panel], config: DomeConfig) -> int:
    """Flag/remove panels intersecting the door opening. Returns panel count affected."""
    door_angle_rad = np.radians(config.door_angle_deg)
    radius_at_door = float(footprint_radius(np.array(door_angle_rad), config))
    half_width = angular_half_width_rad(config.door_width_mm, radius_at_door)
    return _mark_zone(
        panels,
        center_angle_rad=door_angle_rad,
        half_width_rad=half_width,
        z_min=0.0,
        z_max=config.door_height_mm,
        label="OPENING-DOOR",
    )


def apply_window_openings(panels: list[Panel], config: DomeConfig) -> int:
    """Flag panels intersecting any configured rectangular window zones."""
    total = 0
    for i, window in enumerate(config.windows):
        angle_rad = np.radians(window.get("angle_deg", 0.0))
        radius_at_window = float(footprint_radius(np.array(angle_rad), config))
        half_width = angular_half_width_rad(window.get("width_mm", 0.0), radius_at_window)
        sill = window.get("sill_height_mm", 0.0)
        height = window.get("height_mm", 0.0)
        total += _mark_zone(
            panels,
            center_angle_rad=angle_rad,
            half_width_rad=half_width,
            z_min=sill,
            z_max=sill + height,
            label=f"OPENING-WINDOW-{i}",
        )
    return total


def apply_openings(panels: list[Panel], config: DomeConfig) -> dict[str, int]:
    return {
        "door_panels": apply_door_opening(panels, config),
        "window_panels": apply_window_openings(panels, config),
    }
