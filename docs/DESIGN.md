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
│   │       │   ├── offset.rs   polygon offset / inset (round/miter/bevel joins)
│   │       │   ├── sketch.rs   2D constraint solver (Gauss–Newton) → Shape
│   │       │   └── text/       single-stroke font + TrueType glyf outline text
│   │       ├── geom3d.rs       Mesh + primitives, holed extrude, revolve, mass props
│   │       ├── geom3d/
│   │       │   ├── csg.rs      BSP mesh booleans + shell (3D CSG)
│   │       │   └── section.rs  axis-aligned mesh cross-section → 2D Shape
│   │       ├── scene.rs        Units, Geometry, Entity, Scene
│   │       ├── args.rs         positional/option arg parsing
│   │       ├── skills/         Skill trait + registry + 20 built-ins
│   │       │                     (gear/honeycomb/…, mechanical/maker/craft)
│   │       ├── export/         svg, dxf, stl (ascii+binary), obj, gcode writers
│   │       ├── import/          svg, dxf, stl readers (round-trip own output)
│   │       ├── render/         Braille/half-block canvases, wire/HLR/shade, views
│   │       ├── ai/             copilot: Transport trait + curl/jq + agent loop
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
  `polyline`, `arc`; edge ops: `fillet`, `chamfer`, `offset`. Measurement:
  `area`, `perimeter`, `centroid` (area-weighted, holes subtract), `bbox`.
- **`geom2d::offset`** — polygon offset/inset. `offset(shape, dist, join, seg)`
  (`+` grows, `−` shrinks) moves each contour's edges along `−perp(dir)·dist`
  (orientation-free — the winding encodes the material side), reconnects corners
  with a round/miter/bevel **`JoinKind`**, and `normalize_windings()`. A per-edge
  reversal test drops any contour an over-large inset erodes through, so an inset
  past a profile's own width returns empty rather than self-intersecting junk.
  Independent of the boolean engine — fast and robust on high-vertex/concave
  contours.
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
  `cuboid`, `cylinder`, `cone`, `sphere`. Measurement: `signed_volume`,
  `surface_area`, `centroid` (volume-weighted via signed tetrahedra). Free
  functions: `extrude` (holed caps via keyhole bridging over `regions()`, welded
  to one shared vertex ring), `revolve`, `triangulate_with_holes`, `triangulate`
  (ear-clipping with a fan fallback).
- **`geom3d::section`** — `section(mesh, axis, at)` slices a mesh by an
  axis-aligned plane: marching-triangles cut segments are welded on a grid and
  walked into closed loops (a 2D `Shape`); a dangling endpoint (open/non-manifold
  cut) is a clean error.
- **`geom3d::bevel`** — 3D edge `fillet`/`chamfer` for convex/prismatic solids.
  Each sharp convex edge contributes cutting plane(s) (chamfer: one bisector;
  fillet: `seg` cylinder-tangent planes); the beveled solid is rebuilt by
  **vertex enumeration** of the convex polytope bounded by the face planes plus
  those cuts (every valid triple-plane vertex, assembled into faces) — exact and
  closed-manifold by construction, so it sidesteps the accumulating fragility of
  chaining many BSP booleans. Non-convex input is rejected.
- **`geom3d::csg`** — 3D constructive solid geometry. `union`/`difference`/
  `intersection` on closed-manifold meshes via a BSP-tree kernel (csg.js
  approach), then a T-junction healing pass (subdivide edges at stray vertices)
  so the output is a strict closed 2-manifold. `shell(mesh, t)` hollows a solid
  by eroding each face plane inward and subtracting (convex/prismatic-exact).
  Non-manifold input returns a clean error. The `union`/`subtract`/`intersect`
  commands dispatch here for meshes and to `geom2d::boolean` for shapes.
- **`Geometry` = `Shape | Mesh`**, wrapped in **`Entity { name, geom, layer,
  role, group, part }`**, held by **`Scene { entities, units, selected,
  auto_counter }`**. The scene tracks the current selection so most ops default
  to "the selected entity." `layer`/`role` (both `Option<String>`, via
  `Scene::set_attrs`) are export attributes: exporters group by layer (falling
  back to `role`, then `"0"`) and color by role. `group: Option<String>` tags
  entities that move/transform together; `Scene::member_names(target)` expands a
  group name to its members so `translate g` moves the whole group — and the
  same tag names an **assembly** (`assembly`/`export group=`). `part:
  Option<String>` (via `Scene::set_part`) is the bill-of-materials label: `bom`
  counts entities by part label, falling back to the name with array digits
  stripped. Exporters take an optional member-name filter so a group exports as
  one file.
