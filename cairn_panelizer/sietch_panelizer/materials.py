"""Material recipe library, layer assignment, mass/volume estimation, and BOM.

Implements the "Material System Designer" (spec sec. 4.5): a small recipe
database keyed by `recipe_id`, a layer-stack assignment pass that estimates
volume/mass/cost per panel per layer, and a bill-of-materials rollup for the
factory package.

Recipes shipped here (`DEFAULT_RECIPES`) are placeholder formulations for the
four canonical Sietch layers — Reg, Caliche, Erg, Tau — based on the
MKPC (magnesium potassium phosphate ceramic) chemistry described in the Sietch
material mapping. They are marked `unvalidated` until a real lab protocol run
backs their target properties; `assign_recipes` always warns when an
unvalidated or missing recipe is used so a design never silently ships on
unproven chemistry.
"""

from __future__ import annotations

from dataclasses import dataclass, field, fields
from typing import Any

from .config import DomeConfig
from .panel import Panel

VALIDATED = "validated"
UNVALIDATED = "unvalidated"
EXPERIMENTAL = "experimental"


@dataclass
class MaterialRecipe:
    recipe_id: str
    layer: str
    binder_type: str = "MKPC"

    mgo_mass_g: float = 0.0
    kh2po4_mass_g: float = 0.0
    borax_pct_of_mgo: float = 0.0
    water_binder_ratio: float = 0.0
    aggregate_content_pct: float = 0.0
    fiber_content_pct: float = 0.0
    foam_agent_content_pct: float = 0.0
    surfactant_system: str = ""

    density_target_kg_m3: float = 1200.0
    cost_per_kg_usd: float = 1.0
    embodied_carbon_kg_co2e_per_kg: float | None = None

    expected_compressive_strength_mpa: float | None = None
    expected_flexural_strength_mpa: float | None = None
    expected_thermal_conductivity_w_mk: float | None = None
    expected_open_time_min: float | None = None
    expected_demold_time_hr: float | None = None

    validation_status: str = UNVALIDATED
    linked_protocol_runs: list[str] = field(default_factory=list)
    linked_test_results: list[str] = field(default_factory=list)

    @property
    def mg_p_ratio(self) -> float | None:
        """Molar Mg:P ratio from MgO and KH2PO4 masses (M=40.30 / 136.09 g/mol)."""
        if self.kh2po4_mass_g <= 0:
            return None
        mg_mol = self.mgo_mass_g / 40.30
        p_mol = self.kh2po4_mass_g / 136.09
        if p_mol <= 0:
            return None
        return round(mg_mol / p_mol, 3)

    @classmethod
    def from_dict(cls, recipe_id: str, data: dict[str, Any]) -> "MaterialRecipe":
        known = {f.name for f in fields(cls)} - {"recipe_id"}
        filtered = {k: v for k, v in data.items() if k in known}
        return cls(recipe_id=recipe_id, **filtered)

    def to_record(self) -> dict:
        return {
            "recipe_id": self.recipe_id,
            "layer": self.layer,
            "binder_type": self.binder_type,
            "mgo_mass_g": self.mgo_mass_g,
            "kh2po4_mass_g": self.kh2po4_mass_g,
            "mg_p_ratio": self.mg_p_ratio,
            "borax_pct_of_mgo": self.borax_pct_of_mgo,
            "water_binder_ratio": self.water_binder_ratio,
            "aggregate_content_pct": self.aggregate_content_pct,
            "fiber_content_pct": self.fiber_content_pct,
            "foam_agent_content_pct": self.foam_agent_content_pct,
            "surfactant_system": self.surfactant_system,
            "density_target_kg_m3": self.density_target_kg_m3,
            "cost_per_kg_usd": self.cost_per_kg_usd,
            "embodied_carbon_kg_co2e_per_kg": self.embodied_carbon_kg_co2e_per_kg,
            "expected_compressive_strength_mpa": self.expected_compressive_strength_mpa,
            "expected_flexural_strength_mpa": self.expected_flexural_strength_mpa,
            "expected_thermal_conductivity_w_mk": self.expected_thermal_conductivity_w_mk,
            "expected_open_time_min": self.expected_open_time_min,
            "expected_demold_time_hr": self.expected_demold_time_hr,
            "validation_status": self.validation_status,
            "linked_protocol_runs": self.linked_protocol_runs,
            "linked_test_results": self.linked_test_results,
        }


