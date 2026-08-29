# Changelog

All notable changes to cad-assist are recorded here. Versions map to roadmap
phases (see [`docs/ROADMAP.md`](docs/ROADMAP.md)).

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

[0.1.0]: #