- **`geom2d::text`** — text as geometry, two forms:
  - `stroke_text(text, height, align)` lays out the built-in in-tree single-stroke
    vector font (`geom2d::text::font`, `const` data for printable ASCII) into a
    `Shape` of **open** contours (one per pen stroke) — for engraving/plotting;
    correctly ignored by the filled-region ops (`area`/`regions`/`extrude`).
  - `outline_text(bytes, text, height, align, seg)` (`geom2d::text::outline`)
    parses a TrueType font and returns **filled, closed** glyph contours for
    through-cuts — a std-only, bounds-checked `glyf` reader (`head`/`maxp`/
    `hhea`/`hmtx`/`cmap`/`loca`/`glyf`) that flattens quadratic Béziers and
    winding-normalizes so holes are correct. TrueType only; `.otf`/CFF rejected.
- **`Units { Mm, Cm, In }`** — a document-level unit. Exporters scale by
  `to_mm()` so output is always physically correct regardless of authoring unit.

## The interpreter & history

`script::Interpreter` owns a `Scene`, the `SkillRegistry`, the user skill dir, a
store of **variables** (`HashMap<String, f64>`), and a **history** of
scene-modifying commands. The dispatch rule that keeps `save` correct:

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

### Variables & expressions

The language is **parametric**: `set <name> <expr>` defines a numeric variable,
and wherever a command takes a value, a token containing `$` is evaluated as an
arithmetic expression (`geom2d`-free scalar math in `script::expr`) rather than
parsed as a literal. `$name` resolves a variable; `${expr}` braces group an
expression and splice into surrounding text (`part_${w}.svg`). Expansion runs in
`exec_line` after tokenising and before `Args` parsing, touching only the value
side of `key=value` and only tokens that contain `$` — so non-parametric scripts
are byte-for-byte unaffected, and skill arguments inherit variables for free.

Expansion is deliberately *not* baked into history: `exec_line` records the raw,
unexpanded line, so a saved `.cad` keeps `set w 40` and `rect $w $w*2` verbatim
and stays parametric across a round-trip. `set`/`unset` are recorded (ordered
before their uses) and, with the variable store, are carried in the undo/redo
`Snapshot { scene, history, vars, sketch }` so a parametric edit is reversible.
There is no typed value system elsewhere: a token is a `String` and numeric
meaning is still imposed at read time by `Args` — the evaluator sits one level
above, rewriting a token to a number string before it reaches `Args`.

### Constraint sketches

`geom2d::sketch` makes the language a constraint-driven sketcher. A `Sketch`
(held on the interpreter as the active sketch, in the undo snapshot) is points +
line segments plus geometric `Constraint`s (`fix`, `coincident`, `horizontal`/
`vertical`, `parallel`/`perpendicular`, `equal`, `distance`, `length`, `angle`).
`solve` treats the non-fixed point coordinates as the free variables and each
constraint as residual equation(s), then runs Gauss–Newton with Levenberg–
Marquardt damping — a numeric central-difference Jacobian and a small in-tree
dense `solve_linear` (Gaussian elimination; `math.rs` has no general linear
algebra). Residual assembly walks ordered `Vec`s only, so a solve is
deterministic and `save`/replay (which re-runs the recorded `solve` line) is
bit-identical. `sketch close` emits the solved points as a `Shape`. Scope is
straight segments; arcs/circles and tangent/radius constraints are future work.

## Known limitations (addressed by the roadmap)

- **3D edge rounding is convex/prismatic.** `fillet`/`chamfer` round a solid
  mesh's edges (`geom3d::bevel`, Phase 13) for convex/prismatic solids; rounding
  an arbitrary concave mesh's edges is future work, as is a robust general inward
  offset (`shell` is likewise convex/prismatic-limited).
- **Dimensioning is bbox-only.** `dim` annotates a shape's overall width/height;
  per-feature dimensions, tolerances, and 3D dims are future work.
- **Remaining interop/UX gaps** (deferred to keep the kernel zero-dependency):
  REPL tab-completion (needs raw-terminal/line-editor crates), and 3MF/PDF
  export (3MF is a ZIP container). Import (svg/dxf/stl), undo/redo, `watch`,
  binary STL, and G2/G3 G-code shipped in Phase 7.
- **Post-1.0 (held back to stay zero-dependency)**: a sandboxed WASM/dylib
  plugin host for third-party *native* skills (script-skills + `cad plugin`
  cover extension today, see [PLUGINS.md](PLUGINS.md)), coverage-guided
  `cargo-fuzz`, and a generated docs site.

## Extensibility

The extension mechanism is **script-skills**: a `.cad` file with
`${key:default}` placeholders in `~/.cad-assist/skills/` becomes a `skill
<name>` command (managed by `cad plugin`). Native skills implement the `Skill`
trait and register in `SkillRegistry::with_builtins`. Both are std-only and need
no plugin host. See [PLUGINS.md](PLUGINS.md).

## Testing

`cargo test` runs kernel + interpreter tests, the phase integration suites
(`tests/phase1..7.rs`), the crate doctest, and **randomized property tests**
(`tests/properties.rs`): a seeded std-only PRNG checks triangulate / boolean /
CSG invariants across many cases. CI gates `fmt --check`, `clippy -D warnings`,
build, and test on every push. Geometry work should add a unit test for the
math and an end-to-end interpreter test for each new command.
