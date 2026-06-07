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
class MoldConfig:
    enabled: bool = True
    default_type: str = "flat_faceted_mold"
    material: str = "hdpe"

    panel_thickness_mm: float = 12.0
    edge_dam_height_mm: float = 25.0
    bevel_angle_deg: float = 5.0
    draft_angle_deg: float = 3.0

    registration_hole_diameter_mm: float = 8.0
    registration_hole_inset_mm: float = 30.0

    demold_slot_width_mm: float = 12.0
    demold_slot_length_mm: float = 40.0

    insert_locator_diameter_mm: float = 6.0

    edge_clearance_mm: float = 2.0
    flange_width_mm: float = 75.0
    minimum_mold_border_mm: float = 100.0

    label_prefix: str = "CAIRN"

    @classmethod
    def from_dict(cls, data: dict[str, Any] | None) -> "MoldConfig":
        known = {f.name for f in fields(cls)}
        filtered = {k: v for k, v in (data or {}).items() if k in known}
        return cls(**filtered)


@dataclass
class NestingConfig:
    sheet_width_mm: float = 1500.0
    sheet_height_mm: float = 3000.0
    margin_mm: float = 20.0
    spacing_mm: float = 10.0

    @classmethod
    def from_dict(cls, data: dict[str, Any] | None) -> "NestingConfig":
        known = {f.name for f in fields(cls)}
        filtered = {k: v for k, v in (data or {}).items() if k in known}
        return cls(**filtered)


@dataclass
class LayerSpec:
    """One layer of the Cairn material stack assigned to every fabricated panel."""

    name: str
    thickness_mm: float
    recipe_id: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "LayerSpec":
        return cls(
            name=str(data["name"]),
            thickness_mm=float(data.get("thickness_mm", 0.0)),
            recipe_id=str(data.get("recipe_id", "")),
        )


@dataclass
class ManufacturingConfig:
    cnc_bed_width_mm: float = 1200.0
    cnc_bed_height_mm: float = 2400.0
    labor_rate_usd_hr: float = 45.0
    waste_factor_pct: float = 8.0

    @classmethod
    def from_dict(cls, data: dict[str, Any] | None) -> "ManufacturingConfig":
        known = {f.name for f in fields(cls)}
        filtered = {k: v for k, v in (data or {}).items() if k in known}
        return cls(**filtered)


@dataclass
class LoadsConfig:
    """Preliminary load assumptions for the structural pre-check (sec. 4.6). Not a code-compliance input."""

    snow_load_psf: float = 40.0
    wind_speed_mph: float = 120.0
    dead_load_psf: float = 15.0
    seismic_category: str = "placeholder"
    safety_factor_target: float = 2.5

    @classmethod
    def from_dict(cls, data: dict[str, Any] | None) -> "LoadsConfig":
        known = {f.name for f in fields(cls)}
        filtered = {k: v for k, v in (data or {}).items() if k in known}
        return cls(**filtered)


@dataclass
class CureConfig:
    ambient_temp_C: float = 22.0
    ambient_rh_pct: float = 50.0
    target_handling_strength_pct: float = 40.0

    @classmethod
    def from_dict(cls, data: dict[str, Any] | None) -> "CureConfig":
        known = {f.name for f in fields(cls)}
        filtered = {k: v for k, v in (data or {}).items() if k in known}
        return cls(**filtered)


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

    mold: MoldConfig = field(default_factory=MoldConfig)
    nesting: NestingConfig = field(default_factory=NestingConfig)

    # Material system: ordered Cairn layer stack (outermost first) and a
    # recipe library keyed by recipe_id. Recipes not present here fall back
    # to the built-in defaults in `materials.DEFAULT_RECIPES` (and are
    # flagged `unvalidated` until a real lab protocol backs them).
    layers: list[LayerSpec] = field(default_factory=list)
    material_recipes: dict[str, dict[str, Any]] = field(default_factory=dict)

    manufacturing: ManufacturingConfig = field(default_factory=ManufacturingConfig)
    loads: LoadsConfig = field(default_factory=LoadsConfig)
    cure: CureConfig = field(default_factory=CureConfig)

    @property
    def base_radius_mm(self) -> float:
        """Radius of the undeformed footprint circle (lobes oscillate around this)."""
        return self.overall_width_mm / 2.0

    NESTED_CONFIG_FIELDS = ("mold", "nesting", "manufacturing", "loads", "cure")

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "DomeConfig":
        known = {
            f.name for f in fields(cls)
            if f.name not in cls.NESTED_CONFIG_FIELDS and f.name not in ("layers", "material_recipes")
        }
        filtered = {k: v for k, v in data.items() if k in known}
        config = cls(**filtered)
        config.mold = MoldConfig.from_dict(data.get("mold"))
        config.nesting = NestingConfig.from_dict(data.get("nesting"))
        config.manufacturing = ManufacturingConfig.from_dict(data.get("manufacturing"))
        config.loads = LoadsConfig.from_dict(data.get("loads"))
        config.cure = CureConfig.from_dict(data.get("cure"))
        config.layers = [LayerSpec.from_dict(item) for item in (data.get("layers") or [])]
        config.material_recipes = dict(data.get("material_recipes") or {})
        return config

    @classmethod
    def from_yaml(cls, path: str | Path) -> "DomeConfig":
        with open(path, "r", encoding="utf-8") as fh:
            data = yaml.safe_load(fh) or {}
        return cls.from_dict(data)
