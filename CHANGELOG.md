# Changelog

All notable changes to cad-assist are recorded here. Versions map to roadmap
phases (see [`docs/ROADMAP.md`](docs/ROADMAP.md)).

## [1.6.2] — AI copilot ignores prose

- **Fixed:** the copilot executed the model's explanation sentences as commands,
  producing spurious `unknown command 'The'` / `'There's'` errors and wasting
  repair steps (and losing a `DONE` that shared a line with prose). The agent now
  skips non-command "prose" lines as narration — distinguishing them from a
  mistyped command (a lowercase keyword like `widget`, still fed back for
  repair) by the line's first token. `DONE` is also recognized with trailing
  punctuation (`DONE.`) or a list marker. The system prompt reinforces
  "commands only; non-command lines are ignored."

## [1.6.1] — AI error-reporting fix

- **Fixed:** a failed/unexpected AI API response crashed with a cryptic
  `jq: error … Cannot index string with string "message"` instead of a useful
  message. The response filter now tolerates any shape (`?`-guarded) and the
  error path reports the real message — `API error: <message>` for Anthropic
  (`.error.message`), bare `{"error":"…"}`, or `.message` shapes, and the raw
  response (with a hint to check `ANTHROPIC_BASE_URL`/model) for non-JSON.
- **Changed:** default `CAD_AI_MODEL` is now `claude-sonnet-5-5` (a current model
  id); the old `claude-sonnet-5` was stale. Override with `CAD_AI_MODEL`.

## [1.6.0] — Sketch arcs, circles & curve constraints

The constraint sketcher (v1.5) grows curved geometry. Still std-only.

### Added
- **Sketch circles & arcs**: inside an active sketch, `circle <name> <centerPt>
  <r>` adds a circle (radius becomes a solver variable) and
  `arc <name> <center> <start> <end>` adds an arc (auto-constrained so both
  endpoints are co-radial). Outside a sketch, `circle`/`arc` still create scene
  primitives.
- **Curve constraints**: `radius`, `tangent` (line–circle and circle–circle,
  external), `concentric`, `point_on` (point on a circle or a segment), and
  `equal` extended to circle radii. The solver's free variables now include
  circle radii.
- **Emission**: `sketch close` emits the segment/arc boundary (arcs tessellated
  into the outline) plus each circle as its own contour, so a constrained hole
  becomes a real hole after winding classification.
- Example `examples/constrained-washer.cad`.

### Scope
- Straight segments, full circles, and circular arcs; spline/ellipse geometry is
  future work. Under-constrained sketches converge to the nearest solution.

## [1.5.1] — Docs & examples

Documentation and example refresh; no code or behavior changes.

- New examples: `bored-block.cad` (3D CSG through-hole → `measure` showing genus
  1 → `section`) and `sketch-to-part.cad` (constraint sketch → `solve` →
  `extrude` into a solid).
- README: the Examples table lists all 12 scripts; the language quick-reference,
  feature bullets, and status are brought up to date through v1.5 (offset,
  measurement/section, assemblies/BOM, 3D fillet, outline text, constraint
  solver).
- Dropped a stray `export stl` line from the 2D `parametric-flange.cad` example.

## [1.5.0] — 2D constraint sketch solver

cad-assist becomes a constraint-driven sketcher: build a sketch from points and
line segments, relate them geometrically, and `solve` finds the exact geometry —
the biggest "real CAD" addition since the kernel. Still std-only (a small
in-tree Gauss–Newton solver, no dependencies).

### Added
- **Constraint solver** (`geom2d::sketch`): a `Sketch` of points + segments with
  constraints `fix`, `coincident`, `horizontal`, `vertical`, `parallel`,
  `perpendicular`, `equal`, `distance`, `length`, `angle`. `solve` runs a
  Gauss–Newton / Levenberg–Marquardt iteration (numeric Jacobian + an in-tree
  dense `solve_linear`), then the sketch emits a `Shape`. Deterministic — a
  solve replays bit-identically through `save`.
