"""Dome configuration: defaults, dataclass, and YAML loading.

All linear dimensions are in millimeters; all angles in the config file are
degrees and are converted to radians at load time where needed.
"""

from __future__ import annotations

from dataclasses import dataclass, field, fields
from pathlib import Path
from typing import Any

import yaml


@dataclass
class DomeConfig:
    overall_width_mm: float = 6000.0
    height_mm: float = 3200.0
    lobe_amplitude: float = 0.18
    lobe_count: int = 3
    angular_segments: int = 72
    ring_segments: int = 12
    crown_radius_mm: float = 700.0
    wall_thickness_mm: float = 120.0

    door_width_mm: float = 900.0
    door_height_mm: float = 2050.0
    door_angle_deg: float = 180.0

    # Each window: {"angle_deg": float, "width_mm": float, "height_mm": float, "sill_height_mm": float}
    windows: list[dict[str, float]] = field(default_factory=list)

    family_tolerance_mm: float = 2.0

    panel_mode: str = "triangle"  # "triangle" or "quad"

    @property
    def base_radius_mm(self) -> float:
        """Radius of the undeformed footprint circle (lobes oscillate around this)."""
        return self.overall_width_mm / 2.0

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "DomeConfig":
        known = {f.name for f in fields(cls)}
        filtered = {k: v for k, v in data.items() if k in known}
        return cls(**filtered)

    @classmethod
    def from_yaml(cls, path: str | Path) -> "DomeConfig":
        with open(path, "r", encoding="utf-8") as fh:
            data = yaml.safe_load(fh) or {}
        return cls.from_dict(data)
