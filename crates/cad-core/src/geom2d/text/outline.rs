// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Minimal TrueType (`glyf`) outline parser → **filled** letter [`Shape`]s for
//! through-cuts, the companion to the single-stroke [`super::stroke_text`].
//!
//! It reads just the tables an outline needs — `head`, `maxp`, `hhea`/`hmtx`,
//! `cmap`, `loca`, `glyf` — in big-endian, with every read bounds-checked
//! (a font file is untrusted external input, so a malformed one returns a
//! [`CadError`], never a panic). Quadratic Béziers are flattened with TrueType's
//! implied on-curve midpoints; each glyph contour becomes a **closed** polyline,
//! and holes ('O', 'A', 'e') fall out of [`Shape::normalize_windings`] with no
//! manual winding.
//!
//! **Scope.** TrueType `glyf` outlines only; a `.otf`/CFF (PostScript) font has
//! no `glyf` table and is rejected cleanly. Composite glyphs are assembled
//! translation-only (fine for Latin). No kerning. `height` is the em size.

use super::Align;
use crate::error::{CadError, Result};
use crate::geom2d::{Contour, Shape};
use crate::math::Vec2;
use std::collections::HashMap;

fn ferr(m: impl Into<String>) -> CadError {
    CadError::Geometry(format!("font: {}", m.into()))
}

// ---- bounds-checked big-endian readers -------------------------------------

fn u8_at(b: &[u8], o: usize) -> Result<u8> {
    b.get(o)
        .copied()
        .ok_or_else(|| ferr("unexpected end of data"))
}
fn u16_at(b: &[u8], o: usize) -> Result<u16> {
    Ok(((u8_at(b, o)? as u16) << 8) | u8_at(b, o + 1)? as u16)
}
fn i16_at(b: &[u8], o: usize) -> Result<i16> {
    Ok(u16_at(b, o)? as i16)
}
fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    Ok(((u16_at(b, o)? as u32) << 16) | u16_at(b, o + 2)? as u32)
}

/// A parsed TrueType font, holding just enough to render glyph outlines.
pub struct TrueTypeFont {
    data: Vec<u8>,
    units_per_em: f64,
    glyf_off: usize,
    loca: Vec<u32>,
    advances: Vec<u16>,
    cmap: HashMap<u32, u16>,
}

impl TrueTypeFont {
    pub fn parse(bytes: &[u8]) -> Result<TrueTypeFont> {
        let data = bytes.to_vec();
        let b = &data[..];
        let num_tables = u16_at(b, 4)? as usize;
        let mut tables: HashMap<[u8; 4], usize> = HashMap::new();
        for i in 0..num_tables {
            let rec = 12 + 16 * i;
            let tag = [
                u8_at(b, rec)?,
                u8_at(b, rec + 1)?,
                u8_at(b, rec + 2)?,
                u8_at(b, rec + 3)?,
            ];
            let off = u32_at(b, rec + 8)? as usize;
            tables.insert(tag, off);
        }
        let get = |t: &[u8; 4]| tables.get(t).copied();

        let head = get(b"head").ok_or_else(|| ferr("missing head table"))?;
        let units_per_em = u16_at(b, head + 18)? as f64;
        if units_per_em < 1.0 {
            return Err(ferr("invalid unitsPerEm"));
        }
        let loca_fmt = i16_at(b, head + 50)?;

        let maxp = get(b"maxp").ok_or_else(|| ferr("missing maxp table"))?;
        let num_glyphs = u16_at(b, maxp + 4)? as usize;

        let hhea = get(b"hhea").ok_or_else(|| ferr("missing hhea table"))?;
        let num_hmetrics = u16_at(b, hhea + 34)? as usize;
        let hmtx = get(b"hmtx").ok_or_else(|| ferr("missing hmtx table"))?;
        let mut advances = Vec::with_capacity(num_hmetrics.max(1));
        for i in 0..num_hmetrics {
            advances.push(u16_at(b, hmtx + i * 4)?);
        }
        if advances.is_empty() {
            advances.push(0);
        }

        let loca_off = get(b"loca").ok_or_else(|| ferr("missing loca table"))?;
        let mut loca = Vec::with_capacity(num_glyphs + 1);
        for i in 0..=num_glyphs {
            let v = if loca_fmt == 0 {
                u16_at(b, loca_off + i * 2)? as u32 * 2
            } else {
                u32_at(b, loca_off + i * 4)?
            };
            loca.push(v);
        }

        let glyf_off = get(b"glyf").ok_or_else(|| {
            ferr("no glyf table (TrueType outlines only; .otf/CFF is unsupported)")
        })?;
        let cmap_off = get(b"cmap").ok_or_else(|| ferr("missing cmap table"))?;
        let cmap = parse_cmap(b, cmap_off)?;

        Ok(TrueTypeFont {
            data,
            units_per_em,
            glyf_off,
            loca,
            advances,
            cmap,
        })
    }

