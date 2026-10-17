//! Terminal rendering. A [`BrailleCanvas`] packs a 2×4 dot grid into each
//! Unicode braille glyph, giving crisp line art in a plain terminal. 2D shapes
//! are rasterised directly; 3D meshes are drawn as rotated wireframes.

use crate::bbox::Aabb2;
use crate::geom2d::Shape;
use crate::geom3d::Mesh;
use crate::math::{rad, Mat4, Vec2};
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

    pub fn to_string(&self) -> String {
        let mut s = String::with_capacity(self.rows * (self.cols + 1));
        for r in 0..self.rows {
            for c in 0..self.cols {
                let bits = self.cells[r * self.cols + c];
                let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
                s.push(ch);
            }
            s.push('\n');
        }
        s
    }
}

#[derive(Clone, Copy)]
pub struct RenderOptions {
    pub cols: usize,
    pub rows: usize,
    pub yaw_deg: f64,
    pub pitch_deg: f64,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            cols: 80,
            rows: 30,
            yaw_deg: 35.0,
            pitch_deg: 25.0,
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
    let (map, _) = fit_2d(&b, canvas.dot_width(), canvas.dot_height());
    for s in &shapes {
        for c in &s.contours {
            let n = c.points.len();
            if n < 2 {
                continue;
            }
            let last = if c.closed { n } else { n - 1 };
            for i in 0..last {
                let p0 = map(c.points[i]);
                let p1 = map(c.points[(i + 1) % n]);
                canvas.line(p0.0, p0.1, p1.0, p1.1);
            }
        }
    }
    canvas.to_string()
}

/// Render every 3D mesh in the scene as a rotated wireframe.
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

    // Rotate around the model centre, then project orthographically (drop Z).
    let rot = Mat4::rotation_x(rad(opt.pitch_deg)).mul(&Mat4::rotation_z(rad(opt.yaw_deg)));

    // First pass: projected 2D bounds.
    let mut b = Aabb2::empty();
    let mut projected: Vec<(Vec<Vec2>, Vec<(usize, usize)>)> = Vec::new();
    for m in &meshes {
        let center = m.bbox().center();
        let to_c = Mat4::translation(-center);
        let mv = rot.mul(&to_c);
        let pts: Vec<Vec2> = m
            .verts
            .iter()
            .map(|&v| {
                let r = mv.apply_point(v);
                Vec2::new(r.x, r.z) // view: X right, Z up
            })
            .collect();
        for &p in &pts {
            b.expand(p);
        }
        projected.push((pts, m.feature_edges(18.0)));
    }

    let mut canvas = BrailleCanvas::new(opt.cols, opt.rows);
    let (map, _) = fit_2d(&b, canvas.dot_width(), canvas.dot_height());
    for (pts, edges) in &projected {
        for &(a, c) in edges {
            let p0 = map(pts[a]);
            let p1 = map(pts[c]);
            canvas.line(p0.0, p0.1, p1.0, p1.1);
        }
    }
    canvas.to_string()
}

/// Build a mapping from world 2D coords into dot coords, preserving aspect and
/// flipping Y so +Y points up on screen.
fn fit_2d(b: &Aabb2, w: usize, h: usize) -> (impl Fn(Vec2) -> (i64, i64), f64) {
    let pad = 2.0;
    let bw = (b.width()).max(1e-6);
    let bh = (b.height()).max(1e-6);
    let sx = (w as f64 - 2.0 * pad) / bw;
    let sy = (h as f64 - 2.0 * pad) / bh;
    let s = sx.min(sy);
    let ox = pad + (w as f64 - 2.0 * pad - bw * s) / 2.0;
    let oy = pad + (h as f64 - 2.0 * pad - bh * s) / 2.0;
    let minx = b.min.x;
    let miny = b.min.y;
    let hf = h as f64;
    let map = move |p: Vec2| -> (i64, i64) {
        let x = ox + (p.x - minx) * s;
        let y = hf - (oy + (p.y - miny) * s); // flip Y
        (x.round() as i64, y.round() as i64)
    };
    (map, s)
}