# Placeholder formulations for the four canonical Sietch layers. Every one is
# `unvalidated` — these exist so the pipeline produces a coherent, traceable
# BOM out of the box, not so anyone pours a panel from these numbers.
DEFAULT_RECIPES: dict[str, MaterialRecipe] = {
    "reg_default": MaterialRecipe(
        recipe_id="reg_default",
        layer="Reg",
        binder_type="MKPC",
        mgo_mass_g=1000.0,
        kh2po4_mass_g=1450.0,
        borax_pct_of_mgo=10.0,
        water_binder_ratio=0.32,
        aggregate_content_pct=35.0,
        fiber_content_pct=3.0,
        foam_agent_content_pct=0.0,
        surfactant_system="none",
        density_target_kg_m3=1850.0,
        cost_per_kg_usd=1.85,
        embodied_carbon_kg_co2e_per_kg=0.35,
        expected_compressive_strength_mpa=35.0,
        expected_flexural_strength_mpa=6.5,
        expected_thermal_conductivity_w_mk=0.55,
        expected_open_time_min=12.0,
        expected_demold_time_hr=2.0,
        validation_status=UNVALIDATED,
    ),
    "caliche_default": MaterialRecipe(
        recipe_id="caliche_default",
        layer="Caliche",
        binder_type="MKPC",
        mgo_mass_g=1000.0,
        kh2po4_mass_g=1450.0,
        borax_pct_of_mgo=12.0,
        water_binder_ratio=0.30,
        aggregate_content_pct=30.0,
        fiber_content_pct=4.0,
        foam_agent_content_pct=0.0,
        surfactant_system="none",
        density_target_kg_m3=1950.0,
        cost_per_kg_usd=2.10,
        embodied_carbon_kg_co2e_per_kg=0.38,
        expected_compressive_strength_mpa=40.0,
        expected_flexural_strength_mpa=7.5,
        expected_thermal_conductivity_w_mk=0.60,
        expected_open_time_min=10.0,
        expected_demold_time_hr=2.5,
        validation_status=UNVALIDATED,
    ),
    "erg_default": MaterialRecipe(
        recipe_id="erg_default",
        layer="Erg",
        binder_type="MKPC_foam",
        mgo_mass_g=600.0,
        kh2po4_mass_g=900.0,
        borax_pct_of_mgo=8.0,
        water_binder_ratio=0.55,
        aggregate_content_pct=15.0,
        fiber_content_pct=0.0,
        foam_agent_content_pct=2.5,
        surfactant_system="H2O2_perlite",
        density_target_kg_m3=320.0,
        cost_per_kg_usd=2.60,
        embodied_carbon_kg_co2e_per_kg=0.42,
        expected_compressive_strength_mpa=1.2,
        expected_flexural_strength_mpa=0.4,
        expected_thermal_conductivity_w_mk=0.07,
        expected_open_time_min=8.0,
        expected_demold_time_hr=4.0,
        validation_status=UNVALIDATED,
    ),
    "tau_default": MaterialRecipe(
        recipe_id="tau_default",
        layer="Tau",
        binder_type="MKPC_grout",
        mgo_mass_g=1000.0,
        kh2po4_mass_g=1500.0,
        borax_pct_of_mgo=14.0,
        water_binder_ratio=0.28,
        aggregate_content_pct=20.0,
        fiber_content_pct=2.0,
        foam_agent_content_pct=0.0,
        surfactant_system="none",
        density_target_kg_m3=1750.0,
        cost_per_kg_usd=2.40,
        embodied_carbon_kg_co2e_per_kg=0.40,
        expected_compressive_strength_mpa=25.0,
        expected_flexural_strength_mpa=5.0,
        expected_thermal_conductivity_w_mk=0.50,
        expected_open_time_min=6.0,
        expected_demold_time_hr=1.5,
        validation_status=UNVALIDATED,
    ),
}

MM3_PER_M3 = 1.0e9
EXCESSIVE_PANEL_MASS_KG = 40.0  # heuristic two-person manual-handling guideline


def build_recipe_library(config: DomeConfig) -> dict[str, MaterialRecipe]:
    """Merge the built-in defaults with any recipe overrides from `material_recipes:`."""
    library = dict(DEFAULT_RECIPES)
    for recipe_id, data in config.material_recipes.items():
        library[recipe_id] = MaterialRecipe.from_dict(recipe_id, data)
    return library


