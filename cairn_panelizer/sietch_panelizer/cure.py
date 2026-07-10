"""Demold / handling / full-cure timing prediction (spec sec. 4.9).

A first-pass heuristic model: each panel's *controlling layer* — the thickest
layer in its stack, since that's normally the slowest to reach handling
strength — supplies the recipe's expected open/demold timing, which is then
scaled by ambient temperature (a simple Q10-style rate doubling per +10°C,
the standard rough heuristic for inorganic binder reactions), panel
thickness, and humidity. These numbers exist to give every batch record a
*predicted* baseline to compare against real TimescaleDB sensor data later
(sec. 4.9 "Required integration") — they are not a substitute for that data.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .config import DomeConfig
from .materials import MaterialRecipe
from .panel import Panel

REFERENCE_THICKNESS_MM = 50.0
DEFAULT_OPEN_TIME_MIN = 10.0
DEFAULT_DEMOLD_TIME_HR = 2.0

HANDLING_MULTIPLIER = 1.6     # handling-ready time vs. demold time
FULL_CURE_MULTIPLIER = 36.0   # full-cure time vs. demold time (MKPC reaches most strength fast, but not all)

RECOMMENDED_CHAMBER_TEMP_C = 23.0
RECOMMENDED_CHAMBER_RH_PCT = 50.0

LOW_TEMP_WARNING_C = 10.0
HIGH_TEMP_WARNING_C = 35.0
HIGH_RH_WARNING_PCT = 70.0
LOW_RH_WARNING_PCT = 30.0


@dataclass
class CurePrediction:
    panel_id: str
    controlling_layer: str
    recipe_id: str
    estimated_open_time_min: float
    estimated_demold_time_hr: float
    estimated_handling_time_hr: float
    estimated_full_cure_time_hr: float
    recommended_chamber_temp_C: float
    recommended_chamber_rh_pct: float
    warnings: list[str] = field(default_factory=list)

    def to_record(self) -> dict:
        return {
            "panel_id": self.panel_id,
            "controlling_layer": self.controlling_layer,
            "recipe_id": self.recipe_id,
            "estimated_open_time_min": round(self.estimated_open_time_min, 1),
            "estimated_demold_time_hr": round(self.estimated_demold_time_hr, 2),
            "estimated_handling_time_hr": round(self.estimated_handling_time_hr, 2),
            "estimated_full_cure_time_hr": round(self.estimated_full_cure_time_hr, 1),
            "recommended_chamber_temp_C": self.recommended_chamber_temp_C,
            "recommended_chamber_rh_pct": self.recommended_chamber_rh_pct,
            "warnings": self.warnings,
        }


def _temperature_rate_factor(ambient_temp_C: float) -> float:
    """Rough Q10 heuristic: reaction rate ~doubles per +10C around the 22C reference."""
    return 2.0 ** ((ambient_temp_C - 22.0) / 10.0)


def _controlling_layer(panel: Panel) -> dict | None:
    if not panel.recipe_assignments:
        return None
    return max(panel.recipe_assignments, key=lambda a: a["thickness_mm"])


def predict_cure_schedule(
    panels: list[Panel],
    library: dict[str, MaterialRecipe],
    config: DomeConfig,
) -> list[CurePrediction]:
    """Predict open/demold/handling/full-cure timing for every fabricated panel."""
    cure_cfg = config.cure
    rate_factor = _temperature_rate_factor(cure_cfg.ambient_temp_C)
    predictions: list[CurePrediction] = []

    for panel in panels:
        if panel.is_opening:
            continue

        assignment = _controlling_layer(panel)
        warnings: list[str] = []

        if assignment is None:
            warnings.append("no recipe assigned — cure timing uses generic defaults")
            layer_name = "unassigned"
            recipe_id = "unassigned"
            base_open_min = DEFAULT_OPEN_TIME_MIN
            base_demold_hr = DEFAULT_DEMOLD_TIME_HR
            thickness_mm = config.mold.panel_thickness_mm
        else:
            layer_name = assignment["layer"]
            recipe_id = assignment["recipe_id"]
            thickness_mm = assignment["thickness_mm"]
            recipe = library.get(recipe_id)
            if recipe is None or recipe.expected_open_time_min is None or recipe.expected_demold_time_hr is None:
                warnings.append(f"recipe '{recipe_id}' missing expected cure timing — using default estimates")
                base_open_min = (recipe.expected_open_time_min if recipe and recipe.expected_open_time_min
                                 else DEFAULT_OPEN_TIME_MIN)
                base_demold_hr = (recipe.expected_demold_time_hr if recipe and recipe.expected_demold_time_hr
                                  else DEFAULT_DEMOLD_TIME_HR)
            else:
                base_open_min = recipe.expected_open_time_min
                base_demold_hr = recipe.expected_demold_time_hr
            if recipe and recipe.validation_status != "validated":
                warnings.append(f"recipe '{recipe_id}' is {recipe.validation_status} — cure prediction is unverified")

        thickness_factor = max(thickness_mm, 1.0) / REFERENCE_THICKNESS_MM
        humidity_factor = 1.0 + (cure_cfg.ambient_rh_pct - RECOMMENDED_CHAMBER_RH_PCT) / 200.0

        open_time_min = base_open_min / rate_factor
        demold_time_hr = (base_demold_hr / rate_factor) * thickness_factor
        handling_time_hr = demold_time_hr * HANDLING_MULTIPLIER * humidity_factor
        full_cure_time_hr = demold_time_hr * FULL_CURE_MULTIPLIER

        if cure_cfg.ambient_temp_C < LOW_TEMP_WARNING_C:
            warnings.append(f"ambient temperature {cure_cfg.ambient_temp_C:.1f}C is below {LOW_TEMP_WARNING_C:.0f}C — expect slow, possibly incomplete cure")
        elif cure_cfg.ambient_temp_C > HIGH_TEMP_WARNING_C:
            warnings.append(f"ambient temperature {cure_cfg.ambient_temp_C:.1f}C is above {HIGH_TEMP_WARNING_C:.0f}C — risk of flash set / cracking")
        if cure_cfg.ambient_rh_pct > HIGH_RH_WARNING_PCT:
            warnings.append(f"ambient RH {cure_cfg.ambient_rh_pct:.0f}% is high — surface handling readiness may lag prediction")
        elif cure_cfg.ambient_rh_pct < LOW_RH_WARNING_PCT:
            warnings.append(f"ambient RH {cure_cfg.ambient_rh_pct:.0f}% is low — rapid surface drying may cause shrinkage cracking")

        predictions.append(CurePrediction(
            panel_id=panel.panel_id,
            controlling_layer=layer_name,
            recipe_id=recipe_id,
            estimated_open_time_min=open_time_min,
            estimated_demold_time_hr=demold_time_hr,
            estimated_handling_time_hr=handling_time_hr,
            estimated_full_cure_time_hr=full_cure_time_hr,
            recommended_chamber_temp_C=RECOMMENDED_CHAMBER_TEMP_C,
            recommended_chamber_rh_pct=RECOMMENDED_CHAMBER_RH_PCT,
            warnings=warnings,
        ))

    return predictions
