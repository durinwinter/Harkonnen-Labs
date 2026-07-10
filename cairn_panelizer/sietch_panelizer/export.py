"""Fabrication-file exporters: mesh, panel/family/mold schedules, nested flat patterns."""

from __future__ import annotations

import csv
import json
import math
from pathlib import Path

import numpy as np
import trimesh

from .mold import Mold
from .nesting import NestingSheet
from .panel import Panel

try:
    import ezdxf
except ImportError:  # pragma: no cover - exercised only when ezdxf is missing
    ezdxf = None


def export_shell_mesh(mesh: trimesh.Trimesh, panels: list[Panel], path: str | Path) -> None:
    """Export the shell as OBJ, dropping opening panels so they appear as holes."""
    keep_faces = [p.face_index for p in panels if not p.is_opening]
    shell = mesh.submesh([keep_faces], append=True)
    shell.export(str(path))


def export_panel_schedule_json(panels: list[Panel], path: str | Path) -> None:
    records = [p.to_record() for p in panels]
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(records, fh, indent=2)


def export_panel_schedule_csv(panels: list[Panel], path: str | Path) -> None:
    fieldnames = [
        "panel_id", "panel_type", "is_opening", "mold_family_id",
        "vertex_count", "vertices", "edge_lengths_mm", "area_mm2",
        "normal", "centroid", "neighbor_ids", "neighbor_count",
        "dihedral_angles_deg", "layer_stack", "estimated_mass_kg",
        "estimated_cost_usd", "warnings",
    ]
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for panel in panels:
            record = panel.to_record()
            writer.writerow({
                "panel_id": record["panel_id"],
                "panel_type": record["panel_type"],
                "is_opening": record["is_opening"],
                "mold_family_id": record["mold_family_id"],
                "vertex_count": len(record["vertices"]),
                "vertices": json.dumps(record["vertices"]),
                "edge_lengths_mm": json.dumps(record["edge_lengths_mm"]),
                "area_mm2": record["area_mm2"],
                "normal": json.dumps(record["normal"]),
                "centroid": json.dumps(record["centroid"]),
                "neighbor_ids": json.dumps(record["neighbor_ids"]),
                "neighbor_count": len(record["neighbor_ids"]),
                "dihedral_angles_deg": json.dumps(record["dihedral_angles_deg"]),
                "layer_stack": json.dumps(record["layer_stack"]),
                "estimated_mass_kg": record["estimated_mass_kg"],
                "estimated_cost_usd": record["estimated_cost_usd"],
                "warnings": json.dumps(record["warnings"]),
            })


def export_family_summary_csv(panels: list[Panel], path: str | Path) -> None:
    families: dict[str, dict] = {}
    for panel in panels:
        if panel.is_opening or panel.mold_family_id is None:
            continue
        entry = families.setdefault(panel.mold_family_id, {
            "mold_family_id": panel.mold_family_id,
            "panel_count": 0,
            "representative_edge_lengths_mm": panel.edge_lengths,
            "representative_area_mm2": panel.area_mm2,
            "panel_ids": [],
        })
        entry["panel_count"] += 1
        entry["panel_ids"].append(panel.panel_id)

    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=[
            "mold_family_id", "panel_count", "representative_edge_lengths_mm",
            "representative_area_mm2", "panel_ids",
        ])
        writer.writeheader()
        for entry in sorted(families.values(), key=lambda e: e["mold_family_id"]):
            writer.writerow({
                "mold_family_id": entry["mold_family_id"],
                "panel_count": entry["panel_count"],
                "representative_edge_lengths_mm": json.dumps(
                    [round(v, 2) for v in entry["representative_edge_lengths_mm"]]
                ),
                "representative_area_mm2": round(entry["representative_area_mm2"], 2),
                "panel_ids": json.dumps(entry["panel_ids"]),
            })


def export_nesting_schedule_json(sheets: list[NestingSheet], path: str | Path) -> None:
    records = [s.to_record() for s in sheets]
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(records, fh, indent=2)


