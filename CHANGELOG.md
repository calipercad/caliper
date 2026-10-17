# Changelog

All notable changes to cad-assist are recorded here. Versions map to roadmap
phases (see [`docs/ROADMAP.md`](docs/ROADMAP.md)).

## [0.6.0] — Library explosion (15 new skills)

Fifteen new parametric skills — the payoff of the geometry stack built in
Phases 1–4 — bringing the built-in library to 20.

### Added
- **Mechanical** (`skills/mechanical.rs`, accurate engineering profiles):
  `flange` (bolt-circle disc), `sprocket` (ANSI B29.1 roller-chain tooth form),
  `rack` (trapezoidal spur rack), `pulley` (GT2/HTD timing-belt grooves), `cam`
  (harmonic/cycloidal radial cam). Flat shapes, or solids with `thickness=`.
- **Maker / 3D** (`skills/maker.rs`): `gridfinity` (42 mm-grid bin, closed
  manifold), `enclosure` (two-part project box), `standoff_grid`, `knob` (fluted,
  D-shaft bore), `cable_clip`.
- **Craft / laser 2D** (`skills/craft.rs`): `monogram` (framed stroke text),
  `mandala` (polar petal motif), `card` (fold-score net), `box_template` (tuck-box
  net), `living_hinge` (staggered flex slots). Cut/score/engrave on role layers.
- **`docs/SKILLS.md`** — the skill gallery (grouped tables + braille previews).
- **Tests**: `crates/cad-core/tests/phase5.rs` — `all_skills_run_at_defaults`
  (≥20 built-ins, each builds geometry from a bare `skill <name>`), the
  gridfinity-manifold checkpoint, and targeted shape checks.

## [0.5.0] — Drafting & rendering

The terminal preview now reads like an engineering tool, plus the layout
transforms and dimensioning real drawings need.

### Added
- **Hidden-line removal** (`render … mode=hlr`): faces are rasterized into a
  per-dot depth buffer and feature edges behind surfaces are culled, so
  wireframes read as solids.
- **Shaded mode** (`render … mode=shade`): a new ANSI half-block canvas
  (`▀` with per-cell fg/bg grayscale), Lambert-lit from a view-space light.
- **View presets** (`view=iso|top|front|right`) and **multi-view sheets**
  (`layout=views`) tiling top/front/right/iso into a 2×2 grid; a scale-bar
  overlay (`axes=on|off`) on single 3D views.
- **Layout ops**: `array <n> [kind=linear|polar|grid] …]` (polar spins copies
  about Z — a tooth becomes a gear), `mirror [axis=] [about=] [copy]`, and
  `group`/`ungroup` via a new `Entity.group` tag. Transforms (`translate`/
  `rotate`/`scale`/`mirror`) act on a whole group when the target names one.
- **Dimensioning**: `dim <target>` builds witness/extension lines, arrowheads,
  and measured width/height labels (single-stroke text) as a `role="dim"`
  entity; exporting geometry + its `*_dim` entities to SVG yields a dimensioned
  drawing sheet (reuses the Phase 2 role layers).
- **Tests**: `crates/cad-core/tests/phase4.rs` — polar-array checkpoint,
  linear/grid arrays, mirror symmetry, group transforms, HLR edge culling,
  shaded output, multi-view tiling, and the dimensioned SVG sheet.

## [0.4.0] — 3D CSG, shell, rounded edges

True solid modeling: boolean operations on meshes, so `box − cylinder` yields a
real bored solid.

### Added
- **Mesh CSG** (`geom3d::csg`): `union` / `difference` / `intersection` via a
  BSP-tree kernel (csg.js approach), followed by a **T-junction healing pass**
  that subdivides edges at stray vertices so the output is a strict closed
  2-manifold (not just watertight-for-rendering). Inputs are validated as closed
  manifolds; non-manifold input returns a clean error instead of junk.
- **`union`/`subtract`/`intersect` auto-dispatch by operand kind**: two 2D
  shapes run the exact-integer polygon clipper (unchanged); two 3D meshes run the
  CSG kernel; mixing a shape and a mesh errors. `xor` stays 2D-only.
- **`shell <t>`** — hollow a solid to wall thickness `t`, by eroding each face
  plane inward and subtracting (exact for convex / prismatic solids: box,
  cylinder, cone, convex extrusions; conservative error otherwise).
- **Rounded 3D edges via the profile workflow**: `fillet` / `chamfer` the 2D
  profile, then `extrude` → rounded/beveled vertical edges (no new kernel code).
- `Vec3::lerp` and `Vec3::len_sq` (parity with `Vec2`), used by the BSP splits.
- **Tests**: `crates/cad-core/tests/phase3.rs` — the box−cylinder genus-1
  checkpoint, union/intersection/disjoint volume + manifold, shell wall, command
  auto-dispatch (2D vs 3D), non-manifold-input rejection, and rounded-extrude.

## [0.3.0] — Text & engraving + layered export

Single-stroke text and layer/role semantics, so one exported file can drive a
multi-operation cutter job (cut vs score vs engrave).

### Added
- **Single-stroke text** (`geom2d::text`): `stroke_text(text, height, align)`
  lays out a built-in single-stroke vector font into a `Shape` of open
  contours — ideal for pen plotters, drag-engravers, and laser scoring. The
  font (`geom2d::text::font`) is authored in-tree as `const` data covering
  printable ASCII (letters, digits, punctuation); no external files, no deps.
  `Align::{Left,Center,Right}` positions the laid-out ink about the origin.
- **`text` command**: `text "<string>" [h=10] [align=left|center|right]
  [layer=] [role=]`.
