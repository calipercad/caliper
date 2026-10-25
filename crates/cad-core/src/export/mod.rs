//! Exporters. 2D formats (SVG/DXF/G-code) consume [`Shape`]s; 3D formats
//! (STL/OBJ) consume [`Mesh`]es. Coordinates are scaled from scene units to
//! millimetres so downstream tools get real-world sizes.

use crate::error::{CadError, Result};
use crate::geom2d::Shape;
use crate::geom3d::Mesh;
use crate::scene::{Geometry, Scene};
use std::fs;

pub mod dxf;
pub mod gcode;
pub mod obj;
pub mod stl;
pub mod svg;

/// Formats the exporter understands.
pub fn known_formats() -> &'static [&'static str] {
    &["svg", "dxf", "gcode", "stl", "obj"]
}

/// A 2D entity passed to the layered exporters (SVG/DXF), carrying its export
/// `layer` and fabrication `role` alongside the geometry.
pub struct Item<'a> {
    pub name: &'a str,
    pub shape: &'a Shape,
    pub layer: Option<&'a str>,
    pub role: Option<&'a str>,
}

impl<'a> Item<'a> {
    /// A plain item with no layer/role (lands on the default layer).
    pub fn new(name: &'a str, shape: &'a Shape) -> Self {
        Item {
            name,
            shape,
            layer: None,
            role: None,
        }
    }

    /// Effective export layer: explicit `layer`, else `role`, else `"0"`.
    pub fn layer_name(&self) -> &'a str {
        self.layer.or(self.role).unwrap_or("0")
    }
}

/// Map a fabrication role to (SVG stroke hex, DXF ACI color index). Known roles
/// get conventional cutter colors; anything else (and no role) is black/7.
pub fn role_color(role: Option<&str>) -> (&'static str, i32) {
    match role {
        Some("cut") => ("#000000", 7),
        Some("score") => ("#0000ff", 5),
        Some("engrave") => ("#ff0000", 1),
        _ => ("#000000", 7),
    }
}

fn is_3d(format: &str) -> bool {
    matches!(format, "stl" | "obj")
}

/// Whether an entity passes the optional `members` filter (a group/assembly's
/// member names). `None` means no filter — include everything.
fn in_members(members: Option<&[String]>, name: &str) -> bool {
    members.is_none_or(|m| m.iter().any(|n| n == name))
}

/// Collect 2D shapes from the scene (a single named entity, else every 2D
/// entity, optionally filtered to a group's `members`).
fn collect_shapes<'a>(
    scene: &'a Scene,
    target: Option<&str>,
    members: Option<&[String]>,
) -> Result<Vec<(&'a str, &'a Shape)>> {
    let mut out = Vec::new();
    match target {
        Some(name) => {
            let e = scene
                .get(name)
                .ok_or_else(|| CadError::NotFound(name.into()))?;
            match &e.geom {
                Geometry::Shape(s) => out.push((e.name.as_str(), s)),
                Geometry::Mesh(_) => {
                    return Err(CadError::Arg(format!("'{name}' is 3D; use stl/obj")))
                }
            }
        }
        None => {
            for e in &scene.entities {
                if let Geometry::Shape(s) = &e.geom {
                    if in_members(members, &e.name) {
                        out.push((e.name.as_str(), s));
                    }
                }
            }
        }
    }
    if out.is_empty() {
        return Err(CadError::Geometry("no 2D geometry to export".into()));
    }
    Ok(out)
}

