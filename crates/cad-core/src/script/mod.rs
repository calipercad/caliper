//! The cad-assist scripting language: a small, line-oriented command language
//! interpreted against a [`Scene`]. One command per line; `#` starts a comment.
//!
//! The same interpreter powers script files (`cad run file.cad`), the REPL, and
//! AI-generated designs (the model emits these exact commands).

use crate::args::Args;
use crate::error::{CadError, Result};
use crate::export;
use crate::geom2d::boolean::{boolean, BoolOp};
use crate::geom2d::offset;
use crate::geom2d::text::{self, Align};
use crate::geom2d::Shape;
use crate::geom3d::{self, Mesh};
use crate::math::{rad, Mat4, Vec2, Vec3};
use crate::render::{self, RenderMode, RenderOptions};
use crate::scene::{Geometry, Scene, Units};
use crate::skills::SkillRegistry;
use std::collections::HashMap;
use std::path::PathBuf;

pub mod expr;

pub const LANGUAGE_REFERENCE: &str = include_str!("reference.txt");

/// Split a line into tokens, honouring double quotes and stripping `#`/`;`
/// comments. Returns an empty vec for blank/comment lines.
pub fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in line.chars() {
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
    /// Named numeric variables for parametric expressions (`set w 40`, `$w`).
    pub vars: HashMap<String, f64>,
    /// Context of the last AI copilot run that did not converge: `(request,
    /// note)`, consumed by `ai fix`.
    pub last_ai: Option<(String, String)>,
    /// Undo/redo snapshots of `(scene, history, vars)` taken before each
    /// scene-modifying command.
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
}

/// A point-in-time snapshot restored by `undo`/`redo`: the scene, the history
/// log, and the variable store move together so a parametric edit is reversible.
type Snapshot = (Scene, Vec<String>, HashMap<String, f64>);

