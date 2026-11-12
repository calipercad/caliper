# Extending Caliper

Caliper is extensible two ways, both in keeping with its std-only,
zero-dependency design. The library of parametric generators (`skills`) is meant
to grow far faster than the core.

## 1. Script-skills (no code)

A **script-skill** is a `.cad` file dropped in `~/.caliper/skills/`. Its name
becomes a `skill <name>` command, it shows up in `skills`, and it needs no
recompile — this is the plugin mechanism for the toolkit.

Inside the file, `${key}` / `${key:default}` placeholders are filled from the
`key=value` arguments passed to the skill (an unfilled placeholder with no
default is an error).

Manage them from the CLI:

```
cad plugin new coaster          # scaffold ~/.caliper/skills/coaster.cad
cad plugin add ./my-part.cad    # install an external script-skill
cad plugin list                 # built-ins + your script-skills
cad plugin remove coaster
```

Example `~/.caliper/skills/coaster.cad`:

```
# coaster — a rounded tile with an engraved monogram
rect ${size:90} ${size:90} rx=${radius:8} name=coaster role=cut
skill monogram text=${text:AB} size=${letters:40}
```

Run it:

```
skill coaster size=100 text=JD
render ; export svg coaster.svg
```

Because a skill invocation is recorded in history as a single line, `save`
round-trips a design that uses it, re-expanding the skill on replay.

## 2. Native skills (Rust)

For generators that need real computation (involute math, CSG, meshing),
implement the `Skill` trait in `crates/cad-core/src/skills/` and register it:

```rust
use crate::args::Args;
use crate::error::Result;
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

pub struct BracketSkill;
impl Skill for BracketSkill {
    fn name(&self) -> &'static str { "bracket" }
    fn summary(&self) -> &'static str { "L-bracket with mounting holes" }
    fn usage(&self) -> &'static str { "bracket [arm=40] [thickness=4] [bore=5] [name=]" }
    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let arm = args.f64_or("arm", 40.0)?;         // default every parameter
        // …build a Shape / Mesh via the geom2d / geom3d / csg building blocks…
        let name = args.name().unwrap_or_else(|| scene.auto_name("bracket"));
        scene.add(name.clone(), Geometry::Shape(/* … */ todo!()));
        Ok(format!("bracket '{name}' …"))
    }
}
```

Then add `r.register(Box::new(BracketSkill));` to `SkillRegistry::with_builtins`
(`crates/cad-core/src/skills/mod.rs`). Dispatch, the `skills` listing, history,
and `save`/replay all work automatically. Give every parameter a sensible
default so a bare `skill <name>` produces geometry (the `all_skills_run_at_defaults`
test invokes each that way).

The geometry palette (all in `cad_core`): `geom2d::Shape` (`rect`/`circle`/
`polygon`/`polyline`/`arc`, `add_hole`, `fillet`/`chamfer`, booleans),
`geom3d` (`extrude`/`revolve`, `Mesh` primitives, `csg::{union,difference,
intersection,shell}`), and `geom2d::text::stroke_text`. See
[SKILLS.md](SKILLS.md) for the built-in library.

## Beyond this (post-1.0)

A sandboxed WASM / dynamic-library plugin host would allow third-party native
skills without rebuilding the binary. It is intentionally **not** in v1.0: a
WASM runtime or `libloading` is a heavy external dependency at odds with the
zero-dependency kernel. Script-skills cover the no-recompile extension need
today.
