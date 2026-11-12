// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Terminal rendering. A [`BrailleCanvas`] packs a 2×4 dot grid into each
//! Unicode braille glyph, giving crisp line art in a plain terminal. 2D shapes
//! are rasterised directly; 3D meshes are drawn as rotated wireframes.

use crate::bbox::Aabb2;
use crate::geom2d::Shape;
use crate::geom3d::Mesh;
use crate::math::{rad, Mat4, Vec2, Vec3};
use crate::scene::{Geometry, Scene};

/// Braille dot bitmasks indexed as `MASK[dy][dx]` (dy: 0..4, dx: 0..2).
const MASK: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

pub struct BrailleCanvas {
    cols: usize,
    rows: usize,
    // dot resolution
    w: usize,
    h: usize,
    cells: Vec<u8>,
}

impl BrailleCanvas {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        BrailleCanvas {
            cols,
            rows,
            w: cols * 2,
            h: rows * 4,
            cells: vec![0u8; cols * rows],
        }
    }

    pub fn dot_width(&self) -> usize {
        self.w
    }
    pub fn dot_height(&self) -> usize {
        self.h
    }

    pub fn set(&mut self, x: i64, y: i64) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        let cell = (y / 4) * self.cols + (x / 2);
        self.cells[cell] |= MASK[y % 4][x % 2];
    }

    /// Bresenham line in dot coordinates.
    pub fn line(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.set(x, y);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}

impl std::fmt::Display for BrailleCanvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for r in 0..self.rows {
            for c in 0..self.cols {
                let bits = self.cells[r * self.cols + c];
                let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
                f.write_fmt(format_args!("{ch}"))?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// 3D render style.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    /// Feature-edge wireframe (all edges, see-through).
    Wire,
    /// Hidden-line removal: edges behind surfaces are culled (depth-tested).
    Hlr,
    /// Shaded solid via ANSI half-block grayscale.
    Shade,
}

#[derive(Clone, Copy)]
pub struct RenderOptions {
    pub cols: usize,
    pub rows: usize,
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    pub mode: RenderMode,
    /// View-space light direction for shaded mode.
    pub light: Vec3,
    /// Draw a scale bar / axis overlay (single-view 3D).
    pub axes: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            cols: 80,
            rows: 30,
            yaw_deg: 35.0,
            pitch_deg: 25.0,
            mode: RenderMode::Wire,
            light: Vec3::new(-0.4, 1.0, 0.6),
            axes: false,
        }
    }
}

/// Render every 2D entity in the scene into a braille string.
pub fn render_2d(scene: &Scene, target: Option<&str>, opt: &RenderOptions) -> String {
    let shapes: Vec<&Shape> = match target {
        Some(name) => match scene.get(name).map(|e| &e.geom) {
            Some(Geometry::Shape(s)) => vec![s],
            _ => vec![],
        },
        None => scene
            .entities
            .iter()
            .filter_map(|e| match &e.geom {
                Geometry::Shape(s) => Some(s),
                _ => None,
            })
            .collect(),
    };
    if shapes.is_empty() {
        return "(no 2D geometry to render)\n".into();
    }

    let mut b = Aabb2::empty();
    for s in &shapes {
        let sb = s.bbox();
        if !sb.is_empty() {
            b.expand(sb.min);
            b.expand(sb.max);
        }
    }

    let mut canvas = BrailleCanvas::new(opt.cols, opt.rows);
    let fit = fit_2d(&b, canvas.dot_width(), canvas.dot_height());
    for s in &shapes {
        for c in &s.contours {
            let n = c.points.len();
            if n < 2 {
                continue;
            }
            let last = if c.closed { n } else { n - 1 };
            for i in 0..last {
                let p0 = fit.map(c.points[i]);
                let p1 = fit.map(c.points[(i + 1) % n]);
                canvas.line(p0.0, p0.1, p1.0, p1.1);
            }
        }
    }
    canvas.to_string()
}

