# cad-assist

A terminal-native, AI-assisted **parametric CAD toolkit** written in pure Rust.
No GUI, no cloud, no build-time downloads — it compiles offline and stays
auditable (`std` only). You describe parts in a tiny command language (or in
plain English via Claude), preview them as crisp braille line art right in your
terminal, and export real, machine-ready files for **Cricut/Silhouette, laser
cutters, pen plotters, CNC, and 3D printers**.

```
┌─────────────────────────────────────────────────────────────┐
│  cad-assist — terminal CAD with AI + parametric skills        │
│  type `help` for the language, `skills` to list generators    │
│  `ai <describe it>` to design with Claude · `quit` to exit     │
└─────────────────────────────────────────────────────────────┘
```

## Why

- **Terminal-first.** Everything is text: scriptable, diffable, pipeable,
  reproducible. A design is a `.cad` file you can version-control.
- **Real geometry kernel.** Exact-integer 2D booleans, true holed extrusion,
  BSP mesh CSG (`subtract`/`union`/`intersect`), 2D & 3D fillet/chamfer, polygon
  `offset`/`inset`, shell, stroke **and** TrueType outline text, arrays/mirror/
  groups, dimensioning, mass-property `measure`/`interfere`/`section`, and a
  **2D constraint solver** — with manifold verification on 3D output.
- **Parametric & constraint-driven.** `set` variables and `${…}` expressions
  drive any dimension; the `sketch`/`constrain`/`solve` solver finds geometry
  from geometric relations (perpendicular, equal, distance, …).
- **20 parametric "skills."** Gears, sprockets, pulleys, cams, gridfinity bins,
  enclosures, knobs, monograms, box templates, living hinges… driven by real
  engineering math. See [docs/SKILLS.md](docs/SKILLS.md); extend with your own
  ([docs/PLUGINS.md](docs/PLUGINS.md)).
- **AI copilot.** `ai "a 24-tooth gear with a 5mm bore, 6mm thick"` runs an
  inspect→run→repair loop with Claude. Zero HTTP deps — it shells out to
  `curl`/`jq`.
- **Round-trips the real world.** Import SVG/DXF/STL, `undo`/`redo`, `watch` a
  file, and export SVG, DXF, G-code (G2/G3 arcs), STL (ASCII + binary), OBJ.
- **Zero dependencies.** The entire kernel is `std`-only Rust.

## Install & build

Requires a recent stable Rust toolchain.

```sh
git clone <your-fork> cad-assist && cd cad-assist
cargo install --path crates/cad-cli    # installs the `cad` binary to ~/.cargo/bin
cad                                     # start the interactive shell
```

Or build in place:

```sh
cargo build --release                   # binary at target/release/cad
```

During development you can run it straight from cargo:

```sh
cargo run -p cad-cli            # REPL
cargo run -p cad-cli -- help    # language reference
cargo run -p cad-cli -- run examples/print-gear.cad
```

## Testing

The whole suite is `std`-only and offline — no network, no fixtures, no API key
(the AI path is exercised through a mock transport):

```sh
cargo test --workspace          # unit + per-phase integration + property tests
```

This runs the kernel/interpreter unit tests, the per-phase suites
(`crates/cad-core/tests/phase1..16.rs`), the doctest, and the randomized
**property tests** (`tests/properties.rs` — a seeded PRNG checks triangulation,
boolean, and CSG invariants across many cases, deterministically).

The same three gates CI enforces, which you can run locally before a commit:

```sh
cargo fmt --all --check                               # formatting
cargo clippy --all-targets --all-features -- -D warnings   # lints (warnings = error)
cargo test --workspace                                # tests
```

Run a single phase or a filter while iterating:

```sh
cargo test -p cad-core --test phase16                 # one phase suite
cargo test -p cad-core sketch                         # tests matching "sketch"
```

Smoke-test every example end to end (each should run without error):

```sh
for f in examples/*.cad; do cargo run -q -p cad-cli -- run "$f" >/dev/null && echo "ok $f"; done
```

Add a test with every change: geometry math gets a unit test in its module, and
each new command gets an end-to-end interpreter test in the matching
`tests/phaseN.rs` — see [`docs/DESIGN.md`](docs/DESIGN.md).

## Quick start

Interactive shell:

```
$ cargo run -p cad-cli
cad> units mm
cad> skill gear teeth=24 module=2 bore=5 thickness=6 name=g1
cad> render
cad> export stl gear.stl name=g1
```

One-liner from a script file:

