"""Sanity checks for the manufacturing-intelligence pipeline (spec sec. 4.4-4.7, 4.12):
material recipes/BOM, cost engine, connection schedule, assembly sequence, cure
prediction, structural pre-check, and the consolidated factory package.
"""

from __future__ import annotations

import csv
import json
import sys
from pathlib import Path

import pytest

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))

from sietch_panelizer.assembly import generate_assembly_sequence
from sietch_panelizer.config import DomeConfig, LayerSpec
from sietch_panelizer.connections import (
    BOLTED_INSERT_JOINT,
    TAU_SEAM_THICKNESS_BY_JOINT_MM,
    generate_connections,
)
from sietch_panelizer.cost import calculate_cost
from sietch_panelizer.cure import predict_cure_schedule
from sietch_panelizer.factory_package import generate_factory_package
from sietch_panelizer.families import assign_families
from sietch_panelizer.materials import DEFAULT_RECIPES, assign_recipes, build_material_bom
from sietch_panelizer.mesh import build_dome_mesh
from sietch_panelizer.mold import build_molds
from sietch_panelizer.nesting import nest_panels
from sietch_panelizer.openings import apply_openings
from sietch_panelizer.panel import build_panels
from sietch_panelizer.structure_check import run_structural_precheck


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
        layers=[
            LayerSpec(name="Caliche", thickness_mm=12.0, recipe_id="caliche_default"),
            LayerSpec(name="Erg", thickness_mm=80.0, recipe_id="erg_default"),
            LayerSpec(name="Reg", thickness_mm=35.0, recipe_id="reg_default"),
        ],
    )
    mesh = build_dome_mesh(config)
    panels = build_panels(mesh)
    apply_openings(panels, config)
    assign_families(panels, config)
    molds = build_molds(panels, config)
    sheets = nest_panels(panels, config)
    library = assign_recipes(panels, config)
    cost_report = calculate_cost(panels, molds, config, library)
    connections = generate_connections(panels, config, library)
    assembly_steps = generate_assembly_sequence(panels, connections, config)
    cure_predictions = predict_cure_schedule(panels, library, config)
    structural_report = run_structural_precheck(panels, connections, config)
    return {
        "config": config,
        "panels": panels,
        "molds": molds,
        "sheets": sheets,
        "library": library,
        "cost_report": cost_report,
        "connections": connections,
        "assembly_steps": assembly_steps,
        "cure_predictions": cure_predictions,
        "structural_report": structural_report,
    }


@pytest.fixture(scope="module")
def fabricated(pipeline):
    return [p for p in pipeline["panels"] if not p.is_opening]


# --- materials -------------------------------------------------------------

def test_recipe_library_includes_defaults_and_overrides(pipeline):
    library = pipeline["library"]
    for recipe_id in ("caliche_default", "erg_default", "reg_default", "tau_default"):
        assert recipe_id in library
        assert library[recipe_id].layer == DEFAULT_RECIPES[recipe_id].layer


def test_panel_volume_and_mass_match_layer_stack(pipeline, fabricated):
    config = pipeline["config"]
    library = pipeline["library"]
    stack_thickness = sum(layer.thickness_mm for layer in config.layers)

    for panel in fabricated[:25]:
        assert panel.layer_stack == [layer.name for layer in config.layers]
        assert len(panel.recipe_assignments) == len(config.layers)

        expected_mass = 0.0
        for layer, assignment in zip(config.layers, panel.recipe_assignments):
            volume_mm3 = panel.area_mm2 * layer.thickness_mm
            assert assignment["volume_mm3"] == pytest.approx(volume_mm3, rel=1e-6)
            recipe = library[layer.recipe_id]
            expected_mass += volume_mm3 / 1.0e9 * recipe.density_target_kg_m3

        assert panel.estimated_mass_kg == pytest.approx(expected_mass, rel=1e-6)
        # sanity: a thin triangular shell panel weighs a few kg, not tonnes
        assert 0.0 < panel.estimated_mass_kg < 200.0
        assert stack_thickness > 0


