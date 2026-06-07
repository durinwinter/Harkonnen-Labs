"""Mold design generation for fabricating each repeatable panel family.

One mold is generated per `mold_family_id` — the same grouping that already
collapses geometrically-equivalent panels in `families.py` — so equivalent
panels never get duplicate molds. Each mold links back to every panel_id
that is cast from it.

v0.1 scope: every panel the pipeline produces is a flat triangular facet, so
every mold is a `flat_faceted_mold` with a fully-specified cavity, dam,
bevel/draft, registration, demold, and insert-locator layout. The other
named mold types are declared here as the stable vocabulary that
`classify_mold_type` will route to once the geometry pipeline can produce
curved panels, edge ribs, or dedicated material test coupons.
"""

from __future__ import annotations

from collections import OrderedDict
from dataclasses import dataclass, field

import numpy as np

from .config import DomeConfig
from .flatten2d import flatten_triangle
from .panel import Panel

FLAT_FACETED_MOLD = "flat_faceted_mold"
SHALLOW_CURVED_PANEL_MOLD = "shallow_curved_panel_mold"
EDGE_RIB_MOLD = "edge_rib_mold"
TEST_COUPON_MOLD = "test_coupon_mold"

MOLD_TYPES = (FLAT_FACETED_MOLD, SHALLOW_CURVED_PANEL_MOLD, EDGE_RIB_MOLD, TEST_COUPON_MOLD)


@dataclass
class Mold:
    mold_id: str
    mold_type: str
    family_id: str
    panel_ids: list[str]
    material: str
    cavity_outline_mm: list[tuple[float, float]]
    cavity_area_mm2: float
    panel_thickness_mm: float
    edge_dam_height_mm: float
    bevel_angle_deg: float
    draft_angle_deg: float
    registration_holes: list[dict] = field(default_factory=list)
    demold_slots: list[dict] = field(default_factory=list)
    insert_locator_points: list[dict] = field(default_factory=list)
    label_text: str = ""

    def to_record(self) -> dict:
        return {
            "mold_id": self.mold_id,
            "mold_type": self.mold_type,
            "family_id": self.family_id,
            "panel_ids": self.panel_ids,
            "panel_count": len(self.panel_ids),
            "material": self.material,
            "cavity_outline_mm": [[round(x, 2), round(y, 2)] for x, y in self.cavity_outline_mm],
            "cavity_area_mm2": round(self.cavity_area_mm2, 2),
            "panel_thickness_mm": self.panel_thickness_mm,
            "edge_dam_height_mm": self.edge_dam_height_mm,
            "bevel_angle_deg": self.bevel_angle_deg,
            "draft_angle_deg": self.draft_angle_deg,
            "registration_holes": self.registration_holes,
            "demold_slots": self.demold_slots,
            "insert_locator_points": self.insert_locator_points,
            "label_text": self.label_text,
        }


def classify_mold_type(panel: Panel, config: DomeConfig) -> str:
    """Pick a mold type for a panel family's representative panel.

    Every panel the v1 geometry pipeline emits is a flat triangular facet,
    so this always resolves to `flat_faceted_mold` today. The branch is kept
    explicit (rather than hardcoded) so that once the mesh pipeline can
    produce doubly-curved panels, rib-backed edge panels, or dedicated test
    coupons, routing them to `shallow_curved_panel_mold` / `edge_rib_mold` /
    `test_coupon_mold` is a one-line addition here instead of a refactor.
    """
    if len(panel.edge_lengths) == 3 and not panel.is_opening:
        return FLAT_FACETED_MOLD
    return config.mold.default_type


def _outward_unit(point: np.ndarray, centroid: np.ndarray) -> np.ndarray:
    direction = point - centroid
    norm = np.linalg.norm(direction)
    if norm == 0.0:
        return np.array([1.0, 0.0])
    return direction / norm


def _registration_holes(outline: list[tuple[float, float]], inset_mm: float, diameter_mm: float) -> list[dict]:
    """Two corner-anchored holes are enough to fix a triangle's orientation."""
    pts = np.array(outline)
    centroid = pts.mean(axis=0)
    holes = []
    for i in range(2):
        corner = pts[i]
        inward = centroid - corner
        norm = np.linalg.norm(inward)
        offset = inward / norm * min(inset_mm, norm * 0.9) if norm > 0 else np.zeros(2)
        point = corner + offset
        holes.append({
            "label": f"REG-{i}",
            "x": round(float(point[0]), 2),
            "y": round(float(point[1]), 2),
            "diameter_mm": diameter_mm,
        })
    return holes