```sh
cad run examples/laser-box.cad     # emits laser-box.svg + laser-box.dxf
```

Design with AI (needs an API key — see below):

```sh
export ANTHROPIC_API_KEY=sk-ant-...
cad ai "a hexagonal coaster 90mm across with a 10mm rim engraving groove"
```

## The language

One command per line; `#` or `;` starts a comment. Numbers are positional,
`key=value` are options, angles are degrees, lengths use the current units.
Run `cad help` for the full reference. The essentials:

| Group | Commands |
|---|---|
| 2D | `rect w h [rx=] [name=]`, `circle r`, `polygon sides r`, `star pts ro ri`, `arc r a0 a1`, `path x1 y1 … [closed]`, `text "…" [h=] [font=stroke\|outline path=<ttf>]` |
| 3D | `box w d h`, `cylinder r h`, `sphere r`, `cone r h` |
| Ops | `extrude h`, `revolve`, `translate`, `rotate`, `scale`, `array`, `mirror`, `group`/`ungroup`, `fillet`, `chamfer` (2D corners **or** 3D edges), `offset`/`inset`, `shell`, `dim`, `section` |
| Booleans | `union a b`, `subtract a b`, `intersect a b`, `xor a b` (2D shapes **or** 3D meshes, auto-dispatched) |
| Variables | `set w 40`, `unset w`, `vars` — then `$w` / `${w*2}` in any numeric arg |
| Sketch | `sketch <name>`, `point`, `segment`, `circle`, `arc`, `constrain <type> …` (incl. `radius`/`tangent`/`concentric`/`point_on`), `solve`, `sketch close` |
| Measure | `measure [name]`, `interfere [a b]`, `bom [group=]`, `part`, `assembly` |
| Skills | `skill <name> …` (see `skills`, [SKILLS.md](docs/SKILLS.md)) |
| Scene | `units`, `select`, `delete`, `clear`, `list`, `bbox`, `undo`, `redo` |
| I/O | `render [mode=wire\|hlr\|shade] [view=] [layout=views]`, `export <fmt> <path> [group=]`, `import <file>`, `save`, `load`, `ai <text>` |

## Skills — the parametric library

Run `skills` (or `cad skills`) to list all 20, with usage. By group:

- **Mechanical:** `gear`, `sprocket`, `rack`, `pulley`, `cam`, `flange`, `washer`, `standoff`
- **Maker / 3D:** `gridfinity`, `enclosure`, `standoff_grid`, `knob`, `cable_clip`, `boxjoint`
- **Craft / laser:** `monogram`, `mandala`, `card`, `box_template`, `living_hinge`, `honeycomb`

The full gallery with parameters and previews is in [docs/SKILLS.md](docs/SKILLS.md).

### Extending the library

Add your own, no recompile needed — manage script-skills with `cad plugin`:

```sh
cad plugin new coaster     # scaffold ~/.cad-assist/skills/coaster.cad
cad plugin list            # built-ins + your plugins
```

A script-skill is a `.cad` template with `${param:default}` slots; native Rust
skills implement the `Skill` trait. See [docs/PLUGINS.md](docs/PLUGINS.md).

## Rendering

`render` rasterises the scene into Unicode braille (2×4 dots per glyph). 2D
shapes draw directly; 3D meshes draw as rotated wireframes with crease/feature
edges only (no triangle soup). Tune it: `render mode=3d yaw=35 pitch=25 w=100 h=40`.

## Export formats

| Format | For | Notes |
|---|---|---|
| `svg` | Cricut, Silhouette, laser, vinyl | real-mm sizing, one cut path per contour |
| `dxf` | AutoCAD, LibreCAD, Fusion | `LWPOLYLINE`, `AC1015`, unit-tagged |
| `gcode` | pen plotters, drag knives, CNC contour | G2/G3 arcs for circles/arcs |
| `stl` | 3D printing, slicers | ASCII or `binary` |
| `obj` | mesh interchange | |

Omit `name=` to export **every** matching entity — handy for laser layouts
where each panel is its own cut path. SVG/DXF group entities onto layers by
`role` (cut/score/engrave). Read files back with `import <file.svg|dxf|stl>`.

## Text

`text` produces geometry two ways. The default `font=stroke` lays out the
built-in single-stroke font as open paths — ideal for pen plotters, drag
engravers, and laser scoring. `font=outline path=<font.ttf>` parses a TrueType
font into **filled, cut-out letters** (holes and all) for through-cuts — signage,
stencils, inlays. No font ships with cad-assist; point `path=` at one you have:

```
text "OPEN" font=outline path=/System/Library/Fonts/Supplemental/Arial.ttf h=40 role=cut
offset 0.4            # optional: grow for laser kerf
export svg sign.svg
```

TrueType (`glyf`) outlines only; `h` is the em size.

## AI setup

The `ai` command shells out to `curl` and `jq` (both must be on `PATH`) and
calls the Anthropic Messages API.

```sh
export ANTHROPIC_API_KEY=sk-ant-...     # required
export CAD_AI_MODEL=claude-sonnet-5     # optional; this is the default
```

`ai <describe it>` runs an **inspect→run→repair loop**: the model emits
commands, cad-assist runs them and feeds back the render + entity list (and any
error, for the model to fix), until the design matches or a step budget is hit.
`ai edit "<change>"` refines the current scene; `ai fix` repairs the last run.
The system prompt carries the language reference plus a live skills digest.

## Examples

Runnable scripts live in [`examples/`](examples):

| File | Demonstrates | Output |
|---|---|---|
| `keychain-tag.cad` | rounded tag + hole, Cricut/vinyl | SVG |
| `laser-box.cad` | finger-jointed enclosure, flat-packed | SVG + DXF |
| `honeycomb-panel.cad` | ventilated hex grille | SVG + DXF |
| `gasket.cad` | bored plate grown by a 2D **offset** to a uniform rim | SVG + DXF |
| `print-gear.cad` | involute gear, bored, extruded | STL |
| `lathe-knob.cad` | solid of revolution from a profile | STL |
| `rounded-box.cad` | **3D edge fillet** on every rim of a box | STL |
| `bored-block.cad` | **3D CSG** through-hole + **measure** (genus 1) + **section** | STL + SVG |
| `parametric-flange.cad` | **variables** drive a bored flange + bolt ring | SVG + DXF |
| `bolt-flange-assembly.cad` | **parts, assembly & BOM** (polar-arrayed bolts) | SVG |
| `constrained-plate.cad` | **constraint solver** — rough guesses → exact rectangle | SVG |
| `sketch-to-part.cad` | **constraint sketch → extrude** into a 3D solid | STL |
| `constrained-washer.cad` | **sketch circles** — concentric + radius constraints → a washer | SVG + DXF |

```sh
cad run examples/print-gear.cad
cad run examples/sketch-to-part.cad
```

Cut-out text needs a font you supply — see [Text](#text) above:
`text "OPEN" font=outline path=<your-font.ttf> h=40 role=cut`.

## Architecture

A Cargo workspace with two crates:

- **`cad-core`** — the engine (library). Modules: `math`/`bbox` (linear algebra
  & bounds), `geom2d`/`geom3d` (shapes, meshes, extrude, revolve, triangulation),
  `skills` (the parametric library), `export`, `render`, `ai`, and `script` (the
  command language + `Interpreter`).
- **`cad-cli`** — the `cad` binary: REPL, script runner, `ai`, `new` scaffolder.

See [`docs/DESIGN.md`](docs/DESIGN.md) for the design principles and data model,
and [`docs/ROADMAP.md`](docs/ROADMAP.md) for the phased plan (CSG, text
engraving, drafting, an expanded skill library, an AI copilot loop, and more),
each phase with its own design and checkpoint.

## Status

**v1.5 — 15 shipped milestones, all tested end to end.** v1.0 completed the
original eight-phase roadmap (robust 2D geometry, text + layered cut/score/
engrave export, 3D mesh CSG + shell, drafting & rendering, a 20-skill parametric
library, the AI copilot loop, interop, and the `plugin` surface + property tests
+ CI + packaging). Since then: parametric **variables & expressions** (v1.1),
2D **offset/inset** (v1.2), **measurement & analysis** — mass properties,
interference, cross-section (v1.2), **assemblies & BOM** (v1.2), **3D edge
fillet/chamfer** (v1.3), TrueType **outline/stencil text** (v1.4), and the **2D
constraint sketch solver** (v1.5). See [`docs/ROADMAP.md`](docs/ROADMAP.md) and
[`CHANGELOG.md`](CHANGELOG.md).

Still zero-dependency, `std`-only. Candidate next steps: arcs/circles + tangent/
radius constraints in the sketcher, loft & sweep-along-path, PDF vector export,
and (relaxing std-only) a WASM/dylib plugin host + coverage-guided fuzzing.

## License

MIT.
