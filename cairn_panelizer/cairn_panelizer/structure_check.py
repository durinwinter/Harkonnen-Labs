"""Preliminary structural sanity check — NOT a code-compliance certification (spec sec. 4.6).

This module exists to catch obviously-thin geometry and flag the zones a
structural engineer should look at first (openings, crown, high-curvature
transitions), using span-to-thickness "slenderness" as the only metric — a
crude proxy, but a defensible, auditable one for triangulated shell facets at
this stage. Every output is explicitly labeled preliminary and every
borderline zone is flagged `requires_fea` rather than passed/failed, per the
spec's acceptance criteria ("does not claim code compliance").

Future integration targets named in the spec — CalculiX, Code_Aster,
OpenSees, external FEA export — would replace this module's stress estimate
with real finite-element results; this module's job is only to decide *where*
that analysis is most needed.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .config import DomeConfig
from .connections import BASALT_PIN_JOINT, BOLTED_INSERT_JOINT, Connection
from .panel import Panel

DISCLAIMER = (
    "PRELIMINARY STRUCTURAL PRE-CHECK ONLY. This is a geometric sanity pass, "
    "not an engineering analysis and not a code-compliance determination. "
    "Zones flagged `requires_fea` must be reviewed by a structural engineer "
    "using real FEA (CalculiX / Code_Aster / OpenSees) before fabrication."
)

ZONE_FIELD = "field"
ZONE_OPENING_ADJACENT = "opening_adjacent"
ZONE_CROWN_ADJACENT = "crown_adjacent"
ZONE_HIGH_CURVATURE = "high_curvature_transition"

# Heuristic thresholds — placeholders pending real material/FEA validation.
SLENDERNESS_WARNING_RATIO = 60.0   # span_mm / thickness_mm above this -> "thin for its span"
SLENDERNESS_FEA_RATIO = 90.0       # above this -> always flag requires_fea
HIGH_CURVATURE_DIHEDRAL_DEG = 25.0
CROWN_HEIGHT_FRACTION = 0.85       # panels whose centroid sits above this fraction of dome height
REFERENCE_MIN_THICKNESS_MM = 60.0  # preliminary floor below which the configured loads warrant a closer look

# Simplified placeholder pressure conversions — NOT ASCE 7 / building-code formulas.
PSF_TO_KPA = 0.0479
WIND_PRESSURE_COEFFICIENT = 0.00256  # crude V^2-based placeholder, not a code velocity-pressure formula


@dataclass
class StructuralFlag:
    panel_id: str
    zone: str
    approximate_span_mm: float
    slenderness_ratio: float
    stress_flag: bool
    unsupported_span_warning: bool
    requires_fea: bool
    notes: list[str] = field(default_factory=list)

    def to_record(self) -> dict:
        return {
            "panel_id": self.panel_id,
            "zone": self.zone,
            "approximate_span_mm": round(self.approximate_span_mm, 1),
            "slenderness_ratio": round(self.slenderness_ratio, 1),
            "stress_flag": self.stress_flag,
            "unsupported_span_warning": self.unsupported_span_warning,
            "requires_fea": self.requires_fea,
            "notes": self.notes,
        }


@dataclass
class StructuralPreCheckReport:
    disclaimer: str
    design_load_psf: float
    wind_pressure_psf: float
    safety_factor_target: float
    panel_thickness_mm: float
    flags: list[StructuralFlag] = field(default_factory=list)
    thickness_recommendations: list[dict] = field(default_factory=list)
    rib_recommendations: list[dict] = field(default_factory=list)
    joint_load_warnings: list[dict] = field(default_factory=list)
    summary_warnings: list[str] = field(default_factory=list)

    def to_record(self) -> dict:
        flagged = [f for f in self.flags if f.stress_flag or f.unsupported_span_warning or f.requires_fea]
        return {
            "disclaimer": self.disclaimer,
            "design_load_psf": round(self.design_load_psf, 2),
            "wind_pressure_psf": round(self.wind_pressure_psf, 2),
            "safety_factor_target": self.safety_factor_target,
            "panel_thickness_mm": self.panel_thickness_mm,
            "flagged_panel_count": len(flagged),
            "requires_fea_panel_count": sum(1 for f in self.flags if f.requires_fea),
            "flags": [f.to_record() for f in self.flags],
            "thickness_recommendations": self.thickness_recommendations,
            "rib_recommendations": self.rib_recommendations,
            "joint_load_warnings": self.joint_load_warnings,
            "summary_warnings": self.summary_warnings,
        }


def _zone_for_panel(panel: Panel, config: DomeConfig) -> str:
    if panel.panel_type == "opening_adjacent":
        return ZONE_OPENING_ADJACENT
    if panel.centroid[2] >= config.height_mm * CROWN_HEIGHT_FRACTION:
        return ZONE_CROWN_ADJACENT
    if any(angle >= HIGH_CURVATURE_DIHEDRAL_DEG for angle in panel.dihedral_angles_deg.values()):
        return ZONE_HIGH_CURVATURE
    return ZONE_FIELD


def _stack_thickness_mm(config: DomeConfig) -> float:
    if config.layers:
        return sum(layer.thickness_mm for layer in config.layers)
    return config.mold.panel_thickness_mm


def run_structural_precheck(
    panels: list[Panel],
    connections: list[Connection],
    config: DomeConfig,
) -> StructuralPreCheckReport:
    """Run the geometric slenderness/zone pass and emit per-panel + design-level flags."""
    fabricated = [p for p in panels if not p.is_opening]
    thickness_mm = _stack_thickness_mm(config)
    loads = config.loads

    design_load_psf = loads.snow_load_psf + loads.dead_load_psf
    wind_pressure_psf = WIND_PRESSURE_COEFFICIENT * (loads.wind_speed_mph ** 2)

    report = StructuralPreCheckReport(
        disclaimer=DISCLAIMER,
        design_load_psf=design_load_psf,
        wind_pressure_psf=wind_pressure_psf,
        safety_factor_target=loads.safety_factor_target,
        panel_thickness_mm=thickness_mm,
    )

    if not fabricated:
        report.summary_warnings.append("no fabricated panels — structural pre-check skipped")
        return report

    if not config.layers:
        report.summary_warnings.append(
            "no material layers configured — using mold.panel_thickness_mm as a stand-in for stack thickness"
        )

    over_slender_families: dict[str, list[str]] = {}
    rib_candidates: dict[str, list[str]] = {}

    for panel in fabricated:
        span_mm = max(panel.edge_lengths)
        slenderness = span_mm / max(thickness_mm, 1e-6)
        zone = _zone_for_panel(panel, config)

        stress_flag = slenderness >= SLENDERNESS_WARNING_RATIO
        unsupported_span = slenderness >= SLENDERNESS_WARNING_RATIO and zone in (
            ZONE_OPENING_ADJACENT, ZONE_HIGH_CURVATURE,
        )
        requires_fea = (
            slenderness >= SLENDERNESS_FEA_RATIO
            or zone in (ZONE_OPENING_ADJACENT, ZONE_CROWN_ADJACENT, ZONE_HIGH_CURVATURE)
        )

        notes: list[str] = []
        if zone == ZONE_OPENING_ADJACENT:
            notes.append("adjacent to a door/window opening — stress concentration likely")
        elif zone == ZONE_CROWN_ADJACENT:
            notes.append("adjacent to the crown/skylight opening — ring compression + edge bending likely")
        elif zone == ZONE_HIGH_CURVATURE:
            notes.append("sits at a high-curvature facet transition — local bending likely exceeds field-panel estimate")
        if stress_flag:
            notes.append(f"span-to-thickness ratio {slenderness:.0f}:1 exceeds the {SLENDERNESS_WARNING_RATIO:.0f}:1 preliminary guideline")

        flag = StructuralFlag(
            panel_id=panel.panel_id,
            zone=zone,
            approximate_span_mm=span_mm,
            slenderness_ratio=slenderness,
            stress_flag=stress_flag,
            unsupported_span_warning=unsupported_span,
            requires_fea=requires_fea,
            notes=notes,
        )
        report.flags.append(flag)

        if stress_flag and panel.mold_family_id:
            over_slender_families.setdefault(panel.mold_family_id, []).append(panel.panel_id)
        if zone in (ZONE_OPENING_ADJACENT, ZONE_HIGH_CURVATURE) and panel.mold_family_id:
            rib_candidates.setdefault(panel.mold_family_id, []).append(panel.panel_id)

    for family_id, panel_ids in sorted(over_slender_families.items()):
        recommended_thickness = thickness_mm * 1.25
        report.thickness_recommendations.append({
            "mold_family_id": family_id,
            "affected_panel_count": len(panel_ids),
            "current_stack_thickness_mm": round(thickness_mm, 1),
            "recommended_minimum_thickness_mm": round(recommended_thickness, 1),
            "rationale": "slenderness ratio exceeds preliminary guideline; +25% thickness brings it back within range "
                         "at constant span (re-check after any geometry change)",
        })

    for family_id, panel_ids in sorted(rib_candidates.items()):
        report.rib_recommendations.append({
            "mold_family_id": family_id,
            "affected_panel_count": len(panel_ids),
            "recommendation": "candidate for edge_rib_mold reinforcement — sits at an opening or high-curvature "
                              "transition where unreinforced facet bending is least understood",
        })

    high_load_joint_types = (BASALT_PIN_JOINT, BOLTED_INSERT_JOINT)
    flags_by_panel = {f.panel_id: f for f in report.flags}
    for connection in connections:
        flag_a = flags_by_panel.get(connection.panel_a)
        flag_b = flags_by_panel.get(connection.panel_b)
        in_risk_zone = any(
            f and f.zone in (ZONE_OPENING_ADJACENT, ZONE_CROWN_ADJACENT, ZONE_HIGH_CURVATURE)
            for f in (flag_a, flag_b)
        )
        if connection.joint_type in high_load_joint_types and in_risk_zone:
            report.joint_load_warnings.append({
                "joint_id": connection.joint_id,
                "panel_a": connection.panel_a,
                "panel_b": connection.panel_b,
                "joint_type": connection.joint_type,
                "note": "hardware-bearing joint sits in an opening/crown/high-curvature zone — "
                        "verify hardware load path before relying on it structurally",
            })

    if design_load_psf * loads.safety_factor_target > 0 and thickness_mm < REFERENCE_MIN_THICKNESS_MM:
        report.summary_warnings.append(
            f"stack thickness {thickness_mm:.0f}mm is below the {REFERENCE_MIN_THICKNESS_MM:.0f}mm preliminary "
            f"floor for the configured loads — review layer thicknesses before prototyping"
        )

    flagged_count = sum(1 for f in report.flags if f.requires_fea)
    if flagged_count:
        report.summary_warnings.append(
            f"{flagged_count} panel(s) flagged requires_fea — engage real FEA before committing to this geometry"
        )
    if loads.seismic_category == "placeholder":
        report.summary_warnings.append("seismic_category is a placeholder — no seismic load has been evaluated")

    return report