def export_nesting_schedule_csv(sheets: list[NestingSheet], path: str | Path) -> None:
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=["sheet_id", "width_mm", "height_mm", "panel_count", "utilization_pct", "panel_ids"])
        writer.writeheader()
        for sheet in sheets:
            record = sheet.to_record()
            writer.writerow({
                "sheet_id": record["sheet_id"],
                "width_mm": record["width_mm"],
                "height_mm": record["height_mm"],
                "panel_count": record["panel_count"],
                "utilization_pct": record["utilization_pct"],
                "panel_ids": json.dumps([p["panel_id"] for p in record["panels"]]),
            })


def export_nested_sheets_dxf(sheets: list[NestingSheet], path: str | Path, sheet_gap_mm: float = 200.0) -> bool:
    """Draw each sheet's border plus its nested panel outlines, stacked in a column.

    Each panel keeps the position the shelf-packer already computed for it —
    this is the actual cut plan, not a re-layout for display purposes.
    """
    if ezdxf is None or not sheets:
        return False

    doc = ezdxf.new()
    msp = doc.modelspace()

    cursor_y = 0.0
    for sheet in sheets:
        border = [
            (0.0, cursor_y), (sheet.width_mm, cursor_y),
            (sheet.width_mm, cursor_y + sheet.height_mm), (0.0, cursor_y + sheet.height_mm),
        ]
        msp.add_lwpolyline(border + [border[0]], dxfattribs={"layer": "SHEET-BORDER"})
        msp.add_text(
            f"{sheet.sheet_id}  ({len(sheet.panels)} panels, {sheet.utilization_pct:.1f}% utilized)",
            dxfattribs={"height": 40.0},
        ).set_placement((10.0, cursor_y - 50.0))

        for panel in sheet.panels:
            outline = [(x, y + cursor_y) for x, y in panel.outline_mm]
            msp.add_lwpolyline(outline + [outline[0]], dxfattribs={"layer": panel.family_id or "UNGROUPED"})
            label_x = sum(x for x, _ in outline) / len(outline)
            label_y = sum(y for _, y in outline) / len(outline)
            msp.add_text(panel.panel_id, dxfattribs={"height": 18.0}).set_placement((label_x, label_y))

        cursor_y += sheet.height_mm + sheet_gap_mm

    doc.saveas(str(path))
    return True


def export_nesting(sheets: list[NestingSheet], output_dir: str | Path) -> dict[str, str]:
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    paths = {
        "nesting_schedule_json": output_dir / "nesting_schedule.json",
        "nesting_schedule_csv": output_dir / "nesting_schedule.csv",
        "flat_patterns_dxf": output_dir / "flat_patterns.dxf",
    }

    export_nesting_schedule_json(sheets, paths["nesting_schedule_json"])
    export_nesting_schedule_csv(sheets, paths["nesting_schedule_csv"])
    wrote_dxf = export_nested_sheets_dxf(sheets, paths["flat_patterns_dxf"])
    if not wrote_dxf:
        del paths["flat_patterns_dxf"]

    return {k: str(v) for k, v in paths.items()}


def export_mold_schedule_json(molds: list[Mold], path: str | Path) -> None:
    records = [m.to_record() for m in molds]
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(records, fh, indent=2)


def export_mold_schedule_csv(molds: list[Mold], path: str | Path) -> None:
    fieldnames = [
        "mold_id", "mold_type", "family_id", "panel_ids", "panel_count", "material",
        "cavity_outline_mm", "cavity_area_mm2", "panel_thickness_mm", "edge_dam_height_mm",
        "bevel_angle_deg", "draft_angle_deg", "registration_holes", "demold_slots",
        "insert_locator_points", "label_text",
    ]
    nested_fields = {
        "panel_ids", "cavity_outline_mm", "registration_holes", "demold_slots", "insert_locator_points",
    }
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for mold in molds:
            record = mold.to_record()
            writer.writerow({
                key: (json.dumps(record[key]) if key in nested_fields else record[key])
                for key in fieldnames
            })