- **Layer / role on entities**: `Entity` gains optional `layer` and `role`
  (`Scene::set_attrs`). Any 2D primitive accepts `layer=` / `role=` at creation;
  standalone `layer <name> [target=]` and `role <name> [target=]` commands set
  them on an existing entity. Both are history-recorded, so they round-trip
  through `save`.
- **Layered, role-colored export**: SVG groups entities into Inkscape layers
  (`inkscape:groupmode="layer"`) by effective layer (explicit `layer`, else
  `role`, else `"0"`); DXF emits a `TABLES`→`LAYER` section and places each
  entity on its layer. Role colors: cut=black, score=blue, engrave=red
  (`export::role_color`). Exporters now take `export::Item` (name + shape +
  layer + role).
- **Tests**: `crates/cad-core/tests/phase2.rs` — stroke-text bbox/alignment,
  `text`/`layer`/`role` commands, save round-trip, and layered SVG/DXF output.

## [0.2.0] — Robust 2D geometry

Booleans, fillets/chamfers, and **real holes that survive extrusion** — 2D is
now genuinely CAD-grade.

### Added
- **Exact-integer 2D booleans** (`geom2d::boolean`): `union`/`difference`/
  `intersection`/`xor` via a snap-rounded planar-overlay clipper. All topology
  (orientation, crossings, collinearity, angular order) is decided with exact
  `i128` lattice arithmetic; only in/out membership consults the float loops
  (ray-cast winding), so the combinatorial core is immune to float fragility.
  Coincident/collinear/overlapping edges and self-intersecting input are handled
  uniformly; non-manifold input reports a clean error rather than emitting junk.
- **Winding classification**: `Shape::normalize_windings` (solids CCW, holes CW
  by nesting-depth parity) and `Shape::regions` (each hole assigned to its
  smallest containing shell), making boolean results and legacy skill shapes
  extrude as true through-holes.
- **Holed extrusion**: `extrude` now triangulates holed caps via Eberly keyhole
  hole-bridging over `regions()`, welding caps and side walls into one shared
  vertex ring. Bores become real through-walls. Also fixes the v0.1 cap/wall
  weld bug on plain solids.
- **Manifold verification**: `Mesh::is_closed_manifold` (half-edge balance) and
  `Mesh::euler_characteristic` (V−E+F → genus) for watertightness tests.
- **Fillet / chamfer** on 2D contours: `Contour`/`Shape::fillet(r, seg)` and
  `chamfer(d)`, with corner-crossing setback clamping; `fillet`/`chamfer`
  commands.
- **Language**: `union`, `subtract`, `intersect`, `xor` (two operands → new,
  non-destructive entity); `fillet`/`chamfer` edge ops.
- **Exporter fidelity**: contours carry a `ContourKind` primitive hint, so DXF
  emits true `CIRCLE`/`ARC` entities and SVG a real `<circle>` where the
  geometry is a known primitive; any point-mutating op resets the hint to
  polyline. Transforms preserve the hint only under shape-preserving maps.
- **Tests**: `crates/cad-core/tests/phase1.rs` — boolean truth table,
  coincident/shared-edge, disjoint split, circle-from-rect regions,
  `hole_is_manifold` (genus 1) and `multi_hole_genus` (genus 3) checkpoints,
  weld regression, fillet/chamfer/clamp, exporter primitives, determinism, and
  an inclusion–exclusion area property.

### Fixed
- Ear-clipping triangulation is now robust on keyhole-bridged (zero-width slit)
  polygons: the containment test skips corner-coincident bridge duplicates and
  tests strict interior, so holed caps triangulate correctly instead of falling
  back to an overlapping fan.

## [0.1.0] — Foundation

First working release: a complete, tested vertical slice of the tool.

### Added
- **Geometry kernel** (`std`-only): `Vec2`/`Vec3`/`Mat4`, `Contour`/`Shape` (2D)
  and `Mesh` (3D), ear-clipping triangulation with a fan fallback, `extrude`,
  `revolve`, and solid primitives (cuboid/cylinder/cone/sphere).
- **Scripting language & interpreter**: line-based commands for 2D/3D
  primitives, transforms, skills, scene inspection, render, export, and AI, with
  a reproducible command `history` behind `save`/`load`.
- **Parametric skills**: `gear` (involute), `boxjoint` (finger-jointed panels),
  `honeycomb`, `washer`, `standoff`; plus user **script-skills** loaded from
  `~/.cad-assist/skills/<name>.cad` with `${param}` substitution.
- **Exporters**: SVG (real-mm, Cricut/laser), DXF (AutoCAD `AC1015`), G-code
  (pen/CNC), STL and OBJ (3D printing/interchange).
- **Rendering**: Unicode braille canvas; 2D rasterisation and 3D wireframes using
  feature/crease edges for clean silhouettes.
- **AI bridge**: natural-language → script via `curl`/`jq` to the Anthropic
  Messages API (`ANTHROPIC_API_KEY`, `CAD_AI_MODEL`), with the full language
  reference as the system prompt.
- **CLI & REPL** (`cad`): interactive shell, `run`, `ai`, `new`, `skills`,
  `help`, `version`.
- **Examples**: `keychain-tag`, `laser-box`, `print-gear`, `lathe-knob`,
  `honeycomb-panel`.
- **Docs**: `README`, `docs/DESIGN.md`, `docs/ROADMAP.md`.

### Fixed
- Skill invocations no longer double-record in history (a script-skill's
  expansion is rolled back so replay re-expands it exactly once).
- `save` pins document units once and drops duplicate/stale `units` lines.
- `path … closed` now treats `closed` as a flag rather than miscounting it as a
  coordinate; bare flags are recognised generally (`Args::flag`).

[0.6.0]: #
[0.5.0]: #
[0.4.0]: #
[0.3.0]: #
[0.2.0]: #
[0.1.0]: #
