# Sietch Panelizer

A parametric panel-design prototype for the **Sietch Maker Dome**: a
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
cd sietch_panelizer
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
```

## Running

```bash
python scripts/generate_maker_dome.py --config examples/maker_default.yaml
```

Outputs are written to `outputs/`:

| File | Contents |
|---|---|
| `maker_dome.obj` | Triangulated shell mesh (door opening cut out) |
| `panel_schedule.json` | Full per-panel record: vertices, edges, area, normal, centroid, neighbors, dihedral angles, family |
| `panel_schedule.csv` | Same schedule, flattened for spreadsheets |
| `panel_families.csv` | Repetition groups: `mold_family_id`, panel count, representative edge lengths |
| `nesting_schedule.json` | Full per-sheet record: dimensions, utilization, and every nested panel's placement (position, rotation, flattened outline) |
| `nesting_schedule.csv` | Same nesting schedule, flattened for spreadsheets |
| `flat_patterns.dxf` | Every sheet's nested, flattened (unrolled) triangle outlines, drawn with sheet borders and grouped by family layer — ready to send to a cutter |
| `mold_schedule.json` | Full per-mold record: cavity outline, thickness, dam/bevel/draft, registration holes, demold slots, insert locators, linked `panel_ids` |
| `mold_schedule.csv` | Same mold schedule, flattened for spreadsheets |
| `molds.dxf` | One cavity drawing per mold family — outline, registration holes, insert locators, demold slot, label — laid out on a grid for CNC/CAD review |
| `preview.png` | 3D preview, panels colored by mold family |

`viewer/viewer_data.js` is also (re)written on every run — see
[Viewer](#viewer-3d-assembly--flat-pattern-cut-sheets) below.

(`mold_schedule.*` / `molds.dxf` are only written when `mold.enabled: true`.)

A consolidated **factory package** is also written to
`outputs/factory_package/` — see
[Manufacturing intelligence](#manufacturing-intelligence) below.

## Parameters (`examples/maker_default.yaml`)

All linear dimensions are millimeters; angles in the config are degrees.

| Parameter | Meaning |
|---|---|
| `overall_width_mm` | Footprint diameter at the base (drives `base_radius_mm = overall_width_mm / 2`) |
| `height_mm` | Apex height of the dome |
| `lobe_amplitude` | Strength of the maker lobing in `r(θ) = base_radius · (1 + lobe_amplitude · cos(lobe_count · θ))` |
| `lobe_count` | Number of lobes around the footprint (default 3, like the maker flower) |
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

### `nesting:` block

| Parameter | Meaning |
|---|---|
| `sheet_width_mm`, `sheet_height_mm` | Usable cut-sheet dimensions for nesting flattened panel outlines |
| `margin_mm` | Border kept clear around the inside edge of every sheet |
| `spacing_mm` | Minimum gap kept between adjacent nested panels (kerf/handling allowance) |

### `layers:` and `material_recipes:`

`layers:` lists the Sietch material stack assigned to every fabricated panel,
outermost layer first — each entry is `{name, thickness_mm, recipe_id}`. Note
that **Tau is intentionally not a stack layer**: per the Sietch material
mapping it's the structural grout/adhesive that manages joints and cold
interfaces, not a panel skin, so it's only consumed by the connection
schedule (see below), not by per-panel mass/cost.

`recipe_id` keys into a small built-in recipe library
(`materials.DEFAULT_RECIPES`) of placeholder MKPC (magnesium potassium
phosphate ceramic) formulations for the four canonical Sietch layers — Reg,
Caliche, Erg, Tau. Every shipped recipe is `unvalidated`, and
`assign_recipes()` always warns when an unvalidated or missing recipe is used,
so a design never silently ships on unproven chemistry. Add a
`material_recipes:` block (keyed by `recipe_id`, same fields as
`MaterialRecipe`) to override or extend the library — see
`examples/maker_default.yaml` for the commented-out stub.

### `manufacturing:`, `loads:`, and `cure:` blocks

| Parameter | Meaning |
|---|---|
| `manufacturing.cnc_bed_width_mm`, `cnc_bed_height_mm` | CNC bed envelope used to validate mold cavities (`cnc_fit_status`) |
| `manufacturing.labor_rate_usd_hr` | Blended crew labor rate fed into the cost engine |
| `manufacturing.waste_factor_pct` | Material overage applied to BOM batch sizes and the cost engine's waste category |
| `loads.snow_load_psf`, `wind_speed_mph`, `dead_load_psf`, `seismic_category`, `safety_factor_target` | **Preliminary** load assumptions for the structural pre-check — placeholders, not a code-compliance input (see [Structural pre-check](#structural-pre-check-preliminary-only)) |
| `cure.ambient_temp_C`, `ambient_rh_pct`, `target_handling_strength_pct` | Ambient cure-room conditions fed into the cure prediction engine's Q10 timing heuristic |

## How the geometry is built

1. **Footprint** — a polar curve `r(θ) = base_radius · (1 + lobe_amplitude · cos(lobe_count · θ))`
   produces the three-lobed maker outline at the base.
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

## Flat-pattern nesting

`nesting.py` flattens every fabricated panel (the same exact 2D unrolling
used for molds and the old per-panel DXF) and packs their bounding boxes
onto fixed-size cut sheets with a greedy shelf-packing algorithm: panels are
sorted largest-area-first and placed left to right, row by row, honoring
`margin_mm` and `spacing_mm`, starting a new sheet whenever a row or sheet
fills up. Each `NestingSheet` records its panel placements (position,
rotation, flattened outline) and a `utilization_pct` (nested panel area ÷
usable sheet area) so a fabrication run can estimate material yield per
sheet before cutting. `export_nesting()` writes the schedule
(`nesting_schedule.json`/`.csv`) and a single `flat_patterns.dxf` with every
sheet's border and nested outlines, grouped by family layer.

## Viewer (3D assembly + flat-pattern cut sheets)

`viewer/` is a static, browser-based front end for exploring a generated
dome without re-running the pipeline each time. Every run writes
`viewer/viewer_data.js` (via `viewer_data.py`, gitignored as a generated
artifact) — a `window.SIETCH_VIEWER_DATA = {...}` assignment bundling the
config metadata, family list, full panel records, mold records, and nested
sheets as plain JSON, loaded with a `<script>` tag instead of `fetch()` so
it works straight off the filesystem with no server or CORS issues.

To use it:

```bash
python scripts/generate_maker_dome.py --config examples/maker_default.yaml
# then open viewer/index.html in a browser (requires internet access for the
# Three.js / OrbitControls CDN scripts)
```

It has two synchronized views, switchable via the tabs at the top:

- **3D Assembly** — a Three.js scene of the whole shell, each triangular
  panel colored by its `mold_family_id` (same golden-ratio HSV palette as
  `visualize.py`, so colors agree across the toolchain). Orbit/zoom with the
  mouse; click a panel to highlight it and show its schedule details.
- **Flat Patterns** — a Canvas 2D rendering of one nested cut sheet at a
  time (pick from the sheet dropdown, which also reports panel count and
  utilization). Click a panel outline to select it — selection is shared
  with the 3D view, and picking a panel in either view jumps the other view
  to match. **Export sheet as SVG** downloads the current sheet's outlines
  and panel-ID labels as a standalone SVG file for cutting/CAM import.

The info panel on the right shows the selected panel's ID, type, family,
area, edge lengths, centroid, and neighbor IDs, plus a legend mapping every
mold family to its color.

## Manufacturing intelligence

Once geometry, families, molds, and nesting are generated, a second pass of
modules turns that shell into a traceable manufacturing plan — materials,
cost, joints, build sequence, cure timing, and a preliminary structural
sanity check — and rolls all of it into a single `outputs/factory_package/`
export. Every module follows the same "never silently approximate" rule as
the geometry pipeline: whenever it falls back to a default, hits an
unvalidated recipe, or detects a risky condition, it appends a human-readable
string to the relevant record's `warnings` list, and `factory_package.py`
aggregates every one of those into `warnings.json`.

### Material recipes and bill of materials (`materials.py`)

`assign_recipes()` walks the configured `layers:` stack and, for every
fabricated panel, computes each layer's volume (`area_mm2 × thickness_mm`),
mass (`volume × density_target_kg_m3`), and cost (`mass × cost_per_kg_usd`)
from the matching `MaterialRecipe`. It mutates each panel in place
(`layer_stack`, `recipe_assignments`, `estimated_mass_kg`,
`estimated_cost_usd`) and warns when a layer references an unknown
`recipe_id`, uses an `unvalidated`/`experimental` recipe, or produces a panel
heavier than the 40 kg two-person manual-handling guideline.
`build_material_bom()` rolls those per-panel assignments up into a
recipe-level BOM (`material_bom.csv`) with waste-adjusted batch masses.

### Cost engine (`cost.py`)

`calculate_cost()` estimates nine cost categories — material, waste, mold,
machine time, labor, cure-rack occupancy, hardware, shipping, and assembly
labor — from the panels/molds/recipes the upstream stages already produced.
Because every rate constant here is a heuristic placeholder (not a quote),
each category carries a `(low, medium, high)` sensitivity band
(`cost.SENSITIVITY_BANDS`) rather than a single point estimate, and the
report rolls costs up by layer, panel family, and mold family in addition to
the headline `$/sqm` figure.

### Connection designer / Tau seam schedule (`connections.py`)

One `Connection` is generated per unique pair of neighboring fabricated
panels. Every seam is fundamentally a **Tau seam** — Tau is the Sietch
structural grout/adhesive that manages joints and cold interfaces — and gets
classified into a reinforcement sub-type by the dihedral angle between the
two panels (the strongest signal v1 geometry gives us about how much
mechanical interlock a seam needs):

| Joint type | Triggered when | Notes |
|---|---|---|
| `spline_joint` | dihedral < 8° (near-coplanar) | thin, closely-fitted bond line carries shear cleanly |
| `tongue_and_groove` | 8° ≤ dihedral < 25° (moderate fold) | thicker bond line for mechanical interlock |
| `basalt_pin_joint` | dihedral ≥ 25° (sharp fold — crown/eaves/transitions) | pinned, with `insert_count = 2` |
| `bolted_insert_joint` | either panel is `opening_adjacent` | hardware + inserts, primer always required |

Each connection records seam length (shared-edge geometry), a seam thickness
keyed to its joint type (`connections.TAU_SEAM_THICKNESS_BY_JOINT_MM`), the
resulting Tau volume, hardware/insert counts, an assembly tolerance
requirement, a structured `cross_family` flag (true when the two panels come
from different `mold_family_id`s — i.e. likely different cure batches, a
"cold joint" that needs pre-wetting/priming), and `primer_required`. Per-joint
warnings flag a missing `Tau`-layer recipe (so priming can't be validated)
and bond lines specced thinner than `TAU_SEAM_MIN_THICKNESS_MM` can reliably
gap-fill — a guard against a future joint-type spec being too thin, not a
restatement of the current one. Cross-family "cold joint" exposure is real
information worth surfacing, but on a curved dome it's close to universal
(hundreds of mold families across the shell), so rather than repeating the
same string on nearly every connection, `factory_package._collect_warnings()`
rolls it into a single design-level finding when more than half the seams are
cross-family — the per-connection `cross_family`/`primer_required` fields
remain in `connection_schedule.csv` for anyone planning the actual sequence.

### Assembly simulator (`assembly.py`)

`generate_assembly_sequence()` buckets fabricated panels into base-to-crown
rings (the same `ring_segments` bands used for geometry) and emits one
`AssemblyStep` per ring: which panels and joints close out in that step
(linking back to `connection.assembly_step_id`), required tools and Tau
materials, crew actions, QA checkpoints, an estimated duration, and
`temporary_bracing_required`/`crane_lift_required` flags driven by ring
height and panel mass. `export_assembly_checklist_md()` renders the sequence
as a printable Markdown checklist (`assembly_checklist.md`).

### Cure prediction engine (`cure.py`)

`predict_cure_schedule()` predicts open/demold/handling/full-cure timing for
every fabricated panel from its **controlling layer** (the thickest layer in
its stack — the one that drives demold timing) and a Q10 heuristic
(`rate ≈ 2^((ambient_temp_C − 22) / 10)`, i.e. roughly doubling per +10°C)
applied to the recipe's `expected_open_time_min`/`expected_demold_time_hr`.
It also recommends chamber temperature/humidity settings and warns when a
recipe is missing expected timing data or ambient conditions are outside the
recommended cure envelope.

### Structural pre-check (preliminary only) (`structure_check.py`)

`run_structural_precheck()` is explicitly **not** an engineering analysis or
a code-compliance determination — every report carries a `disclaimer` saying
so, and every flagged panel must be reviewed by a structural engineer (real
FEA via CalculiX/Code_Aster/OpenSees is the intended next step, not this
heuristic). It estimates an approximate span and slenderness ratio
(`span_mm / stack_thickness_mm`) per panel, classifies each into a zone
(`field`, `opening_adjacent`, `crown_adjacent`, `high_curvature_transition`),
and flags panels whose slenderness or zone crosses heuristic thresholds as
`requires_fea`, alongside thickness/rib recommendations and joint-load
warnings derived from the `loads:` config.

### Factory package (`factory_package.py`)

`generate_factory_package()` is a pure consolidation pass — it assumes every
upstream stage already ran — that writes the complete
`outputs/factory_package/` export tree: a `design_summary.json` overview,
per-module schedules (panels, families, molds, material BOM, recipe
assignments, cost, connections, assembly sequence, cure, structural
pre-check), a single aggregated `warnings.json` manifest, and consolidated
drawings copied into `preview/`, `molds/`, and `panels/` subdirectories. This
is the closest thing in the repo today to "the thing that tells the factory
floor what to make."

## Current limitations

- Panels are flat triangles only; no doubly-curved or quad-folded panels yet.
- Door/window cut zones are simple angular-wedge × height-band tests, not
  true arched or trimmed boundary geometry — edges of openings are jagged at
  the panel level, not cleanly trimmed.
- `wall_thickness_mm` is recorded but not yet used to generate inner/outer
  offset surfaces — the layered material stack (`layers:`) is now modeled for
  mass/cost/cure purposes (see [Manufacturing intelligence](#manufacturing-intelligence)),
  but panel geometry itself is still a single flat facet, not a true
  multi-layer solid.
- DXF export only handles triangular panels (exact unrolling via the law of
  cosines); quad flattening needs a fold-line decomposition.
- No STEP/solid export yet (CadQuery/FreeCAD).
- Flat-pattern nesting uses bounding-box shelf-packing, not true polygon
  nesting (no rotation search, no nesting of mold cavities) — utilization is
  in the 30-40% range for triangular panels and could be substantially
  improved by a rotation-aware or polygon-fitting packer.
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
- Every rate constant in the cost engine, every recipe in
  `materials.DEFAULT_RECIPES`, and every threshold in the structural pre-check
  and connection designer is a placeholder heuristic — useful for producing an
  internally-consistent, traceable estimate end to end, but **none of it is
  validated against a real lab protocol run, vendor quote, or engineering
  analysis**. `assign_recipes()` and `run_structural_precheck()` both warn
  loudly about this on every run; treat the numbers as a structured starting
  point for those validations, not as ship-ready specs.
- Thermal and moisture pre-check (R-value, heat loss, condensation risk,
  vapor-trap warnings) is not yet implemented.
- The factory package generates one consolidated `molds.dxf` /
  `flat_patterns.dxf` per run rather than per-mold/per-panel individual export
  files (`mold_<id>.dxf`/`.svg`/`.stl`/`_drawing.png`, etc.) — a deliberate
  choice to avoid producing thousands of near-duplicate files for hundreds of
  mold families; per-unit exports remain a stretch goal if a downstream tool
  needs them.

## Next steps

- Trim opening boundaries to clean polygons instead of whole-panel removal.
- Generate inner/outer offset shells from `wall_thickness_mm` and model the
  layered material stack.
- Add CadQuery/FreeCAD STEP export for solid molds (swept dam/bevel/draft,
  3D registration & locator features) and panel solids (bevels, ribs, hub
  connectors).
- Upgrade nesting from bounding-box shelf-packing to rotation-aware polygon
  nesting (and extend it to mold cavities) for material-efficient layout.
- Quad panel mode with fold-line-aware flattening.
- Curved-panel, edge-rib, and test-coupon geometry, routed through
  `classify_mold_type()` to their dedicated mold types.
- Viewer: per-family filtering/isolation, exploded assembly view, and
  exporting the full multi-sheet nesting layout (not just the active sheet).
- Thermal and moisture pre-check (R-value, heat loss, condensation risk,
  vapor-trap warnings) alongside the existing structural pre-check.
- Replace placeholder material recipes, cost rates, and structural thresholds
  with validated values as real lab protocol runs and engineering analyses
  come in — the `validation_status`/`linked_protocol_runs` fields on
  `MaterialRecipe` and the disclaimers throughout this module exist
  specifically so that transition is traceable rather than a silent swap.

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

Nesting checks cover: every fabricated panel placed on exactly one sheet (no
duplicates, no omissions), every nested outline staying within its sheet's
margin-bounded usable area, sheet IDs being unique and sequential, utilization
percentages being sane (0-100%, positive whenever a sheet has panels), and the
nesting schedule/DXF files being written.

Manufacturing-intelligence checks (`test_manufacturing.py`) cover the whole
second-pass pipeline on a shared fixture: recipe-library merging, per-panel
volume/mass/cost matching the layer stack and rolling up correctly into the
material BOM, cost-report categories/bands/rollups summing to their totals,
connection schedules being unique unordered neighbor pairs with joint-type-
appropriate seam thicknesses and structured `cross_family`/`primer_required`
flags, the assembly sequence covering every panel exactly once in base-to-
crown order and linking back to real connections, cure predictions covering
every panel with sane open→demold→handling→full-cure ordering, the structural
pre-check flagging every panel while carrying its PRELIMINARY/not-code-
compliance disclaimer, and the factory package writing every expected export
(including the aggregated `warnings.json` manifest) with internally consistent
counts.
