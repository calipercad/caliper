# cad-assist — Roadmap

A **spread-out, checkpointed plan** for growing cad-assist from a solid v0.1
foundation into a deeply feature-rich, AutoCAD-class terminal CAD toolkit.

## How to use this roadmap

- Each **phase** is an independently shippable milestone with its own version,
  **design**, work items, and a **checkpoint** — a concrete, testable exit
  criterion plus a demo command. A phase is "done" only when its checkpoint
  passes with a green `cargo test`.
- Phases are ordered by dependency and value. You can stop after any phase and
  have a coherent, releasable tool. To **resume across sessions**, find the first
  unchecked box, re-read that phase's Design, and continue — see *Resuming* at
  the bottom.
- Keep [`DESIGN.md`](DESIGN.md) in sync as the data model evolves.

## Status at a glance

**🎉 v1.0 released — all eight foundational phases complete. Now building beyond.**

| Phase | Version | Theme | State |
|---|---|---|---|
| 0 | v0.1 | Foundation | ✅ complete |
| 1 | v0.2 | Robust 2D geometry (booleans, fillets, real holes) | ✅ complete |
| 2 | v0.3 | Text & engraving + layered cut/score/engrave export | ✅ complete |
| 3 | v0.4 | 3D CSG, fillets/chamfers, shell/offset | ✅ complete |
| 4 | v0.5 | Drafting & rendering (HLR, shading, views, arrays, dims) | ✅ complete |
| 5 | v0.6 | Library explosion (15+ maker/craftsman skills) | ✅ complete |
| 6 | v0.7 | AI copilot (inspect-iterate-repair loop) | ✅ complete |
| 7 | v0.8 | Interop & UX (import, undo, watch, richer export) | ✅ complete |
| 8 | v1.0 | Extensibility & release (plugins, fuzz, CI, packaging) | ✅ complete |
| 9 | v1.1 | Parametric expressions & variables | ✅ complete |

---

## Post-1.0 directions

The eight-phase roadmap delivered a coherent v1.0. Growth continues as
independent, checkpointed phases in the same spirit (std-only, text-first,
every fix gets a test). Candidate follow-ups: robust 2D polygon offset/inset
(kerf & tool compensation, outline text, general shell); measurement & analysis
(mass properties, interference, cross-sections); assemblies & BOM. The first
post-1.0 phase is **Phase 9**, below.

## Phase 9 — Parametric expressions & variables ✅ (v1.1)

**Objective.** Make designs *parametric*: name a value once and drive the whole
part from it, with arithmetic wherever a number is accepted.

**Delivered.**
- **Variables** — `set <name> <expr>`, `unset`, `vars`. Values are expressions
  over the current variables (`set h $w*2`), stored on the `Interpreter` and
  carried in the undo/redo snapshot.
- **Expressions** — any argument token containing `$` is evaluated: `$name`,
  bare `$w*2`, and `${expr}` braces (escape hatch + string interpolation into
  filenames/text). Only the value side of `key=value` is expanded; tokens with
  no `$` stay literal (fully backward compatible). Skill args inherit variables.
- **Evaluator** (`src/script/expr.rs`) — `+ - * / %`, right-assoc `^`, unary
  sign, parentheses; functions (`sin`/`cos`/`sqrt`/`min`/`max`/`clamp`/`rad`/
  `deg`/…); constants `pi`/`tau`/`e`; clear errors.
- **Round-trip** — `save` records raw lines, so expressions and `set` survive
  verbatim and re-`load` reproduces the scene.

**Checkpoint (met).** `set w 40 ; rect $w $w*2` builds a 40×80 plate; `save`
emits the literal expressions; `undo` reverts a variable; `cargo test` green
(`tests/phase9.rs` + `expr` unit tests), `fmt`/`clippy -D warnings` clean.

---

## Phase 0 — Foundation ✅ (v0.1)

**Delivered.** Geometry kernel (`Vec/Mat`, `Contour`/`Shape`, `Mesh`,
triangulation, extrude, revolve), the scripting language + `Interpreter` with a
reproducible `history`, five parametric skills (`gear`, `boxjoint`, `honeycomb`,
`washer`, `standoff`), five exporters (SVG/DXF/STL/OBJ/G-code), braille 2D/3D
rendering with feature-edge wireframes, the `curl`/`jq` AI bridge, a REPL and
CLI, five runnable examples, and a test suite (10 tests + doctest, all green).

**Checkpoint (met):** `cargo test` green; all five `examples/*.cad` run and
produce valid files; 3D wireframes are clean; AI fails gracefully without a key.

