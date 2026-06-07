"""Factory package generator — the single coherent manufacturing export (spec sec. 4.12).

Bundles every module's output (geometry, families, molds, materials, cost,
connections, assembly, cure, structure) into one traceable
`outputs/factory_package/` tree. This module does no analysis of its own —
it assumes the caller has already run the pipeline stages in dependency
order (geometry -> panels -> openings -> families -> molds -> nesting ->
recipes -> cost -> connections -> assembly -> cure -> structure check) and
just consolidates, cross-links, and writes.

`preview/`, `molds/`, and `panels/` carry the consolidated drawings the
pipeline already produces (`preview.png`, `molds.dxf`, `flat_patterns.dxf`)
rather than one file per mold/panel — see README "Current limitations" for
why per-unit export files (`mold_<id>.dxf` etc.) are a deferred stretch goal,
not an oversight: at hundreds of mold families, per-unit files would be
slow to generate and mostly redundant with the consolidated schedules.
"""

from __future__ import annotations

import csv
import json
import shutil
from pathlib import Path

from .assembly import AssemblyStep, export_assembly_checklist_md
from .config import DomeConfig
from .connections import CROSS_FAMILY_DESIGN_WARNING_FRACTION, Connection
from .cost import CostReport
from .cure import CurePrediction
from .export import (
    export_family_summary_csv,
    export_mold_schedule_csv,
    export_mold_schedule_json,
    export_panel_schedule_csv,
    export_panel_schedule_json,
)
from .materials import MaterialRecipe, build_material_bom
from .mold import Mold
from .nesting import NestingSheet
from .panel import Panel
from .structure_check import StructuralPreCheckReport

PACKAGE_SUBDIRS = ("preview", "molds", "panels")


def _write_json(path: Path, data) -> None:
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(data, fh, indent=2)


def _export_material_bom_csv(bom: list[dict], path: Path) -> None:
    fieldnames = [
        "recipe_id", "layer", "panel_count", "total_volume_m3", "total_mass_kg",
        "batch_mass_with_waste_kg", "total_cost_usd", "cost_per_kg_usd", "validation_status",
    ]
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in bom:
            writer.writerow(row)


def _export_recipe_assignments_json(panels: list[Panel], library: dict[str, MaterialRecipe], path: Path) -> None:
    fabricated = [p for p in panels if not p.is_opening]
    _write_json(path, {
        "recipes": {recipe_id: recipe.to_record() for recipe_id, recipe in sorted(library.items())},
        "panel_assignments": [
            {
                "panel_id": panel.panel_id,
                "mold_family_id": panel.mold_family_id,
                "layer_stack": panel.layer_stack,
                "recipe_assignments": panel.recipe_assignments,
                "estimated_mass_kg": round(panel.estimated_mass_kg, 3) if panel.estimated_mass_kg is not None else None,
                "estimated_cost_usd": round(panel.estimated_cost_usd, 2) if panel.estimated_cost_usd is not None else None,
            }
            for panel in fabricated
        ],
    })


def _export_cost_report_csv(report: CostReport, path: Path) -> None:
    fieldnames = ["scope", "key", "low_usd", "medium_usd", "high_usd"]
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in report.to_csv_rows():
            writer.writerow(row)


def _export_connection_schedule_csv(connections: list[Connection], path: Path) -> None:
    fieldnames = [
        "joint_id", "panel_a", "panel_b", "joint_type", "seam_length_mm", "seam_thickness_mm",
        "fuse_volume_mm3", "cross_family", "primer_required", "insert_count", "hardware_count",
        "tolerance_requirement_mm", "assembly_step_id", "warnings",
    ]
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for connection in connections:
            record = connection.to_record()
            record["warnings"] = json.dumps(record["warnings"])
            writer.writerow(record)


def _export_cure_schedule_csv(predictions: list[CurePrediction], path: Path) -> None:
    fieldnames = [
        "panel_id", "controlling_layer", "recipe_id", "estimated_open_time_min",
        "estimated_demold_time_hr", "estimated_handling_time_hr", "estimated_full_cure_time_hr",
        "recommended_chamber_temp_C", "recommended_chamber_rh_pct", "warnings",
    ]
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for prediction in predictions:
            record = prediction.to_record()
            record["warnings"] = json.dumps(record["warnings"])
            writer.writerow(record)


