"""Bundle dome/panel/mold/nesting data into a JS file the static viewer loads.

The viewer is a plain HTML/JS page opened directly via `file://`, so the
data is written as a `<script>`-loadable `window.SIETCH_VIEWER_DATA = {...}`
assignment rather than fetched as JSON — `fetch()` of local files is blocked
by browser CORS policy, but `<script src>` is not.
"""

from __future__ import annotations

import json
from pathlib import Path

from .config import DomeConfig
from .mold import Mold
from .nesting import NestingSheet
from .panel import Panel


def build_viewer_bundle(config: DomeConfig, panels: list[Panel], molds: list[Mold],
                        sheets: list[NestingSheet]) -> dict:
    fabricated = [p for p in panels if not p.is_opening]
    family_ids = sorted({p.mold_family_id for p in fabricated if p.mold_family_id is not None})

    return {
        "meta": {
            "overall_width_mm": config.overall_width_mm,
            "height_mm": config.height_mm,
            "lobe_count": config.lobe_count,
            "crown_radius_mm": config.crown_radius_mm,
            "panel_count": len(panels),
            "fabricated_panel_count": len(fabricated),
            "opening_panel_count": len(panels) - len(fabricated),
            "family_count": len(family_ids),
            "mold_count": len(molds),
            "sheet_count": len(sheets),
        },
        "families": family_ids,
        "panels": [
            {
                "panel_id": p.panel_id,
                "panel_type": p.panel_type,
                "family_id": p.mold_family_id,
                "is_opening": p.is_opening,
                "vertices": [[round(c, 2) for c in v] for v in p.vertices],
                "centroid": [round(c, 2) for c in p.centroid],
                "area_mm2": round(p.area_mm2, 2),
                "edge_lengths_mm": [round(e, 2) for e in p.edge_lengths],
                "neighbor_ids": p.neighbor_ids,
            }
            for p in panels
        ],
        "molds": [m.to_record() for m in molds],
        "sheets": [s.to_record() for s in sheets],
    }


def write_viewer_data(config: DomeConfig, panels: list[Panel], molds: list[Mold],
                       sheets: list[NestingSheet], path: str | Path) -> Path:
    bundle = build_viewer_bundle(config, panels, molds, sheets)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        fh.write("window.SIETCH_VIEWER_DATA = ")
        json.dump(bundle, fh)
        fh.write(";\n")
    return path