    fn advance(&self, gid: u16) -> f64 {
        let i = (gid as usize).min(self.advances.len() - 1);
        self.advances[i] as f64
    }

    /// The contours of one glyph in font units (empty for a blank glyph like a
    /// space). `seg` = Bézier flattening steps per quadratic.
    fn glyph_contours(&self, gid: u16, seg: usize, depth: u8) -> Result<Vec<Vec<Vec2>>> {
        let g = gid as usize;
        if g + 1 >= self.loca.len() {
            return Ok(vec![]);
        }
        let (start, end) = (self.loca[g] as usize, self.loca[g + 1] as usize);
        if end <= start {
            return Ok(vec![]); // empty glyph
        }
        let b = &self.data[..];
        let base = self.glyf_off + start;
        let num_contours = i16_at(b, base)?;
        if num_contours < 0 {
            return self.composite(base, seg, depth);
        }
        let nc = num_contours as usize;
        let mut o = base + 10;
        let mut end_pts = Vec::with_capacity(nc);
        for i in 0..nc {
            end_pts.push(u16_at(b, o + i * 2)? as usize);
        }
        o += nc * 2;
        let num_pts = end_pts.last().map(|&e| e + 1).unwrap_or(0);
        let instr_len = u16_at(b, o)? as usize;
        o += 2 + instr_len;

        // Flags (with repeat compression).
        let mut flags = Vec::with_capacity(num_pts);
        while flags.len() < num_pts {
            let f = u8_at(b, o)?;
            o += 1;
            flags.push(f);
            if f & 0x08 != 0 {
                let r = u8_at(b, o)?;
                o += 1;
                for _ in 0..r {
                    if flags.len() < num_pts {
                        flags.push(f);
                    }
                }
            }
        }
        // X then Y delta streams.
        let mut xs = Vec::with_capacity(num_pts);
        let mut x = 0i32;
        for &f in &flags {
            if f & 0x02 != 0 {
                let d = u8_at(b, o)? as i32;
                o += 1;
                x += if f & 0x10 != 0 { d } else { -d };
            } else if f & 0x10 == 0 {
                x += i16_at(b, o)? as i32;
                o += 2;
            }
            xs.push(x);
        }
        let mut ys = Vec::with_capacity(num_pts);
        let mut y = 0i32;
        for &f in &flags {
            if f & 0x04 != 0 {
                let d = u8_at(b, o)? as i32;
                o += 1;
                y += if f & 0x20 != 0 { d } else { -d };
            } else if f & 0x20 == 0 {
                y += i16_at(b, o)? as i32;
                o += 2;
            }
            ys.push(y);
        }

        // Split into contours and flatten each.
        let mut out = Vec::new();
        let mut s = 0usize;
        for &e in &end_pts {
            if e >= num_pts || e < s {
                break;
            }
            let raw: Vec<(f64, f64, bool)> = (s..=e)
                .map(|i| (xs[i] as f64, ys[i] as f64, flags[i] & 0x01 != 0))
                .collect();
            let c = flatten_contour(&raw, seg);
            if c.len() >= 3 {
                out.push(c);
            }
            s = e + 1;
        }
        Ok(out)
    }