def _collect_warnings(
    config: DomeConfig,
    panels: list[Panel],
    molds: list[Mold],
    cost_report: CostReport,
    connections: list[Connection],
    assembly_steps: list[AssemblyStep],
    cure_predictions: list[CurePrediction],
    structural_report: StructuralPreCheckReport,
) -> dict:
    """Aggregate every module's warnings into one traceable manifest."""
    fabricated = [p for p in panels if not p.is_opening]
    bed_w, bed_h = config.manufacturing.cnc_bed_width_mm, config.manufacturing.cnc_bed_height_mm

    mold_cnc_warnings = []
    for mold in molds:
        xs = [p[0] for p in mold.cavity_outline_mm]
        ys = [p[1] for p in mold.cavity_outline_mm]
        width, height = max(xs) - min(xs), max(ys) - min(ys)
        if width > bed_w or height > bed_h:
            mold_cnc_warnings.append({
                "mold_id": mold.mold_id,
                "family_id": mold.family_id,
                "cavity_bbox_mm": [round(width, 1), round(height, 1)],
                "cnc_bed_mm": [bed_w, bed_h],
                "warning": "mold cavity exceeds the configured CNC bed envelope",
            })

    family_count = len({p.mold_family_id for p in fabricated if p.mold_family_id})
    design_warnings = []
    if family_count > len(fabricated) * 0.5 and fabricated:
        design_warnings.append(
            f"excessive unique panel/mold-family count — {family_count} families for "
            f"{len(fabricated)} panels ({family_count / len(fabricated):.0%}); "
            f"consider loosening family_tolerance_mm to improve manufacturability"
        )

    cross_family_count = sum(1 for c in connections if c.cross_family)
    if connections and cross_family_count > len(connections) * CROSS_FAMILY_DESIGN_WARNING_FRACTION:
        design_warnings.append(
            f"cold joint water theft risk across the assembly — {cross_family_count} of "
            f"{len(connections)} seams ({cross_family_count / len(connections):.0%}) join panels "
            f"from different mold families (likely different cure batches); plan a substrate "
            f"pre-wet/primer pass into the assembly sequence rather than handling it joint-by-joint "
            f"(see each connection's `cross_family`/`primer_required` flags for which seams need it)"
        )

    return {
        "design": design_warnings,
        "mold_cnc_fit": mold_cnc_warnings,
        "panel": {p.panel_id: p.warnings for p in fabricated if p.warnings},
        "mold": {m.mold_id: m.warnings for m in molds if getattr(m, "warnings", None)},
        "cost": cost_report.warnings,
        "connections": {c.joint_id: c.warnings for c in connections if c.warnings},
        "assembly": {s.step_id: s.warnings for s in assembly_steps if s.warnings},
        "cure": {p.panel_id: p.warnings for p in cure_predictions if p.warnings},
        "structure": structural_report.summary_warnings,
    }


def _design_summary(
    config: DomeConfig,
    panels: list[Panel],
    molds: list[Mold],
    sheets: list[NestingSheet],
    connections: list[Connection],
    assembly_steps: list[AssemblyStep],
    cost_report: CostReport,
    structural_report: StructuralPreCheckReport,
    warnings: dict,
) -> dict:
    fabricated = [p for p in panels if not p.is_opening]
    family_ids = sorted({p.mold_family_id for p in fabricated if p.mold_family_id})
    return {
        "design_id": f"cairn-trillium-{int(config.overall_width_mm)}x{int(config.height_mm)}",
        "geometry": {
            "overall_width_mm": config.overall_width_mm,
            "height_mm": config.height_mm,
            "lobe_count": config.lobe_count,
            "lobe_amplitude": config.lobe_amplitude,
            "crown_radius_mm": config.crown_radius_mm,
            "panel_mode": config.panel_mode,
        },
        "panel_count": len(fabricated),
        "panel_family_count": len(family_ids),
        "mold_count": len(molds),
        "nesting_sheet_count": len(sheets),
        "connection_count": len(connections),
        "assembly_step_count": len(assembly_steps),
        "layer_stack": [{"name": layer.name, "thickness_mm": layer.thickness_mm, "recipe_id": layer.recipe_id}
                        for layer in config.layers],
        "cost_summary_usd": {
            "low": round(cost_report.total_low_usd, 2),
            "medium": round(cost_report.total_medium_usd, 2),
            "high": round(cost_report.total_high_usd, 2),
            "per_sqm": round(cost_report.cost_per_sqm_usd, 2),
        },
        "structural_disclaimer": structural_report.disclaimer,
        "requires_fea_panel_count": sum(1 for f in structural_report.flags if f.requires_fea),
        "warning_counts": {
            "design": len(warnings["design"]),
            "mold_cnc_fit": len(warnings["mold_cnc_fit"]),
            "panel": len(warnings["panel"]),
            "mold": len(warnings["mold"]),
            "cost": len(warnings["cost"]),
            "connections": len(warnings["connections"]),
            "assembly": len(warnings["assembly"]),
            "cure": len(warnings["cure"]),
            "structure": len(warnings["structure"]),
        },
    }


