// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! File import: read SVG / DXF / STL back into the scene. Parsers are
//! std-only and target the subset Caliper itself writes (see `export/`), so
//! a design round-trips: `export` then `import` preserves the geometry.
//!
//! Coordinates in exported files are in millimetres; on import they are divided
//! by `scene.units.to_mm()` so they land back in the document's units.

use crate::error::{CadError, Result};
use crate::geom2d::Shape;
use crate::geom3d::Mesh;
use crate::math::{Mat4, Vec2, Vec3};
use crate::scene::{Geometry, Scene};
use std::collections::HashMap;
use std::path::Path;

/// Import `path` (by extension) into `scene` as a new entity. Returns a summary.
pub fn import(scene: &mut Scene, path: &str) -> Result<String> {
    let mm = scene.units.to_mm();
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("import")
        .to_string();

    let geom = match ext.as_str() {
        "svg" => Geometry::Shape(parse_svg(&read_text(path)?, mm)?),
        "dxf" => Geometry::Shape(parse_dxf(&read_text(path)?, mm)?),
        "stl" => Geometry::Mesh(parse_stl(&std::fs::read(path)?, mm)?),
        other => {
            return Err(CadError::Arg(format!(
                "cannot import '.{other}' (supported: svg, dxf, stl)"
            )))
        }
    };

    let name = scene.add(stem, geom);
    Ok(format!("imported '{name}' from {path}"))
}

fn read_text(path: &str) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| CadError::Io(format!("{path}: {e}")))
}

// ---- DXF (exact round-trip: the writer does not Y-flip) --------------------

fn parse_dxf(text: &str, mm: f64) -> Result<Shape> {
    // Collect (group-code, value) pairs.
    let mut pairs: Vec<(i32, String)> = Vec::new();
    let mut lines = text.lines();
    while let Some(code_line) = lines.next() {
        let code: i32 = match code_line.trim().parse() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let val = lines.next().unwrap_or("").trim().to_string();
        pairs.push((code, val));
    }

    let getf = |fields: &[(i32, String)], code: i32| -> Option<f64> {
        fields
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.parse().ok())
    };

    let mut contours = Vec::new();
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i].0 != 0 {
            i += 1;
            continue;
        }
        let etype = pairs[i].1.clone();
        let mut j = i + 1;
        let mut fields: Vec<(i32, String)> = Vec::new();
        while j < pairs.len() && pairs[j].0 != 0 {
            fields.push(pairs[j].clone());
            j += 1;
        }
        i = j;

        match etype.as_str() {
            "CIRCLE" => {
                if let (Some(cx), Some(cy), Some(r)) =
                    (getf(&fields, 10), getf(&fields, 20), getf(&fields, 40))
                {
                    let mut s = Shape::circle(r / mm, 64);
                    s.transform(&Mat4::translation(Vec3::new(cx / mm, cy / mm, 0.0)));
                    contours.push(s.contours.remove(0));
                }
            }
            "ARC" => {
                if let (Some(cx), Some(cy), Some(r), Some(a0), Some(a1)) = (
                    getf(&fields, 10),
                    getf(&fields, 20),
                    getf(&fields, 40),
                    getf(&fields, 50),
                    getf(&fields, 51),
                ) {
                    let mut s = Shape::arc(r / mm, a0, a1, 48);
                    s.transform(&Mat4::translation(Vec3::new(cx / mm, cy / mm, 0.0)));
                    contours.push(s.contours.remove(0));
                }
            }
            "LWPOLYLINE" => {
                let closed = getf(&fields, 70)
                    .map(|f| (f as i64 & 1) == 1)
                    .unwrap_or(false);
                let mut pts = Vec::new();
                let mut cur_x: Option<f64> = None;
                for (c, v) in &fields {
                    match *c {
                        10 => cur_x = v.parse().ok(),
                        20 => {
                            if let (Some(x), Ok(y)) = (cur_x.take(), v.parse::<f64>()) {
                                pts.push(Vec2::new(x / mm, y / mm));
                            }
                        }
                        _ => {}
                    }
                }
                if pts.len() >= 2 {
                    contours.push(crate::geom2d::Contour::new(pts, closed));
                }
            }
            _ => {}
        }
    }

    if contours.is_empty() {
        return Err(CadError::Geometry("no importable entities in DXF".into()));
    }
    Ok(Shape::new(contours))
}

// ---- SVG (invert the writer's Y-flip via the viewBox height) ---------------