---

## Phase 1 — Robust 2D geometry (v0.2)

**Objective.** Make 2D genuinely CAD-grade: boolean operations, fillets/chamfers,
and — the headline — **real holes that survive extrusion**.

**Design.**
- **Polygon booleans.** Implement a Greiner–Hormann (or Vatti) clipper over
  `Contour` lists producing a `Shape` (outer + holes). New module
  `geom2d/boolean.rs` exposing `union`, `difference`, `intersection`,
  `xor`. Robustness: snap-rounding to an epsilon grid; handle coincident edges
  and multiple result loops; classify each output loop by winding to assign
  outer-vs-hole. This is the load-bearing algorithm of the phase — budget test
  time accordingly.
- **Holed extrusion.** Once a `Shape` can carry true holes, extend `extrude` to
  triangulate the capped face **with holes** (bridge each hole to the outer
  boundary before ear-clipping, or use a constrained triangulation). This fixes
  the documented v1 limitation so a washer/gear bore is actually hollow in STL.
- **Fillet/chamfer (2D).** Corner rounding/beveling on `Contour` vertices:
  `fillet <r> [target=]`, `chamfer <d> [target=]`. Operates on straight-segment
  joints; arcs are re-tessellated.
- **Exporter fidelity.** Emit true `CIRCLE`/`ARC` in DXF where a contour is known
  to be a circle/arc (tag builders with an optional primitive hint), and real
  `<circle>`/`<rect>` in SVG for cleaner cutter paths.

**Work items.**
- [x] `geom2d/boolean.rs`: `union`/`difference`/`intersection`/`xor` on shapes.
- [x] Winding-based outer/hole classification for boolean results.
- [x] Language: `union`, `subtract`, `intersect` (+ `xor`) commands (2 operands → new entity).
- [x] Rework `extrude` to triangulate holed faces (holes cut through caps).
- [x] `fillet` / `chamfer` commands for 2D contours.
- [x] DXF/SVG: emit primitive `CIRCLE`/`ARC`/`<circle>` when geometry is known.
- [x] Tests: boolean truth table, hole-through-extrude (manifold + Euler/genus
      check), fillet corner count/tangency, exporter primitive emission.

**Checkpoint.** `subtract` a circle from a rect, `extrude`, export STL → the hole
is a real through-hole (verified by a manifold/genus test in code). Demo:
```
rect 40 40 name=plate ; circle 8 name=bore ; subtract plate bore name=part
extrude 5 target=part name=solid ; export stl part.stl name=solid
```
`cargo test` green including a new `hole_is_manifold` test.

**Risks.** Boolean robustness is the classic CAD hard problem. Mitigate with
snap-rounding, a fuzz target (pulled forward from Phase 8 if needed), and a
conservative fallback that reports non-manifold input rather than emitting junk.

---

## Phase 2 — Text & engraving + layered export (v0.3)

**Objective.** First-class text for labels, engraving, and cut files, plus the
**layer semantics Cricut/laser workflows need** (cut vs score vs engrave).

**Design.**
- **Stroke text (engrave/plot).** Embed a public-domain single-stroke Hershey
  font as static data; `text "HELLO" h=10 [font=stroke]` emits open `Contour`s —
  ideal for pen plotters, drag-engravers, and laser scoring. No dependency; the
  font tables are `const` data in `geom2d/text/`.
- **Outline text (cut).** A minimal TrueType (`glyf`) parser producing filled
  `Shape`s for through-cuts, quadratic béziers tessellated to `seg`. Kept
  optional and self-contained (`font=outline path=/…/Font.ttf`).
- **Layers.** Add an optional `layer` (and `role`: cut/score/engrave) to
  `Entity`. SVG export groups by layer with conventional per-role stroke colors
  and `<g inkscape:label>`; DXF writes entities onto named layers. This is what
  makes a single SVG drive a multi-op cutter job.

**Work items.**
- [x] `geom2d/text/` with an embedded single-stroke font + `stroke_text()`.
- [x] `text` command (stroke path); alignment (`align=left|center|right`) & spacing.
- [ ] Optional `glyf` outline parser → filled shapes for cutting. *(deferred — a
      self-contained follow-up; stroke text is the shipped primary path.)*
- [x] `Entity.layer` / `role`; `layer <name>` and `role <cut|score|engrave>` cmds.
- [x] SVG layered/colored export; DXF named layers.
- [x] Tests: stroke text bbox/alignment, layered SVG structure, DXF layers.

