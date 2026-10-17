//! The cad-assist scripting language: a small, line-oriented command language
//! interpreted against a [`Scene`]. One command per line; `#` starts a comment.
//!
//! The same interpreter powers script files (`cad run file.cad`), the REPL, and
//! AI-generated designs (the model emits these exact commands).

use crate::args::Args;
use crate::error::{CadError, Result};
use crate::export;
use crate::geom2d::boolean::{boolean, BoolOp};
use crate::geom2d::text::{self, Align};
use crate::geom2d::Shape;
use crate::geom3d::{self, Mesh};
use crate::math::{rad, Mat4, Vec2, Vec3};
use crate::render::{self, RenderOptions};
use crate::scene::{Geometry, Scene, Units};
use crate::skills::SkillRegistry;
use std::path::PathBuf;

pub const LANGUAGE_REFERENCE: &str = include_str!("reference.txt");

/// Split a line into tokens, honouring double quotes and stripping `#`/`;`
/// comments. Returns an empty vec for blank/comment lines.
pub fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quote = !in_quote,
            '#' | ';' if !in_quote => break, // comment to end of line
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

pub struct Interpreter {
    pub scene: Scene,
    pub skills: SkillRegistry,
    pub user_skill_dir: Option<PathBuf>,
    /// Reproducible log of scene-modifying commands (for `save`).
    pub history: Vec<String>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            scene: Scene::new(),
            skills: SkillRegistry::with_builtins(),
            user_skill_dir: default_skill_dir(),
            history: Vec::new(),
        }
    }

    /// Execute a whole script, returning any accumulated display output.
    pub fn exec_script(&mut self, text: &str) -> Result<String> {
        let mut out = String::new();
        for (i, line) in text.lines().enumerate() {
            match self.exec_line(line) {
                Ok(msg) => {
                    if !msg.is_empty() {
                        out.push_str(&msg);
                        if !msg.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                }
                Err(e) => return Err(CadError::Parse(format!("line {}: {e}", i + 1))),
            }
        }
        Ok(out)
    }

    /// Execute one line. Returns user-facing output (may be empty).
    pub fn exec_line(&mut self, raw: &str) -> Result<String> {
        let tokens = tokenize(raw);
        if tokens.is_empty() {
            return Ok(String::new());
        }
        let cmd = tokens[0].as_str();
        let rest = &tokens[1..];
        let args = Args::from_tokens(rest);

        let result = match cmd {
            // ---- 2D primitives ----
            "rect" => self.create_rect(&args),
            "circle" => self.create_circle(&args),
            "polygon" => self.create_polygon(&args),
            "star" => self.create_star(&args),
            "arc" => self.create_arc(&args),
            "path" => self.create_path(&args),
            "text" => self.create_text(&args),
            // ---- 3D primitives ----
            "box" => self.create_box(&args),
            "cylinder" => self.create_cylinder(&args),
            "sphere" => self.create_sphere(&args),
            "cone" => self.create_cone(&args),
            // ---- operations ----
            "extrude" => self.op_extrude(&args),
            "revolve" => self.op_revolve(&args),
            "translate" => self.op_translate(&args),
            "rotate" => self.op_rotate(&args),
            "scale" => self.op_scale(&args),
            // ---- 2D boolean & edge ops (deterministic → recorded in history) ----
            "union" => self.op_bool(&args, BoolOp::Union),
            "subtract" => self.op_bool(&args, BoolOp::Difference),
            "intersect" => self.op_bool(&args, BoolOp::Intersection),
            "xor" => self.op_bool(&args, BoolOp::Xor),
            "fillet" => self.op_fillet(&args),
            "chamfer" => self.op_chamfer(&args),
            "shell" => self.op_shell(&args),
            // ---- layer / role attributes (recorded → round-trip through save) ----
            "layer" => self.op_layer(&args),
            "role" => self.op_role(&args),
            // ---- skills ----
            "skill" => return self.run_skill(rest, raw),
            // ---- scene / meta ----
            "units" => self.set_units(&args),
            "select" => self.select(&args),
            "delete" => self.delete(&args),
            "clear" => {
                self.scene.clear();
                Ok("scene cleared".into())
            }
            "list" => return Ok(self.list()),
            "bbox" => return Ok(self.bbox(&args)),
            "skills" => return Ok(self.list_skills()),
            "help" => return Ok(LANGUAGE_REFERENCE.to_string()),
            // ---- io / render (not recorded in history) ----
            "render" => return self.render(&args),
            "export" => return self.export(&args),
            "load" => return self.load(&args),
            "save" => return self.save(&args),
            "ai" => return self.ai(raw),
            other => Err(CadError::Parse(format!("unknown command '{other}'"))),
        };

        // Record scene-modifying commands for reproducible `save`.
        if result.is_ok() {
            self.history.push(raw.trim().to_string());
        }
        result
    }

    // ---- 2D creators -------------------------------------------------------

    fn name_or_auto(&mut self, args: &Args, prefix: &str) -> String {
        args.name().unwrap_or_else(|| self.scene.auto_name(prefix))
    }

    /// Apply optional `layer=` / `role=` arguments to a just-created entity.
    /// A no-op when neither is present.
    fn apply_attrs(&mut self, name: &str, args: &Args) {
        let layer = args.get_str("layer").map(str::to_string);
        let role = args.get_str("role").map(str::to_string);
        if layer.is_some() || role.is_some() {
            let _ = self.scene.set_attrs(Some(name), layer, role);
        }
    }

    /// `text <string> [h=10] [align=left|center|right] [layer=] [role=]` —
    /// single-stroke engraving/plotting text as a new entity of open contours.
    fn create_text(&mut self, args: &Args) -> Result<String> {
        let s = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("text needs a string".into()))?
            .to_string();
        let h = args.f64_or("h", 10.0)?;
        let align = match args.str_or("align", "left") {
            "center" => Align::Center,
            "right" => Align::Right,
            _ => Align::Left,
        };
        let shape = text::stroke_text(&s, h, align);
        let name = self.name_or_auto(args, "text");
        self.scene.add(name.clone(), Geometry::Shape(shape));
        self.apply_attrs(&name, args);
        Ok(format!("text '{name}' \"{s}\" h{h}"))
    }

    fn create_rect(&mut self, args: &Args) -> Result<String> {
        let w = args.pos_f64(0)?;
        let h = args.pos_f64(1)?;
        let rx = args.f64_or("rx", 0.0)?;
        let seg = args.usize_or("seg", 8)?;
        let shape = Shape::rect(w, h, rx, seg);
        let name = self.name_or_auto(args, "rect");
        self.scene.add(name.clone(), Geometry::Shape(shape));
        self.apply_attrs(&name, args);
        Ok(format!("rect '{name}' {w}×{h}"))
    }

    fn create_circle(&mut self, args: &Args) -> Result<String> {
        let r = args.pos_f64(0)?;
        let seg = args.usize_or("seg", 64)?;
        let name = self.name_or_auto(args, "circle");
        self.scene
            .add(name.clone(), Geometry::Shape(Shape::circle(r, seg)));
        self.apply_attrs(&name, args);
        Ok(format!("circle '{name}' r{r}"))
    }

    fn create_polygon(&mut self, args: &Args) -> Result<String> {
        let sides = args.pos_f64(0)? as usize;
        let r = args.pos_f64(1)?;
        let name = self.name_or_auto(args, "poly");
        self.scene
            .add(name.clone(), Geometry::Shape(Shape::polygon(sides, r)));
        self.apply_attrs(&name, args);
        Ok(format!("polygon '{name}' {sides}-gon r{r}"))
    }

    fn create_star(&mut self, args: &Args) -> Result<String> {
        let points = args.pos_f64(0)? as usize;
        let ro = args.pos_f64(1)?;
        let ri = args.pos_f64(2)?;
        let name = self.name_or_auto(args, "star");
        self.scene
            .add(name.clone(), Geometry::Shape(Shape::star(points, ro, ri)));
        self.apply_attrs(&name, args);
        Ok(format!("star '{name}' {points}pt"))
    }

    fn create_arc(&mut self, args: &Args) -> Result<String> {
        let r = args.pos_f64(0)?;
        let a0 = args.pos_f64(1)?;
        let a1 = args.pos_f64(2)?;
        let seg = args.usize_or("seg", 48)?;
        let name = self.name_or_auto(args, "arc");
        self.scene
            .add(name.clone(), Geometry::Shape(Shape::arc(r, a0, a1, seg)));
        self.apply_attrs(&name, args);
        Ok(format!("arc '{name}' r{r} {a0}°→{a1}°"))
    }

    fn create_path(&mut self, args: &Args) -> Result<String> {
        // Coordinates are the leading run of numeric positionals; a trailing
        // bare word (e.g. `closed`) is a flag, not a coordinate.
        let mut coords = Vec::new();
        for t in &args.positional {
            match t.parse::<f64>() {
                Ok(n) => coords.push(n),
                Err(_) => break,
            }
        }
        if coords.len() < 4 || coords.len() % 2 != 0 {
            return Err(CadError::Arg(
                "path needs an even list of coords: path x1 y1 x2 y2 ... [closed]".into(),
            ));
        }
        let mut pts = Vec::new();
        let mut i = 0;
        while i < coords.len() {
            pts.push(Vec2::new(coords[i], coords[i + 1]));
            i += 2;
        }
        let closed = args.flag("closed");
        let name = self.name_or_auto(args, "path");
        self.scene
            .add(name.clone(), Geometry::Shape(Shape::polyline(pts, closed)));
        self.apply_attrs(&name, args);
        Ok(format!("path '{name}' ({} pts)", args.positional.len() / 2))
    }

    // ---- 3D creators -------------------------------------------------------

    fn create_box(&mut self, args: &Args) -> Result<String> {
        let w = args.pos_f64(0)?;
        let d = args.pos_f64(1)?;
        let h = args.pos_f64(2)?;
        let name = self.name_or_auto(args, "box");
        self.scene
            .add(name.clone(), Geometry::Mesh(Mesh::cuboid(w, d, h)));
        Ok(format!("box '{name}' {w}×{d}×{h}"))
    }

    fn create_cylinder(&mut self, args: &Args) -> Result<String> {
        let r = args.pos_f64(0)?;
        let h = args.pos_f64(1)?;
        let seg = args.usize_or("seg", 48)?;
        let name = self.name_or_auto(args, "cyl");
        self.scene
            .add(name.clone(), Geometry::Mesh(Mesh::cylinder(r, h, seg)));
        Ok(format!("cylinder '{name}' r{r} h{h}"))
    }

    fn create_sphere(&mut self, args: &Args) -> Result<String> {
        let r = args.pos_f64(0)?;
        let seg = args.usize_or("seg", 32)?;
        let name = self.name_or_auto(args, "sphere");
        self.scene
            .add(name.clone(), Geometry::Mesh(Mesh::sphere(r, seg)));
        Ok(format!("sphere '{name}' r{r}"))
    }

    fn create_cone(&mut self, args: &Args) -> Result<String> {
        let r = args.pos_f64(0)?;
        let h = args.pos_f64(1)?;
        let seg = args.usize_or("seg", 48)?;
        let name = self.name_or_auto(args, "cone");
        self.scene
            .add(name.clone(), Geometry::Mesh(Mesh::cone(r, h, seg)));
        Ok(format!("cone '{name}' r{r} h{h}"))
    }

    // ---- operations --------------------------------------------------------

    fn op_extrude(&mut self, args: &Args) -> Result<String> {
        let height = args.pos_f64(0)?;
        let target = args.get_str("target").map(|s| s.to_string());
        let (src_name, shape) = {
            let e = self.scene.resolve(target.as_deref())?;
            match &e.geom {
                Geometry::Shape(s) => (e.name.clone(), s.clone()),
                Geometry::Mesh(_) => return Err(CadError::Arg("extrude needs a 2D shape".into())),
            }
        };
        let mesh = geom3d::extrude(&shape, height)?;
        let name = args.name().unwrap_or_else(|| format!("{src_name}_3d"));
        self.scene.add(name.clone(), Geometry::Mesh(mesh));
        Ok(format!("extruded '{src_name}' → '{name}' ({height}mm)"))
    }

    fn op_revolve(&mut self, args: &Args) -> Result<String> {
        let seg = args.usize_or("seg", 48)?;
        let target = args.get_str("target").map(|s| s.to_string());
        let (src_name, shape) = {
            let e = self.scene.resolve(target.as_deref())?;
            match &e.geom {
                Geometry::Shape(s) => (e.name.clone(), s.clone()),
                Geometry::Mesh(_) => {
                    return Err(CadError::Arg("revolve needs a 2D profile".into()))
                }
            }
        };
        let profile = &shape
            .contours
            .first()
            .ok_or_else(|| CadError::Geometry("revolve target has no contour".into()))?;
        let mesh = geom3d::revolve(&profile.points, seg)?;
        let name = args.name().unwrap_or_else(|| format!("{src_name}_rev"));
        self.scene.add(name.clone(), Geometry::Mesh(mesh));
        Ok(format!("revolved '{src_name}' → '{name}'"))
    }

    fn op_translate(&mut self, args: &Args) -> Result<String> {
        let x = args.pos_f64(0)?;
        let y = args.pos_f64_or(1, 0.0)?;
        let z = args.pos_f64_or(2, 0.0)?;
        let target = args.get_str("target").map(|s| s.to_string());
        let e = self.scene.resolve_mut(target.as_deref())?;
        let name = e.name.clone();
        e.geom.transform(&Mat4::translation(Vec3::new(x, y, z)));
        Ok(format!("translated '{name}' by ({x}, {y}, {z})"))
    }

    fn op_rotate(&mut self, args: &Args) -> Result<String> {
        let deg = args.pos_f64(0)?;
        let axis = args.str_or("axis", "z").to_string();
        let about = args.str_or("about", "center").to_string();
        let target = args.get_str("target").map(|s| s.to_string());

        let e = self.scene.resolve_mut(target.as_deref())?;
        let name = e.name.clone();
        let pivot = if about == "origin" {
            Vec3::ZERO
        } else {
            match &e.geom {
                Geometry::Shape(s) => s.bbox().center().to3(),
                Geometry::Mesh(m) => m.bbox().center(),
            }
        };
        let rot = match axis.as_str() {
            "x" => Mat4::rotation_x(rad(deg)),
            "y" => Mat4::rotation_y(rad(deg)),
            _ => Mat4::rotation_z(rad(deg)),
        };
        let m = Mat4::translation(pivot)
            .mul(&rot)
            .mul(&Mat4::translation(-pivot));
        e.geom.transform(&m);
        Ok(format!("rotated '{name}' {deg}° about {axis} ({about})"))
    }

    fn op_scale(&mut self, args: &Args) -> Result<String> {
        let s = args.pos_f64(0)?;
        let about = args.str_or("about", "center").to_string();
        let target = args.get_str("target").map(|s| s.to_string());
        let e = self.scene.resolve_mut(target.as_deref())?;
        let name = e.name.clone();
        let pivot = if about == "origin" {
            Vec3::ZERO
        } else {
            match &e.geom {
                Geometry::Shape(sh) => sh.bbox().center().to3(),
                Geometry::Mesh(m) => m.bbox().center(),
            }
        };
        let m = Mat4::translation(pivot)
            .mul(&Mat4::scale(Vec3::new(s, s, s)))
            .mul(&Mat4::translation(-pivot));
        e.geom.transform(&m);
        Ok(format!("scaled '{name}' ×{s} ({about})"))
    }

    // ---- 2D boolean & edge operations --------------------------------------

    /// Clone out the [`Shape`] geometry of a named entity, ending the borrow so
    /// the caller can mutate the scene afterwards. Missing entities and meshes
    /// both surface as the same operand error.
    fn clone_shape(&self, name: &str) -> Result<Shape> {
        let entity = self
            .scene
            .get(name)
            .ok_or_else(|| CadError::Arg("boolean needs two 2D shapes".into()))?;
        match &entity.geom {
            Geometry::Shape(s) => Ok(s.clone()),
            Geometry::Mesh(_) => Err(CadError::Arg("boolean needs two 2D shapes".into())),
        }
    }

    /// Clone out the [`Mesh`] geometry of a named entity (the 3D counterpart of
    /// [`Self::clone_shape`]).
    fn clone_mesh(&self, name: &str) -> Result<Mesh> {
        let entity = self
            .scene
            .get(name)
            .ok_or_else(|| CadError::Arg("boolean needs two 3D meshes".into()))?;
        match &entity.geom {
            Geometry::Mesh(m) => Ok(m.clone()),
            Geometry::Shape(_) => Err(CadError::Arg("boolean needs two 3D meshes".into())),
        }
    }

    /// `union|subtract|intersect|xor <a> <b> [name=]` — combine two operands into
    /// a NEW entity, dispatching by kind: two 2D shapes run the exact-integer
    /// polygon clipper, two 3D meshes run BSP CSG (`xor` is 2D-only). Operands
    /// are left untouched (non-destructive).
    fn op_bool(&mut self, args: &Args, op: BoolOp) -> Result<String> {
        let a_name = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("boolean needs two operands".into()))?
            .to_string();
        let b_name = args
            .pos_str(1)
            .ok_or_else(|| CadError::Arg("boolean needs two operands".into()))?
            .to_string();

        // Dispatch on operand kinds (borrows end before we mutate the scene).
        let ka = self
            .scene
            .get(&a_name)
            .ok_or_else(|| CadError::NotFound(a_name.clone()))?
            .geom
            .kind();
        let kb = self
            .scene
            .get(&b_name)
            .ok_or_else(|| CadError::NotFound(b_name.clone()))?
            .geom
            .kind();

        let name = args.name().unwrap_or_else(|| {
            if op == BoolOp::Difference {
                format!("{a_name}_sub_{b_name}")
            } else {
                self.scene.auto_name("bool")
            }
        });

        match (ka, kb) {
            ("2d", "2d") => {
                let a_shape = self.clone_shape(&a_name)?;
                let b_shape = self.clone_shape(&b_name)?;
                let result = boolean(&a_shape, &b_shape, op)?;
                if result.is_empty() {
                    return Err(CadError::Geometry("empty result".into()));
                }
                self.scene.add(name.clone(), Geometry::Shape(result));
            }
            ("3d", "3d") => {
                let a_mesh = self.clone_mesh(&a_name)?;
                let b_mesh = self.clone_mesh(&b_name)?;
                let result = match op {
                    BoolOp::Union => geom3d::csg::union(&a_mesh, &b_mesh)?,
                    BoolOp::Difference => geom3d::csg::difference(&a_mesh, &b_mesh)?,
                    BoolOp::Intersection => geom3d::csg::intersection(&a_mesh, &b_mesh)?,
                    BoolOp::Xor => {
                        return Err(CadError::Arg("xor is only defined for 2D shapes".into()))
                    }
                };
                if result.tris.is_empty() {
                    return Err(CadError::Geometry("empty result".into()));
                }
                self.scene.add(name.clone(), Geometry::Mesh(result));
            }
            _ => {
                return Err(CadError::Arg(
                    "cannot combine a 2D shape with a 3D mesh".into(),
                ))
            }
        }

        let verb = match op {
            BoolOp::Union => "union",
            BoolOp::Difference => "subtract",
            BoolOp::Intersection => "intersect",
            BoolOp::Xor => "xor",
        };
        Ok(format!("{verb} '{a_name}','{b_name}' → '{name}'"))
    }

    fn op_fillet(&mut self, args: &Args) -> Result<String> {
        let r = args.pos_f64(0)?;
        let seg = args.usize_or("seg", 16)?;
        let target = args.get_str("target").map(|s| s.to_string());
        let e = self.scene.resolve_mut(target.as_deref())?;
        let name = e.name.clone();
        match &mut e.geom {
            Geometry::Shape(s) => *s = s.fillet(r, seg),
            Geometry::Mesh(_) => return Err(CadError::Arg("fillet needs a 2D shape".into())),
        }
        Ok(format!("filleted '{name}' (r{r}, {seg} seg)"))
    }

    fn op_chamfer(&mut self, args: &Args) -> Result<String> {
        let d = args.pos_f64(0)?;
        let target = args.get_str("target").map(|s| s.to_string());
        let e = self.scene.resolve_mut(target.as_deref())?;
        let name = e.name.clone();
        match &mut e.geom {
            Geometry::Shape(s) => *s = s.chamfer(d),
            Geometry::Mesh(_) => return Err(CadError::Arg("chamfer needs a 2D shape".into())),
        }
        Ok(format!("chamfered '{name}' (d{d})"))
    }

    /// `shell <t> [target=]` — hollow a solid mesh to wall thickness `t`
    /// (convex/prismatic solids). Replaces the target mesh in place.
    fn op_shell(&mut self, args: &Args) -> Result<String> {
        let t = args.pos_f64(0)?;
        let target = args.get_str("target").map(|s| s.to_string());
        let e = self.scene.resolve_mut(target.as_deref())?;
        let name = e.name.clone();
        match &mut e.geom {
            Geometry::Mesh(m) => *m = geom3d::csg::shell(m, t)?,
            Geometry::Shape(_) => return Err(CadError::Arg("shell needs a 3D mesh".into())),
        }
        Ok(format!("shelled '{name}' (t{t})"))
    }

    /// `layer <name> [target=]` — set the export layer of an entity (defaults to
    /// the current selection). Recorded in history so it round-trips via `save`.
    fn op_layer(&mut self, args: &Args) -> Result<String> {
        let layer = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("layer needs a name: layer <name> [target=]".into()))?
            .to_string();
        let target = args.get_str("target").map(|s| s.to_string());
        let name = self
            .scene
            .set_attrs(target.as_deref(), Some(layer.clone()), None)?;
        Ok(format!("'{name}' layer={layer}"))
    }

    /// `role <name> [target=]` — set the fabrication role (`cut`/`score`/
    /// `engrave`/…) of an entity, which drives export layer color.
    fn op_role(&mut self, args: &Args) -> Result<String> {
        let role = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("role needs a name: role <name> [target=]".into()))?
            .to_string();
        let target = args.get_str("target").map(|s| s.to_string());
        let name = self
            .scene
            .set_attrs(target.as_deref(), None, Some(role.clone()))?;
        Ok(format!("'{name}' role={role}"))
    }

    // ---- skills ------------------------------------------------------------

    fn run_skill(&mut self, rest: &[String], raw: &str) -> Result<String> {
        let name = rest
            .first()
            .ok_or_else(|| CadError::Arg("usage: skill <name> [args...]".into()))?
            .clone();
        let args = Args::from_tokens(&rest[1..]);

        // 1) built-in Rust skill
        if let Some(skill) = self.skills.get(&name) {
            let msg = skill.run(&args, &mut self.scene)?;
            self.history.push(raw.trim().to_string());
            return Ok(msg);
        }
        // 2) user script-skill file: <skill_dir>/<name>.cad with ${param} slots
        if let Some(msg) = self.run_script_skill(&name, &args)? {
            self.history.push(raw.trim().to_string());
            return Ok(msg);
        }
        Err(CadError::NotFound(format!(
            "skill '{name}' (try `skills` to list)"
        )))
    }

    /// Run a parametric `.cad` file from the user skill directory. `${key}` and
    /// `${key:default}` placeholders are substituted from the skill args.
    fn run_script_skill(&mut self, name: &str, args: &Args) -> Result<Option<String>> {
        let dir = match &self.user_skill_dir {
            Some(d) => d.clone(),
            None => return Ok(None),
        };
        let path = dir.join(format!("{name}.cad"));
        if !path.exists() {
            return Ok(None);
        }
        let template = std::fs::read_to_string(&path)?;
        let expanded = substitute(&template, args)?;
        // The skill's expanded sub-commands must NOT be recorded individually:
        // the caller (`run_skill`) records the single `skill <name> ...` line,
        // which is deterministic and re-expands on replay. Roll back anything
        // the expansion pushed so `save` stays compact and idempotent.
        let mark = self.history.len();
        let out = self.exec_script(&expanded)?;
        self.history.truncate(mark);
        Ok(Some(if out.is_empty() {
            format!("ran script-skill '{name}'")
        } else {
            out
        }))
    }

    // ---- scene / meta ------------------------------------------------------

    fn set_units(&mut self, args: &Args) -> Result<String> {
        let u = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: units mm|cm|in".into()))?;
        let units = Units::parse(u).ok_or_else(|| CadError::Arg(format!("unknown unit '{u}'")))?;
        self.scene.units = units;
        Ok(format!("units set to {}", units.suffix()))
    }

    fn select(&mut self, args: &Args) -> Result<String> {
        let name = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: select <name>".into()))?;
        if self.scene.get(name).is_none() {
            return Err(CadError::NotFound(name.into()));
        }
        self.scene.selected = Some(name.to_string());
        Ok(format!("selected '{name}'"))
    }

    fn delete(&mut self, args: &Args) -> Result<String> {
        let name = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: delete <name>".into()))?;
        self.scene.delete(name)?;
        Ok(format!("deleted '{name}'"))
    }

    fn list(&self) -> String {
        if self.scene.entities.is_empty() {
            return "(scene is empty)\n".into();
        }
        let mut out = format!(
            "{} entit{}:\n",
            self.scene.entities.len(),
            if self.scene.entities.len() == 1 {
                "y"
            } else {
                "ies"
            }
        );
        let sel = self.scene.selected.clone();
        for e in &self.scene.entities {
            let marker = if sel.as_deref() == Some(e.name.as_str()) {
                "*"
            } else {
                " "
            };
            let dims = match &e.geom {
                Geometry::Shape(s) => {
                    let b = s.bbox();
                    format!(
                        "2D  {:.1}×{:.1}{}",
                        b.width(),
                        b.height(),
                        self.scene.units.suffix()
                    )
                }
                Geometry::Mesh(m) => {
                    let sz = m.bbox().size();
                    format!(
                        "3D  {:.1}×{:.1}×{:.1}{}  ({} tris)",
                        sz.x,
                        sz.y,
                        sz.z,
                        self.scene.units.suffix(),
                        m.triangle_count()
                    )
                }
            };
            out.push_str(&format!("  {marker} {:<16} {dims}\n", e.name));
        }
        out
    }

    fn bbox(&self, args: &Args) -> String {
        let target = args.pos_str(0);
        match self.scene.resolve(target) {
            Ok(e) => match &e.geom {
                Geometry::Shape(s) => {
                    let b = s.bbox();
                    format!(
                        "'{}' bbox: ({:.2},{:.2}) → ({:.2},{:.2}), size {:.2}×{:.2}{}\n",
                        e.name,
                        b.min.x,
                        b.min.y,
                        b.max.x,
                        b.max.y,
                        b.width(),
                        b.height(),
                        self.scene.units.suffix()
                    )
                }
                Geometry::Mesh(m) => {
                    let b = m.bbox();
                    let s = b.size();
                    format!(
                        "'{}' bbox size {:.2}×{:.2}×{:.2}{}\n",
                        e.name,
                        s.x,
                        s.y,
                        s.z,
                        self.scene.units.suffix()
                    )
                }
            },
            Err(e) => format!("{e}\n"),
        }
    }

    fn list_skills(&self) -> String {
        let mut out = String::from("Built-in skills (invoke with `skill <name> ...`):\n");
        for s in self.skills.iter() {
            out.push_str(&format!(
                "  {:<12} {}\n      {}\n",
                s.name(),
                s.summary(),
                s.usage()
            ));
        }
        if let Some(dir) = &self.user_skill_dir {
            out.push_str(&format!(
                "\nUser script-skills directory: {}\n",
                dir.display()
            ));
            if let Ok(rd) = std::fs::read_dir(dir) {
                let mut any = false;
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("cad") {
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            out.push_str(&format!("  {stem} (script-skill)\n"));
                            any = true;
                        }
                    }
                }
                if !any {
                    out.push_str("  (none yet — drop a parametric .cad file here)\n");
                }
            }
        }
        out
    }

    // ---- render / export / io ---------------------------------------------

    fn render(&mut self, args: &Args) -> Result<String> {
        let target = args.pos_str(0).map(|s| s.to_string());
        let opt = RenderOptions {
            cols: args.usize_or("w", 80)?,
            rows: args.usize_or("h", 30)?,
            yaw_deg: args.f64_or("yaw", 35.0)?,
            pitch_deg: args.f64_or("pitch", 25.0)?,
        };
        // Choose mode: explicit, else by target kind, else prefer 3D if present.
        let mode = args.get_str("mode").map(|s| s.to_string());
        let mode = mode.unwrap_or_else(|| {
            if let Some(t) = target.as_deref() {
                match self.scene.get(t).map(|e| e.geom.kind()) {
                    Some("3d") => "3d".into(),
                    _ => "2d".into(),
                }
            } else if !self.scene.bbox3().is_empty() {
                "3d".into()
            } else {
                "2d".into()
            }
        });
        let art = if mode == "3d" {
            render::render_3d(&self.scene, target.as_deref(), &opt)
        } else {
            render::render_2d(&self.scene, target.as_deref(), &opt)
        };
        Ok(art)
    }

    fn export(&mut self, args: &Args) -> Result<String> {
        let format = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: export <format> <path> [name=..]".into()))?
            .to_string();
        let path = args
            .pos_str(1)
            .ok_or_else(|| CadError::Arg("export needs an output path".into()))?
            .to_string();
        let target = args.get_str("name");
        export::export(&self.scene, &format, &path, target)
    }

    fn load(&mut self, args: &Args) -> Result<String> {
        let path = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: load <file.cad>".into()))?;
        let text = std::fs::read_to_string(path)?;
        let out = self.exec_script(&text)?;
        Ok(format!("loaded {path}\n{out}"))
    }

    fn save(&mut self, args: &Args) -> Result<String> {
        let path = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: save <file.cad>".into()))?;
        let mut text = String::from("# cad-assist script\n");
        // Pin the document's current units once at the top; skip any recorded
        // `units` commands below so we never emit a duplicate or stale unit.
        text.push_str(&format!("units {}\n", self.scene.units.suffix()));
        let mut written = 0usize;
        for line in &self.history {
            if tokenize(line).first().map(String::as_str) == Some("units") {
                continue;
            }
            text.push_str(line);
            text.push('\n');
            written += 1;
        }
        std::fs::write(path, text)?;
        Ok(format!("saved {written} commands to {path}"))
    }

    fn ai(&mut self, raw: &str) -> Result<String> {
        // Everything after the leading `ai` token is the prompt.
        let prompt = raw.trim().strip_prefix("ai").unwrap_or("").trim();
        let prompt = prompt.trim_matches('"').trim();
        if prompt.is_empty() {
            return Err(CadError::Arg("usage: ai <describe what to make>".into()));
        }
        let script = crate::ai::generate(prompt)?;
        let mut out = String::from("── AI generated script ──\n");
        for line in script.lines() {
            out.push_str(&format!("  {line}\n"));
        }
        out.push_str("─────────────────────────\n");
        let run_out = self.exec_script(&script)?;
        out.push_str(&run_out);
        Ok(out)
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Interpreter::new()
    }
}

/// Substitute `${key}` / `${key:default}` placeholders in a script-skill body.
fn substitute(template: &str, args: &Args) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(pos) = rest.find("${") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| CadError::Parse("unterminated ${...} in script-skill".into()))?;
        let expr = &after[..end];
        let (key, default) = match expr.split_once(':') {
            Some((k, d)) => (k, Some(d)),
            None => (expr, None),
        };
        let val = args
            .get_str(key)
            .map(|s| s.to_string())
            .or_else(|| default.map(|d| d.to_string()))
            .ok_or_else(|| CadError::Arg(format!("script-skill needs `{key}=`")))?;
        out.push_str(&val);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Default user skill directory: `~/.cad-assist/skills`.
fn default_skill_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".cad-assist").join("skills"))
}