fn parse_svg(text: &str, mm: f64) -> Result<Shape> {
    // viewBox="0 0 wmm hmm" gives the height we flip around.
    let hmm = attr_str(text, "viewBox")
        .and_then(|v| {
            v.split_whitespace()
                .nth(3)
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);
    let fy = |y: f64| (hmm - y) / mm;
    let fx = |x: f64| x / mm;

    let mut contours = Vec::new();
    for el in elements(text, "<circle") {
        if let (Some(cx), Some(cy), Some(r)) = (attr(el, "cx"), attr(el, "cy"), attr(el, "r")) {
            let mut s = Shape::circle(r / mm, 64);
            s.transform(&Mat4::translation(Vec3::new(fx(cx), fy(cy), 0.0)));
            contours.push(s.contours.remove(0));
        }
    }
    for el in elements(text, "<path") {
        if let Some(d) = attr_str(el, "d") {
            let (pts, closed) = parse_path_d(d, fx, fy);
            if pts.len() >= 2 {
                contours.push(crate::geom2d::Contour::new(pts, closed));
            }
        }
    }

    if contours.is_empty() {
        return Err(CadError::Geometry("no importable elements in SVG".into()));
    }
    Ok(Shape::new(contours))
}

/// Parse the `M x y L x y … [Z]` polyline the writer emits. Command letters may
/// be glued to the first coordinate (`M12.3 4.5`), so strip any leading letters
/// per token and pair up every number into vertices.
fn parse_path_d(d: &str, fx: impl Fn(f64) -> f64, fy: impl Fn(f64) -> f64) -> (Vec<Vec2>, bool) {
    let mut closed = false;
    let mut nums: Vec<f64> = Vec::new();
    for tok in d.split_whitespace() {
        let t = tok.trim_start_matches(|c: char| {
            let is_cmd = matches!(c, 'M' | 'L' | 'm' | 'l' | 'Z' | 'z');
            if matches!(c, 'Z' | 'z') {
                closed = true;
            }
            is_cmd
        });
        if !t.is_empty() {
            if let Ok(v) = t.parse::<f64>() {
                nums.push(v);
            }
        }
    }
    let mut pts = Vec::new();
    let mut k = 0;
    while k + 1 < nums.len() {
        pts.push(Vec2::new(fx(nums[k]), fy(nums[k + 1])));
        k += 2;
    }
    (pts, closed)
}

fn attr(elem: &str, name: &str) -> Option<f64> {
    attr_str(elem, name).and_then(|s| s.trim().parse().ok())
}

fn attr_str<'a>(elem: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = elem.find(&key)? + key.len();
    let rest = &elem[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Each element substring from `tag` up to the next `>`.
fn elements<'a>(text: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(tag) {
        let start = from + i;
        match text[start..].find('>') {
            Some(j) => {
                out.push(&text[start..start + j]);
                from = start + j + 1;
            }
            None => break,
        }
    }
    out
}

// ---- STL (ASCII or binary) -------------------------------------------------

fn parse_stl(bytes: &[u8], mm: f64) -> Result<Mesh> {
    // Binary iff the size matches the triangle count declared at offset 80.
    let binary = bytes.len() >= 84 && {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        bytes.len() == 84 + 50 * n
    };
    let raw: Vec<Vec3> = if binary {
        parse_stl_binary(bytes)
    } else {
        parse_stl_ascii(std::str::from_utf8(bytes).map_err(|_| {
            CadError::Geometry("STL is neither valid binary nor UTF-8 ASCII".into())
        })?)
    };
    if raw.len() < 3 {
        return Err(CadError::Geometry("no triangles in STL".into()));
    }

    // Weld repeated vertices onto a grid scaled to the model extent.
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for v in &raw {
        lo = lo.min(v.x).min(v.y).min(v.z);
        hi = hi.max(v.x).max(v.y).max(v.z);
    }
    let grid = ((hi - lo).abs() * 1e-6).max(1e-9);
    let inv = 1.0 / grid;
    let key = |v: Vec3| {
        (
            (v.x * inv).round() as i64,
            (v.y * inv).round() as i64,
            (v.z * inv).round() as i64,
        )
    };

    let mut map: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut mesh = Mesh::new();
    let idx = |mesh: &mut Mesh, map: &mut HashMap<(i64, i64, i64), usize>, v: Vec3| -> usize {
        *map.entry(key(v)).or_insert_with(|| {
            mesh.verts.push(v / mm);
            mesh.verts.len() - 1
        })
    };
    for t in raw.chunks_exact(3) {
        let a = idx(&mut mesh, &mut map, t[0]);
        let b = idx(&mut mesh, &mut map, t[1]);
        let c = idx(&mut mesh, &mut map, t[2]);
        if a != b && b != c && c != a {
            mesh.tris.push([a, b, c]);
        }
    }
    Ok(mesh)
}

fn parse_stl_ascii(text: &str) -> Vec<Vec3> {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("vertex ") {
            let n: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            if n.len() == 3 {
                out.push(Vec3::new(n[0], n[1], n[2]));
            }
        }
    }
    out
}

fn parse_stl_binary(bytes: &[u8]) -> Vec<Vec3> {
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let mut out = Vec::with_capacity(n * 3);
    let f32_at =
        |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as f64;
    for i in 0..n {
        let base = 84 + 50 * i;
        // Skip the 3-float normal; read 3 vertices (9 floats).
        for v in 0..3 {
            let o = base + 12 + v * 12;
            out.push(Vec3::new(f32_at(o), f32_at(o + 4), f32_at(o + 8)));
        }
    }
    out
}
