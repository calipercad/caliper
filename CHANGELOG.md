# Changelog

All notable changes to cad-assist are recorded here. Versions map to roadmap
phases (see [`docs/ROADMAP.md`](docs/ROADMAP.md)).

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

[0.2.0]: #
[0.1.0]: #
