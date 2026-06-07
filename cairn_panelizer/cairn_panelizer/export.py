"""Fabrication-file exporters: mesh, panel schedules, family summary, flat DXF."""

from __future__ import annotations

import csv
import json
import math
from pathlib import Path

import numpy as np
import trimesh

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
        "dihedral_angles_deg",
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


def _flatten_triangle(edge_lengths: list[float]) -> list[tuple[float, float]]:
    """Unroll a planar triangle into 2D, preserving its three edge lengths exactly."""
    e0, e1, e2 = edge_lengths
    p0 = (0.0, 0.0)
    p1 = (e0, 0.0)
    cos_angle = (e0 ** 2 + e2 ** 2 - e1 ** 2) / (2.0 * e0 * e2)
    cos_angle = max(-1.0, min(1.0, cos_angle))
    angle = math.acos(cos_angle)
    p2 = (e2 * math.cos(angle), e2 * math.sin(angle))
    return [p0, p1, p2]


def export_flat_panels_dxf(panels: list[Panel], path: str | Path, spacing_mm: float = 100.0) -> bool:
    """Lay out flattened triangular panels on a grid in a single DXF for cutting/nesting.

    Returns False (and writes nothing) if ezdxf is unavailable or panels are
    not triangles — flattening quads requires a fold-line decomposition that
    is left for a later iteration.
    """
    fab_panels = [p for p in panels if not p.is_opening]
    if ezdxf is None or not fab_panels or len(fab_panels[0].edge_lengths) != 3:
        return False

    doc = ezdxf.new()
    msp = doc.modelspace()

    cols = max(1, int(math.sqrt(len(fab_panels))))
    cell = max((p.area_mm2 for p in fab_panels), default=1.0) ** 0.5 + spacing_mm

    for idx, panel in enumerate(fab_panels):
        outline = _flatten_triangle(panel.edge_lengths)
        row, col = divmod(idx, cols)
        offset = (col * cell, row * cell)
        shifted = [(x + offset[0], y + offset[1]) for x, y in outline]
        msp.add_lwpolyline(shifted + [shifted[0]], dxfattribs={"layer": panel.mold_family_id or "UNGROUPED"})
        label_pos = (offset[0], offset[1] - 20.0)
        msp.add_text(panel.panel_id, dxfattribs={"height": 30.0}).set_placement(label_pos)

    doc.saveas(str(path))
    return True


def export_all(mesh: trimesh.Trimesh, panels: list[Panel], output_dir: str | Path) -> dict[str, str]:
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    paths = {
        "mesh_obj": output_dir / "trillium_dome.obj",
        "panel_schedule_json": output_dir / "panel_schedule.json",
        "panel_schedule_csv": output_dir / "panel_schedule.csv",
        "panel_families_csv": output_dir / "panel_families.csv",
        "flat_panels_dxf": output_dir / "flat_panels.dxf",
    }

    export_shell_mesh(mesh, panels, paths["mesh_obj"])
    export_panel_schedule_json(panels, paths["panel_schedule_json"])
    export_panel_schedule_csv(panels, paths["panel_schedule_csv"])
    export_family_summary_csv(panels, paths["panel_families_csv"])
    wrote_dxf = export_flat_panels_dxf(panels, paths["flat_panels_dxf"])
    if not wrote_dxf:
        del paths["flat_panels_dxf"]

    return {k: str(v) for k, v in paths.items()}