def _demold_slots(outline: list[tuple[float, float]], edge_lengths: list[float],
                  width_mm: float, length_mm: float) -> list[dict]:
    """One pry slot, set into the panel's longest edge for the best leverage."""
    pts = np.array(outline)
    centroid = pts.mean(axis=0)
    n = len(pts)
    longest = max(range(n), key=lambda i: edge_lengths[i])
    p_a, p_b = pts[longest], pts[(longest + 1) % n]
    midpoint = (p_a + p_b) / 2.0
    outward = _outward_unit(midpoint, centroid)
    point = midpoint + outward * (length_mm / 2.0)
    edge_vector = p_b - p_a
    angle_deg = float(np.degrees(np.arctan2(edge_vector[1], edge_vector[0])))
    return [{
        "label": "DEMOLD-0",
        "x": round(float(point[0]), 2),
        "y": round(float(point[1]), 2),
        "width_mm": width_mm,
        "length_mm": length_mm,
        "angle_deg": round(angle_deg, 2),
    }]


def _insert_locator_points(outline: list[tuple[float, float]], diameter_mm: float) -> list[dict]:
    """One locator per edge midpoint — where this panel meets its rib/hub neighbors."""
    pts = np.array(outline)
    n = len(pts)
    points = []
    for i in range(n):
        midpoint = (pts[i] + pts[(i + 1) % n]) / 2.0
        points.append({
            "label": f"INSERT-{i}",
            "x": round(float(midpoint[0]), 2),
            "y": round(float(midpoint[1]), 2),
            "diameter_mm": diameter_mm,
        })
    return points


def _build_flat_faceted_mold(mold_id: str, family_id: str, family_panels: list[Panel],
                             config: DomeConfig, mold_type: str) -> Mold:
    mold_cfg = config.mold
    representative = family_panels[0]
    outline = flatten_triangle(representative.edge_lengths)

    return Mold(
        mold_id=mold_id,
        mold_type=mold_type,
        family_id=family_id,
        panel_ids=[p.panel_id for p in family_panels],
        material=mold_cfg.material,
        cavity_outline_mm=outline,
        cavity_area_mm2=representative.area_mm2,
        panel_thickness_mm=mold_cfg.panel_thickness_mm,
        edge_dam_height_mm=mold_cfg.edge_dam_height_mm,
        bevel_angle_deg=mold_cfg.bevel_angle_deg,
        draft_angle_deg=mold_cfg.draft_angle_deg,
        registration_holes=_registration_holes(
            outline, mold_cfg.registration_hole_inset_mm, mold_cfg.registration_hole_diameter_mm
        ),
        demold_slots=_demold_slots(
            outline, representative.edge_lengths, mold_cfg.demold_slot_width_mm, mold_cfg.demold_slot_length_mm
        ),
        insert_locator_points=_insert_locator_points(outline, mold_cfg.insert_locator_diameter_mm),
        label_text=f"{mold_cfg.label_prefix}-{mold_id}-{family_id}",
    )


def build_molds(panels: list[Panel], config: DomeConfig) -> list[Mold]:
    """Generate one mold per unique panel family (skipping opening panels).

    Families with equivalent geometry already share a `mold_family_id`
    (see `families.assign_families`), so grouping on it here is sufficient
    to avoid duplicate molds — each mold simply lists every panel_id it casts.
    """
    if not config.mold.enabled:
        return []

    families: "OrderedDict[str, list[Panel]]" = OrderedDict()
    for panel in panels:
        if panel.is_opening or panel.mold_family_id is None:
            continue
        families.setdefault(panel.mold_family_id, []).append(panel)

    molds: list[Mold] = []
    for index, (family_id, family_panels) in enumerate(families.items()):
        mold_id = f"MOLD-{index:03d}"
        mold_type = classify_mold_type(family_panels[0], config)
        molds.append(_build_flat_faceted_mold(mold_id, family_id, family_panels, config, mold_type))

    return molds
