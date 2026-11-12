//! Phase 14 integration tests (roadmap v1.4): outline / stencil text — a minimal
//! TrueType `glyf` parser turning a `.ttf` into FILLED, closed-contour glyphs.
//!
//! No font is bundled (licensing/size + std-only), so these tests **synthesize a
//! minimal valid TrueType font in memory** — table directory + head/maxp/hhea/
//! hmtx + cmap format 4 + short loca + glyf with two drawn glyphs: 'I' (a solid
//! square) and 'O' (a square with a hole, plus one off-curve point to exercise
//! the quadratic-Bézier flattener). This mirrors the import tests' "build the
//! bytes in-test" convention and exercises the real parser end to end.

use cad_core::geom2d::text::{outline_text, Align};
use cad_core::scene::Geometry;
use cad_core::Interpreter;

// ---- minimal TrueType font builder -----------------------------------------

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// Build a tiny valid TrueType font: unitsPerEm=1000, glyphs 0=.notdef(empty),
/// 1='I' (square), 2='O' (square + hole, one off-curve point). cmap maps
/// 'I'→1, 'O'→2.
fn make_ttf() -> Vec<u8> {
    // ---- glyf: concatenated glyph descriptions ----
    // glyph 'I': 1 contour, 4 on-curve pts (100,0)(400,0)(400,700)(100,700).
    let mut g_i: Vec<u8> = Vec::new();
    g_i.extend(be16(1)); // numberOfContours
    g_i.extend(be16(0)); // xMin..yMax (unused by parser)
    g_i.extend(be16(0));
    g_i.extend(be16(0));
    g_i.extend(be16(0));
    g_i.extend(be16(3)); // endPtsOfContours = [3]
    g_i.extend(be16(0)); // instructionLength
    g_i.extend([0x01, 0x01, 0x01, 0x01]); // flags: all on-curve, 2-byte deltas
    for d in [100i16, 300, 0, -300] {
        g_i.extend(be16(d as u16)); // x deltas
    }
    for d in [0i16, 0, 700, 0] {
        g_i.extend(be16(d as u16)); // y deltas
    }
    assert!(g_i.len().is_multiple_of(2));

    // glyph 'O': 2 contours. Outer (5 pts, one off-curve top), inner hole (4 pts).
    let mut g_o: Vec<u8> = Vec::new();
    g_o.extend(be16(2)); // numberOfContours
    g_o.extend(be16(0));
    g_o.extend(be16(0));
    g_o.extend(be16(0));
    g_o.extend(be16(0));
    g_o.extend(be16(4)); // endPts contour 0 → idx 4
    g_o.extend(be16(8)); // endPts contour 1 → idx 8
    g_o.extend(be16(0)); // instructionLength
                         // flags: pt 3 is off-curve (0x00), the rest on-curve (0x01).
    g_o.extend([0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01]);
    // x deltas (abs: outer 0,600,600,300,0 ; hole 200,400,400,200)
    for d in [0i16, 600, 0, -300, -300, 200, 200, 0, -200] {
        g_o.extend(be16(d as u16));
    }
    // y deltas (abs: outer 0,0,700,800,700 ; hole 200,200,500,500)
    for d in [0i16, 0, 700, 100, -100, -500, 0, 300, 0] {
        g_o.extend(be16(d as u16));
    }
    if !g_o.len().is_multiple_of(2) {
        g_o.push(0); // pad to even (short loca stores offset/2)
    }

    let mut glyf: Vec<u8> = Vec::new();
    // glyph 0 .notdef is empty (loca[0]==loca[1])
    let off_i = glyf.len();
    glyf.extend(&g_i);
    let off_o = glyf.len();
    glyf.extend(&g_o);
    let off_end = glyf.len();

    // ---- loca (short): byte offsets / 2 ----
    let loca_vals = [0usize, off_i, off_o, off_end];
    let mut loca: Vec<u8> = Vec::new();
    for v in loca_vals {
        loca.extend(be16((v / 2) as u16));
    }

    // ---- head (54 bytes) ----
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&be16(1000)); // unitsPerEm
    head[50..52].copy_from_slice(&be16(0)); // indexToLocFormat = short

    // ---- maxp (v1.0, 32 bytes) ----
    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&be32(0x0001_0000));
    maxp[4..6].copy_from_slice(&be16(3)); // numGlyphs

    // ---- hhea (36 bytes) ----
    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&be16(3)); // numberOfHMetrics

    // ---- hmtx (3 × (advance u16, lsb i16)) ----
    let mut hmtx: Vec<u8> = Vec::new();
    for adv in [500u16, 500, 700] {
        hmtx.extend(be16(adv));
        hmtx.extend(be16(0)); // lsb
    }

    // ---- cmap: format 4, maps 'I'(73)→1, 'O'(79)→2 ----
    let mut sub: Vec<u8> = Vec::new();
    sub.extend(be16(4)); // format
    sub.extend(be16(40)); // length
    sub.extend(be16(0)); // language
    sub.extend(be16(6)); // segCountX2 (3 segments)
    sub.extend(be16(4)); // searchRange (unused by parser)
    sub.extend(be16(1)); // entrySelector
    sub.extend(be16(2)); // rangeShift
    for e in [73u16, 79, 0xFFFF] {
        sub.extend(be16(e)); // endCode
    }
    sub.extend(be16(0)); // reservedPad
    for s in [73u16, 79, 0xFFFF] {
        sub.extend(be16(s)); // startCode
    }
    for d in [(1i32 - 73) as u16, (2i32 - 79) as u16, 1u16] {
        sub.extend(be16(d)); // idDelta
    }
    for _ in 0..3 {
        sub.extend(be16(0)); // idRangeOffset
    }
    let mut cmap: Vec<u8> = Vec::new();
    cmap.extend(be16(0)); // version
    cmap.extend(be16(1)); // numTables
    cmap.extend(be16(3)); // platformID (Windows)
    cmap.extend(be16(1)); // encodingID (BMP)
    cmap.extend(be32(12)); // subtable offset (after 4+8 byte header)
    cmap.extend(&sub);

    // ---- assemble file: offset table + directory + tables ----
    let tables: [(&[u8; 4], Vec<u8>); 7] = [
        (b"head", head),
        (b"maxp", maxp),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"cmap", cmap),
        (b"loca", loca),
        (b"glyf", glyf),
    ];
    let num_tables = tables.len();
    let mut file: Vec<u8> = Vec::new();
    file.extend(be32(0x0001_0000)); // sfnt version
    file.extend(be16(num_tables as u16));
    file.extend(be16(0)); // searchRange
    file.extend(be16(0)); // entrySelector
    file.extend(be16(0)); // rangeShift

    let dir_start = file.len();
    let mut offset = dir_start + num_tables * 16;
    // First pass: directory records with computed offsets (4-byte aligned).
    let mut body: Vec<u8> = Vec::new();
    for (tag, data) in &tables {
        file.extend(tag.iter());
        file.extend(be32(0)); // checksum (unused by parser)
        file.extend(be32(offset as u32));
        file.extend(be32(data.len() as u32));
        body.extend(data.iter());
        let mut pad = (4 - (data.len() % 4)) % 4;
        offset += data.len();
        while pad > 0 {
            body.push(0);
            offset += 1;
            pad -= 1;
        }
    }
    file.extend(body);
    file
}