**Checkpoint.** Engrave a label and export a layered SVG whose cut and engrave
paths are on separate, correctly-colored layers. Demo:
```
text "MAKER" h=12 name=label role=engrave
rect 80 24 rx=4 name=badge role=cut
export svg badge.svg
```
SVG contains two `<g>` layers with distinct stroke colors; `cargo test` green.

**Risks.** TTF parsing scope-creep — keep the outline parser to `glyf` only and
document unsupported tables; stroke text (self-contained) is the primary path.

---

## Phase 3 — 3D CSG, fillets/chamfers, shell (v0.4)

**Objective.** True solid modeling: subtract/union/intersect meshes, round/bevel
edges, and hollow parts.

**Design.**
- **Mesh booleans.** A BSP-based CSG (split each mesh by the other's planes,
  classify fragments inside/outside, recombine) in `geom3d/csg.rs`, exposing
  `union`/`difference`/`intersection` on `Mesh`. Follow the well-trodden
  csg.js/BSP approach; add coplanar-face handling and an epsilon. Validate
  manifoldness on output.
- **Edge fillet/chamfer (3D).** Start with fillets/chamfers on the vertical edges
  produced by `extrude` (analytic — replace the sharp corner with an arc/bevel in
  2D *before* extruding), then generalize to mesh-edge rounding later.
- **Shell / offset.** `shell <thickness>` to hollow a solid (offset inward and
  subtract) — critical for enclosures and print weight.

**Work items.**
- [x] `geom3d/csg.rs`: BSP build, clip, invert, `union`/`difference`/`intersection`
      (plus a T-junction healing pass so output is a strict closed 2-manifold).
- [x] Language: `union`/`subtract`/`intersect` auto-dispatch by geometry kind.
- [x] `shell <t>` on meshes (convex/prismatic-limited). 3D edge rounding is the
      profile workflow: `fillet`/`chamfer` the 2D profile, then `extrude`.
- [x] Manifold / signed-volume validators used by tests (from Phase 1).
- [x] Tests: box−cylinder genus/volume, union & intersection volume, disjoint
      union, shell wall, command auto-dispatch, rounded-extrude.

**Checkpoint.** `box − cylinder` yields a solid with a real bored hole, exported
STL passes the manifold validator. Demo:
```
box 40 40 20 name=block ; cylinder 8 30 name=drill ; translate 20 20 -5 target=drill
subtract block drill name=holed ; export stl holed.stl name=holed
```
`cargo test` green with a `csg_manifold` test.

**Risks.** CSG numerical robustness. Mitigate with plane snapping, a fuzz target,
and clear "input not manifold / self-intersecting" errors instead of silent bad
output. Ties back to Phase 8 fuzzing.

---

## Phase 4 — Drafting & rendering (v0.5)

**Objective.** Make the terminal preview read like an engineering tool, and add
the transforms/layout every real drawing needs.

**Design.**
- **Rendering upgrades.** Hidden-line removal for 3D (depth-sort faces, or a
  z-buffer over the braille grid); optional shaded mode using ANSI grayscale via
  half-block characters and a light direction; on-screen axes/grid and a scale
  bar. New `render` modes: `shade`, `hlr`.
- **Multi-view & iso.** `view top|front|right|iso` presets; `render layout=views`
  to tile four viewports (three orthographic + iso), the classic CAD sheet.
- **Dimensioned drawings.** A dimensioning pass that annotates a 2D shape's bbox
  and key features with witness/extension lines and measured text (reusing Phase
  2 stroke text). `dim <target>` and export to SVG/DXF as a drawing sheet.
- **Layout ops.** `array linear|polar|grid`, `mirror axis=x|y|z`, `group`/`ungroup`
  — so a single tooth/bolt becomes a ring/grid, and assemblies move as one.

**Work items.**
- [x] HLR and `shade` (ANSI half-block) renderers; scale-bar overlay.
- [x] `view` presets (iso/top/front/right) + `render layout=views` tiling.
- [x] `array` (linear/polar/grid), `mirror`, `group`/`ungroup` (group tag on `Entity`).
- [x] Dimensioning pass (`dim`) + drawing-sheet SVG (reuses role layers).
- [x] Tests: array counts/positions, mirror symmetry, HLR edge culling, shade,
      view tiling, group transform, dim sheet.

**Checkpoint.** A polar `array` of a single gear tooth reconstructs a full gear;
`render layout=views` shows four correct viewports; a dimensioned rect exports a
readable SVG sheet. `cargo test` green.

**Risks.** HLR performance on dense meshes — cap resolution and cull by silhouette
first. Dimensioning aesthetics are iterative; ship "correct and legible" first.