def assign_recipes(panels: list[Panel], config: DomeConfig) -> dict[str, MaterialRecipe]:
    """Assign the configured layer stack + recipes to every fabricated panel.

    Mutates each Panel in place: `layer_stack`, `recipe_assignments`,
    `estimated_mass_kg`, `estimated_cost_usd`, and appends to `warnings`.
    Returns the recipe library actually used (defaults + overrides) so
    callers (cost engine, BOM, factory package) share one source of truth.
    """
    library = build_recipe_library(config)
    layer_stack = [layer.name for layer in config.layers]

    if not config.layers:
        for panel in panels:
            if panel.is_opening:
                continue
            panel.warnings.append("no material layers configured — recipe assignment skipped")
        return library

    for panel in panels:
        if panel.is_opening:
            continue

        panel.layer_stack = list(layer_stack)
        panel.recipe_assignments = []
        total_mass = 0.0
        total_cost = 0.0

        for layer in config.layers:
            recipe = library.get(layer.recipe_id)
            if recipe is None:
                panel.warnings.append(
                    f"layer '{layer.name}' references unknown recipe_id '{layer.recipe_id}'"
                )
                continue
            if recipe.validation_status != VALIDATED:
                panel.warnings.append(
                    f"layer '{layer.name}' uses {recipe.validation_status} recipe '{recipe.recipe_id}'"
                )

            volume_mm3 = panel.area_mm2 * layer.thickness_mm
            mass_kg = volume_mm3 / MM3_PER_M3 * recipe.density_target_kg_m3
            cost_usd = mass_kg * recipe.cost_per_kg_usd

            panel.recipe_assignments.append({
                "layer": layer.name,
                "recipe_id": recipe.recipe_id,
                "thickness_mm": layer.thickness_mm,
                "volume_mm3": round(volume_mm3, 1),
                "mass_kg": round(mass_kg, 4),
                "cost_usd": round(cost_usd, 2),
                "validation_status": recipe.validation_status,
            })
            total_mass += mass_kg
            total_cost += cost_usd

        panel.estimated_mass_kg = total_mass
        panel.estimated_cost_usd = total_cost
        if total_mass > EXCESSIVE_PANEL_MASS_KG:
            panel.warnings.append(
                f"excessive panel mass — {total_mass:.1f}kg exceeds the {EXCESSIVE_PANEL_MASS_KG:.0f}kg "
                f"two-person manual-handling guideline; plan for lift assistance"
            )

    return library


def build_material_bom(panels: list[Panel], config: DomeConfig, library: dict[str, MaterialRecipe]) -> list[dict]:
    """Roll per-panel layer assignments up into a recipe-level bill of materials."""
    rollup: dict[str, dict] = {}

    for panel in panels:
        if panel.is_opening:
            continue
        for assignment in panel.recipe_assignments:
            recipe_id = assignment["recipe_id"]
            entry = rollup.setdefault(recipe_id, {
                "recipe_id": recipe_id,
                "layer": assignment["layer"],
                "panel_count": 0,
                "total_volume_mm3": 0.0,
                "total_mass_kg": 0.0,
                "total_cost_usd": 0.0,
                "validation_status": assignment["validation_status"],
            })
            entry["panel_count"] += 1
            entry["total_volume_mm3"] += assignment["volume_mm3"]
            entry["total_mass_kg"] += assignment["mass_kg"]
            entry["total_cost_usd"] += assignment["cost_usd"]

    waste_factor = 1.0 + config.manufacturing.waste_factor_pct / 100.0
    bom: list[dict] = []
    for recipe_id, entry in sorted(rollup.items()):
        recipe = library.get(recipe_id)
        batch_mass_with_waste_kg = entry["total_mass_kg"] * waste_factor
        bom.append({
            "recipe_id": recipe_id,
            "layer": entry["layer"],
            "panel_count": entry["panel_count"],
            "total_volume_m3": round(entry["total_volume_mm3"] / MM3_PER_M3, 4),
            "total_mass_kg": round(entry["total_mass_kg"], 2),
            "batch_mass_with_waste_kg": round(batch_mass_with_waste_kg, 2),
            "total_cost_usd": round(entry["total_cost_usd"], 2),
            "cost_per_kg_usd": recipe.cost_per_kg_usd if recipe else None,
            "validation_status": entry["validation_status"],
        })
    return bom