/// Maximum undo depth kept in memory.
const MAX_UNDO: usize = 100;

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            scene: Scene::new(),
            skills: SkillRegistry::with_builtins(),
            user_skill_dir: default_skill_dir(),
            history: Vec::new(),
            vars: HashMap::new(),
            last_ai: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Push a pre-command snapshot onto the undo stack, bounding its depth and
    /// invalidating the redo stack (a new edit forks history).
    fn push_undo(&mut self, snap: Snapshot) {
        self.undo_stack.push(snap);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// The directory user script-skills (`*.cad` plugins) live in, if known.
    pub fn skill_dir(&self) -> Option<&std::path::Path> {
        self.user_skill_dir.as_deref()
    }

    /// Run `~/.cad-assist/config.cad` if present (units, defaults). Best-effort;
    /// errors are ignored and the config is not left on the undo stack.
    pub fn load_config(&mut self) -> Option<String> {
        let path = default_config_path()?;
        let text = std::fs::read_to_string(&path).ok()?;
        let out = self.exec_script(&text).ok();
        self.undo_stack.clear();
        self.redo_stack.clear();
        out
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

        // Variable commands own their raw tokens: the variable *name* is a
        // literal (never expanded) and the value is evaluated against `vars`.
        match cmd {
            "set" => return self.op_set(&tokens, raw),
            "unset" => return self.op_unset(&tokens, raw),
            "vars" => return Ok(self.list_vars()),
            _ => {}
        }

        // Expand `$var` / `${expr}` in every argument before parsing. Only the
        // value side of `key=value` is touched; tokens without `$` pass through
        // unchanged, so non-parametric scripts behave exactly as before.
        let expanded = expand_tokens(&tokens[1..], &self.vars)?;
        let rest = &expanded[..];
        let args = Args::from_tokens(rest);

        // Snapshot state before a scene-modifying command, for undo.
        let snap = if is_modifying(cmd) {
            Some((self.scene.clone(), self.history.clone(), self.vars.clone()))
        } else {
            None
        };

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
            "array" => self.op_array(&args),
            "mirror" => self.op_mirror(&args),
            "group" => self.op_group(&args),
            "ungroup" => self.op_ungroup(&args),
            "dim" => self.op_dim(&args),
            // ---- 2D boolean & edge ops (deterministic → recorded in history) ----
            "union" => self.op_bool(&args, BoolOp::Union),
            "subtract" => self.op_bool(&args, BoolOp::Difference),
            "intersect" => self.op_bool(&args, BoolOp::Intersection),
            "xor" => self.op_bool(&args, BoolOp::Xor),
            "fillet" => self.op_fillet(&args),
            "chamfer" => self.op_chamfer(&args),
            "offset" => self.op_offset(&args, false),
            "inset" => self.op_offset(&args, true),
            "shell" => self.op_shell(&args),
            // ---- layer / role attributes (recorded → round-trip through save) ----
            "layer" => self.op_layer(&args),
            "role" => self.op_role(&args),
            "part" => self.op_part(&args),
            "assembly" => self.op_assembly(&args),
            "bom" => return Ok(self.bom(&args)),
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
            "import" => self.op_import(&args),
            "list" => return Ok(self.list()),
            "bbox" => return Ok(self.bbox(&args)),
            "measure" => return Ok(self.measure(&args)),
            "interfere" => return Ok(self.interfere(&args)),
            "section" => self.op_section(&args),
            "skills" => return Ok(self.list_skills()),
            "help" => return Ok(LANGUAGE_REFERENCE.to_string()),
            "undo" => return self.op_undo(),
            "redo" => return self.op_redo(),
            // ---- io / render (not recorded in history) ----
            "render" => return self.render(&args),
            "export" => return self.export(&args),
            "load" => return self.load(&args),
            "save" => return self.save(&args),
            "ai" => return self.ai(raw),
            other => Err(CadError::Parse(format!("unknown command '{other}'"))),
        };

        // Record scene-modifying commands for reproducible `save`, and keep an
        // undo snapshot of the state from before the command.
        if result.is_ok() {
            if let Some(s) = snap {
                self.push_undo(s);
            }
            self.history.push(raw.trim().to_string());
        }
        result
    }

    /// `undo` — restore the scene + history + vars to before the last command.
    fn op_undo(&mut self) -> Result<String> {
        match self.undo_stack.pop() {
            Some((scene, history, vars)) => {
                self.redo_stack.push((
                    std::mem::replace(&mut self.scene, scene),
                    std::mem::take(&mut self.history),
                    std::mem::replace(&mut self.vars, vars),
                ));
                self.history = history;
                Ok("undid last change".into())
            }
            None => Err(CadError::Arg("nothing to undo".into())),
        }
    }

    /// `redo` — reapply the change most recently undone.
    fn op_redo(&mut self) -> Result<String> {
        match self.redo_stack.pop() {
            Some((scene, history, vars)) => {
                self.undo_stack.push((
                    std::mem::replace(&mut self.scene, scene),
                    std::mem::take(&mut self.history),
                    std::mem::replace(&mut self.vars, vars),
                ));
                self.history = history;
                Ok("redid change".into())
            }
            None => Err(CadError::Arg("nothing to redo".into())),
        }
    }

    // ---- variables ---------------------------------------------------------

    /// `set <name> <expr>` — define/update a numeric variable. The value is an
    /// expression evaluated against the current variables, so `set d $w/2`
    /// works. Recorded in history (ordered before its uses) and snapshotted for
    /// undo, so `save` round-trips the parametric definition.
    fn op_set(&mut self, tokens: &[String], raw: &str) -> Result<String> {
        let name = tokens
            .get(1)
            .ok_or_else(|| CadError::Arg("usage: set <name> <expr>".into()))?;
        if !is_var_name(name) {
            return Err(CadError::Arg(format!(
                "'{name}' is not a valid variable name (letters, digits, `_`; not starting with a digit)"
            )));
        }
        if tokens.len() < 3 {
            return Err(CadError::Arg(format!(
                "set {name} needs a value: set {name} <expr>"
            )));
        }
        let src = tokens[2..].join(" ");
        let val = expr::eval(&src, &self.vars)?;
        let snap = (self.scene.clone(), self.history.clone(), self.vars.clone());
        self.vars.insert(name.clone(), val);
        self.push_undo(snap);
        self.history.push(raw.trim().to_string());
        Ok(format!("{name} = {}", expr::fmt_num(val)))
    }

    /// `unset <name>` — remove a variable.
    fn op_unset(&mut self, tokens: &[String], raw: &str) -> Result<String> {
        let name = tokens
            .get(1)
            .ok_or_else(|| CadError::Arg("usage: unset <name>".into()))?;
        if !self.vars.contains_key(name) {
            return Err(CadError::NotFound(format!("variable `{name}`")));
        }
        let snap = (self.scene.clone(), self.history.clone(), self.vars.clone());
        self.vars.remove(name);
        self.push_undo(snap);
        self.history.push(raw.trim().to_string());
        Ok(format!("unset {name}"))
    }

    /// `vars` — list current variables (read-only; not recorded).
    fn list_vars(&self) -> String {
        if self.vars.is_empty() {
            return "no variables set (try `set w 40`)".into();
        }
        let mut names: Vec<&String> = self.vars.keys().collect();
        names.sort();
        let mut out = String::new();
        for n in names {
            out.push_str(&format!("{n} = {}\n", expr::fmt_num(self.vars[n])));
        }
        out.pop();
        out
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

    /// Apply a transform to every named entity (a group's members or a single
    /// entity). Reflections (`reflect`) also reverse mesh winding so solids keep
    /// outward normals.
    fn transform_members(&mut self, names: &[String], m: &Mat4, reflect: bool) {
        for n in names {
            if let Some(e) = self.scene.get_mut(n) {
                e.geom.transform(m);
                if reflect {
                    if let Geometry::Mesh(me) = &mut e.geom {
                        for t in &mut me.tris {
                            t.swap(1, 2);
                        }
                    }
                }
            }
        }
    }

    fn members_label(names: &[String]) -> String {
        if names.len() == 1 {
            format!("'{}'", names[0])
        } else {
            format!("{} entities", names.len())
        }
    }

    fn op_translate(&mut self, args: &Args) -> Result<String> {
        let x = args.pos_f64(0)?;
        let y = args.pos_f64_or(1, 0.0)?;
        let z = args.pos_f64_or(2, 0.0)?;
        let target = args.get_str("target").map(|s| s.to_string());
        let names = self.scene.member_names(target.as_deref())?;
        self.transform_members(&names, &Mat4::translation(Vec3::new(x, y, z)), false);
        Ok(format!(
            "translated {} by ({x}, {y}, {z})",
            Self::members_label(&names)
        ))
    }

    fn op_rotate(&mut self, args: &Args) -> Result<String> {
        let deg = args.pos_f64(0)?;
        let axis = args.str_or("axis", "z").to_string();
        let about = args.str_or("about", "center").to_string();
        let target = args.get_str("target").map(|s| s.to_string());
        let names = self.scene.member_names(target.as_deref())?;
        let pivot = if about == "origin" {
            Vec3::ZERO
        } else {
            self.scene.group_center(&names)
        };
        let rot = match axis.as_str() {
            "x" => Mat4::rotation_x(rad(deg)),
            "y" => Mat4::rotation_y(rad(deg)),
            _ => Mat4::rotation_z(rad(deg)),
        };
        let m = Mat4::translation(pivot)
            .mul(&rot)
            .mul(&Mat4::translation(-pivot));
        self.transform_members(&names, &m, false);
        Ok(format!(
            "rotated {} {deg}° about {axis} ({about})",
            Self::members_label(&names)
        ))
    }

    fn op_scale(&mut self, args: &Args) -> Result<String> {
        let s = args.pos_f64(0)?;
        let about = args.str_or("about", "center").to_string();
        let target = args.get_str("target").map(|s| s.to_string());
        let names = self.scene.member_names(target.as_deref())?;
        let pivot = if about == "origin" {
            Vec3::ZERO
        } else {
            self.scene.group_center(&names)
        };
        let m = Mat4::translation(pivot)
            .mul(&Mat4::scale(Vec3::new(s, s, s)))
            .mul(&Mat4::translation(-pivot));
        self.transform_members(&names, &m, false);
        Ok(format!(
            "scaled {} ×{s} ({about})",
            Self::members_label(&names)
        ))
    }

    /// `mirror [axis=x|y|z] [about=center|origin] [copy] [target=]` — reflect a
    /// shape/mesh (or a whole group). `copy` keeps the original and adds a
    /// mirrored duplicate instead of reflecting in place.
    fn op_mirror(&mut self, args: &Args) -> Result<String> {
        let axis = args.str_or("axis", "x").to_string();
        let about = args.str_or("about", "center").to_string();
        let copy = args.flag("copy");
        let target = args.get_str("target").map(|s| s.to_string());
        let names = self.scene.member_names(target.as_deref())?;
        let pivot = if about == "origin" {
            Vec3::ZERO
        } else {
            self.scene.group_center(&names)
        };
        let sv = match axis.as_str() {
            "y" => Vec3::new(1.0, -1.0, 1.0),
            "z" => Vec3::new(1.0, 1.0, -1.0),
            _ => Vec3::new(-1.0, 1.0, 1.0),
        };
        let m = Mat4::translation(pivot)
            .mul(&Mat4::scale(sv))
            .mul(&Mat4::translation(-pivot));
        if copy {
            // Duplicate each member, reflect the copy, add under a fresh name.
            let sources: Vec<(String, Geometry)> = names
                .iter()
                .filter_map(|n| self.scene.get(n).map(|e| (n.clone(), e.geom.clone())))
                .collect();
            for (n, mut g) in sources {
                g.transform(&m);
                if let Geometry::Mesh(me) = &mut g {
                    for t in &mut me.tris {
                        t.swap(1, 2);
                    }
                }
                self.scene.add(format!("{n}_mir"), g);
            }
            Ok(format!(
                "mirrored copy of {} ({axis})",
                Self::members_label(&names)
            ))
        } else {
            self.transform_members(&names, &m, true);
            Ok(format!("mirrored {} ({axis})", Self::members_label(&names)))
        }
    }

    /// `array <n> [kind=linear|polar|grid] [dx= dy= dz=] [nx= ny=]
    /// [about=center|origin] [target=] [name=]` — replicate one entity. The
    /// original stays as instance 0; copies are added (and tagged into `name=`'s
    /// group when given). Polar spins copies about Z to close a full circle.
    fn op_array(&mut self, args: &Args) -> Result<String> {
        let kind = args.str_or("kind", "linear").to_string();
        let target = args.get_str("target").map(|s| s.to_string());
        let src = self.scene.resolve(target.as_deref())?.name.clone();
        let base = self.scene.get(&src).unwrap().geom.clone();
        let group = args.name();

        // Transforms for the ADDED instances (original is instance 0).
        let mut transforms: Vec<Mat4> = Vec::new();
        match kind.as_str() {
            "polar" => {
                let n = args.pos_f64(0)? as usize;
                if n < 2 {
                    return Err(CadError::Arg("polar array needs count >= 2".into()));
                }
                // Polar arrays spin about the world axis by default (the gear
                // center); `about=center` spins each copy about the part center.
                let about = args.str_or("about", "origin").to_string();
                let pivot = if about == "center" {
                    self.scene.group_center(std::slice::from_ref(&src))
                } else {
                    Vec3::ZERO
                };
                for i in 1..n {
                    let ang = 360.0 * (i as f64) / (n as f64);
                    transforms.push(
                        Mat4::translation(pivot)
                            .mul(&Mat4::rotation_z(rad(ang)))
                            .mul(&Mat4::translation(-pivot)),
                    );
                }
            }
            "grid" => {
                let nx = args.usize_or("nx", 1)?;
                let ny = args.usize_or("ny", 1)?;
                let dx = args.f64_or("dx", 0.0)?;
                let dy = args.f64_or("dy", 0.0)?;
                for iy in 0..ny {
                    for ix in 0..nx {
                        if ix == 0 && iy == 0 {
                            continue;
                        }
                        transforms.push(Mat4::translation(Vec3::new(
                            dx * ix as f64,
                            dy * iy as f64,
                            0.0,
                        )));
                    }
                }
            }
            _ => {
                let n = args.pos_f64(0)? as usize;
                if n < 2 {
                    return Err(CadError::Arg("linear array needs count >= 2".into()));
                }
                let dx = args.f64_or("dx", 0.0)?;
                let dy = args.f64_or("dy", 0.0)?;
                let dz = args.f64_or("dz", 0.0)?;
                for i in 1..n {
                    transforms.push(Mat4::translation(Vec3::new(
                        dx * i as f64,
                        dy * i as f64,
                        dz * i as f64,
                    )));
                }
            }
        }

        // Tag the original into the group too, so the whole array moves together.
        if let Some(g) = &group {
            if let Some(e) = self.scene.get_mut(&src) {
                e.group = Some(g.clone());
            }
        }
        let mut count = 1usize;
        for m in transforms {
            let mut g = base.clone();
            g.transform(&m);
            let nm = self.scene.auto_name(&src);
            self.scene.add(nm.clone(), g);
            if let Some(gr) = &group {
                if let Some(e) = self.scene.get_mut(&nm) {
                    e.group = Some(gr.clone());
                }
            }
            count += 1;
        }
        let g_note = group
            .map(|g| format!(" in group '{g}'"))
            .unwrap_or_default();
        Ok(format!("arrayed '{src}' → {count} instances{g_note}"))
    }

    /// `group <a> <b> … name=<tag>` — tag entities into a group.
    fn op_group(&mut self, args: &Args) -> Result<String> {
        let g = args
            .name()
            .ok_or_else(|| CadError::Arg("group needs name=<tag>".into()))?;
        if args.positional.is_empty() {
            return Err(CadError::Arg("group needs entity names".into()));
        }
        for n in &args.positional {
            let e = self
                .scene
                .get_mut(n)
                .ok_or_else(|| CadError::NotFound(n.clone()))?;
            e.group = Some(g.clone());
        }
        Ok(format!(
            "grouped {} entities as '{g}'",
            args.positional.len()
        ))
    }

    /// `ungroup <tag>` — clear the group tag from its members.
    fn op_ungroup(&mut self, args: &Args) -> Result<String> {
        let g = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("ungroup needs a group name".into()))?
            .to_string();
        let mut n = 0;
        for e in &mut self.scene.entities {
            if e.group.as_deref() == Some(&g) {
                e.group = None;
                n += 1;
            }
        }
        if n == 0 {
            return Err(CadError::NotFound(g));
        }
        Ok(format!("ungrouped '{g}' ({n} entities)"))
    }

    /// `dim <target> [name=]` — build a dimension annotation (witness lines,
    /// dimension lines with arrowheads, and measured width/height labels via
    /// single-stroke text) for a 2D shape's bounding box, as a new `role="dim"`
    /// entity so exporters route it to its own layer.
    fn op_dim(&mut self, args: &Args) -> Result<String> {
        use crate::geom2d::Contour;
        let target = args.get_str("target").map(|s| s.to_string());
        let src = self.scene.resolve(target.as_deref())?.name.clone();
        let b = match &self.scene.get(&src).unwrap().geom {
            Geometry::Shape(s) => s.bbox(),
            Geometry::Mesh(_) => return Err(CadError::Arg("dim needs a 2D shape".into())),
        };
        let w = b.width();
        let h = b.height();
        let unit = self.scene.units.suffix();
        let th = (w.max(h) * 0.06).max(2.0); // text / arrow size
        let off = th * 2.0; // dim-line offset from the shape

        let mut cs: Vec<Contour> = Vec::new();
        let line = |a: Vec2, b: Vec2| Contour::new(vec![a, b], false);
        // Arrowhead V at `p` pointing along unit `d`.
        let arrow = |cs: &mut Vec<Contour>, p: Vec2, d: Vec2| {
            let back = p - d * th;
            let n = d.perp() * (th * 0.35);
            cs.push(Contour::new(vec![back + n, p, back - n], false));
        };
        // Pull a stroke-text label's contours, translated into place.
        let label = |cs: &mut Vec<Contour>, text: &str, at: Vec2, height: f64| {
            let mut t = text::stroke_text(text, height, Align::Center);
            t.transform(&Mat4::translation(Vec3::new(at.x, at.y, 0.0)));
            cs.extend(t.contours);
        };

        // Horizontal dimension (width), below the shape.
        let yd = b.min.y - off;
        cs.push(line(
            Vec2::new(b.min.x, b.min.y),
            Vec2::new(b.min.x, yd - th * 0.3),
        ));
        cs.push(line(
            Vec2::new(b.max.x, b.min.y),
            Vec2::new(b.max.x, yd - th * 0.3),
        ));
        cs.push(line(Vec2::new(b.min.x, yd), Vec2::new(b.max.x, yd)));
        arrow(&mut cs, Vec2::new(b.min.x, yd), Vec2::new(1.0, 0.0));
        arrow(&mut cs, Vec2::new(b.max.x, yd), Vec2::new(-1.0, 0.0));
        label(
            &mut cs,
            &format!("{w:.1}{unit}"),
            Vec2::new((b.min.x + b.max.x) * 0.5, yd + th * 0.4),
            th,
        );

        // Vertical dimension (height), left of the shape.
        let xd = b.min.x - off;
        cs.push(line(
            Vec2::new(b.min.x, b.min.y),
            Vec2::new(xd - th * 0.3, b.min.y),
        ));
        cs.push(line(
            Vec2::new(b.min.x, b.max.y),
            Vec2::new(xd - th * 0.3, b.max.y),
        ));
        cs.push(line(Vec2::new(xd, b.min.y), Vec2::new(xd, b.max.y)));
        arrow(&mut cs, Vec2::new(xd, b.min.y), Vec2::new(0.0, 1.0));
        arrow(&mut cs, Vec2::new(xd, b.max.y), Vec2::new(0.0, -1.0));
        label(
            &mut cs,
            &format!("{h:.1}{unit}"),
            Vec2::new(xd - th * 0.6, (b.min.y + b.max.y) * 0.5),
            th,
        );

        let name = args.name().unwrap_or_else(|| format!("{src}_dim"));
        self.scene
            .add(name.clone(), Geometry::Shape(crate::geom2d::Shape::new(cs)));
        self.scene
            .set_attrs(Some(&name), None, Some("dim".into()))?;
        Ok(format!(
            "dimensioned '{src}' → '{name}' ({w:.1}×{h:.1}{unit})"
        ))
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

    /// `offset <dist>` / `inset <dist>` — grow (+) or shrink (−) a 2D shape by a
    /// distance. `inset` always shrinks. `join=round|miter|bevel` shapes convex
    /// corners on an outward offset; `seg=` controls round smoothness. With a new
    /// `name=` the result is added as a separate entity (e.g. a kerf/tool path);
    /// otherwise the target shape is replaced in place.
    fn op_offset(&mut self, args: &Args, inward: bool) -> Result<String> {
        let mut dist = args.pos_f64(0)?;
        if inward {
            dist = -dist.abs();
        }
        let seg = args.usize_or("seg", 16)?;
        let miter = args.f64_or("miter", 4.0)?;
        let join = offset::parse_join(args.str_or("join", "round"), miter)?;
        let target = args.get_str("target").map(|s| s.to_string());

        let src = self.scene.resolve(target.as_deref())?;
        let src_name = src.name.clone();
        let shape = match &src.geom {
            Geometry::Shape(s) => s.clone(),
            Geometry::Mesh(_) => return Err(CadError::Arg("offset needs a 2D shape".into())),
        };
        let result = offset::offset(&shape, dist, join, seg)?;
        if result.is_empty() {
            return Err(CadError::Geometry(
                "offset removed all geometry (inset distance too large?)".into(),
            ));
        }
        let verb = if inward { "inset" } else { "offset" };
        match args.name() {
            Some(name) if name != src_name => {
                self.scene.add(name.clone(), Geometry::Shape(result));
                Ok(format!("{verb} '{src_name}' → '{name}' ({dist:+})"))
            }
            _ => {
                let e = self.scene.resolve_mut(target.as_deref())?;
                if let Geometry::Shape(s) = &mut e.geom {
                    *s = result;
                }
                Ok(format!("{verb} '{src_name}' ({dist:+})"))
            }
        }
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

    // ---- assemblies & BOM --------------------------------------------------

    /// `part <label> [target=]` — tag an entity (or a whole group) with a
    /// part-type label used by `bom`.
    fn op_part(&mut self, args: &Args) -> Result<String> {
        let label = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("part needs a label: part <label> [target=]".into()))?
            .to_string();
        let target = args.get_str("target").map(|s| s.to_string());
        let n = self.scene.set_part(target.as_deref(), &label)?;
        Ok(format!(
            "part '{label}' ({n} entit{})",
            if n == 1 { "y" } else { "ies" }
        ))
    }

    /// `assembly <tag> <member> …` — tag the named entities into a group so the
    /// assembly moves/exports as one unit (a friendly alias for `group`).
    fn op_assembly(&mut self, args: &Args) -> Result<String> {
        let tag = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: assembly <tag> <member> ...".into()))?
            .to_string();
        let members = &args.positional[1..];
        if members.is_empty() {
            return Err(CadError::Arg("assembly needs at least one member".into()));
        }
        for m in members {
            let e = self
                .scene
                .get_mut(m)
                .ok_or_else(|| CadError::NotFound(m.clone()))?;
            e.group = Some(tag.clone());
        }
        Ok(format!("assembly '{tag}' ({} parts)", members.len()))
    }

    /// `bom [group=<tag>]` — bill of materials: quantities of each part across
    /// the scene (or one assembly). Parts are grouped by their `part` label,
    /// falling back to the entity name with trailing array digits stripped (so
    /// `bolt`, `bolt1`, `bolt2` count as `bolt ×3`).
    fn bom(&self, args: &Args) -> String {
        let names: Vec<String> = match args.get_str("group") {
            Some(tag) => match self.scene.member_names(Some(tag)) {
                Ok(ns) => ns,
                Err(e) => return format!("{e}\n"),
            },
            None => self.scene.entities.iter().map(|e| e.name.clone()).collect(),
        };
        if names.is_empty() {
            return "(nothing to bill)\n".into();
        }
        // Accumulate per part key, preserving first-seen order.
        let mut order: Vec<String> = Vec::new();
        let mut qty: std::collections::HashMap<String, (usize, String, f64, f64, f64)> =
            std::collections::HashMap::new();
        for name in &names {
            let e = match self.scene.get(name) {
                Some(e) => e,
                None => continue,
            };
            let key = e
                .part
                .clone()
                .unwrap_or_else(|| strip_trailing_digits(&e.name).to_string());
            let (kind, w, h, d) = match &e.geom {
                Geometry::Shape(s) => {
                    let b = s.bbox();
                    ("2D", b.width(), b.height(), 0.0)
                }
                Geometry::Mesh(m) => {
                    let s = m.bbox().size();
                    ("3D", s.x, s.y, s.z)
                }
            };
            let entry = qty
                .entry(key.clone())
                .or_insert_with(|| (0, kind.to_string(), w, h, d));
            if entry.0 == 0 {
                order.push(key);
            }
            entry.0 += 1;
        }
        let u = self.scene.units.suffix();
        let mut out = format!("Bill of materials ({} part types):\n", order.len());
        let mut total = 0usize;
        for key in &order {
            let (n, kind, w, h, d) = &qty[key];
            total += n;
            let size = if kind == "2D" {
                format!("{w:.1}×{h:.1}{u}")
            } else {
                format!("{w:.1}×{h:.1}×{d:.1}{u}")
            };
            out.push_str(&format!("  {key:<16} ×{n:<4} {kind}  {size}\n"));
        }
        out.push_str(&format!("  {:-<16}\n  {:<16} {total} items\n", "", "total"));
        out
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
            // Append part/group tags when present, so assemblies are visible.
            let mut tags = String::new();
            if let Some(p) = &e.part {
                tags.push_str(&format!("  part={p}"));
            }
            if let Some(g) = &e.group {
                tags.push_str(&format!("  group={g}"));
            }
            out.push_str(&format!("  {marker} {:<16} {dims}{tags}\n", e.name));
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

    /// `measure [target]` — mass properties of one entity (defaults to the
    /// selection). 2D: area, perimeter, centroid, bbox. 3D: volume, surface
    /// area, centroid, bbox, and watertightness + genus.
    fn measure(&self, args: &Args) -> String {
        let target = args.pos_str(0);
        let u = self.scene.units.suffix();
        match self.scene.resolve(target) {
            Ok(e) => match &e.geom {
                Geometry::Shape(s) => {
                    let b = s.bbox();
                    let c = s.centroid();
                    format!(
                        "'{}' (2D)\n  area      {:.3} {u}²\n  perimeter {:.3} {u}\n  centroid  ({:.3}, {:.3})\n  bbox      {:.3}×{:.3} {u}\n",
                        e.name,
                        s.area(),
                        s.perimeter(),
                        c.x,
                        c.y,
                        b.width(),
                        b.height(),
                    )
                }
                Geometry::Mesh(m) => {
                    let b = m.bbox();
                    let sz = b.size();
                    let c = m.centroid();
                    let closed = m.is_closed_manifold();
                    let genus = if closed {
                        let chi = m.euler_characteristic();
                        format!("yes (χ={chi}, genus {})", (2 - chi) / 2)
                    } else {
                        "no (not watertight)".to_string()
                    };
                    format!(
                        "'{}' (3D)\n  volume    {:.3} {u}³\n  surface   {:.3} {u}²\n  centroid  ({:.3}, {:.3}, {:.3})\n  bbox      {:.3}×{:.3}×{:.3} {u}\n  closed    {genus}\n",
                        e.name,
                        m.signed_volume(),
                        m.surface_area(),
                        c.x,
                        c.y,
                        c.z,
                        sz.x,
                        sz.y,
                        sz.z,
                    )
                }
            },
            Err(e) => format!("{e}\n"),
        }
    }

    /// `interfere [a b]` — clash detection. With two names, report whether they
    /// overlap and by how much; with none, scan every pair (AABB broad-phase)
    /// and list the clashes.
    fn interfere(&self, args: &Args) -> String {
        match (args.pos_str(0), args.pos_str(1)) {
            (Some(a), Some(b)) => match self.pair_overlap(a, b) {
                Ok(Some(msg)) => format!("{msg}\n"),
                Ok(None) => format!("'{a}' and '{b}' are clear (no overlap)\n"),
                Err(e) => format!("{e}\n"),
            },
            _ => {
                let mut out = String::new();
                let ents = &self.scene.entities;
                let mut clashes = 0;
                for i in 0..ents.len() {
                    for j in (i + 1)..ents.len() {
                        if let Ok(Some(msg)) = self.pair_overlap(&ents[i].name, &ents[j].name) {
                            out.push_str(&format!("  {msg}\n"));
                            clashes += 1;
                        }
                    }
                }
                if clashes == 0 {
                    "no clashes detected\n".to_string()
                } else {
                    format!("{clashes} clash(es):\n{out}")
                }
            }
        }
    }

    /// Overlap of two entities by name: `Ok(Some(report))` when they clash,
    /// `Ok(None)` when clear, `Err` for mixed dimensionality / bad input. Uses an
    /// AABB broad-phase before the exact boolean/CSG intersection.
    fn pair_overlap(&self, a: &str, b: &str) -> Result<Option<String>> {
        let ea = self.scene.resolve(Some(a))?;
        let eb = self.scene.resolve(Some(b))?;
        let u = self.scene.units.suffix();
        match (&ea.geom, &eb.geom) {
            (Geometry::Shape(sa), Geometry::Shape(sb)) => {
                if !sa.bbox().intersects(&sb.bbox(), 0.0) {
                    return Ok(None);
                }
                let inter = sa.intersection(sb)?;
                let area = inter.area();
                if area > 1e-9 {
                    Ok(Some(format!("'{a}' ∩ '{b}' overlap area {area:.3} {u}²")))
                } else {
                    Ok(None)
                }
            }
            (Geometry::Mesh(ma), Geometry::Mesh(mb)) => {
                if !ma.bbox().intersects(&mb.bbox(), 0.0) {
                    return Ok(None);
                }
                let inter = geom3d::csg::intersection(ma, mb)?;
                let vol = inter.signed_volume();
                if vol > 1e-9 {
                    Ok(Some(format!("'{a}' ∩ '{b}' overlap volume {vol:.3} {u}³")))
                } else {
                    Ok(None)
                }
            }
            _ => Err(CadError::Arg(format!(
                "cannot compare 2D '{a}' with 3D '{b}'"
            ))),
        }
    }

    /// `section <target> [axis=z] [at=<coord>] [name=]` — slice a mesh by an
    /// axis-aligned plane into a new 2D shape entity.
    fn op_section(&mut self, args: &Args) -> Result<String> {
        let target = args.get_str("target").or_else(|| args.pos_str(0));
        let src = self.scene.resolve(target)?;
        let src_name = src.name.clone();
        let mesh = match &src.geom {
            Geometry::Mesh(m) => m.clone(),
            Geometry::Shape(_) => return Err(CadError::Arg("section needs a 3D mesh".into())),
        };
        let axis = geom3d::section::Axis::parse(args.str_or("axis", "z"))
            .ok_or_else(|| CadError::Arg("axis must be x, y, or z".into()))?;
        let at = match args.get_str("at") {
            Some(_) => args.f64("at")?,
            None => {
                let b = mesh.bbox();
                let c = b.center();
                match axis {
                    geom3d::section::Axis::X => c.x,
                    geom3d::section::Axis::Y => c.y,
                    geom3d::section::Axis::Z => c.z,
                }
            }
        };
        let shape = geom3d::section::section(&mesh, axis, at)?;
        let name = args
            .name()
            .unwrap_or_else(|| self.scene.auto_name(&format!("{src_name}_sec")));
        self.scene.add(name.clone(), Geometry::Shape(shape));
        Ok(format!(
            "section '{src_name}' at {axis:?}={at:.3} → '{name}'"
        ))
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
        let mut target = args.pos_str(0).map(|s| s.to_string());

        // A group tag isn't an entity: render the whole scene (which holds the
        // group) rather than nothing.
        if let Some(t) = target.as_deref() {
            let is_group = self.scene.get(t).is_none()
                && self
                    .scene
                    .entities
                    .iter()
                    .any(|e| e.group.as_deref() == Some(t));
            if is_group {
                target = None;
            }
        }

        // 2D vs 3D: by the target's kind, else 3D if the scene has any solid.
        let is_3d = if let Some(t) = target.as_deref() {
            self.scene.get(t).map(|e| e.geom.kind()) == Some("3d")
        } else {
            !self.scene.bbox3().is_empty()
        };

        let cols = args.usize_or("w", 80)?;
        let rows = args.usize_or("h", 30)?;

        if !is_3d {
            let opt = RenderOptions {
                cols,
                rows,
                ..RenderOptions::default()
            };
            return Ok(render::render_2d(&self.scene, target.as_deref(), &opt));
        }

        // 3D style and camera.
        let mode = match args.str_or("mode", "wire") {
            "hlr" => RenderMode::Hlr,
            "shade" => RenderMode::Shade,
            _ => RenderMode::Wire,
        };
        // View preset → (yaw, pitch); explicit yaw=/pitch= override.
        let (vyaw, vpitch) = match args.str_or("view", "iso") {
            "top" => (0.0, 90.0),
            "front" => (0.0, 0.0),
            "right" => (90.0, 0.0),
            _ => (35.0, 25.0),
        };
        let yaw = if args.get_str("yaw").is_some() {
            args.f64_or("yaw", vyaw)?
        } else {
            vyaw
        };
        let pitch = if args.get_str("pitch").is_some() {
            args.f64_or("pitch", vpitch)?
        } else {
            vpitch
        };

        // Multi-view sheet: four viewports tiled 2×2 (shade coerced to wire).
        if args.str_or("layout", "single") == "views" {
            let tile_mode = if mode == RenderMode::Shade {
                RenderMode::Wire
            } else {
                mode
            };
            let views = [
                ("top", 0.0, 90.0),
                ("front", 0.0, 0.0),
                ("right", 90.0, 0.0),
                ("iso", 35.0, 25.0),
            ];
            let blocks: Vec<(String, String)> = views
                .iter()
                .map(|&(label, y, p)| {
                    let opt = RenderOptions {
                        cols: cols / 2,
                        rows: rows / 2,
                        yaw_deg: y,
                        pitch_deg: p,
                        mode: tile_mode,
                        axes: false,
                        ..RenderOptions::default()
                    };
                    (
                        label.to_string(),
                        render::render_3d(&self.scene, target.as_deref(), &opt),
                    )
                })
                .collect();
            return Ok(render::tile(&blocks, 2));
        }

        let axes = mode != RenderMode::Shade && args.str_or("axes", "on") != "off";
        let opt = RenderOptions {
            cols,
            rows,
            yaw_deg: yaw,
            pitch_deg: pitch,
            mode,
            axes,
            ..RenderOptions::default()
        };
        Ok(render::render_3d(&self.scene, target.as_deref(), &opt))
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
        let binary = args.flag("binary");
        // `group=<tag>` exports a whole assembly (all its members) to one file.
        let members = match args.get_str("group") {
            Some(tag) => Some(self.scene.member_names(Some(tag))?),
            None => None,
        };
        export::export(
            &self.scene,
            &format,
            &path,
            target,
            members.as_deref(),
            binary,
        )
    }

    fn load(&mut self, args: &Args) -> Result<String> {
        let path = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: load <file.cad>".into()))?;
        let text = std::fs::read_to_string(path)?;
        let out = self.exec_script(&text)?;
        Ok(format!("loaded {path}\n{out}"))
    }

    /// `import <file.svg|dxf|stl>` — read an external/exported file into the scene.
    fn op_import(&mut self, args: &Args) -> Result<String> {
        let path = args
            .pos_str(0)
            .ok_or_else(|| CadError::Arg("usage: import <file.svg|dxf|stl>".into()))?
            .to_string();
        crate::import::import(&mut self.scene, &path)
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
        use crate::ai::agent::{run_agent, AgentMode, DEFAULT_BUDGET};

        // Parse the leading subcommand: `ai edit <change>`, `ai fix`, else `ai <request>`.
        let rest = raw.trim().strip_prefix("ai").unwrap_or("").trim();
        let (mode, task) = if let Some(c) = rest.strip_prefix("edit") {
            (
                AgentMode::Edit,
                c.trim().trim_matches('"').trim().to_string(),
            )
        } else if rest == "fix" || rest.starts_with("fix ") {
            (AgentMode::Fix, String::new())
        } else {
            (AgentMode::Create, rest.trim_matches('"').trim().to_string())
        };
        if matches!(mode, AgentMode::Create | AgentMode::Edit) && task.is_empty() {
            return Err(CadError::Arg(
                "usage: ai <describe what to make> | ai edit \"<change>\" | ai fix".into(),
            ));
        }

        let tx = crate::ai::CurlTransport;
        let report = run_agent(&tx, self, &task, mode, DEFAULT_BUDGET)?;
        let status = if report.converged {
            format!("✓ converged in {} step(s)", report.steps)
        } else {
            format!(
                "⚠ stopped after {} step(s) without DONE (try `ai fix`)",
                report.steps
            )
        };
        Ok(format!("{}\n{status}\n", report.transcript))
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Interpreter::new()
    }
}

/// Substitute `${key}` / `${key:default}` placeholders in a script-skill body.
/// Strip trailing ASCII digits from a name so array copies (`bolt`, `bolt1`,
/// `bolt2`) share a BOM key. A name that is all digits is returned unchanged.
fn strip_trailing_digits(name: &str) -> &str {
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        name
    } else {
        trimmed
    }
}

