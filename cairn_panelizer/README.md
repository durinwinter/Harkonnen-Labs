# Cairn Panelizer

A parametric panel-design prototype for the **Cairn Trillium Dome**: a
three-lobed, faceted dome shell generated from a small set of geometric
parameters, divided into buildable triangular panels, and exported as
fabrication-ready schedules and drawings.

This is a first prototype. It favors **correct, simple geometry** over
visual polish — flat/faceted panels only, no doubly-curved surfaces, no
mold-curve fitting. The goal is a believable shell and panel schedule that a
fabrication team could start sanity-checking against real material and
tooling constraints.

## Installation

```bash
cd cairn_panelizer
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
```

## Running

```bash
python scripts/generate_trillium_dome.py --config examples/trillium_default.yaml
```

Outputs are written to `outputs/`:

| File | Contents |
|---|---|
| `trillium_dome.obj` | Triangulated shell mesh (door opening cut out) |
| `panel_schedule.json` | Full per-panel record: vertices, edges, area, normal, centroid, neighbors, dihedral angles, family |
| `panel_schedule.csv` | Same schedule, flattened for spreadsheets |
| `panel_families.csv` | Repetition groups: `mold_family_id`, panel count, representative edge lengths |
| `flat_panels.dxf` | Flattened (unrolled) triangle outlines laid out on a cut sheet, grouped by family layer |
| `preview.png` | 3D preview, panels colored by mold family |

## Parameters (`examples/trillium_default.yaml`)

All linear dimensions are millimeters; angles in the config are degrees.

| Parameter | Meaning |
|---|---|
| `overall_width_mm` | Footprint diameter at the base (drives `base_radius_mm = overall_width_mm / 2`) |
| `height_mm` | Apex height of the dome |
| `lobe_amplitude` | Strength of the trillium lobing in `r(θ) = base_radius · (1 + lobe_amplitude · cos(lobe_count · θ))` |
| `lobe_count` | Number of lobes around the footprint (default 3, like the trillium flower) |
| `angular_segments` | Number of facets around the dome |
| `ring_segments` | Number of horizontal rings from base to crown |
| `crown_radius_mm` | Radius of the open circular skylight at the apex |
| `wall_thickness_mm` | Shell thickness (recorded for the panel schedule / future solid export; not yet used to generate offset surfaces) |
| `door_width_mm`, `door_height_mm`, `door_angle_deg` | Rectangular door cut zone: angular position (degrees, 0° = +X axis) and size |
| `windows` | List of `{angle_deg, width_mm, height_mm, sill_height_mm}` rectangular window cut zones (optional, default empty) |
| `family_tolerance_mm` | Edge-length matching tolerance used to group panels into mold families |
| `panel_mode` | `triangle` (only mode implemented in v1; `quad` is reserved for a later iteration) |

## How the geometry is built

1. **Footprint** — a polar curve `r(θ) = base_radius · (1 + lobe_amplitude · cos(lobe_count · θ))`
   produces the three-lobed trillium outline at the base.
2. **Profile** — each ring `i` (0 = base, `ring_segments` = crown) is scaled by a
   quarter-sine profile: the radius blends from the lobed footprint toward the
   circular `crown_radius_mm`, while height rises smoothly from 0 to `height_mm`.
   The crown ring is left open, forming the central skylight.
3. **Triangulation** — each ring-to-ring quad is split into two triangles,
   giving every panel a unique ID, three straight edges, a flat surface
   normal, and well-defined neighbors.
4. **Openings** — panels whose centroid falls inside the door's angular wedge
   and height band are flagged `opening_adjacent` and removed from the
   exported shell mesh, carving a believable door hole into one lobe.
   Window zones use the same mechanism.
5. **Families** — panels are grouped by their sorted, tolerance-rounded edge
   lengths into `mold_family_id` buckets — the repeatable panel "molds" a
   fabrication run would actually need to produce.

## Current limitations

- Panels are flat triangles only; no doubly-curved or quad-folded panels yet.
- Door/window cut zones are simple angular-wedge × height-band tests, not
  true arched or trimmed boundary geometry — edges of openings are jagged at
  the panel level, not cleanly trimmed.
- `wall_thickness_mm` is recorded but not yet used to generate inner/outer
  offset surfaces or the three-layer Cairn material stack (Flint S / Marrow /
  Flint E / interior finish).
- DXF export only handles triangular panels (exact unrolling via the law of
  cosines); quad flattening needs a fold-line decomposition.
- No STEP/solid export yet (CadQuery/FreeCAD).
- No panel-nesting optimization for sheet goods or molds.

## Next steps

- Trim opening boundaries to clean polygons instead of whole-panel removal.
- Generate inner/outer offset shells from `wall_thickness_mm` and model the
  layered material stack.
- Add CadQuery/FreeCAD STEP export for solids (bevels, ribs, hub connectors).
- Nest flattened panels for material-efficient sheet layout.
- Quad panel mode with fold-line-aware flattening.

## Tests

```bash
pytest tests/
```

Geometry sanity checks cover: no duplicate vertices, positive panel areas,
valid (positive, finite) edge lengths, every non-opening panel having at
least one neighbor, and the panel schedule files actually being produced.