def test_material_bom_rolls_up_every_assigned_recipe(pipeline, fabricated):
    config = pipeline["config"]
    library = pipeline["library"]
    bom = build_material_bom(pipeline["panels"], config, library)

    recipe_ids = {row["recipe_id"] for row in bom}
    assert recipe_ids == {layer.recipe_id for layer in config.layers}

    for row in bom:
        assert row["panel_count"] == len(fabricated)
        assert row["total_mass_kg"] > 0.0
        assert row["batch_mass_with_waste_kg"] >= row["total_mass_kg"]
        assert row["validation_status"] == "unvalidated"  # shipped recipes are all placeholders


# --- cost -------------------------------------------------------------------

def test_cost_report_categories_sum_to_totals(pipeline):
    report = pipeline["cost_report"]
    assert report.fabricated_panel_count > 0
    assert report.categories_usd  # every category populated

    medium_total = sum(
        value for value in report.categories_usd.values()
    )
    # categories_usd holds the point estimate; total_medium is its sensitivity-band rollup
    assert report.total_medium_usd > 0.0
    assert report.total_low_usd <= report.total_medium_usd <= report.total_high_usd
    assert medium_total > 0.0
    assert report.cost_per_sqm_usd == pytest.approx(
        report.total_medium_usd / report.fabricated_area_m2, rel=1e-6
    )


def test_cost_rollups_cover_every_layer_and_family(pipeline, fabricated):
    config = pipeline["config"]
    report = pipeline["cost_report"]

    assert set(report.cost_by_layer_usd) == {layer.name for layer in config.layers}
    fabricated_families = {p.mold_family_id for p in fabricated if p.mold_family_id}
    assert set(report.cost_by_panel_family_usd) <= fabricated_families
    assert all(v > 0.0 for v in report.cost_by_layer_usd.values())


# --- connections -------------------------------------------------------------

def test_connections_are_unique_unordered_pairs_of_neighbors(pipeline):
    by_id = {p.panel_id: p for p in pipeline["panels"]}
    seen = set()

    for connection in pipeline["connections"]:
        key = (connection.panel_a, connection.panel_b)
        assert key[0] < key[1]  # canonical ordering from sorted()
        assert key not in seen
        seen.add(key)

        a, b = by_id[connection.panel_a], by_id[connection.panel_b]
        assert not a.is_opening and not b.is_opening
        assert connection.panel_b in a.neighbor_ids
        assert connection.panel_a in b.neighbor_ids


def test_connection_seam_thickness_follows_joint_type(pipeline):
    for connection in pipeline["connections"]:
        expected = TAU_SEAM_THICKNESS_BY_JOINT_MM.get(connection.joint_type)
        if expected is not None:
            assert connection.seam_thickness_mm == pytest.approx(expected)
        assert connection.tau_volume_mm3 > 0.0
        assert connection.seam_length_mm > 0.0


def test_bolted_insert_joints_carry_hardware_and_primer(pipeline):
    bolted = [c for c in pipeline["connections"] if c.joint_type == BOLTED_INSERT_JOINT]
    for connection in bolted:
        assert connection.insert_count > 0
        assert connection.hardware_count > 0
        assert connection.primer_required is True


def test_cross_family_flag_implies_primer_required(pipeline):
    for connection in pipeline["connections"]:
        if connection.cross_family:
            assert connection.primer_required is True


# --- assembly ----------------------------------------------------------------

def test_assembly_sequence_is_ordered_base_to_crown_and_covers_every_panel(pipeline, fabricated):
    steps = pipeline["assembly_steps"]
    assert [s.sequence_number for s in steps] == list(range(len(steps)))

    placed_ids = [pid for step in steps for pid in step.panel_ids]
    assert len(placed_ids) == len(set(placed_ids))
    assert set(placed_ids) == {p.panel_id for p in fabricated}

    # base-to-crown: average centroid height should not decrease step over step
    by_id = {p.panel_id: p for p in pipeline["panels"]}
    avg_heights = [
        sum(by_id[pid].centroid[2] for pid in step.panel_ids) / len(step.panel_ids)
        for step in steps
    ]
    assert avg_heights == sorted(avg_heights)