    /// Composite glyph: assemble component glyphs (translation offsets only).
    fn composite(&self, base: usize, seg: usize, depth: u8) -> Result<Vec<Vec<Vec2>>> {
        if depth > 5 {
            return Ok(vec![]); // guard against cyclic/deep components
        }
        let b = &self.data[..];
        let mut o = base + 10;
        let mut out = Vec::new();
        loop {
            let flags = u16_at(b, o)?;
            let comp_gid = u16_at(b, o + 2)?;
            o += 4;
            let (dx, dy) = if flags & 0x0001 != 0 {
                let a = i16_at(b, o)? as f64;
                let c = i16_at(b, o + 2)? as f64;
                o += 4;
                (a, c)
            } else {
                let a = (u8_at(b, o)? as i8) as f64;
                let c = (u8_at(b, o + 1)? as i8) as f64;
                o += 2;
                (a, c)
            };
            // Skip any component scale / 2×2 transform (approximated as identity).
            if flags & 0x0008 != 0 {
                o += 2;
            } else if flags & 0x0040 != 0 {
                o += 4;
            } else if flags & 0x0080 != 0 {
                o += 8;
            }
            let mut sub = self.glyph_contours(comp_gid, seg, depth + 1)?;
            if flags & 0x0002 != 0 {
                // ARGS_ARE_XY_VALUES: translate the component.
                for c in &mut sub {
                    for p in c.iter_mut() {
                        p.x += dx;
                        p.y += dy;
                    }
                }
            }
            out.extend(sub);
            if flags & 0x0020 == 0 {
                break; // no MORE_COMPONENTS
            }
        }
        Ok(out)
    }
}

/// Flatten one TrueType contour (on/off-curve points) into a closed polyline.
fn flatten_contour(raw: &[(f64, f64, bool)], seg: usize) -> Vec<Vec2> {
    let n = raw.len();
    if n == 0 {
        return vec![];
    }
    // Insert the implied on-curve midpoint between consecutive off-curve points.
    let mut exp: Vec<(f64, f64, bool)> = Vec::with_capacity(n * 2);
    for i in 0..n {
        let cur = raw[i];
        exp.push(cur);
        let nxt = raw[(i + 1) % n];
        if !cur.2 && !nxt.2 {
            exp.push(((cur.0 + nxt.0) / 2.0, (cur.1 + nxt.1) / 2.0, true));
        }
    }
    let m = exp.len();
    // Rotate so the walk starts on an on-curve point (fallback: treat first as on).
    let start = exp.iter().position(|p| p.2).unwrap_or(0);
    let seq: Vec<(f64, f64, bool)> = (0..m).map(|i| exp[(start + i) % m]).collect();

    let mut out: Vec<Vec2> = vec![Vec2::new(seq[0].0, seq[0].1)];
    let mut i = 0usize;
    while i < m {
        let b = seq[(i + 1) % m];
        if b.2 {
            out.push(Vec2::new(b.0, b.1));
            i += 1;
        } else {
            let a = seq[i % m];
            let c = seq[(i + 2) % m];
            for k in 1..=seg {
                let t = k as f64 / seg as f64;
                let mt = 1.0 - t;
                let x = mt * mt * a.0 + 2.0 * mt * t * b.0 + t * t * c.0;
                let yy = mt * mt * a.1 + 2.0 * mt * t * b.1 + t * t * c.1;
                out.push(Vec2::new(x, yy));
            }
            i += 2;
        }
    }
    // The walk returns to the start; drop the duplicate closing point.
    if out.len() > 1 && (out[0] - *out.last().unwrap()).len() < 1e-9 {
        out.pop();
    }
    out
}

// ---- cmap ------------------------------------------------------------------

fn parse_cmap(b: &[u8], off: usize) -> Result<HashMap<u32, u16>> {
    let num = u16_at(b, off + 2)? as usize;
    // Prefer a Unicode subtable: (3,10) full > (3,1) BMP > (0,*) Unicode > (3,0).
    let mut best: Option<(i32, usize)> = None;
    for i in 0..num {
        let rec = off + 4 + 8 * i;
        let plat = u16_at(b, rec)?;
        let enc = u16_at(b, rec + 2)?;
        let sub = off + u32_at(b, rec + 4)? as usize;
        let score = match (plat, enc) {
            (3, 10) => 4,
            (3, 1) => 3,
            (0, _) => 2,
            (3, 0) => 1,
            _ => 0,
        };
        if best.map(|(s, _)| score > s).unwrap_or(true) {
            best = Some((score, sub));
        }
    }
    let sub = best.ok_or_else(|| ferr("no usable cmap subtable"))?.1;
    match u16_at(b, sub)? {
        0 => parse_cmap0(b, sub),
        4 => parse_cmap4(b, sub),
        6 => parse_cmap6(b, sub),
        12 => parse_cmap12(b, sub),
        f => Err(ferr(format!("unsupported cmap format {f}"))),
    }
}