/// Render every 3D mesh in the scene, dispatching on [`RenderOptions::mode`].
pub fn render_3d(scene: &Scene, target: Option<&str>, opt: &RenderOptions) -> String {
    let meshes: Vec<&Mesh> = match target {
        Some(name) => match scene.get(name).map(|e| &e.geom) {
            Some(Geometry::Mesh(m)) => vec![m],
            _ => vec![],
        },
        None => scene
            .entities
            .iter()
            .filter_map(|e| match &e.geom {
                Geometry::Mesh(m) => Some(m),
                _ => None,
            })
            .collect(),
    };
    if meshes.is_empty() {
        return "(no 3D geometry to render)\n".into();
    }
    let rot = Mat4::rotation_x(rad(opt.pitch_deg)).mul(&Mat4::rotation_z(rad(opt.yaw_deg)));
    match opt.mode {
        RenderMode::Shade => render_shade(&meshes, &rot, opt),
        mode => {
            let (proj, b) = project_meshes(&meshes, &rot);
            let mut canvas = BrailleCanvas::new(opt.cols, opt.rows);
            let fit = fit_2d(&b, canvas.dot_width(), canvas.dot_height());
            let (w, h) = (canvas.dot_width(), canvas.dot_height());
            let bias = 0.01 * b.width().max(b.height()).max(1e-6);

            // HLR: fill a depth buffer with the solid faces first.
            let depth = if mode == RenderMode::Hlr {
                let mut dbuf = vec![f64::NEG_INFINITY; w * h];
                for pm in &proj {
                    for t in &pm.tris {
                        let p = [
                            fit.mapf(pm.screen[t[0]]),
                            fit.mapf(pm.screen[t[1]]),
                            fit.mapf(pm.screen[t[2]]),
                        ];
                        let d = [pm.depth[t[0]], pm.depth[t[1]], pm.depth[t[2]]];
                        raster_tri(w, h, p, d, &mut dbuf, |_| {});
                    }
                }
                Some(dbuf)
            } else {
                None
            };

            for pm in &proj {
                for &(a, c) in &pm.edges {
                    match &depth {
                        Some(dbuf) => draw_edge_hlr(
                            &mut canvas,
                            dbuf,
                            w,
                            h,
                            fit.mapf(pm.screen[a]),
                            fit.mapf(pm.screen[c]),
                            pm.depth[a],
                            pm.depth[c],
                            bias,
                        ),
                        None => {
                            let p0 = fit.map(pm.screen[a]);
                            let p1 = fit.map(pm.screen[c]);
                            canvas.line(p0.0, p0.1, p1.0, p1.1);
                        }
                    }
                }
            }
            if opt.axes {
                draw_scale_bar(&mut canvas, fit.s, scene.units.suffix());
            }
            canvas.to_string()
        }
    }
}

/// Draw a feature edge with a per-dot depth test, culling parts behind the
/// solid (hidden-line removal). `bias` lets an edge coincident with its own
/// face win the test.
#[allow(clippy::too_many_arguments)]
fn draw_edge_hlr(
    canvas: &mut BrailleCanvas,
    depth: &[f64],
    w: usize,
    h: usize,
    a: (f64, f64),
    c: (f64, f64),
    da: f64,
    dc: f64,
    bias: f64,
) {
    let steps = ((c.0 - a.0).abs().max((c.1 - a.1).abs())).ceil() as i64;
    let steps = steps.max(1);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = a.0 + (c.0 - a.0) * t;
        let y = a.1 + (c.1 - a.1) * t;
        let d = da + (dc - da) * t;
        let (xi, yi) = (x.round() as i64, y.round() as i64);
        if xi < 0 || yi < 0 || xi as usize >= w || yi as usize >= h {
            continue;
        }
        let idx = yi as usize * w + xi as usize;
        if d >= depth[idx] - bias {
            canvas.set(xi, yi);
        }
    }
}

/// A short scale bar in the bottom-left: a round-number world length, drawn as a
/// ruler with end ticks. (Overlay for the single-view 3D render.)
fn draw_scale_bar(canvas: &mut BrailleCanvas, scale: f64, _unit: &str) {
    if scale <= 0.0 || !scale.is_finite() {
        return;
    }
    // Pick a "nice" world length whose pixel width is ~1/5 of the canvas.
    let target_px = canvas.dot_width() as f64 * 0.2;
    let raw = target_px / scale;
    let mag = 10f64.powf(raw.log10().floor());
    let nice = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .min_by(|a, b| {
            (a * scale - target_px)
                .abs()
                .partial_cmp(&(b * scale - target_px).abs())
                .unwrap()
        })
        .unwrap_or(mag);
    let px = (nice * scale).round() as i64;
    let (x0, y0) = (4i64, canvas.dot_height() as i64 - 4);
    canvas.line(x0, y0, x0 + px, y0);
    canvas.line(x0, y0 - 2, x0, y0 + 2);
    canvas.line(x0 + px, y0 - 2, x0 + px, y0 + 2);
}