// ---- tests -----------------------------------------------------------------

#[test]
fn parses_and_produces_closed_contours() {
    let ttf = make_ttf();
    let shape = outline_text(&ttf, "I", 100.0, Align::Left, 8).unwrap();
    assert!(!shape.contours.is_empty(), "glyph produced contours");
    assert!(
        shape.contours.iter().all(|c| c.closed),
        "outline glyphs are CLOSED (unlike stroke text)"
    );
    // 'I' is a 300×700-unit square at em=1000 scaled to height 100 → 30×70.
    let b = shape.bbox();
    assert!((b.height() - 70.0).abs() < 1.0, "height {}", b.height());
    assert!((b.width() - 30.0).abs() < 1.0, "width {}", b.width());
}

#[test]
fn height_scales() {
    let ttf = make_ttf();
    let a = outline_text(&ttf, "I", 100.0, Align::Left, 8).unwrap();
    let c = outline_text(&ttf, "I", 200.0, Align::Left, 8).unwrap();
    assert!((c.bbox().height() - 2.0 * a.bbox().height()).abs() < 1.0);
}

#[test]
fn glyph_with_hole_has_a_region_and_hole() {
    let ttf = make_ttf();
    let shape = outline_text(&ttf, "O", 100.0, Align::Left, 8).unwrap();
    let regions = shape.regions();
    assert_eq!(regions.len(), 1, "one solid region");
    assert_eq!(regions[0].holes.len(), 1, "with one hole");
    // Net area = outer − hole, both scaled from em=1000 to height=100 (×0.1).
    // outer ≈ 600×~730, hole = 200×300, in font units → ×0.01 for area.
    assert!(shape.area() > 0.0, "positive net area");
}

#[test]
fn align_center_is_symmetric() {
    let ttf = make_ttf();
    let s = outline_text(&ttf, "IOI", 100.0, Align::Center, 8).unwrap();
    let b = s.bbox();
    assert!(
        (b.min.x + b.max.x).abs() < 1e-6,
        "centered: {} {}",
        b.min.x,
        b.max.x
    );
}

#[test]
fn unmapped_char_is_blank_advance() {
    let ttf = make_ttf();
    // 'Z' is not in the cmap → no contours, but the pen still advances, so
    // "IZI" is wider than "II".
    let zi = outline_text(&ttf, "IZI", 100.0, Align::Left, 8).unwrap();
    let ii = outline_text(&ttf, "II", 100.0, Align::Left, 8).unwrap();
    assert!(
        zi.bbox().width() > ii.bbox().width(),
        "blank advance widens"
    );
}

#[test]
fn malformed_font_errors_not_panics() {
    assert!(outline_text(b"not a font", "A", 20.0, Align::Left, 8).is_err());
    assert!(outline_text(&[0u8; 4], "A", 20.0, Align::Left, 8).is_err());
    let mut truncated = make_ttf();
    truncated.truncate(30);
    assert!(outline_text(&truncated, "I", 20.0, Align::Left, 8).is_err());
}

#[test]
fn text_command_outline_font() {
    let ttf = make_ttf();
    let p = std::env::temp_dir().join("cad_p14_test.ttf");
    std::fs::write(&p, &ttf).unwrap();
    let mut it = Interpreter::new();
    let msg = it
        .exec_line(&format!(
            "text \"O\" font=outline path={} h=50 name=sign role=cut",
            p.display()
        ))
        .unwrap();
    assert!(msg.contains("outline"), "{msg}");
    match &it.scene.get("sign").unwrap().geom {
        Geometry::Shape(s) => {
            assert!(s.contours.iter().all(|c| c.closed), "filled glyph");
            assert!(s.area() > 0.0);
        }
        _ => panic!("outline text should be a 2D shape"),
    }
    assert_eq!(it.scene.get("sign").unwrap().role.as_deref(), Some("cut"));
}

#[test]
fn outline_font_needs_path() {
    let mut it = Interpreter::new();
    assert!(it.exec_line("text \"HI\" font=outline h=20").is_err());
}