- Commands: `sketch <name>`, `point <name> <x> <y>`, `segment <name> <p1> <p2>`,
  `constrain <type> <refs…> [value]`, `solve`, `sketch close [open]`. Point
  coordinates and constraint values accept `$var`/expressions (Phase 9), so one
  number can drive a constrained design. Undo/redo cover sketch state.
- Under-constrained sketches converge to the nearest solution from the initial
  guess; conflicting constraints minimize residual and are reported, never crash.
- Example `examples/constrained-plate.cad`.

### Also
- **`ANTHROPIC_BASE_URL`** env var now overrides the AI API host (defaults to
  `https://api.anthropic.com`), alongside `ANTHROPIC_API_KEY` / `CAD_AI_MODEL` —
  for proxies and gateways.

## [1.4.0] — Outline / stencil text

`text` can now emit **filled, cut-out letters** from a TrueType font, alongside
the existing single-stroke engraving paths — for signage, stencils, and inlays.
Completes the one item deferred from Phase 2. Still std-only, zero dependencies.

### Added
- **TrueType `glyf` outline parser** (`geom2d::text::outline`, std-only,
  big-endian, every read bounds-checked): reads `head`/`maxp`/`hhea`/`hmtx`/
  `cmap` (formats 0/4/6/12)/`loca`/`glyf`, flattens quadratic Béziers with
  implied on-curve midpoints, and returns closed-contour glyphs. Holes ('O','A',
  'e') and composite glyphs (translation components) are handled; windings are
  normalized so fills/holes are correct regardless of the font's convention.
- `text "…" font=outline path=<font.ttf> [seg=8]` produces a filled `Shape`
  (default `font=stroke` is unchanged). Composes with `offset`/`inset`, layers,
  roles, extrude, and export like any shape.
- `tests/phase14.rs` synthesizes a minimal in-memory TrueType font (no font is
  bundled) to exercise the parser end to end.

### Scope
- TrueType `glyf` outlines only; `.otf`/CFF (PostScript) fonts are rejected with
  a clear error. No kerning; `h` is the em size. A malformed font returns a
  `CadError`, never panics.

## [1.3.0] — 3D edge fillet & chamfer

`fillet` and `chamfer` now round/bevel the edges of a solid `Mesh`, not just the
2D profile before `extrude` — so the top/bottom rims of an extrusion and the
edges of a CSG result can be softened. Both commands auto-dispatch by geometry
kind (2D shape → contour corners, as before; 3D mesh → edge bevel).

### Added
- **3D edge bevel** (`geom3d::bevel`): `fillet(mesh, r, seg, angle)` replaces each
  sharp convex edge with `seg` facets tangent to a radius-`r` cylinder (a faceted
  round); `chamfer(mesh, d, angle)` cuts each edge back by `d`. Corners where
  edges meet blend automatically.
- Built by **vertex enumeration** of the convex polytope bounded by the solid's
  face planes plus the per-edge cutting planes — exact and closed-manifold by
  construction, avoiding the fragility of many sequential BSP booleans.
- Commands: `fillet <r> [seg=3] [angle=20] [target=]` and
  `chamfer <d> [angle=20] [target=]` on a mesh. `angle=` is the sharpness
  threshold (so a cylinder's side facets aren't beveled, only its rims).
- Example `examples/rounded-box.cad`.

### Scope
- Defined for **convex / prismatic solids** (boxes, extruded/revolved prisms,
  wedges) — the same regime as `shell`. A non-convex solid is rejected with a
  clean error. The result is verified a closed manifold.

## [1.2.0] — Geometry & assembly expansion

Three post-1.0 phases in one release: a real 2D offset, measurement/analysis,
and multi-part assemblies with a bill of materials. Still std-only and
zero-dependency.