// ---- shaded (ANSI half-block) rendering -----------------------------------

/// A pixel grid rendered with Unicode upper-half-blocks: each character is two
/// vertically-stacked pixels (`▀` with ANSI fg = top, bg = bottom).
struct HalfBlockCanvas {
    cols: usize,
    rows: usize,
    w: usize,
    h: usize,
    px: Vec<Option<u8>>, // grayscale 0..=255, None = background
}

impl HalfBlockCanvas {
    fn new(cols: usize, rows: usize) -> Self {
        let (cols, rows) = (cols.max(1), rows.max(1));
        HalfBlockCanvas {
            cols,
            rows,
            w: cols,
            h: rows * 2,
            px: vec![None; cols * rows * 2],
        }
    }
}

impl std::fmt::Display for HalfBlockCanvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for r in 0..self.rows {
            for c in 0..self.cols {
                let top = self.px[(2 * r) * self.w + c];
                let bot = self.px[(2 * r + 1) * self.w + c];
                match (top, bot) {
                    (None, None) => f.write_str(" ")?,
                    _ => {
                        let t = top.unwrap_or(0);
                        let b = bot.unwrap_or(0);
                        write!(f, "\x1b[38;2;{t};{t};{t}m\x1b[48;2;{b};{b};{b}m\u{2580}")?;
                    }
                }
            }
            writeln!(f, "\x1b[0m")?;
        }
        Ok(())
    }
}

fn render_shade(meshes: &[&Mesh], rot: &Mat4, opt: &RenderOptions) -> String {
    let (proj, b) = project_meshes(meshes, rot);
    let mut canvas = HalfBlockCanvas::new(opt.cols, opt.rows);
    let (w, h) = (canvas.w, canvas.h);
    let fit = fit_2d(&b, w, h);
    let light = opt.light.normalized();
    let mut depth = vec![f64::NEG_INFINITY; w * h];
    for (pm, m) in proj.iter().zip(meshes.iter()) {
        for t in &pm.tris {
            // View-space normal → Lambert shade.
            let n = rot.apply_dir(m.face_normal(*t)).normalized();
            let lambert = n.dot(light).max(0.0);
            let gray = (40.0 + 215.0 * lambert).round().clamp(0.0, 255.0) as u8;
            let p = [
                fit.mapf(pm.screen[t[0]]),
                fit.mapf(pm.screen[t[1]]),
                fit.mapf(pm.screen[t[2]]),
            ];
            let d = [pm.depth[t[0]], pm.depth[t[1]], pm.depth[t[2]]];
            let px = &mut canvas.px;
            raster_tri(w, h, p, d, &mut depth, |idx| px[idx] = Some(gray));
        }
    }
    canvas.to_string()
}

