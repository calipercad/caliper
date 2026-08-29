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
- **Parametric "skills."** Gears, finger-jointed boxes, honeycomb panels,
  washers, standoffs — generators driven by real engineering math, not
  hand-drawn shapes. Extend the library with your own skills (see below).
- **AI assisted.** `ai "a 24-tooth gear with a 5mm bore, 6mm thick"` asks Claude
  to emit the exact commands, then runs them. Zero HTTP dependencies — it shells
  out to `curl`/`jq`.
- **Manufacturing-grade export.** SVG (real mm), DXF (AutoCAD), G-code, STL, OBJ.
- **Zero dependencies.** The entire kernel is `std`-only Rust.

## Install & build

Requires a recent stable Rust toolchain.

```sh
git clone <your-fork> cad-assist && cd cad-assist
cargo build --release
# the binary lands at target/release/cad
./target/release/cad            # start the interactive shell
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
| 2D | `rect w h [rx=] [name=]`, `circle r`, `polygon sides r`, `star pts ro ri`, `arc r a0 a1`, `path x1 y1 … [closed]` |
| 3D | `box w d h`, `cylinder r h`, `sphere r`, `cone r h` |
| Ops | `extrude h`, `revolve`, `translate x y z`, `rotate deg [axis=] [about=]`, `scale f` |
| Skills | `skill <name> …` (see `skills`) |
| Scene | `units`, `select`, `delete`, `clear`, `list`, `bbox` |
| I/O | `render [mode=2d\|3d]`, `export <fmt> <path>`, `save`, `load`, `ai <text>` |

## Skills — the parametric library

Run `skills` (or `cad skills`) to list them. Built-ins today:

| Skill | Makes |
|---|---|
| `gear` | Involute spur gear (2D profile or extruded solid) |
| `boxjoint` | Finger-jointed box panels laid out flat for laser/CNC |
| `honeycomb` | Plate perforated with a hexagonal grid |
| `washer` | Flat washer / annulus (2D ring or solid tube) |
| `standoff` | Cylindrical standoff/spacer with a through bore |

### Extending the library

Two ways to add skills — no recompile needed for the first:

1. **Script-skills (`.cad` templates).** Drop a parametric file in
   `~/.cad-assist/skills/<name>.cad` using `${param}` / `${param:default}`
   placeholders. It's instantly available as `skill <name> …`:

   ```
   # ~/.cad-assist/skills/coaster.cad
   rect ${size:90} ${size:90} rx=${corner:12} name=coaster
   ```
   ```
   cad> skill coaster size=100 corner=15
   ```

2. **Native Rust skills.** Implement the `Skill` trait in `cad-core` and
   register it in `SkillRegistry::with_builtins()`. See
   `crates/cad-core/src/skills/` for worked examples (`gear.rs` is the richest).

## Rendering

`render` rasterises the scene into Unicode braille (2×4 dots per glyph). 2D
shapes draw directly; 3D meshes draw as rotated wireframes with crease/feature
edges only (no triangle soup). Tune it: `render mode=3d yaw=35 pitch=25 w=100 h=40`.

## Export formats

| Format | For | Notes |
|---|---|---|
| `svg` | Cricut, Silhouette, laser, vinyl | real-mm sizing, one cut path per contour |
| `dxf` | AutoCAD, LibreCAD, Fusion | `LWPOLYLINE`, `AC1015`, unit-tagged |
| `gcode` | pen plotters, drag knives, CNC contour | configurable feeds/Z |
| `stl` | 3D printing, slicers | ASCII, computed normals |
| `obj` | mesh interchange | |

Omit `name=` to export **every** matching entity — handy for laser layouts
where each panel is its own cut path.

## AI setup

The `ai` command shells out to `curl` and `jq` (both must be on `PATH`) and
calls the Anthropic Messages API.

```sh
export ANTHROPIC_API_KEY=sk-ant-...     # required
export CAD_AI_MODEL=claude-sonnet-5     # optional; this is the default
```

The model is given the full language reference as a system prompt and asked to
return only runnable commands, which cad-assist then executes.

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

v0.1 — foundation complete: geometry kernel, scripting language, five skills,
five exporters, braille rendering, and the AI bridge, all tested end to end.
The known v1 limitation is that 3D `extrude` caps the outer contour and does not
yet cut holes through caps (true booleans/CSG are Phase 1/3 on the roadmap); 2D
output already renders holes correctly. Details and the plan forward are in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## License

MIT.