fn parse_cmap0(b: &[u8], sub: usize) -> Result<HashMap<u32, u16>> {
    let mut map = HashMap::new();
    for c in 0..256u32 {
        let gid = u8_at(b, sub + 6 + c as usize)? as u16;
        if gid != 0 {
            map.insert(c, gid);
        }
    }
    Ok(map)
}

fn parse_cmap4(b: &[u8], sub: usize) -> Result<HashMap<u32, u16>> {
    let segx2 = u16_at(b, sub + 6)? as usize;
    let seg_count = segx2 / 2;
    let end_off = sub + 14;
    let start_off = end_off + segx2 + 2; // +2 reservedPad
    let delta_off = start_off + segx2;
    let range_off = delta_off + segx2;
    let mut map = HashMap::new();
    for s in 0..seg_count {
        let end = u16_at(b, end_off + s * 2)?;
        let start = u16_at(b, start_off + s * 2)?;
        let delta = u16_at(b, delta_off + s * 2)?;
        let range = u16_at(b, range_off + s * 2)? as usize;
        for c in start..=end {
            if c == 0xFFFF {
                break;
            }
            let gid = if range == 0 {
                c.wrapping_add(delta)
            } else {
                let gi = range_off + s * 2 + range + (c - start) as usize * 2;
                let g = u16_at(b, gi)?;
                if g == 0 {
                    0
                } else {
                    g.wrapping_add(delta)
                }
            };
            if gid != 0 {
                map.insert(c as u32, gid);
            }
        }
    }
    Ok(map)
}

fn parse_cmap6(b: &[u8], sub: usize) -> Result<HashMap<u32, u16>> {
    let first = u16_at(b, sub + 6)? as u32;
    let count = u16_at(b, sub + 8)? as usize;
    let mut map = HashMap::new();
    for i in 0..count {
        let gid = u16_at(b, sub + 10 + i * 2)?;
        if gid != 0 {
            map.insert(first + i as u32, gid);
        }
    }
    Ok(map)
}

fn parse_cmap12(b: &[u8], sub: usize) -> Result<HashMap<u32, u16>> {
    let groups = u32_at(b, sub + 12)? as usize;
    let mut map = HashMap::new();
    for i in 0..groups {
        let g = sub + 16 + i * 12;
        let start = u32_at(b, g)?;
        let end = u32_at(b, g + 4)?;
        let start_gid = u32_at(b, g + 8)?;
        for (k, c) in (start..=end).enumerate() {
            let gid = (start_gid + k as u32) as u16;
            if gid != 0 {
                map.insert(c, gid);
            }
        }
    }
    Ok(map)
}

/// Lay out `text` from a TrueType font as filled, closed-contour glyphs.
/// `height` is the em size; `seg` is Bézier steps per quadratic.
pub fn outline_text(
    bytes: &[u8],
    text: &str,
    height: f64,
    align: Align,
    seg: usize,
) -> Result<Shape> {
    let font = TrueTypeFont::parse(bytes)?;
    let scale = height / font.units_per_em;
    let seg = seg.max(1);
    let blank = font.advance(0).max(font.units_per_em * 0.3);

    let mut pen = 0.0f64;
    let mut contours: Vec<Contour> = Vec::new();
    for ch in text.chars() {
        match font.cmap.get(&(ch as u32)).copied() {
            Some(gid) => {
                for gc in font.glyph_contours(gid, seg, 0)? {
                    let pts: Vec<Vec2> = gc
                        .iter()
                        .map(|p| Vec2::new(pen + p.x * scale, p.y * scale))
                        .collect();
                    contours.push(Contour::new(pts, true));
                }
                pen += font.advance(gid) * scale;
            }
            None => pen += blank * scale,
        }
    }

    // Horizontal alignment: shift the laid-out ink (mirrors `stroke_text`).
    let (mut minx, mut maxx) = (f64::MAX, f64::MIN);
    for c in &contours {
        for p in &c.points {
            minx = minx.min(p.x);
            maxx = maxx.max(p.x);
        }
    }
    if minx <= maxx {
        let dx = match align {
            Align::Left => -minx,
            Align::Center => -(minx + maxx) / 2.0,
            Align::Right => -maxx,
        };
        if dx != 0.0 {
            for c in &mut contours {
                for p in &mut c.points {
                    p.x += dx;
                }
            }
        }
    }
    // Classify solids/holes by nesting parity so fills and holes are correct
    // regardless of the font's native winding convention.
    let mut shape = Shape::new(contours);
    shape.normalize_windings();
    Ok(shape)
}
