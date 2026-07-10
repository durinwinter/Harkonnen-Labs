"""Geometry sanity checks for the maker dome pipeline."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pytest

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))

from sietch_panelizer.config import DomeConfig
from sietch_panelizer.export import export_all
from sietch_panelizer.families import assign_families
from sietch_panelizer.mesh import build_dome_mesh
from sietch_panelizer.openings import apply_openings
from sietch_panelizer.panel import build_panels


@pytest.fixture(scope="module")
def small_config() -> DomeConfig:
    # Small grid keeps the test fast while exercising the full pipeline.
    return DomeConfig(
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


@pytest.fixture(scope="module")
def pipeline(small_config):
    mesh = build_dome_mesh(small_config)
    panels = build_panels(mesh)
    apply_openings(panels, small_config)
    assign_families(panels, small_config)
    return small_config, mesh, panels


def test_mesh_has_no_duplicate_vertices(pipeline):
    _, mesh, _ = pipeline
    unique = np.unique(np.round(mesh.vertices, 6), axis=0)
    assert len(unique) == len(mesh.vertices)


def test_panel_areas_are_positive(pipeline):
    _, _, panels = pipeline
    for panel in panels:
        assert panel.area_mm2 > 0.0


def test_edge_lengths_are_valid(pipeline):
    _, _, panels = pipeline
    for panel in panels:
        assert len(panel.edge_lengths) == 3
        for length in panel.edge_lengths:
            assert np.isfinite(length)
            assert length > 0.0


def test_non_opening_panels_have_neighbors(pipeline):
    _, _, panels = pipeline
    for panel in panels:
        if panel.is_opening:
            continue
        assert len(panel.neighbor_ids) > 0


def test_door_opening_was_flagged(pipeline):
    _, _, panels = pipeline
    opening_panels = [p for p in panels if p.is_opening]
    assert len(opening_panels) > 0
    assert all(p.panel_type == "opening_adjacent" for p in opening_panels)


def test_families_were_assigned(pipeline):
    _, _, panels = pipeline
    fabricated = [p for p in panels if not p.is_opening]
    assert all(p.mold_family_id is not None for p in fabricated)
    family_ids = {p.mold_family_id for p in fabricated}
    assert 0 < len(family_ids) <= len(fabricated)


def test_export_writes_panel_schedule(pipeline, tmp_path):
    config, mesh, panels = pipeline
    written = export_all(mesh, panels, tmp_path)

    assert Path(written["panel_schedule_json"]).exists()
    assert Path(written["panel_schedule_csv"]).exists()
    assert Path(written["panel_families_csv"]).exists()
    assert Path(written["mesh_obj"]).exists()
    assert Path(written["mesh_obj"]).stat().st_size > 0
    assert Path(written["panel_schedule_json"]).stat().st_size > 0
