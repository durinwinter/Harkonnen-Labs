"""Sanity checks for mold generation: one mold per family, fully specified."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pytest

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))

from sietch_panelizer.config import DomeConfig
from sietch_panelizer.export import export_molds
from sietch_panelizer.families import assign_families
from sietch_panelizer.mesh import build_dome_mesh
from sietch_panelizer.mold import FLAT_FACETED_MOLD, build_molds
from sietch_panelizer.openings import apply_openings
from sietch_panelizer.panel import build_panels


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
    molds = build_molds(panels, config)
    return config, panels, molds


def test_one_mold_per_family_no_duplicates(pipeline):
    _, panels, molds = pipeline
    family_ids = {p.mold_family_id for p in panels if not p.is_opening}
    mold_family_ids = [m.family_id for m in molds]

    assert len(molds) == len(family_ids)
    assert len(mold_family_ids) == len(set(mold_family_ids))
    assert set(mold_family_ids) == family_ids


def test_mold_panel_links_cover_every_fabricated_panel(pipeline):
    _, panels, molds = pipeline
    fabricated_ids = {p.panel_id for p in panels if not p.is_opening}

    linked_ids = [pid for mold in molds for pid in mold.panel_ids]
    assert len(linked_ids) == len(set(linked_ids))  # no panel claimed by two molds
    assert set(linked_ids) == fabricated_ids


def test_mold_geometry_matches_its_family(pipeline):
    _, panels, molds = pipeline
    panels_by_id = {p.panel_id: p for p in panels}

    for mold in molds:
        assert mold.mold_type == FLAT_FACETED_MOLD
        representative = panels_by_id[mold.panel_ids[0]]
        outline_edges = [
            float(np.linalg.norm(np.subtract(mold.cavity_outline_mm[(i + 1) % 3], mold.cavity_outline_mm[i])))
            for i in range(3)
        ]
        assert outline_edges == pytest.approx(representative.edge_lengths, abs=1e-6)
        assert mold.cavity_area_mm2 == pytest.approx(representative.area_mm2)


def test_mold_features_are_present(pipeline):
    _, _, molds = pipeline
    for mold in molds:
        assert len(mold.registration_holes) == 2
        assert len(mold.demold_slots) == 1
        assert len(mold.insert_locator_points) == 3
        assert mold.label_text
        assert mold.material
        assert mold.panel_thickness_mm > 0
        assert mold.edge_dam_height_mm > 0


def test_mold_disabled_via_config_yields_no_molds(pipeline):
    import dataclasses
    from sietch_panelizer.config import MoldConfig

    config, panels, _ = pipeline
    disabled = dataclasses.replace(config, mold=dataclasses.replace(config.mold, enabled=False))
    assert isinstance(disabled.mold, MoldConfig)
    assert build_molds(panels, disabled) == []


def test_mold_export_writes_schedule(pipeline, tmp_path):
    _, _, molds = pipeline
    written = export_molds(molds, tmp_path)

    assert Path(written["mold_schedule_json"]).exists()
    assert Path(written["mold_schedule_csv"]).exists()
    assert Path(written["mold_schedule_json"]).stat().st_size > 0