/// Collect 2D entities with their layer/role for the layered exporters.
fn collect_items<'a>(
    scene: &'a Scene,
    target: Option<&str>,
    members: Option<&[String]>,
) -> Result<Vec<Item<'a>>> {
    let push = |out: &mut Vec<Item<'a>>, e: &'a crate::scene::Entity| {
        if let Geometry::Shape(s) = &e.geom {
            out.push(Item {
                name: e.name.as_str(),
                shape: s,
                layer: e.layer.as_deref(),
                role: e.role.as_deref(),
            });
        }
    };
    let mut out = Vec::new();
    match target {
        Some(name) => {
            let e = scene
                .get(name)
                .ok_or_else(|| CadError::NotFound(name.into()))?;
            match &e.geom {
                Geometry::Shape(_) => push(&mut out, e),
                Geometry::Mesh(_) => {
                    return Err(CadError::Arg(format!("'{name}' is 3D; use stl/obj")))
                }
            }
        }
        None => {
            for e in &scene.entities {
                if in_members(members, &e.name) {
                    push(&mut out, e);
                }
            }
        }
    }
    if out.is_empty() {
        return Err(CadError::Geometry("no 2D geometry to export".into()));
    }
    Ok(out)
}

/// Collect 3D meshes from the scene (a single named entity, else every mesh,
/// optionally filtered to a group's `members`).
fn collect_meshes<'a>(
    scene: &'a Scene,
    target: Option<&str>,
    members: Option<&[String]>,
) -> Result<Vec<(&'a str, &'a Mesh)>> {
    let mut out = Vec::new();
    match target {
        Some(name) => {
            let e = scene
                .get(name)
                .ok_or_else(|| CadError::NotFound(name.into()))?;
            match &e.geom {
                Geometry::Mesh(m) => out.push((e.name.as_str(), m)),
                Geometry::Shape(_) => {
                    return Err(CadError::Arg(format!(
                        "'{name}' is 2D; use svg/dxf/gcode (or extrude it first)"
                    )))
                }
            }
        }
        None => {
            for e in &scene.entities {
                if let Geometry::Mesh(m) = &e.geom {
                    if in_members(members, &e.name) {
                        out.push((e.name.as_str(), m));
                    }
                }
            }
        }
    }
    if out.is_empty() {
        return Err(CadError::Geometry("no 3D geometry to export".into()));
    }
    Ok(out)
}

/// Export `scene` to `path` in `format`. `target` restricts to one entity;
/// `members` (a group/assembly's names) restricts to a subset. `binary` selects
/// binary STL (ignored by other formats).
pub fn export(
    scene: &Scene,
    format: &str,
    path: &str,
    target: Option<&str>,
    members: Option<&[String]>,
    binary: bool,
) -> Result<String> {
    let format = format.to_lowercase();
    let mm = scene.units.to_mm();
    // Binary STL is bytes, not text — write it directly.
    if format == "stl" && binary {
        let meshes = collect_meshes(scene, target, members)?;
        let bytes = stl::to_binary_stl(&meshes, mm);
        fs::write(path, bytes)?;
        let n = meshes.len();
        return Ok(format!(
            "wrote {n} entit{} to {path} (stl binary)",
            if n == 1 { "y" } else { "ies" }
        ));
    }
    let text = if is_3d(&format) {
        let meshes = collect_meshes(scene, target, members)?;
        match format.as_str() {
            "stl" => stl::to_ascii_stl(&meshes, mm),
            "obj" => obj::to_obj(&meshes, mm),
            _ => unreachable!(),
        }
    } else {
        match format.as_str() {
            "svg" => svg::to_svg(&collect_items(scene, target, members)?, mm),
            "dxf" => dxf::to_dxf(&collect_items(scene, target, members)?, mm),
            "gcode" => gcode::to_gcode(
                &collect_shapes(scene, target, members)?,
                mm,
                &gcode::GcodeOptions::default(),
            ),
            other => return Err(CadError::Arg(format!("unknown format '{other}'"))),
        }
    };
    fs::write(path, text)?;
    let count = if is_3d(&format) {
        collect_meshes(scene, target, members)?.len()
    } else {
        collect_shapes(scene, target, members)?.len()
    };
    Ok(format!(
        "wrote {count} entit{} to {path} ({format})",
        if count == 1 { "y" } else { "ies" }
    ))
}
