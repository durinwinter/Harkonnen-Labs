"""Panel-to-panel connection (joint) schedule generation (spec sec. 4.7).

One `Connection` is generated per unique pair of neighboring fabricated
panels (deduplicated from the bidirectional `neighbor_ids` adjacency already
computed in `panel.py`). Every seam is fundamentally a **Fuse seam** — Fuse is
the Cairn structural grout/adhesive that manages joints and cold interfaces —
classified into a reinforcement sub-type by the dihedral angle between the two
panels, since that's the strongest signal v1 geometry gives us about how much
mechanical interlock a seam needs.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from .config import DomeConfig
from .materials import MaterialRecipe
from .panel import Panel

FUSE_SEAM = "fuse_seam"
TONGUE_AND_GROOVE = "tongue_and_groove"
SPLINE_JOINT = "spline_joint"
BASALT_PIN_JOINT = "basalt_pin_joint"
BOLTED_INSERT_JOINT = "bolted_insert_joint"
COMPRESSION_GASKET = "compression_gasket"      # placeholder, not yet routed to
HUB_STRUT_NODE = "hub_strut_node"              # placeholder, not yet routed to

# Dihedral-angle thresholds (degrees) used to pick a reinforcement sub-type.
# Below FLAT: panels read as nearly coplanar -> a spline carries shear cleanly.
# Below FOLD: a moderate fold -> tongue-and-groove gives mechanical interlock.
# Above FOLD: a sharp fold (crown/eaves/opening transitions) -> a pinned joint
# resists the higher peel/rotation loads those zones see.
FLAT_ANGLE_DEG = 8.0
FOLD_ANGLE_DEG = 25.0

# Fuse seam thickness is the adhesive bond-line gap between two panel edges —
# it does not need to scale with panel-stack thickness (a glue line's gap-fill
# requirement is set by the *joint style's* registration tolerance, not by what
# it's bonding). Interlocking joints (spline/T&G) rely on a thin, closely-fitted
# line; pinned and bolted joints carry larger tolerance stack-up and hardware
# clearances, so they need a thicker gap-fill bead.
FUSE_SEAM_THICKNESS_BY_JOINT_MM: dict[str, float] = {
    SPLINE_JOINT: 4.0,
    TONGUE_AND_GROOVE: 6.0,
    BASALT_PIN_JOINT: 10.0,
    BOLTED_INSERT_JOINT: 12.0,
}
DEFAULT_SEAM_THICKNESS_MM = 6.0

# Absolute minimum bond-line thickness below which Fuse can't reliably gap-fill
# panel-edge tolerance stack-up, regardless of how thick the panels themselves are.
# Set below every value in FUSE_SEAM_THICKNESS_BY_JOINT_MM on purpose — under the
# shipped joint specs this check should stay quiet; it exists to catch a future
# joint-type addition (or a constant edit) that specs a bond line too thin to
# gap-fill reliably, not to restate the current spec back at the schedule.
FUSE_SEAM_MIN_THICKNESS_MM = 3.5

# Fraction of a dome's connections that may be cross-mold-family "cold joints"
# before it's worth surfacing as a single design-level finding (see
# factory_package._collect_warnings) rather than repeating per joint.
CROSS_FAMILY_DESIGN_WARNING_FRACTION = 0.5

PIN_JOINT_TOLERANCE_MM = 2.0
INTERLOCK_JOINT_TOLERANCE_MM = 1.0
BOLTED_JOINT_TOLERANCE_MM = 1.5

CROWN_ACCESS_WARNING_RADIUS_FRAC = 0.35  # fraction of overall_width treated as "near crown"


@dataclass
class Connection:
    joint_id: str
    panel_a: str
    panel_b: str
    joint_type: str
    seam_length_mm: float
    seam_thickness_mm: float
    fuse_volume_mm3: float
    cross_family: bool
    primer_required: bool
    insert_count: int
    hardware_count: int
    tolerance_requirement_mm: float
    assembly_step_id: str | None = None
    warnings: list[str] = field(default_factory=list)

    def to_record(self) -> dict:
        return {
            "joint_id": self.joint_id,
            "panel_a": self.panel_a,
            "panel_b": self.panel_b,
            "joint_type": self.joint_type,
            "seam_length_mm": round(self.seam_length_mm, 2),
            "seam_thickness_mm": round(self.seam_thickness_mm, 2),
            "fuse_volume_mm3": round(self.fuse_volume_mm3, 1),
            "cross_family": self.cross_family,
            "primer_required": self.primer_required,
            "insert_count": self.insert_count,
            "hardware_count": self.hardware_count,
            "tolerance_requirement_mm": self.tolerance_requirement_mm,
            "assembly_step_id": self.assembly_step_id,
            "warnings": self.warnings,
        }


def _shared_edge_length(a: Panel, b: Panel) -> float:
    """Length of the edge shared by two triangular panels (matched by shared vertices)."""
    verts_a = [tuple(v) for v in a.vertices]
    verts_b = [tuple(v) for v in b.vertices]
    shared = [v for v in verts_a if v in verts_b]
    if len(shared) < 2:
        # Adjacency came from trimesh's face-adjacency table even if rounded
        # vertex coords didn't line up exactly; fall back to the shorter of
        # the two panels' shortest edges as a conservative seam-length estimate.
        return min(min(a.edge_lengths), min(b.edge_lengths))
    p, q = np.array(shared[0]), np.array(shared[1])
    return float(np.linalg.norm(q - p))


def _classify_joint_type(panel_a: Panel, panel_b: Panel, dihedral_deg: float, config: DomeConfig) -> str:
    near_opening = panel_a.panel_type == "opening_adjacent" or panel_b.panel_type == "opening_adjacent"
    if near_opening:
        return BOLTED_INSERT_JOINT
    if dihedral_deg < FLAT_ANGLE_DEG:
        return SPLINE_JOINT
    if dihedral_deg < FOLD_ANGLE_DEG:
        return TONGUE_AND_GROOVE
    return BASALT_PIN_JOINT


def _stack_thickness_mm(config: DomeConfig) -> float:
    if config.layers:
        return sum(layer.thickness_mm for layer in config.layers)
    return config.mold.panel_thickness_mm


def _is_near_crown(panel_a: Panel, panel_b: Panel, config: DomeConfig) -> bool:
    threshold = config.overall_width_mm * CROWN_ACCESS_WARNING_RADIUS_FRAC
    for panel in (panel_a, panel_b):
        cx, cy, _ = panel.centroid
        if (cx ** 2 + cy ** 2) ** 0.5 <= threshold and panel.centroid[2] > config.height_mm * 0.6:
            return True
    return False


def generate_connections(
    panels: list[Panel],
    config: DomeConfig,
    library: dict[str, MaterialRecipe] | None = None,
) -> list[Connection]:
    """Generate one Connection per unique neighboring fabricated-panel pair."""
    by_id = {p.panel_id: p for p in panels}
    stack_thickness = _stack_thickness_mm(config)
    fuse_recipe = next((r for r in (library or {}).values() if r.layer == "Fuse"), None)

    seen: set[tuple[str, str]] = set()
    connections: list[Connection] = []
    counter = 0

    for panel in panels:
        if panel.is_opening:
            continue
        for neighbor_id in panel.neighbor_ids:
            neighbor = by_id.get(neighbor_id)
            if neighbor is None or neighbor.is_opening:
                continue
            key = tuple(sorted((panel.panel_id, neighbor_id)))
            if key in seen:
                continue
            seen.add(key)

            dihedral = panel.dihedral_angles_deg.get(neighbor_id, 0.0)
            joint_type = _classify_joint_type(panel, neighbor, dihedral, config)
            seam_length = _shared_edge_length(panel, neighbor)
            seam_thickness = FUSE_SEAM_THICKNESS_BY_JOINT_MM.get(joint_type, DEFAULT_SEAM_THICKNESS_MM)
            fuse_volume = seam_length * seam_thickness * stack_thickness

            cross_family = panel.mold_family_id != neighbor.mold_family_id
            primer_required = cross_family or joint_type == BOLTED_INSERT_JOINT

            insert_count = 0
            hardware_count = 0
            if joint_type == BOLTED_INSERT_JOINT:
                insert_count = 2
                hardware_count = 2
            elif joint_type == BASALT_PIN_JOINT:
                insert_count = 2
                hardware_count = 0

            if joint_type in (TONGUE_AND_GROOVE, SPLINE_JOINT):
                tolerance = INTERLOCK_JOINT_TOLERANCE_MM
            elif joint_type == BOLTED_INSERT_JOINT:
                tolerance = BOLTED_JOINT_TOLERANCE_MM
            else:
                tolerance = PIN_JOINT_TOLERANCE_MM

            warnings: list[str] = []
            if primer_required and fuse_recipe is None:
                warnings.append("missing Fuse recipe — no recipe in the library is assigned layer='Fuse' to validate priming requirement")
            if seam_thickness < FUSE_SEAM_MIN_THICKNESS_MM:
                warnings.append(
                    f"insufficient seam thickness — {seam_thickness:.1f}mm is below the "
                    f"{FUSE_SEAM_MIN_THICKNESS_MM:.1f}mm minimum Fuse bond-line thickness "
                    f"recommended for a {joint_type} joint's tolerance stack-up"
                )
            if joint_type == BOLTED_INSERT_JOINT and _is_near_crown(panel, neighbor, config):
                warnings.append("inaccessible fastener location — joint sits in the high-curvature crown/opening zone")

            counter += 1
            connections.append(Connection(
                joint_id=f"J{counter:04d}",
                panel_a=key[0],
                panel_b=key[1],
                joint_type=joint_type,
                seam_length_mm=seam_length,
                seam_thickness_mm=seam_thickness,
                fuse_volume_mm3=fuse_volume,
                cross_family=cross_family,
                primer_required=primer_required,
                insert_count=insert_count,
                hardware_count=hardware_count,
                tolerance_requirement_mm=tolerance,
                warnings=warnings,
            ))

    return connections