### Phase 10 — 2D offset / inset
- `offset <dist>` / `inset <dist>` grow (+) or shrink (−) a 2D profile:
  `join=round|miter|bevel`, `seg=`, `miter=`, in place or (with a new `name=`)
  as a separate entity (kerf / tool-path). The engine
  (`geom2d::offset`, `Shape::offset`) offsets each contour and winding-classifies
  the result, dropping features an over-large inset erodes away. Unlocks laser
  kerf & CNC tool compensation, clearances, and gaskets.

### Phase 11 — Measurement & analysis
- `measure [target]` — mass properties: 2D area/perimeter/centroid/bbox; 3D
  volume/surface-area/centroid/bbox + watertightness & genus.
- `interfere [a b]` — clash detection for a pair, or an all-pairs scan with no
  args (AABB broad-phase + exact 2D boolean / 3D CSG intersection).
- `section <target> [axis=x|y|z] [at=] [name=]` — slice a mesh by an
  axis-aligned plane into a new 2D outline (`geom3d::section`).
- New library helpers: `Shape::{perimeter,centroid}`,
  `Mesh::{surface_area,centroid}`, `Aabb2/Aabb3::intersects`.

### Phase 12 — Assemblies & BOM
- `part <label> [target=]` tags an entity or group with a part type
  (`Entity.part`); `assembly <tag> <members…>` groups parts so they move/export
  as one; `bom [group=]` reports quantities (by part label, or name with array
  digits stripped) with per-part size and a total.
- `export … group=<tag>` writes a whole assembly (just its members) to one file.
- `list` now shows `part=`/`group=` tags; `part`/`assembly` round-trip through
  `save`.

## [1.1.0] — Parametric expressions & variables

The first post-1.0 feature: the script language becomes truly parametric. One
`set` drives the whole part, and any argument can be an arithmetic expression.
Still std-only and zero-dependency; fully backward compatible (expansion fires
only on a `$`).

### Added
- **Variables**: `set <name> <expr>`, `unset <name>`, `vars`. Values are
  expressions evaluated against the current variables (`set h $w*2`). Stored on
  the interpreter and included in undo/redo snapshots.
- **Expressions anywhere a value appears** — a token containing `$` is
  evaluated: `$name` references, bare `$w*2`, and `${expr}` braces (an escape
  hatch needing no variable, and string interpolation like `part_${w}.svg`).
  Only the value side of `key=value` is touched; tokens without `$` are
  literals. Skill arguments see variables too (`skill gear teeth=$t`).
- **Evaluator** (`src/script/expr.rs`, unit-tested): `+ - * / %`, right-assoc
  power `^`, unary sign, parentheses; functions `sin cos tan asin acos atan
  atan2 sqrt abs floor ceil round sign min max clamp pow hypot rad deg`;
  constants `pi tau e`. Clear errors for unknown variable/function, division by
  zero, and malformed input.
- `tests/phase9.rs` end-to-end suite; `help`/reference and `docs/DESIGN.md`
  updated.

### Preserved
- `save` records the raw command lines, so a saved `.cad` keeps the `set` lines
  and expressions verbatim — the parametric intent round-trips and re-`load`
  reproduces the scene.

## [1.0.0] — Extensibility & release

The roadmap is complete. This release makes cad-assist extensible, adds
correctness infrastructure and CI, and polishes packaging — all while keeping
the kernel std-only and zero-dependency.

### Added
- **Plugin surface**: `cad plugin list|new|add|remove` manages script-skills in
  `~/.cad-assist/skills/` (a `.cad` with `${param:default}` slots is runnable as
  `skill <name>` and listed in `skills`, no recompile). `Interpreter::skill_dir`
  exposes the directory. Full model — script-skills and native `Skill`-trait
  skills — documented in `docs/PLUGINS.md`.
