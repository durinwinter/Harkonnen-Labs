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
| `mold_schedule.json` | Full per-mold record: cavity outline, thickness, dam/bevel/draft, registration holes, demold slots, insert locators, linked `panel_ids` |
| `mold_schedule.csv` | Same mold schedule, flattened for spreadsheets |
| `molds.dxf` | One cavity drawing per mold family — outline, registration holes, insert locators, demold slot, label — laid out on a grid for CNC/CAD review |
| `preview.png` | 3D preview, panels colored by mold family |

(`mold_schedule.*` / `molds.dxf` are only written when `mold.enabled: true`.)

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

### `mold:` block

| Parameter | Meaning |
|---|---|
| `enabled` | Generate mold designs and `mold_schedule.*`/`molds.dxf` outputs (default `true`) |
| `default_type` | Mold type used when a panel's geometry doesn't match a more specific class (see Mold types below) |
| `material` | Mold material recorded on every mold (e.g. `hdpe`) |
| `panel_thickness_mm` | Cast panel thickness — sets the cavity depth |
| `edge_dam_height_mm` | Height of the dam wall around the cavity perimeter |
| `bevel_angle_deg` | Edge bevel/chamfer angle baked into the cavity walls |
| `draft_angle_deg` | Mold-release draft angle on the cavity walls |
| `registration_hole_diameter_mm`, `registration_hole_inset_mm` | Size and corner inset of the two alignment-pin holes |
| `demold_slot_width_mm`, `demold_slot_length_mm` | Size of the pry/demold slot set into the panel's longest edge |
| `insert_locator_diameter_mm` | Diameter of the rib/hub insert-locator points placed at each edge midpoint |
| `label_prefix` | Prefix used when engraving each mold's `label_text` |

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

## Mold generation

Each `mold_family_id` produces exactly **one** mold design (`mold.py`),
linked back to every `panel_id` cast from it — equivalent panels never get
duplicate molds, since family grouping already collapsed them.

### Mold types

| Type | Status in v0.1 |
|---|---|
| `flat_faceted_mold` | **Implemented.** Every panel the pipeline produces today is a flat triangular facet, so every mold is this type. |
| `shallow_curved_panel_mold` | Declared, not yet routed to — needs doubly-curved panel geometry the mesh pipeline doesn't generate yet. |
| `edge_rib_mold` | Declared, not yet routed to — needs structural rib panels distinct from skin panels. |
| `test_coupon_mold` | Declared, not yet routed to — needs a dedicated material-QA coupon generator. |

`classify_mold_type()` in `mold.py` is the single hook where future panel
types get routed to these molds; it currently always resolves triangular
panels to `flat_faceted_mold`.

### What's in a flat-faceted mold

For each family, `mold.py` flattens the representative panel (exact 2D
unrolling via `flatten2d.flatten_triangle`, reusing the same law-of-cosines
math as the panel DXF export) and lays out, relative to that cavity outline:

- **cavity outline + area** — the panel shape itself, at `panel_thickness_mm` depth
- **edge dam** — `edge_dam_height_mm` wall around the cavity perimeter
- **bevel/chamfer** — `bevel_angle_deg` on the cavity edge walls
- **mold-release draft** — `draft_angle_deg` on the cavity side walls
- **registration holes** — two holes inset from two corners toward the
  centroid by `registration_hole_inset_mm`, enough to fix the panel's
  orientation against alignment pins
- **demold/pry slot** — one slot set into the midpoint of the panel's
  *longest* edge (best leverage for popping the part free), oriented along
  that edge
- **insert locator points** — one point per edge midpoint, marking where
  this panel meets its rib/hub neighbors
- **label engraving text** — `{label_prefix}-{mold_id}-{family_id}`,
  baked into both the schedule and the `molds.dxf` drawing

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
- Molds are 2D schedules + flat DXF drawings, not 3D solids — dam height,
  bevel, and draft angle are recorded as parameters but not yet swept into
  CNC-ready cavity geometry (CadQuery/FreeCAD solid export is the natural
  next step for that).
- Only `flat_faceted_mold` is generated; `shallow_curved_panel_mold`,
  `edge_rib_mold`, and `test_coupon_mold` are declared but unrouted because
  the mesh pipeline doesn't yet produce the panel types they need.
- Registration-hole, demold-slot, and insert-locator placement use simple
  geometric heuristics (corners, longest edge, edge midpoints) rather than
  a manufacturability/clash analysis.

## Next steps

- Trim opening boundaries to clean polygons instead of whole-panel removal.
- Generate inner/outer offset shells from `wall_thickness_mm` and model the
  layered material stack.
- Add CadQuery/FreeCAD STEP export for solid molds (swept dam/bevel/draft,
  3D registration & locator features) and panel solids (bevels, ribs, hub
  connectors).
- Nest flattened panels — and mold cavities — for material-efficient sheet layout.
- Quad panel mode with fold-line-aware flattening.
- Curved-panel, edge-rib, and test-coupon geometry, routed through
  `classify_mold_type()` to their dedicated mold types.

## Tests

```bash
pytest tests/
```

Geometry sanity checks cover: no duplicate vertices, positive panel areas,
valid (positive, finite) edge lengths, every non-opening panel having at
least one neighbor, and the panel schedule files actually being produced.

Mold checks cover: exactly one mold per family with no duplicates, every
fabricated panel linked to exactly one mold, cavity geometry matching its
family's edge lengths/area, every required mold feature present (registration
holes, demold slot, insert locators, label, material, thickness, dam height),
`mold.enabled: false` producing no molds, and the mold schedule files being
written.
