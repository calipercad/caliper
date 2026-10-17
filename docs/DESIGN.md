# cad-assist — Design

This document captures the **design principles** and the **current data model**
of cad-assist. It is the baseline that every roadmap phase builds on; read it
before making architectural changes, and update it when the model changes.

## Principles

1. **`std`-only, offline, auditable.** No cargo dependencies in the kernel. If a
   feature seems to need a crate, first ask whether a focused, well-tested
   in-tree implementation is small enough (it usually is — triangulation,
   matrices, braille rasterisation, JSON string escaping all fit in a few dozen
   lines). External *programs* (`curl`, `jq`) are acceptable at the edges where
   reimplementing them would be irresponsible (TLS).
2. **Text is the interface.** Every capability is reachable from the line-based
   language, so it is scriptable, diffable, and reproducible. GUIs, if they ever
   come, are clients of the same core.
3. **One geometry representation per dimension.** All 2D curves tessellate to
   polylines (`Contour`); all 3D is triangle meshes (`Mesh`). Export, render, and
   the kernel ops all consume these uniform forms — no special cases per curve
   type. Precision is a function of segment count (`seg=`), chosen at creation.
4. **Skills are first-class.** A "skill" is a parametric generator. The library
   is meant to grow far faster than the core, and users must be able to extend it
   without recompiling (script-skills) or with full power (native `Skill` impls).
5. **The AI speaks the same language.** The model emits ordinary commands; there
   is no separate AI code path in the kernel. Anything the AI can make, a human
   can type, and vice-versa. This keeps the surface honest and testable.
6. **Every fix gets a test.** The kernel is geometry — regressions are subtle and
   silent. Lock behaviour in `crates/cad-core/src/lib.rs` (or module tests).

## Crate layout

```
cad-assist/                 workspace root (no package)
├── crates/
│   ├── cad-core/           the engine (library `cad_core`)
│   │   └── src/
│   │       ├── math.rs         Vec2/Vec3/Mat4, angles, EPS
│   │       ├── bbox.rs         Aabb2/Aabb3
│   │       ├── geom2d.rs       Contour, Shape + builders, windings, fillet/chamfer
│   │       ├── geom2d/
│   │       │   ├── boolean.rs  exact-integer union/difference/intersection/xor
│   │       │   └── text/       single-stroke font + stroke_text layout
│   │       ├── geom3d.rs       Mesh + primitives, holed extrude, revolve, triangulate
│   │       ├── geom3d/
│   │       │   └── csg.rs      BSP mesh booleans + shell (3D CSG)
│   │       ├── scene.rs        Units, Geometry, Entity, Scene
│   │       ├── args.rs         positional/option arg parsing
│   │       ├── skills/         Skill trait + registry + 20 built-ins
│   │       │                     (gear/honeycomb/…, mechanical/maker/craft)
│   │       ├── export/         svg, dxf, stl, obj, gcode writers
│   │       ├── render/         Braille/half-block canvases, wire/HLR/shade, views
│   │       ├── ai/             natural-language → script bridge (curl/jq)
│   │       ├── script/         tokenizer, Interpreter, reference.txt
│   │       └── lib.rs          module wiring + tests
│   └── cad-cli/            the `cad` binary (REPL, run, ai, new)
│   │   └── tests/            integration tests (e.g. phase1.rs)
├── examples/               runnable .cad scripts
└── docs/                   this file + ROADMAP.md
```

## Data model

- **`Vec2`/`Vec3`/`Mat4`** — column-major 4×4 affine transforms. All movement,
  rotation, and scaling flows through `Mat4` so 2D and 3D share one path.
- **`Contour { points: Vec<Vec2>, closed: bool, kind: ContourKind }`** — a single
  tessellated loop or open polyline, with `signed_area`/`area`/`length`/
  `ensure_ccw`/`fillet`/`chamfer`. **`ContourKind`** (`Polyline | Circle | Arc`)
  is a primitive hint recording when the points came from a known circle/arc, so
  exporters can round-trip native entities. The points stay the single source of
  truth; any point-mutating op (boolean, fillet, chamfer, non-rigid transform)
  resets the hint to `Polyline`.
- **`Shape { contours: Vec<Contour> }`** — one or more contours. After
  `normalize_windings`, solids are CCW and holes CW, decided by nesting-depth
  parity (so legacy CCW-hole shapes are repaired); `regions()` groups each hole
  under its smallest containing shell as a **`Region { outer, holes }`**. Boolean
  ops (`union`/`difference`/`intersection`/`xor`) delegate to `geom2d::boolean`
  and return a classified `Shape`. Builders: `rect`, `circle`, `polygon`, `star`,
  `polyline`, `arc`; edge ops: `fillet`, `chamfer`.