- **Property tests** (`tests/properties.rs`): a seeded SplitMix64 PRNG drives
  randomized invariant checks — triangulate (n−2 triangles + area),
  boolean inclusion–exclusion + determinism, CSG manifold + positive volume +
  volume inclusion–exclusion. Deterministic, std-only (no cargo-fuzz).
- `Mesh::signed_volume` — public (was a duplicated test helper).
- **CI** (`.github/workflows/ci.yml`): gates `fmt --check`, `clippy -D warnings`,
  build, and test on stable; the crate is now clippy-clean.
- **Packaging**: workspace metadata (`repository`/`readme`/`keywords`/
  `categories`/`homepage`), `cargo install --path crates/cad-cli`, a README
  refresh, and a Homebrew formula template (`dist/cad-assist.rb`).

### Deferred (post-1.0, to stay zero-dependency)
- A WASM/dylib plugin host, coverage-guided `cargo-fuzz`, and a generated docs
  site.

## [0.8.0] — Interop & UX

cad-assist can now read files back, undo, watch, and emit richer output —
rounding out the "edit this existing cut file" workflow.

### Added
- **Import** (new `import` module, `import <file>` command): SVG (`<circle>`,
  `<path M/L/Z>`), DXF (`CIRCLE`/`ARC`/`LWPOLYLINE`), and STL (ASCII + binary).
  Round-trips cad-assist's own output — DXF is exact; SVG inverts the writer's
  Y-flip via the viewBox; STL welds repeated vertices into a `Mesh`.
- **undo / redo**: a `(scene, history)` snapshot stack taken before each
  scene-modifying command (bounded depth); `undo`/`redo` commands.
- **`cad watch <file.cad>`**: re-runs and re-renders the script whenever the
  file is saved (mtime polling, pure std).
- **Richer export**: binary STL (`export stl … binary`) and true `G2`/`G3` arc
  moves in G-code for `Circle`/`Arc` contours (smoother tool-paths, smaller
  files); arcs survive a DXF round-trip.
- **Startup config**: `~/.cad-assist/config.cad` runs at launch (units/defaults).
- **Tests**: `crates/cad-core/tests/phase7.rs` — SVG/DXF/STL (ascii+binary)
  round-trips, binary-STL byte size, undo/redo, and G-code arcs.

### Deferred (std-only principle)
- REPL tab-completion (needs raw-terminal or a line-editor crate), 3MF (ZIP
  container), and PDF export — out of scope while the kernel stays
  zero-dependency.

## [0.7.0] — AI copilot

`ai` is now an iterative inspect→run→repair copilot instead of a one-shot
generate-and-run, and the whole loop is testable offline.

### Added
- **Transport seam** (`ai::Transport`): the Claude call is behind a trait, with
  the real `CurlTransport` (curl/jq, now a multi-turn messages array) and a
  public `MockTransport` that returns scripted replies and records what it saw —
  so the copilot is driven fully offline in tests (no network, no API key).
- **Copilot loop** (`ai::agent::run_agent`): each turn the model emits
  cad-assist commands (or `DONE`); the loop runs them, and on an error **rolls
  the turn back atomically** (scene + history) and feeds the exact error back for
  a fix; on success it feeds back the entity list + render. Bounded by a step
  budget; converges early on `DONE`. The system prompt now includes a live
  skills digest so the model prefers parametric skills.
- **Commands**: `ai <describe>` iterates by default; `ai edit "<change>"` refines
  the current scene; `ai fix` repairs the previous attempt. Successful commands
  land in `history`, so `save` replays an AI-built design.
- **Tests**: `crates/cad-core/tests/phase6.rs` — offline `MockTransport` loop
  harness: repair convergence (error → feedback → success), step-budget
  termination, one-shot `DONE`, and atomic per-turn rollback.

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

[1.0.0]: #
[0.8.0]: #
[0.7.0]: #
[0.6.0]: #
[0.5.0]: #
[0.4.0]: #
[0.3.0]: #
[0.2.0]: #
[0.1.0]: #