/// Is `s` a valid variable name? Letters/digits/`_`, not starting with a digit.
fn is_var_name(s: &str) -> bool {
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Expand `$var` / `${expr}` in each argument token against `vars`.
fn expand_tokens(tokens: &[String], vars: &HashMap<String, f64>) -> Result<Vec<String>> {
    tokens.iter().map(|t| expand_token(t, vars)).collect()
}

/// Expand one token. A token with no `$` is returned verbatim. For a
/// `key=value` token only the value is expanded (keys never contain `$`).
fn expand_token(tok: &str, vars: &HashMap<String, f64>) -> Result<String> {
    if !tok.contains('$') {
        return Ok(tok.to_string());
    }
    if let Some((k, v)) = tok.split_once('=') {
        if !v.contains('$') {
            return Ok(tok.to_string());
        }
        return Ok(format!("{k}={}", expand_value(v, vars)?));
    }
    expand_value(tok, vars)
}

/// Expand a value string: first splice any `${expr}` occurrences into the
/// surrounding text (interpolation, e.g. `out_${w}.svg`), then — if a bare
/// `$name` expression remains — evaluate the whole string as a number.
fn expand_value(s: &str, vars: &HashMap<String, f64>) -> Result<String> {
    let interpolated = if s.contains("${") {
        interpolate(s, vars)?
    } else {
        s.to_string()
    };
    if interpolated.contains('$') {
        let n = expr::eval(&interpolated, vars)?;
        Ok(expr::fmt_num(n))
    } else {
        Ok(interpolated)
    }
}

/// Replace every `${expr}` in `s` with the formatted value of `expr`.
fn interpolate(s: &str, vars: &HashMap<String, f64>) -> Result<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(pos) = rest.find("${") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| CadError::Parse("unterminated ${...} expression".into()))?;
        let n = expr::eval(&after[..end], vars)?;
        out.push_str(&expr::fmt_num(n));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

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

/// `~/.cad-assist/config.cad` — optional startup script (units, defaults).
fn default_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".cad-assist").join("config.cad"))
}

/// Whether a command mutates the scene (and is therefore recorded to history
/// and snapshotted for undo). These are exactly the non-early-return arms of the
/// `exec_line` dispatch.
fn is_modifying(cmd: &str) -> bool {
    matches!(
        cmd,
        "rect"
            | "circle"
            | "polygon"
            | "star"
            | "arc"
            | "path"
            | "text"
            | "box"
            | "cylinder"
            | "sphere"
            | "cone"
            | "extrude"
            | "revolve"
            | "translate"
            | "rotate"
            | "scale"
            | "array"
            | "mirror"
            | "group"
            | "ungroup"
            | "dim"
            | "section"
            | "union"
            | "subtract"
            | "intersect"
            | "xor"
            | "fillet"
            | "chamfer"
            | "offset"
            | "inset"
            | "shell"
            | "layer"
            | "role"
            | "part"
            | "assembly"
            | "units"
            | "select"
            | "delete"
            | "clear"
            | "import"
    )
}
