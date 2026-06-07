#!/usr/bin/env python3
"""CLI: generate a parametric Cairn Trillium Dome shell and fabrication files.

Usage:
    python scripts/generate_trillium_dome.py --config examples/trillium_default.yaml
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))

from cairn_panelizer.config import DomeConfig
from cairn_panelizer.export import export_all, export_molds, export_nesting
from cairn_panelizer.families import assign_families
from cairn_panelizer.mesh import build_dome_mesh
from cairn_panelizer.mold import build_molds
from cairn_panelizer.nesting import nest_panels
from cairn_panelizer.openings import apply_openings
from cairn_panelizer.panel import build_panels
from cairn_panelizer.viewer_data import write_viewer_data
from cairn_panelizer.visualize import render_preview


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config",
        default=str(PACKAGE_ROOT / "examples" / "trillium_default.yaml"),
        help="Path to a YAML dome configuration file.",
    )
    parser.add_argument(
        "--outputs",
        default=str(PACKAGE_ROOT / "outputs"),
        help="Directory to write generated fabrication files into.",
    )
    parser.add_argument(
        "--label-panels",
        action="store_true",
        help="Render panel IDs on the preview image (slower, busier on dense meshes).",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)

    config = DomeConfig.from_yaml(args.config)
    print(f"[cairn-panelizer] loaded config from {args.config}")
    print(f"[cairn-panelizer] base_radius={config.base_radius_mm:.0f}mm "
          f"height={config.height_mm:.0f}mm lobes={config.lobe_count}")

    mesh = build_dome_mesh(config)
    panels = build_panels(mesh)
    print(f"[cairn-panelizer] generated {len(panels)} panels "
          f"({config.ring_segments} rings x {config.angular_segments} segments)")

    opening_counts = apply_openings(panels, config)
    print(f"[cairn-panelizer] flagged openings: {opening_counts}")

    family_count = assign_families(panels, config)
    print(f"[cairn-panelizer] grouped panels into {family_count} mold families "
          f"(tolerance={config.family_tolerance_mm}mm)")

    outputs_dir = Path(args.outputs)
    written = export_all(mesh, panels, outputs_dir)

    molds = []
    if config.mold.enabled:
        molds = build_molds(panels, config)
        print(f"[cairn-panelizer] generated {len(molds)} mold designs "
              f"(type={config.mold.default_type}, material={config.mold.material})")
        written.update(export_molds(molds, outputs_dir))
    else:
        print("[cairn-panelizer] mold generation disabled (mold.enabled: false)")

    sheets = nest_panels(panels, config)
    avg_utilization = sum(s.utilization_pct for s in sheets) / len(sheets) if sheets else 0.0
    print(f"[cairn-panelizer] nested flat patterns onto {len(sheets)} sheet(s) "
          f"({config.nesting.sheet_width_mm:.0f}x{config.nesting.sheet_height_mm:.0f}mm, "
          f"avg utilization {avg_utilization:.1f}%)")
    written.update(export_nesting(sheets, outputs_dir))

    preview_path = outputs_dir / "preview.png"
    render_preview(panels, preview_path, label_panels=args.label_panels)
    written["preview_png"] = str(preview_path)

    viewer_data_path = write_viewer_data(config, panels, molds, sheets, PACKAGE_ROOT / "viewer" / "viewer_data.js")
    written["viewer_data"] = str(viewer_data_path)
    print(f"[cairn-panelizer] wrote viewer data — open viewer/index.html in a browser to explore it")

    print("[cairn-panelizer] wrote outputs:")
    for name, path in written.items():
        print(f"    {name}: {path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