def generate_factory_package(
    *,
    config: DomeConfig,
    panels: list[Panel],
    molds: list[Mold],
    sheets: list[NestingSheet],
    connections: list[Connection],
    assembly_steps: list[AssemblyStep],
    cure_predictions: list[CurePrediction],
    cost_report: CostReport,
    structural_report: StructuralPreCheckReport,
    library: dict[str, MaterialRecipe],
    output_dir: str | Path,
    source_outputs_dir: str | Path | None = None,
) -> dict[str, str]:
    """Write the complete `outputs/factory_package/` manufacturing export tree."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    for sub in PACKAGE_SUBDIRS:
        (output_dir / sub).mkdir(parents=True, exist_ok=True)

    bom = build_material_bom(panels, config, library)
    warnings = _collect_warnings(config, panels, molds, cost_report, connections, assembly_steps, cure_predictions, structural_report)
    summary = _design_summary(config, panels, molds, sheets, connections, assembly_steps, cost_report, structural_report, warnings)

    paths = {
        "design_summary": output_dir / "design_summary.json",
        "panel_schedule_json": output_dir / "panel_schedule.json",
        "panel_schedule_csv": output_dir / "panel_schedule.csv",
        "panel_families_csv": output_dir / "panel_families.csv",
        "mold_schedule_json": output_dir / "mold_schedule.json",
        "mold_schedule_csv": output_dir / "mold_schedule.csv",
        "material_bom_csv": output_dir / "material_bom.csv",
        "recipe_assignments_json": output_dir / "recipe_assignments.json",
        "cost_report_json": output_dir / "cost_report.json",
        "cost_report_csv": output_dir / "cost_report.csv",
        "assembly_sequence_json": output_dir / "assembly_sequence.json",
        "assembly_checklist_md": output_dir / "assembly_checklist.md",
        "connection_schedule_csv": output_dir / "connection_schedule.csv",
        "cure_schedule_csv": output_dir / "cure_schedule.csv",
        "structural_precheck_json": output_dir / "structural_precheck.json",
        "warnings_json": output_dir / "warnings.json",
    }

    _write_json(paths["design_summary"], summary)
    export_panel_schedule_json(panels, paths["panel_schedule_json"])
    export_panel_schedule_csv(panels, paths["panel_schedule_csv"])
    export_family_summary_csv(panels, paths["panel_families_csv"])
    export_mold_schedule_json(molds, paths["mold_schedule_json"])
    export_mold_schedule_csv(molds, paths["mold_schedule_csv"])
    _export_material_bom_csv(bom, paths["material_bom_csv"])
    _export_recipe_assignments_json(panels, library, paths["recipe_assignments_json"])
    _write_json(paths["cost_report_json"], cost_report.to_record())
    _export_cost_report_csv(cost_report, paths["cost_report_csv"])
    _write_json(paths["assembly_sequence_json"], [s.to_record() for s in assembly_steps])
    export_assembly_checklist_md(assembly_steps, paths["assembly_checklist_md"])
    _export_connection_schedule_csv(connections, paths["connection_schedule_csv"])
    _export_cure_schedule_csv(cure_predictions, paths["cure_schedule_csv"])
    _write_json(paths["structural_precheck_json"], structural_report.to_record())
    _write_json(paths["warnings_json"], warnings)

    if source_outputs_dir is not None:
        source = Path(source_outputs_dir)
        copy_map = {
            "preview.png": "preview",
            "molds.dxf": "molds",
            "flat_patterns.dxf": "panels",
        }
        for filename, subdir in copy_map.items():
            src = source / filename
            if src.exists():
                dest = output_dir / subdir / filename
                shutil.copyfile(src, dest)
                paths[f"{subdir}_{filename.replace('.', '_')}"] = dest

    return {k: str(v) for k, v in paths.items()}