---

## Phase 5 — Library explosion (v0.6)

**Objective.** Deliver "extremely feature rich" as a *breadth* of parametric
skills for makers, woodworkers, and machinists — the payoff of Phases 1–4.

**Design.** Each skill is a `Skill` impl (or, where purely 2D/compositional, a
shipped script-skill) leaning on the now-available booleans, fillets, text, and
arrays. Group by audience. Where a skill needs holes/subtraction it depends on
Phase 1/3 (call it out in the skill's doc).

**Candidate skills (target ≥15).**
- *Makers/electronics:* `gridfinity` bins/baseplates, `enclosure` (2-part snap/
  screw box with standoffs & vents), `standoff_grid`, `cable_clip`, `knob`.
- *Mechanical:* `sprocket` (roller-chain), `pulley` (GT2/HTD), `bevel_gear`,
  `rack`, `cam`, `bearing_spacer`, `flange`.
- *Woodworking/CNC:* `dovetail_box`, `living_hinge`, `tslot_joint`, `drawer`,
  `shelf_pin`, `template` (router template with guide bushing offset).
- *Craft/Cricut:* `card` (fold lines as score layer), `box_template` (gift box
  net), `stencil`, `monogram` (text + frame), `mandala` (polar-arrayed motif).

**Work items.**
- [x] Implemented 15 new skills across 3 modules (`mechanical`/`maker`/`craft`):
      flange, sprocket, rack, pulley, cam; gridfinity, enclosure, standoff_grid,
      knob, cable_clip; monogram, mandala, card, box_template, living_hinge.
- [x] `docs/SKILLS.md` gallery (grouped tables + braille previews).
- [x] `all_skills_run_at_defaults` smoke test iterating `SkillRegistry` (≥20
      built-ins), gridfinity-manifold checkpoint, + targeted shape checks.

**Checkpoint.** `skills` lists ≥15 built-ins besides the original five; every
skill runs at defaults in a single smoke test; the gallery doc builds. Demo:
```
skill gridfinity units_x=3 units_y=2 name=bin ; render mode=3d
```
`cargo test` green with a `all_skills_run_at_defaults` test.

**Risks.** Scope. Timebox each skill; a script-skill is acceptable where it
suffices. Prefer correct, well-parameterized skills over many shallow ones.

---

## Phase 6 — AI copilot (v0.7)

**Objective.** Turn the single-shot `ai` into an **iterative copilot** that can
inspect what it built, correct errors, and refine toward a described goal.

**Design.**
- **Tool-use loop.** Give the model a small tool set mapped to the interpreter:
  `run_commands`, `render` (returns the braille preview as text), `list`/`bbox`
  (returns scene state), and `errors`. Loop: generate → execute → feed back
  render+state/errors → refine, until the model declares done or a step budget is
  hit. Implemented in `ai/agent.rs`; still only `curl`/`jq`, now multi-turn with
  a JSON message array.
- **Repair on error.** When a generated command fails, return the exact error to
  the model for a fix rather than aborting (`ai` currently aborts on first error).
- **Skill-aware retrieval.** Include the skill list + usages (and user
  script-skills) in context so the model prefers parametric skills over ad-hoc
  geometry.
- **UX.** `ai edit "<change>"` refines the current scene; `ai fix` repairs the
  last failed run; optional streaming of the model's commands as they arrive.

**Work items.**
- [x] `ai/agent.rs`: multi-turn message array, conversational command protocol,
      step budget, per-turn atomic rollback.
- [x] `Transport` trait + `CurlTransport` + offline `MockTransport` (the seam);
      loop drives the interpreter (`exec_line`/render/list) and feeds errors back.
- [x] `ai` iterates by default + `ai edit "<change>"` + `ai fix` (one-shot is
      early convergence on `DONE`).
- [x] Skill usages (name+summary+usage) injected into the system prompt.
- [x] Tests: `tests/phase6.rs` loop harness via `MockTransport` (no network) —
      repair convergence, step-budget termination, one-shot, atomic rollback.

**Checkpoint.** With a mocked transport, the copilot converges a task that
requires one self-correction (a first attempt errors, second succeeds) and
terminates within budget. With a real key, `ai "…"` visibly iterates. `cargo
test` green (mocked, offline).

**Risks.** Non-determinism and cost. Bound steps and tokens; make the loop fully
testable offline via a transport trait so CI never hits the network.

---

## Phase 7 — Interop & UX (v0.8)

**Objective.** Round-trip with the outside world and make daily use pleasant.

**Design.**
- **Import.** SVG path/`<rect>`/`<circle>` → `Shape`; DXF `LWPOLYLINE`/`CIRCLE`/
  `ARC` → `Shape`; ASCII+binary STL → `Mesh`. New `import <file>` command and
  `geom2d`/`geom3d` readers. Enables "edit this existing cut file."
- **Editing UX.** `undo`/`redo` (snapshot or command-inverse stack), REPL
  tab-completion for commands/skills/entity names, `watch <file.cad>` to
  re-render on save, a `~/.cad-assist/config` for defaults (units, seg, render
  size), and error messages that suggest the nearest command/skill.
- **Richer export.** Binary STL, 3MF, PDF (vector) for print shops, and G2/G3
  arc output in G-code for smoother CNC/plot paths.

**Work items.**
- [x] SVG/DXF/STL (ASCII+binary) importers + `import` command (new `import/`).
- [x] `undo`/`redo`: `(scene, history)` snapshot stack.
- [x] `watch` mode (CLI, mtime poll) + `~/.cad-assist/config.cad` at startup.
- [x] Binary STL export + G2/G3 arcs in G-code.
- [~] REPL tab-completion, 3MF, PDF — **deferred** (real completion needs
      raw-terminal/line-editor crates and 3MF a ZIP container, both at odds with
      the std-only, zero-dependency principle; revisit if that rule is relaxed).
- [x] Tests: import→export round-trips (svg/dxf/stl), undo restores, gcode arcs.

**Checkpoint.** Import an SVG produced earlier, modify it, and re-export with no
geometry loss; `undo` returns the scene to its previous state exactly. Demo:
```
import badge.svg ; scale 1.5 ; export svg badge-big.svg
```
`cargo test` green with a `svg_roundtrip` and `undo_restores` test.

**Risks.** Import format sprawl. Support the common subset well; reject the rest
with clear messages rather than silently mangling.

---

## Phase 8 — Extensibility & release (v1.0)

**Objective.** Make the library truly extensible by third parties and ship a
polished, trustworthy 1.0.

**Design.**
- **Plugins.** A stable way to add native skills without forking: either a
  WASM skill host (sandboxed, portable) or a documented dynamic-library ABI, with
  `cad plugin add/list/remove` and a discovery dir. Prefer WASM for safety if the
  `std`-only rule can be preserved at the host boundary.
- **Correctness infra.** Property tests (triangulation always produces n-2
  triangles with correct total area; booleans preserve area invariants) and
  `cargo-fuzz` targets for the triangulator and CSG. Benchmarks for the hot paths.
- **Release.** CI (build/test/clippy/fmt on push), prebuilt binaries, `cargo
  install` support, a Homebrew formula, and a docs site generated from
  `reference.txt` + these docs.

**Work items.**
- [x] Plugin model = std-only **script-skills** + `cad plugin list|new|add|remove`
      + `docs/PLUGINS.md` (native `Skill` trait also documented).
- [x] Randomized std-only property tests (seeded PRNG) for triangulate / boolean
      / CSG invariants (`tests/properties.rs`); `Mesh::signed_volume` added.
- [x] CI workflow gating fmt + `clippy -D warnings` + build + test; all clippy
      warnings cleaned.
- [x] Packaging: Cargo metadata, `cargo install` support, README polish, a
      Homebrew formula template (`dist/`).
- [~] WASM/dylib plugin host, `cargo-fuzz`, prebuilt binaries + a generated docs
      site — **post-1.0** (external deps / nightly, at odds with the std-only,
      zero-dependency kernel; script-skills + seeded property tests cover the
      need today).

**Checkpoint.** A third-party plugin loads and its skill appears in `skills`;
fuzz targets run clean for a fixed corpus; CI is green; `cargo install` yields a
working `cad`. Tag **v1.0**.

**Risks.** Plugin ABI stability is a long-term commitment — version it explicitly
and keep the surface minimal.

---

## Resuming across sessions

1. Open the **Status at a glance** table; the first non-✅ row is the active phase.
2. Re-read that phase's **Design** and **Work items**; the first unchecked box is
   the next task.
3. Make the change, add its test, and only check the box when `cargo test` is
   green. When every box in a phase is checked **and** its Checkpoint demo passes,
   flip the phase to ✅, bump the version, and add a `CHANGELOG` entry.
4. If the data model changed, update [`DESIGN.md`](DESIGN.md) in the same commit.

Each checkpoint is a safe stopping point: the tool is coherent and releasable
there, so work can pause and resume without losing the thread.
