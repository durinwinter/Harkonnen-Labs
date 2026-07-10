"""Design-level and panel-level cost estimation (spec sec. 4.5).

This is a *preliminary* cost model: every per-unit rate below is a heuristic
placeholder (clearly named and grouped so they can be swapped for real shop
rates later), not a quote. Every total is reported as a low/medium/high band
rather than a single number, because that's the only honest way to present a
cost estimate this early in a design's life — see `SENSITIVITY_BANDS`.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .config import DomeConfig
from .materials import MaterialRecipe
from .mold import Mold
from .panel import Panel

# --- Heuristic rate assumptions -------------------------------------------------
# All placeholders. Replace with real shop/lab figures as they become available.

MOLD_BASE_COST_USD = 150.0
MOLD_COST_PER_M2_USD = 380.0          # CNC machining + stock, per m^2 of cavity
MACHINE_MINUTES_PER_PANEL = 6.0       # CNC trim / edge prep per cast panel
MACHINE_RATE_USD_PER_HR = 65.0
CAST_LABOR_MINUTES_PER_PANEL = 25.0   # mixing, pouring, finishing
DEMOLD_LABOR_MINUTES_PER_PANEL = 10.0
CURE_RACK_COST_USD_PER_SLOT_DAY = 4.0
DEFAULT_CURE_RACK_DAYS = 1.5          # used when a layer's demold time is unknown
HARDWARE_COST_PER_PANEL_USD = 3.5     # inserts, fasteners, registration hardware
SHIPPING_USD_PER_KG = 0.85
ASSEMBLY_LABOR_HOURS_PER_PANEL = 0.6

# (low, medium, high) multipliers per cost category — the "uncertainty band".
# Categories driven mostly by known geometry (material, mold) carry tighter
# bands; categories driven by process assumptions we haven't measured yet
# (cure rack occupancy, shipping) carry wider ones.
SENSITIVITY_BANDS: dict[str, tuple[float, float, float]] = {
    "material": (0.90, 1.00, 1.25),
    "waste": (0.50, 1.00, 1.50),
    "mold": (0.80, 1.00, 1.40),
    "machine_time": (0.85, 1.00, 1.30),
    "labor": (0.85, 1.00, 1.30),
    "cure_rack": (0.70, 1.00, 1.60),
    "hardware": (0.90, 1.00, 1.20),
    "shipping": (0.80, 1.00, 1.40),
    "assembly_labor": (0.80, 1.00, 1.40),
}

MM2_PER_M2 = 1.0e6


@dataclass
class CostReport:
    categories_usd: dict[str, float] = field(default_factory=dict)
    total_low_usd: float = 0.0
    total_medium_usd: float = 0.0
    total_high_usd: float = 0.0
    cost_by_layer_usd: dict[str, float] = field(default_factory=dict)
    cost_by_panel_family_usd: dict[str, float] = field(default_factory=dict)
    cost_by_mold_family_usd: dict[str, float] = field(default_factory=dict)
    cost_per_sqm_usd: float = 0.0
    fabricated_panel_count: int = 0
    fabricated_area_m2: float = 0.0
    assumptions: dict[str, float] = field(default_factory=dict)
    warnings: list[str] = field(default_factory=list)

    def to_record(self) -> dict:
        return {
            "total_low_usd": round(self.total_low_usd, 2),
            "total_medium_usd": round(self.total_medium_usd, 2),
            "total_high_usd": round(self.total_high_usd, 2),
            "cost_per_sqm_usd": round(self.cost_per_sqm_usd, 2),
            "fabricated_panel_count": self.fabricated_panel_count,
            "fabricated_area_m2": round(self.fabricated_area_m2, 3),
            "categories_usd": {k: round(v, 2) for k, v in self.categories_usd.items()},
            "category_bands_usd": {
                category: {
                    "low": round(value * SENSITIVITY_BANDS[category][0], 2),
                    "medium": round(value * SENSITIVITY_BANDS[category][1], 2),
                    "high": round(value * SENSITIVITY_BANDS[category][2], 2),
                }
                for category, value in self.categories_usd.items()
            },
            "cost_by_layer_usd": {k: round(v, 2) for k, v in self.cost_by_layer_usd.items()},
            "cost_by_panel_family_usd": {k: round(v, 2) for k, v in self.cost_by_panel_family_usd.items()},
            "cost_by_mold_family_usd": {k: round(v, 2) for k, v in self.cost_by_mold_family_usd.items()},
            "assumptions": self.assumptions,
            "warnings": self.warnings,
        }

    def to_csv_rows(self) -> list[dict]:
        rows: list[dict] = []
        for category, value in self.categories_usd.items():
            low, med, high = SENSITIVITY_BANDS[category]
            rows.append({
                "scope": "category",
                "key": category,
                "low_usd": round(value * low, 2),
                "medium_usd": round(value * med, 2),
                "high_usd": round(value * high, 2),
            })
        for layer, value in self.cost_by_layer_usd.items():
            rows.append({"scope": "layer", "key": layer, "low_usd": "", "medium_usd": round(value, 2), "high_usd": ""})
        for family, value in self.cost_by_panel_family_usd.items():
            rows.append({"scope": "panel_family", "key": family, "low_usd": "", "medium_usd": round(value, 2), "high_usd": ""})
        for family, value in self.cost_by_mold_family_usd.items():
            rows.append({"scope": "mold_family", "key": family, "low_usd": "", "medium_usd": round(value, 2), "high_usd": ""})
        rows.append({
            "scope": "total", "key": "dome",
            "low_usd": round(self.total_low_usd, 2),
            "medium_usd": round(self.total_medium_usd, 2),
            "high_usd": round(self.total_high_usd, 2),
        })
        return rows


def _cure_rack_days(library: dict[str, MaterialRecipe], config: DomeConfig) -> float:
    times = [r.expected_demold_time_hr for r in library.values() if r.expected_demold_time_hr]
    relevant = [
        library[layer.recipe_id].expected_demold_time_hr
        for layer in config.layers
        if layer.recipe_id in library and library[layer.recipe_id].expected_demold_time_hr
    ]
    pool = relevant or times
    if not pool:
        return DEFAULT_CURE_RACK_DAYS
    return max(pool) / 24.0


def calculate_cost(
    panels: list[Panel],
    molds: list[Mold],
    config: DomeConfig,
    library: dict[str, MaterialRecipe],
) -> CostReport:
    """Estimate design-level cost from already-assigned recipes and generated molds.

    Requires `materials.assign_recipes()` to have run first (panels carry
    `estimated_mass_kg` / `estimated_cost_usd` / `recipe_assignments`).
    """
    fabricated = [p for p in panels if not p.is_opening]
    report = CostReport(fabricated_panel_count=len(fabricated))
    if not fabricated:
        report.warnings.append("no fabricated panels — cost report is empty")
        return report

    waste_factor = 1.0 + config.manufacturing.waste_factor_pct / 100.0
    panel_count = len(fabricated)
    total_mass_kg = sum(p.estimated_mass_kg or 0.0 for p in fabricated)
    total_area_m2 = sum(p.area_mm2 for p in fabricated) / MM2_PER_M2
    report.fabricated_area_m2 = total_area_m2

    if not config.layers:
        report.warnings.append("no material layers configured — material/waste cost is zero")

    material_base = sum(p.estimated_cost_usd or 0.0 for p in fabricated)
    waste_cost = material_base * (waste_factor - 1.0)

    mold_cost = sum(
        MOLD_BASE_COST_USD + (mold.cavity_area_mm2 / MM2_PER_M2) * MOLD_COST_PER_M2_USD
        for mold in molds
    )
    if not molds:
        report.warnings.append("no molds generated — mold cost is zero")

    machine_time_cost = panel_count * (MACHINE_MINUTES_PER_PANEL / 60.0) * MACHINE_RATE_USD_PER_HR
    labor_cost = panel_count * (
        (CAST_LABOR_MINUTES_PER_PANEL + DEMOLD_LABOR_MINUTES_PER_PANEL) / 60.0
    ) * config.manufacturing.labor_rate_usd_hr

    cure_rack_days = _cure_rack_days(library, config)
    cure_rack_cost = panel_count * cure_rack_days * CURE_RACK_COST_USD_PER_SLOT_DAY

    hardware_cost = panel_count * HARDWARE_COST_PER_PANEL_USD
    shipping_cost = total_mass_kg * waste_factor * SHIPPING_USD_PER_KG
    assembly_labor_cost = panel_count * ASSEMBLY_LABOR_HOURS_PER_PANEL * config.manufacturing.labor_rate_usd_hr

    report.categories_usd = {
        "material": material_base,
        "waste": waste_cost,
        "mold": mold_cost,
        "machine_time": machine_time_cost,
        "labor": labor_cost,
        "cure_rack": cure_rack_cost,
        "hardware": hardware_cost,
        "shipping": shipping_cost,
        "assembly_labor": assembly_labor_cost,
    }

    report.total_low_usd = sum(v * SENSITIVITY_BANDS[k][0] for k, v in report.categories_usd.items())
    report.total_medium_usd = sum(v * SENSITIVITY_BANDS[k][1] for k, v in report.categories_usd.items())
    report.total_high_usd = sum(v * SENSITIVITY_BANDS[k][2] for k, v in report.categories_usd.items())

    if total_area_m2 > 0:
        report.cost_per_sqm_usd = report.total_medium_usd / total_area_m2

    by_layer: dict[str, float] = {}
    for panel in fabricated:
        for assignment in panel.recipe_assignments:
            by_layer[assignment["layer"]] = by_layer.get(assignment["layer"], 0.0) + assignment["cost_usd"]
    report.cost_by_layer_usd = {layer: cost * waste_factor for layer, cost in by_layer.items()}

    by_panel_family: dict[str, float] = {}
    for panel in fabricated:
        family = panel.mold_family_id or "unassigned"
        by_panel_family[family] = by_panel_family.get(family, 0.0) + (panel.estimated_cost_usd or 0.0) * waste_factor
    report.cost_by_panel_family_usd = by_panel_family

    by_mold_family: dict[str, float] = {}
    for mold in molds:
        by_mold_family[mold.family_id] = (
            MOLD_BASE_COST_USD + (mold.cavity_area_mm2 / MM2_PER_M2) * MOLD_COST_PER_M2_USD
        )
    report.cost_by_mold_family_usd = by_mold_family

    report.assumptions = {
        "waste_factor_pct": config.manufacturing.waste_factor_pct,
        "labor_rate_usd_hr": config.manufacturing.labor_rate_usd_hr,
        "mold_cost_per_m2_usd": MOLD_COST_PER_M2_USD,
        "machine_rate_usd_hr": MACHINE_RATE_USD_PER_HR,
        "cure_rack_days_assumed": round(cure_rack_days, 2),
        "shipping_usd_per_kg": SHIPPING_USD_PER_KG,
        "total_mass_kg": round(total_mass_kg, 2),
    }

    return report