def export_molds_dxf(molds: list[Mold], path: str | Path, spacing_mm: float = 150.0) -> bool:
    """Lay out each mold's cavity, registration holes, demold slot, and insert
    locators on a grid, one drawing per family — labeled and ready for CNC review.
    """
    if ezdxf is None or not molds:
        return False

    doc = ezdxf.new()
    msp = doc.modelspace()

    cols = max(1, int(math.sqrt(len(molds))))
    cell = max((m.cavity_area_mm2 for m in molds), default=1.0) ** 0.5 + spacing_mm

    for idx, mold in enumerate(molds):
        row, col = divmod(idx, cols)
        offset = np.array([col * cell, row * cell])

        outline = [tuple(np.array(p) + offset) for p in mold.cavity_outline_mm]
        msp.add_lwpolyline(outline + [outline[0]], dxfattribs={"layer": "CAVITY"})

        for hole in mold.registration_holes:
            center = (hole["x"] + offset[0], hole["y"] + offset[1])
            msp.add_circle(center, hole["diameter_mm"] / 2.0, dxfattribs={"layer": "REGISTRATION"})

        for point in mold.insert_locator_points:
            center = (point["x"] + offset[0], point["y"] + offset[1])
            msp.add_circle(center, point["diameter_mm"] / 2.0, dxfattribs={"layer": "INSERT-LOCATOR"})

        for slot in mold.demold_slots:
            cx, cy = slot["x"] + offset[0], slot["y"] + offset[1]
            half_len, half_wid = slot["length_mm"] / 2.0, slot["width_mm"] / 2.0
            angle = math.radians(slot["angle_deg"])
            cos_a, sin_a = math.cos(angle), math.sin(angle)
            corners = []
            for dx, dy in [(-half_len, -half_wid), (half_len, -half_wid), (half_len, half_wid), (-half_len, half_wid)]:
                corners.append((cx + dx * cos_a - dy * sin_a, cy + dx * sin_a + dy * cos_a))
            msp.add_lwpolyline(corners + [corners[0]], dxfattribs={"layer": "DEMOLD-SLOT"})

        label_pos = (offset[0], offset[1] - 20.0)
        msp.add_text(mold.label_text, dxfattribs={"height": 24.0}).set_placement(label_pos)

    doc.saveas(str(path))
    return True


def export_molds(molds: list[Mold], output_dir: str | Path) -> dict[str, str]:
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    paths = {
        "mold_schedule_json": output_dir / "mold_schedule.json",
        "mold_schedule_csv": output_dir / "mold_schedule.csv",
        "molds_dxf": output_dir / "molds.dxf",
    }

    export_mold_schedule_json(molds, paths["mold_schedule_json"])
    export_mold_schedule_csv(molds, paths["mold_schedule_csv"])
    wrote_dxf = export_molds_dxf(molds, paths["molds_dxf"])
    if not wrote_dxf:
        del paths["molds_dxf"]

    return {k: str(v) for k, v in paths.items()}


def export_all(mesh: trimesh.Trimesh, panels: list[Panel], output_dir: str | Path) -> dict[str, str]:
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    paths = {
        "mesh_obj": output_dir / "maker_dome.obj",
        "panel_schedule_json": output_dir / "panel_schedule.json",
        "panel_schedule_csv": output_dir / "panel_schedule.csv",
        "panel_families_csv": output_dir / "panel_families.csv",
    }

    export_shell_mesh(mesh, panels, paths["mesh_obj"])
    export_panel_schedule_json(panels, paths["panel_schedule_json"])
    export_panel_schedule_csv(panels, paths["panel_schedule_csv"])
    export_family_summary_csv(panels, paths["panel_families_csv"])

    return {k: str(v) for k, v in paths.items()}
