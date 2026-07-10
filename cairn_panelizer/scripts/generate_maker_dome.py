#!/usr/bin/env python3
"""CLI: generate a parametric Sietch Maker Dome shell and fabrication files.

Usage:
    python scripts/generate_maker_dome.py --config examples/maker_default.yaml
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))

from sietch_panelizer.assembly import generate_assembly_sequence
from sietch_panelizer.config import DomeConfig
from sietch_panelizer.connections import generate_connections
from sietch_panelizer.cost import calculate_cost
from sietch_panelizer.cure import predict_cure_schedule
from sietch_panelizer.export import export_all, export_molds, export_nesting
from sietch_panelizer.factory_package import generate_factory_package
from sietch_panelizer.families import assign_families
from sietch_panelizer.materials import assign_recipes
from sietch_panelizer.mesh import build_dome_mesh
from sietch_panelizer.mold import build_molds
from sietch_panelizer.nesting import nest_panels
from sietch_panelizer.openings import apply_openings
from sietch_panelizer.panel import build_panels
from sietch_panelizer.structure_check import run_structural_precheck
from sietch_panelizer.viewer_data import write_viewer_data
from sietch_panelizer.visualize import render_preview


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config",
        default=str(PACKAGE_ROOT / "examples" / "maker_default.yaml"),
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
    print(f"[sietch-panelizer] loaded config from {args.config}")
    print(f"[sietch-panelizer] base_radius={config.base_radius_mm:.0f}mm "
          f"height={config.height_mm:.0f}mm lobes={config.lobe_count}")

    mesh = build_dome_mesh(config)
    panels = build_panels(mesh)
    print(f"[sietch-panelizer] generated {len(panels)} panels "
          f"({config.ring_segments} rings x {config.angular_segments} segments)")

    opening_counts = apply_openings(panels, config)
    print(f"[sietch-panelizer] flagged openings: {opening_counts}")

    family_count = assign_families(panels, config)
    print(f"[sietch-panelizer] grouped panels into {family_count} mold families "
          f"(tolerance={config.family_tolerance_mm}mm)")

    outputs_dir = Path(args.outputs)
    written = export_all(mesh, panels, outputs_dir)

    molds = []
    if config.mold.enabled:
        molds = build_molds(panels, config)
        print(f"[sietch-panelizer] generated {len(molds)} mold designs "
              f"(type={config.mold.default_type}, material={config.mold.material})")
        written.update(export_molds(molds, outputs_dir))
    else:
        print("[sietch-panelizer] mold generation disabled (mold.enabled: false)")

    sheets = nest_panels(panels, config)
    avg_utilization = sum(s.utilization_pct for s in sheets) / len(sheets) if sheets else 0.0
    print(f"[sietch-panelizer] nested flat patterns onto {len(sheets)} sheet(s) "
          f"({config.nesting.sheet_width_mm:.0f}x{config.nesting.sheet_height_mm:.0f}mm, "
          f"avg utilization {avg_utilization:.1f}%)")
    written.update(export_nesting(sheets, outputs_dir))

    preview_path = outputs_dir / "preview.png"
    render_preview(panels, preview_path, label_panels=args.label_panels)
    written["preview_png"] = str(preview_path)

    viewer_data_path = write_viewer_data(config, panels, molds, sheets, PACKAGE_ROOT / "viewer" / "viewer_data.js")
    written["viewer_data"] = str(viewer_data_path)
    print(f"[sietch-panelizer] wrote viewer data — open viewer/index.html in a browser to explore it")

    # --- Manufacturing intelligence: materials -> cost -> connections -> assembly -> cure -> structure ---
    library = assign_recipes(panels, config)
    if config.layers:
        total_mass_kg = sum(p.estimated_mass_kg or 0.0 for p in panels if not p.is_opening)
        print(f"[sietch-panelizer] assigned {len(config.layers)}-layer material stack "
              f"({', '.join(layer.name for layer in config.layers)}) — est. {total_mass_kg:,.0f}kg total")
    else:
        print("[sietch-panelizer] no material layers configured — skipping mass/cost estimation detail")

    cost_report = calculate_cost(panels, molds, config, library)
    print(f"[sietch-panelizer] cost estimate: ${cost_report.total_medium_usd:,.0f} "
          f"(${cost_report.total_low_usd:,.0f}-${cost_report.total_high_usd:,.0f}), "
          f"${cost_report.cost_per_sqm_usd:,.0f}/sqm")

    connections = generate_connections(panels, config, library)
    print(f"[sietch-panelizer] generated {len(connections)} panel-to-panel connections")

    assembly_steps = generate_assembly_sequence(panels, connections, config)
    total_assembly_hr = sum(s.estimated_duration_min for s in assembly_steps) / 60.0
    print(f"[sietch-panelizer] generated a {len(assembly_steps)}-step assembly sequence "
          f"(~{total_assembly_hr:.0f} crew-hours estimated)")

    cure_predictions = predict_cure_schedule(panels, library, config)
    print(f"[sietch-panelizer] predicted cure timing for {len(cure_predictions)} panels "
          f"(ambient {config.cure.ambient_temp_C:.0f}C / {config.cure.ambient_rh_pct:.0f}% RH)")

    structural_report = run_structural_precheck(panels, connections, config)
    fea_count = sum(1 for f in structural_report.flags if f.requires_fea)
    print(f"[sietch-panelizer] structural pre-check (PRELIMINARY ONLY): "
          f"{fea_count} panel(s) flagged requires_fea")

    package_dir = outputs_dir / "factory_package"
    package_paths = generate_factory_package(
        config=config,
        panels=panels,
        molds=molds,
        sheets=sheets,
        connections=connections,
        assembly_steps=assembly_steps,
        cure_predictions=cure_predictions,
        cost_report=cost_report,
        structural_report=structural_report,
        library=library,
        output_dir=package_dir,
        source_outputs_dir=outputs_dir,
    )
    written["factory_package_dir"] = str(package_dir)
    print(f"[sietch-panelizer] wrote complete factory package to {package_dir}")

    print("[sietch-panelizer] wrote outputs:")
    for name, path in written.items():
        print(f"    {name}: {path}")
    print("[sietch-panelizer] factory package contents:")
    for name, path in package_paths.items():
        print(f"    {name}: {path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