- **`geom2d::boolean`** — the exact-integer planar-overlay clipper. Both shapes'
  edges snap to a shared integer lattice (grid `= ext·1e-9`, keeping coords
  `≲2^30` so `i128` orient/intersection stay exact); the arrangement of atomic
  (crossing-free) edges is classified against the original float loops by op
  predicate, traced into loops by exact rotational order (interior-on-left), and
  classified outer/hole by nesting. A balance/trace guard returns a clean
  "non-manifold or self-intersecting" error instead of junk.
- **`Mesh { verts: Vec<Vec3>, tris: Vec<[usize;3]> }`** — CCW-from-outside
  triangles. `feature_edges(angle)` yields boundary + crease edges for clean
  wireframes; `is_closed_manifold` (half-edge balance) and
  `euler_characteristic` (V−E+F → genus) verify watertightness. Primitives:
  `cuboid`, `cylinder`, `cone`, `sphere`. Free functions: `extrude` (holed caps
  via keyhole bridging over `regions()`, welded to one shared vertex ring),
  `revolve`, `triangulate_with_holes`, `triangulate` (ear-clipping with a fan
  fallback).
- **`geom3d::csg`** — 3D constructive solid geometry. `union`/`difference`/
  `intersection` on closed-manifold meshes via a BSP-tree kernel (csg.js
  approach), then a T-junction healing pass (subdivide edges at stray vertices)
  so the output is a strict closed 2-manifold. `shell(mesh, t)` hollows a solid
  by eroding each face plane inward and subtracting (convex/prismatic-exact).
  Non-manifold input returns a clean error. The `union`/`subtract`/`intersect`
  commands dispatch here for meshes and to `geom2d::boolean` for shapes.
- **`Geometry` = `Shape | Mesh`**, wrapped in **`Entity { name, geom, layer,
  role, group }`**, held by **`Scene { entities, units, selected,
  auto_counter }`**. The scene tracks the current selection so most ops default
  to "the selected entity." `layer`/`role` (both `Option<String>`, via
  `Scene::set_attrs`) are export attributes: exporters group by layer (falling
  back to `role`, then `"0"`) and color by role. `group: Option<String>` tags
  entities that move/transform together; `Scene::member_names(target)` expands a
  group name to its members so `translate g` moves the whole group.
- **`geom2d::text`** — single-stroke text for engraving/plotting.
  `stroke_text(text, height, align)` lays out the built-in in-tree single-stroke
  vector font (`geom2d::text::font`, `const` data for printable ASCII) into a
  `Shape` of **open** contours (one per pen stroke). Open, sub-triangle text is
  for render/export only and is correctly ignored/rejected by the filled-region
  ops (`area`/`regions`/`extrude`).
- **`Units { Mm, Cm, In }`** — a document-level unit. Exporters scale by
  `to_mm()` so output is always physically correct regardless of authoring unit.

## The interpreter & history

`script::Interpreter` owns a `Scene`, the `SkillRegistry`, the user skill dir,
and a **history** of scene-modifying commands. The dispatch rule that keeps
`save` correct:

> **Record the highest-level deterministic command.**

- Primitives/ops record their own line — including the deterministic 2D boolean
  (`union`/`subtract`/`intersect`/`xor`) and edge (`fillet`/`chamfer`) commands.
- Skills (native *and* script) record only the single `skill …` line — a
  script-skill's expansion is rolled back out of history so replay re-expands it
  once, not twice.
- `ai` is *non-deterministic*, so it records the concrete generated commands and
  drops the `ai …` line.
- `render`/`export`/`load`/`save` never record.

`save` pins the current units once at the top and skips recorded `units` lines,
so a saved script has exactly one, correct unit declaration.

## Known limitations (addressed by the roadmap)

- **3D edge rounding is profile-only.** Mesh CSG (`box − cylinder`) and `shell`
  ship in Phase 3, but rounded edges come from filleting the 2D profile before
  `extrude` — rounding an arbitrary existing mesh's edges is future work, and
  `shell` is convex/prismatic-limited (a robust general inward offset is future
  work).
- **Dimensioning is bbox-only.** `dim` annotates a shape's overall width/height;
  per-feature dimensions, tolerances, and 3D dims are future work.
- **Exporter gaps remain**: no G2/G3 arc output in G-code, ASCII STL only (no
  binary/3MF/PDF). DXF `CIRCLE`/`ARC` and SVG `<circle>` now round-trip. →
  Phase 7.
- **AI is single-shot** (generate → run), with no inspect-and-iterate loop. →
  Phase 6.

## Testing

`cargo test` runs kernel + interpreter tests and the crate doctest. Geometry
work should add: unit tests for the math, end-to-end interpreter tests for new
commands, and (Phase 8) property/fuzz tests for the triangulator and booleans.
