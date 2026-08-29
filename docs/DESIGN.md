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
│   │       ├── geom2d.rs       Contour, Shape + builders
│   │       ├── geom3d.rs       Mesh + primitives, extrude, revolve, triangulate
│   │       ├── scene.rs        Units, Geometry, Entity, Scene
│   │       ├── args.rs         positional/option arg parsing
│   │       ├── skills/         Skill trait + registry + built-ins
│   │       ├── export/         svg, dxf, stl, obj, gcode writers
│   │       ├── render/         BrailleCanvas + 2D/3D renderers
│   │       ├── ai/             natural-language → script bridge (curl/jq)
│   │       ├── script/         tokenizer, Interpreter, reference.txt
│   │       └── lib.rs          module wiring + tests
│   └── cad-cli/            the `cad` binary (REPL, run, ai, new)
├── examples/               runnable .cad scripts
└── docs/                   this file + ROADMAP.md
```

## Data model

- **`Vec2`/`Vec3`/`Mat4`** — column-major 4×4 affine transforms. All movement,
  rotation, and scaling flows through `Mat4` so 2D and 3D share one path.
- **`Contour { points: Vec<Vec2>, closed: bool }`** — a single tessellated loop
  or open polyline, with `signed_area`/`area`/`length`/`ensure_ccw`.
- **`Shape { contours: Vec<Contour> }`** — one or more contours. Contour `[0]` is
  the outer boundary; later contours are holes (`add_hole`). Builders: `rect`,
  `circle`, `polygon`, `star`, `polyline`, `arc`.
- **`Mesh { verts: Vec<Vec3>, tris: Vec<[usize;3]> }`** — CCW-from-outside
  triangles. `feature_edges(angle)` yields boundary + crease edges for clean
  wireframes. Primitives: `cuboid`, `cylinder`, `cone`, `sphere`. Free functions:
  `extrude`, `revolve`, `triangulate` (ear-clipping with a fan fallback).
- **`Geometry` = `Shape | Mesh`**, wrapped in **`Entity { name, geom }`**, held by
  **`Scene { entities, units, selected, auto_counter }`**. The scene tracks the
  current selection so most ops default to "the selected entity."
- **`Units { Mm, Cm, In }`** — a document-level unit. Exporters scale by
  `to_mm()` so output is always physically correct regardless of authoring unit.

## The interpreter & history

`script::Interpreter` owns a `Scene`, the `SkillRegistry`, the user skill dir,
and a **history** of scene-modifying commands. The dispatch rule that keeps
`save` correct:

> **Record the highest-level deterministic command.**

- Primitives/ops record their own line.
- Skills (native *and* script) record only the single `skill …` line — a
  script-skill's expansion is rolled back out of history so replay re-expands it
  once, not twice.
- `ai` is *non-deterministic*, so it records the concrete generated commands and
  drops the `ai …` line.
- `render`/`export`/`load`/`save` never record.

`save` pins the current units once at the top and skips recorded `units` lines,
so a saved script has exactly one, correct unit declaration.

## Known limitations (addressed by the roadmap)

- **No booleans / CSG.** `extrude` builds side walls for every contour but caps
  only the outer contour, so holes are correct in 2D output but *not* cut through
  3D caps. True holes and part subtraction need polygon booleans (2D) and mesh
  booleans (3D). → Roadmap Phases 1 & 3.
- **No fillets/chamfers, no text, no dimensions.** → Phases 1, 2, 4.
- **Exporters emit polylines only** (no true DXF `ARC`/`CIRCLE`, no G2/G3 arcs,
  ASCII STL only). → Phases 1 & 7.
- **AI is single-shot** (generate → run), with no inspect-and-iterate loop. →
  Phase 6.

## Testing

`cargo test` runs kernel + interpreter tests and the crate doctest. Geometry
work should add: unit tests for the math, end-to-end interpreter tests for new
commands, and (Phase 8) property/fuzz tests for the triangulator and booleans.
