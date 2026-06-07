"""Build-sequence generation: base ring to crown (spec sec. 4.8).

Panels don't carry an explicit ring index, so this module buckets each
fabricated panel by its centroid height into `config.ring_segments` bands —
the same banding the geometry engine used to build the rings in the first
place — and emits one `AssemblyStep` per band, ordered base-to-crown. Each
step links back to the `Connection` records that join its panels to each
other and to the ring below (and stamps `assembly_step_id` onto those
connections so the joint schedule and the build sequence stay traceable to
each other).
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .config import DomeConfig
from .connections import (
    BASALT_PIN_JOINT,
    BOLTED_INSERT_JOINT,
    Connection,
    SPLINE_JOINT,
    TONGUE_AND_GROOVE,
)
from .panel import Panel

PLACEMENT_MINUTES_PER_PANEL = 12.0
FUSE_APPLICATION_MINUTES_PER_JOINT = 8.0
QA_MINUTES_PER_STEP = 15.0

CRANE_LIFT_HEIGHT_MM = 2400.0     # rings whose panels sit above this height need lift assistance
CRANE_LIFT_MASS_KG = 35.0         # or whose panels are individually this heavy
BRACING_RING_FRACTION = 0.5       # rings above this fraction of total rings need temporary bracing

TOOL_BY_JOINT_TYPE = {
    SPLINE_JOINT: "spline stock + Fuse applicator",
    TONGUE_AND_GROOVE: "Fuse applicator + alignment clamps",
    BASALT_PIN_JOINT: "basalt pin driver + Fuse applicator",
    BOLTED_INSERT_JOINT: "torque wrench + insert driver + Fuse applicator",
}


@dataclass
class AssemblyStep:
    step_id: str
    sequence_number: int
    action: str
    panel_ids: list[str]
    panel_families: list[str]
    joint_ids: list[str]
    required_materials: list[str]
    required_tools: list[str]
    crew_actions: list[str]
    qa_checks: list[str]
    fuse_application_steps: list[str]
    estimated_duration_min: float
    temporary_bracing_required: bool
    crane_lift_required: bool
    warnings: list[str] = field(default_factory=list)

    def to_record(self) -> dict:
        return {
            "step_id": self.step_id,
            "sequence_number": self.sequence_number,
            "action": self.action,
            "panel_ids": self.panel_ids,
            "panel_count": len(self.panel_ids),
            "panel_families": self.panel_families,
            "joint_ids": self.joint_ids,
            "required_materials": self.required_materials,
            "required_tools": self.required_tools,
            "crew_actions": self.crew_actions,
            "qa_checks": self.qa_checks,
            "fuse_application_steps": self.fuse_application_steps,
            "estimated_duration_min": round(self.estimated_duration_min, 1),
            "temporary_bracing_required": self.temporary_bracing_required,
            "crane_lift_required": self.crane_lift_required,
            "warnings": self.warnings,
        }


def _ring_index(panel: Panel, config: DomeConfig) -> int:
    band = max(config.height_mm / max(config.ring_segments, 1), 1.0)
    idx = int(panel.centroid[2] // band)
    return max(0, min(idx, config.ring_segments - 1))


def generate_assembly_sequence(
    panels: list[Panel],
    connections: list[Connection],
    config: DomeConfig,
) -> list[AssemblyStep]:
    """Group fabricated panels into base-to-crown rings and emit one step per ring."""
    fabricated = [p for p in panels if not p.is_opening]
    by_id = {p.panel_id: p for p in panels}
    connections_by_pair = {(c.panel_a, c.panel_b): c for c in connections}

    rings: dict[int, list[Panel]] = {}
    for panel in fabricated:
        rings.setdefault(_ring_index(panel, config), []).append(panel)

    placed: set[str] = set()
    steps: list[AssemblyStep] = []

    for sequence_number, ring_idx in enumerate(sorted(rings)):
        ring_panels = sorted(rings[ring_idx], key=lambda p: p.panel_id)
        panel_ids = [p.panel_id for p in ring_panels]
        families = sorted({p.mold_family_id for p in ring_panels if p.mold_family_id})

        # Joints that close out this step: both endpoints already placed by now
        # (within-ring seams, or seams back to the previously placed ring).
        step_placed = placed | set(panel_ids)
        joint_ids: list[str] = []
        joint_types_seen: set[str] = set()
        materials: set[str] = set()
        for panel in ring_panels:
            for neighbor_id in panel.neighbor_ids:
                neighbor = by_id.get(neighbor_id)
                if neighbor is None or neighbor.is_opening or neighbor_id not in step_placed:
                    continue
                key = tuple(sorted((panel.panel_id, neighbor_id)))
                connection = connections_by_pair.get(key)
                if connection is None or connection.joint_id in joint_ids:
                    continue
                connection.assembly_step_id = f"A{sequence_number:03d}"
                joint_ids.append(connection.joint_id)
                joint_types_seen.add(connection.joint_type)
            for assignment in panel.recipe_assignments:
                materials.add(assignment["recipe_id"])

        ring_height_mm = (ring_idx + 0.5) * (config.height_mm / max(config.ring_segments, 1))
        max_panel_mass = max((p.estimated_mass_kg or 0.0 for p in ring_panels), default=0.0)
        crane_lift = ring_height_mm > CRANE_LIFT_HEIGHT_MM or max_panel_mass > CRANE_LIFT_MASS_KG
        bracing = (ring_idx / max(config.ring_segments - 1, 1)) >= BRACING_RING_FRACTION and ring_idx > 0

        tools = sorted({TOOL_BY_JOINT_TYPE[t] for t in joint_types_seen if t in TOOL_BY_JOINT_TYPE})
        if not tools:
            tools = ["Fuse applicator"]

        crew_actions = [
            f"Stage and dry-fit panels {panel_ids[0]}–{panel_ids[-1]} ({len(panel_ids)} panels, "
            f"{len(families)} mold families) against the previous ring's registration marks.",
            "Apply Fuse to mating seams per the connection schedule, priming cross-batch joints first.",
            "Set and brace panels to the ring profile; confirm plumb/level before cure set.",
        ]
        if crane_lift:
            crew_actions.insert(0, "Stage crane/lift support — ring height or panel mass exceeds manual-handling limits.")

        fuse_steps = [
            f"{joint_type.replace('_', ' ')} seams: prime cross-batch interfaces, lay Fuse bead, set panel, "
            f"tool joint flush, hold per Fuse open-time before loading."
            for joint_type in sorted(joint_types_seen)
        ] or ["No new seams close out at this ring — proceed to bracing/QA only."]

        qa_checks = [
            "Verify each panel seated to its registration features and family ID matches the schedule.",
            "Inspect Fuse seam coverage and bead profile along every closed-out joint.",
            "Confirm ring profile (radius/height) within tolerance before releasing bracing from the prior ring.",
        ]
        if crane_lift:
            qa_checks.append("Confirm rigging points and tag-line control before each lift.")

        warnings: list[str] = []
        if not panel_ids:
            warnings.append("ring has no fabricated panels — check geometry/opening flags")

        duration = (
            len(panel_ids) * PLACEMENT_MINUTES_PER_PANEL
            + len(joint_ids) * FUSE_APPLICATION_MINUTES_PER_JOINT
            + QA_MINUTES_PER_STEP
        )

        steps.append(AssemblyStep(
            step_id=f"A{sequence_number:03d}",
            sequence_number=sequence_number,
            action=f"place_ring_{ring_idx:02d}_panels",
            panel_ids=panel_ids,
            panel_families=families,
            joint_ids=joint_ids,
            required_materials=sorted(materials),
            required_tools=tools,
            crew_actions=crew_actions,
            qa_checks=qa_checks,
            fuse_application_steps=fuse_steps,
            estimated_duration_min=duration,
            temporary_bracing_required=bracing,
            crane_lift_required=crane_lift,
            warnings=warnings,
        ))

        placed |= set(panel_ids)

    return steps


def export_assembly_checklist_md(steps: list[AssemblyStep], path) -> None:
    """Render the assembly sequence as a human-readable Markdown checklist."""
    lines = ["# Cairn Dome — Assembly Checklist", ""]
    for step in steps:
        lines.append(f"## Step {step.sequence_number}: {step.action} (`{step.step_id}`)")
        lines.append("")
        lines.append(f"- Panels ({len(step.panel_ids)}): {', '.join(step.panel_ids)}")
        lines.append(f"- Mold families: {', '.join(step.panel_families) or '—'}")
        lines.append(f"- Joints closed out: {', '.join(step.joint_ids) or '—'}")
        lines.append(f"- Required materials: {', '.join(step.required_materials) or '—'}")
        lines.append(f"- Required tools: {', '.join(step.required_tools)}")
        lines.append(f"- Estimated duration: {step.estimated_duration_min:.0f} min")
        lines.append(f"- Temporary bracing required: {'yes' if step.temporary_bracing_required else 'no'}")
        lines.append(f"- Crane/lift required: {'yes' if step.crane_lift_required else 'no'}")
        lines.append("")
        lines.append("**Crew actions**")
        for action in step.crew_actions:
            lines.append(f"- [ ] {action}")
        lines.append("")
        lines.append("**Fuse application**")
        for fuse_step in step.fuse_application_steps:
            lines.append(f"- [ ] {fuse_step}")
        lines.append("")
        lines.append("**QA checkpoints**")
        for check in step.qa_checks:
            lines.append(f"- [ ] {check}")
        if step.warnings:
            lines.append("")
            lines.append("**Warnings**")
            for warning in step.warnings:
                lines.append(f"- ⚠ {warning}")
        lines.append("")

    with open(path, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))
