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
  BSP mesh CSG (`subtract`/`union`/`intersect`), fillet/chamfer, shell,
  single-stroke text, arrays/mirror/groups, and dimensioning — with manifold
  verification on 3D output.
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
| 2D | `rect w h [rx=] [name=]`, `circle r`, `polygon sides r`, `star pts ro ri`, `arc r a0 a1`, `path x1 y1 … [closed]`, `text "…" [h=]` |
| 3D | `box w d h`, `cylinder r h`, `sphere r`, `cone r h` |
| Ops | `extrude h`, `revolve`, `translate`, `rotate`, `scale`, `array`, `mirror`, `group`/`ungroup`, `fillet`, `chamfer`, `shell`, `dim` |
| Booleans | `union a b`, `subtract a b`, `intersect a b`, `xor a b` (2D shapes **or** 3D meshes, auto-dispatched) |
| Skills | `skill <name> …` (see `skills`, [SKILLS.md](docs/SKILLS.md)) |
| Scene | `units`, `select`, `delete`, `clear`, `list`, `bbox`, `undo`, `redo` |
| I/O | `render [mode=wire\|hlr\|shade] [view=] [layout=views]`, `export <fmt> <path>`, `import <file>`, `save`, `load`, `ai <text>` |

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
| `print-gear.cad` | involute gear, bored, extruded | STL |
| `lathe-knob.cad` | solid of revolution from a profile | STL |
| `honeycomb-panel.cad` | ventilated hex grille | SVG + DXF |

```sh
cad run examples/print-gear.cad
```

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

**v1.0 — the roadmap is complete.** Eight shipped milestones: robust 2D
geometry (exact-integer booleans, holed extrusion, fillets), text + layered
cut/score/engrave export, 3D mesh CSG + shell, drafting & rendering (HLR,
shaded, multi-view, arrays, dimensions), a 20-skill parametric library, the AI
copilot loop, interop (import, undo, watch, richer export), and this release
(the `plugin` surface, randomized property tests, CI, packaging). All tested
end to end — see [`docs/ROADMAP.md`](docs/ROADMAP.md) and
[`CHANGELOG.md`](CHANGELOG.md).

Post-1.0 ideas (held back to keep the kernel zero-dependency): a WASM/dylib
plugin host, coverage-guided fuzzing, and a generated docs site.

## License

MIT.
