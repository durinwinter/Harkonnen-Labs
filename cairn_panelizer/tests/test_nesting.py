"""Sanity checks for flat-pattern nesting: every panel placed once, in bounds."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))

from cairn_panelizer.config import DomeConfig
from cairn_panelizer.export import export_nesting
from cairn_panelizer.families import assign_families
from cairn_panelizer.mesh import build_dome_mesh
from cairn_panelizer.nesting import nest_panels
from cairn_panelizer.openings import apply_openings
from cairn_panelizer.panel import build_panels


@pytest.fixture(scope="module")
def pipeline():
    config = DomeConfig(
        overall_width_mm=6000,
        height_mm=3200,
        lobe_amplitude=0.18,
        lobe_count=3,
        angular_segments=24,
        ring_segments=6,
        crown_radius_mm=700,
        door_width_mm=900,
        door_height_mm=2050,
        door_angle_deg=180,
    )
    mesh = build_dome_mesh(config)
    panels = build_panels(mesh)
    apply_openings(panels, config)
    assign_families(panels, config)
    sheets = nest_panels(panels, config)
    return config, panels, sheets


def test_every_fabricated_panel_nested_exactly_once(pipeline):
    _, panels, sheets = pipeline
    fabricated_ids = {p.panel_id for p in panels if not p.is_opening}

    nested_ids = [nested.panel_id for sheet in sheets for nested in sheet.panels]
    assert len(nested_ids) == len(set(nested_ids))  # no panel placed twice
    assert set(nested_ids) == fabricated_ids


def test_nested_panels_stay_within_sheet_bounds(pipeline):
    config, _, sheets = pipeline
    margin = config.nesting.margin_mm

    for sheet in sheets:
        for panel in sheet.panels:
            xs = [x for x, _ in panel.outline_mm]
            ys = [y for _, y in panel.outline_mm]
            assert min(xs) >= margin - 1e-6
            assert min(ys) >= margin - 1e-6
            assert max(xs) <= sheet.width_mm - margin + 1e-6
            assert max(ys) <= sheet.height_mm - margin + 1e-6


def test_sheet_ids_are_unique_and_ordered(pipeline):
    _, _, sheets = pipeline
    sheet_ids = [s.sheet_id for s in sheets]
    assert len(sheet_ids) == len(set(sheet_ids))
    assert sheet_ids == [f"SHEET-{i:02d}" for i in range(len(sheets))]


def test_utilization_is_a_sane_percentage(pipeline):
    _, _, sheets = pipeline
    for sheet in sheets:
        assert 0.0 <= sheet.utilization_pct <= 100.0
        assert sheet.usable_area_mm2 > 0
        if sheet.panels:
            assert sheet.utilization_pct > 0.0


def test_nesting_export_writes_schedule(pipeline, tmp_path):
    _, _, sheets = pipeline
    written = export_nesting(sheets, tmp_path)

    assert Path(written["nesting_schedule_json"]).exists()
    assert Path(written["nesting_schedule_csv"]).exists()
    assert Path(written["flat_patterns_dxf"]).exists()
    assert Path(written["nesting_schedule_json"]).stat().st_size > 0