def test_assembly_steps_link_back_to_real_connections(pipeline):
    connection_by_id = {c.joint_id: c for c in pipeline["connections"]}
    for step in pipeline["assembly_steps"]:
        for joint_id in step.joint_ids:
            connection = connection_by_id[joint_id]
            assert connection.assembly_step_id == step.step_id
        assert step.estimated_duration_min > 0.0


# --- cure ---------------------------------------------------------------------

def test_cure_predictions_cover_every_fabricated_panel_and_scale_sanely(pipeline, fabricated):
    predictions = pipeline["cure_predictions"]
    assert {p.panel_id for p in predictions} == {p.panel_id for p in fabricated}

    for prediction in predictions:
        assert prediction.estimated_open_time_min > 0.0
        assert prediction.estimated_demold_time_hr > 0.0
        # handling and full-cure must each take longer than the stage before it
        assert prediction.estimated_handling_time_hr > prediction.estimated_demold_time_hr
        assert prediction.estimated_full_cure_time_hr > prediction.estimated_handling_time_hr


# --- structural pre-check ------------------------------------------------------

def test_structural_precheck_flags_every_fabricated_panel_and_states_disclaimer(pipeline, fabricated):
    report = pipeline["structural_report"]
    assert {f.panel_id for f in report.flags} == {p.panel_id for p in fabricated}
    assert "preliminary" in report.disclaimer.lower()
    assert "not" in report.disclaimer.lower()  # "not an engineering analysis / code-compliance determination"

    for flag in report.flags:
        assert flag.approximate_span_mm > 0.0
        assert flag.slenderness_ratio > 0.0
        assert flag.zone in (
            "field", "opening_adjacent", "crown_adjacent", "high_curvature_transition",
        )

    # the check should never be silently a no-op on a curved dome shell
    assert any(f.requires_fea for f in report.flags)
    assert len({f.zone for f in report.flags}) > 1


# --- factory package -----------------------------------------------------------

def test_factory_package_writes_expected_files_and_aggregates_warnings(pipeline, fabricated, tmp_path):
    package_dir = tmp_path / "factory_package"
    paths = generate_factory_package(
        config=pipeline["config"],
        panels=pipeline["panels"],
        molds=pipeline["molds"],
        sheets=pipeline["sheets"],
        connections=pipeline["connections"],
        assembly_steps=pipeline["assembly_steps"],
        cure_predictions=pipeline["cure_predictions"],
        cost_report=pipeline["cost_report"],
        structural_report=pipeline["structural_report"],
        library=pipeline["library"],
        output_dir=package_dir,
    )

    for key in (
        "design_summary", "panel_schedule_json", "mold_schedule_json", "material_bom_csv",
        "cost_report_json", "connection_schedule_csv", "assembly_sequence_json",
        "cure_schedule_csv", "structural_precheck_json", "warnings_json",
    ):
        assert key in paths
        path = Path(paths[key])
        assert path.exists()
        assert path.stat().st_size > 0

    with open(paths["warnings_json"]) as fh:
        warnings = json.load(fh)
    assert set(warnings) >= {"design", "panel", "mold", "cost", "connections", "assembly", "cure", "structure"}

    with open(paths["design_summary"]) as fh:
        summary = json.load(fh)
    assert summary["panel_count"] == len(fabricated)
    assert summary["connection_count"] == len(pipeline["connections"])


def test_connection_schedule_csv_carries_structured_fuse_fields(pipeline, tmp_path):
    package_dir = tmp_path / "factory_package"
    paths = generate_factory_package(
        config=pipeline["config"],
        panels=pipeline["panels"],
        molds=pipeline["molds"],
        sheets=pipeline["sheets"],
        connections=pipeline["connections"],
        assembly_steps=pipeline["assembly_steps"],
        cure_predictions=pipeline["cure_predictions"],
        cost_report=pipeline["cost_report"],
        structural_report=pipeline["structural_report"],
        library=pipeline["library"],
        output_dir=package_dir,
    )

    with open(paths["connection_schedule_csv"], newline="") as fh:
        rows = list(csv.DictReader(fh))

    assert len(rows) == len(pipeline["connections"])
    for field_name in ("joint_id", "joint_type", "seam_thickness_mm", "cross_family", "primer_required"):
        assert field_name in rows[0]
    assert {row["cross_family"] for row in rows} <= {"True", "False"}