/// Tile several rendered blocks into a grid of `ncol` columns (used by
/// `layout=views`). Blocks are padded to equal width and separated by a gutter.
pub fn tile(blocks: &[(String, String)], ncol: usize) -> String {
    if blocks.is_empty() {
        return String::new();
    }
    let ncol = ncol.max(1);
    let mut out = String::new();
    for row in blocks.chunks(ncol) {
        // Split each block into (label line + body lines).
        let grids: Vec<Vec<String>> = row
            .iter()
            .map(|(label, body)| {
                let mut v = vec![label.clone()];
                v.extend(body.lines().map(|l| l.to_string()));
                v
            })
            .collect();
        let width = grids
            .iter()
            .flat_map(|g| g.iter().map(|l| l.chars().count()))
            .max()
            .unwrap_or(0);
        let height = grids.iter().map(|g| g.len()).max().unwrap_or(0);
        for i in 0..height {
            for (c, g) in grids.iter().enumerate() {
                if c > 0 {
                    out.push_str("  ");
                }
                let line = g.get(i).map(|s| s.as_str()).unwrap_or("");
                out.push_str(line);
                for _ in line.chars().count()..width {
                    out.push(' ');
                }
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// An aspect-preserving, Y-flipped fit of world 2D coords into a `w×h` pixel/dot
/// grid. `s` is the world→pixel scale (used by the scale-bar overlay).
#[derive(Clone, Copy)]
struct Fit {
    s: f64,
    ox: f64,
    oy: f64,
    minx: f64,
    miny: f64,
    hf: f64,
}

impl Fit {
    /// Map to fractional pixel coordinates (origin top-left, +Y up).
    fn mapf(&self, p: Vec2) -> (f64, f64) {
        let x = self.ox + (p.x - self.minx) * self.s;
        let y = self.hf - (self.oy + (p.y - self.miny) * self.s); // flip Y
        (x, y)
    }
    /// Map to rounded integer pixel coordinates.
    fn map(&self, p: Vec2) -> (i64, i64) {
        let (x, y) = self.mapf(p);
        (x.round() as i64, y.round() as i64)
    }
}

fn fit_2d(b: &Aabb2, w: usize, h: usize) -> Fit {
    let pad = 2.0;
    let bw = (b.width()).max(1e-6);
    let bh = (b.height()).max(1e-6);
    let sx = (w as f64 - 2.0 * pad) / bw;
    let sy = (h as f64 - 2.0 * pad) / bh;
    let s = sx.min(sy);
    let ox = pad + (w as f64 - 2.0 * pad - bw * s) / 2.0;
    let oy = pad + (h as f64 - 2.0 * pad - bh * s) / 2.0;
    Fit {
        s,
        ox,
        oy,
        minx: b.min.x,
        miny: b.min.y,
        hf: h as f64,
    }
}

/// Edge function (twice the signed area of triangle a,b,c).
fn edge(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Rasterize a triangle (fractional pixel coords + per-vertex depth) into a
/// `w×h` depth buffer, keeping the nearest (max-depth) surface. `plot(idx)` runs
/// for each pixel that becomes the new nearest (used to record shading).
fn raster_tri(
    w: usize,
    h: usize,
    p: [(f64, f64); 3],
    d: [f64; 3],
    depth: &mut [f64],
    mut plot: impl FnMut(usize),
) {
    let area = edge(p[0], p[1], p[2]);
    if area.abs() < 1e-9 {
        return;
    }
    let minx = p
        .iter()
        .map(|q| q.0)
        .fold(f64::MAX, f64::min)
        .floor()
        .max(0.0) as i64;
    let maxx = p
        .iter()
        .map(|q| q.0)
        .fold(f64::MIN, f64::max)
        .ceil()
        .min(w as f64 - 1.0) as i64;
    let miny = p
        .iter()
        .map(|q| q.1)
        .fold(f64::MAX, f64::min)
        .floor()
        .max(0.0) as i64;
    let maxy = p
        .iter()
        .map(|q| q.1)
        .fold(f64::MIN, f64::max)
        .ceil()
        .min(h as f64 - 1.0) as i64;
    for py in miny..=maxy {
        for px in minx..=maxx {
            let pt = (px as f64 + 0.5, py as f64 + 0.5);
            let w0 = edge(p[1], p[2], pt);
            let w1 = edge(p[2], p[0], pt);
            let w2 = edge(p[0], p[1], pt);
            let inside =
                (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
            if !inside {
                continue;
            }
            let dep = (w0 * d[0] + w1 * d[1] + w2 * d[2]) / area;
            let idx = py as usize * w + px as usize;
            if dep > depth[idx] {
                depth[idx] = dep;
                plot(idx);
            }
        }
    }
}

/// Per-mesh projection: screen points, per-vertex depth, feature edges, and the
/// original triangles (for the depth raster). Camera is `rot` about each mesh
/// center; screen = `(x, z)`, depth = `y` (larger = nearer).
struct ProjMesh {
    screen: Vec<Vec2>,
    depth: Vec<f64>,
    edges: Vec<(usize, usize)>,
    tris: Vec<[usize; 3]>,
}

fn project_meshes(meshes: &[&Mesh], rot: &Mat4) -> (Vec<ProjMesh>, Aabb2) {
    let mut b = Aabb2::empty();
    let mut out = Vec::new();
    for m in meshes {
        let mv = rot.mul(&Mat4::translation(-m.bbox().center()));
        let mut screen = Vec::with_capacity(m.verts.len());
        let mut depth = Vec::with_capacity(m.verts.len());
        for &v in &m.verts {
            let r = mv.apply_point(v);
            let p = Vec2::new(r.x, r.z);
            b.expand(p);
            screen.push(p);
            depth.push(r.y);
        }
        out.push(ProjMesh {
            screen,
            depth,
            edges: m.feature_edges(18.0),
            tris: m.tris.clone(),
        });
    }
    (out, b)
}
