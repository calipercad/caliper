// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! A compact **single-stroke** vector font for engraving / plotting / laser
//! scoring, authored in-tree (no external font files, no dependencies —
//! consistent with the crate's `std`-only, offline, auditable principle).
//!
//! Each glyph is a set of *strokes*; a stroke is a polyline the pen traces with
//! no fill (ideal for pen plotters and drag/engrave tools). Coordinates live on
//! an integer grid: baseline at `y = 0`, capital height at `y = CAP`, lowercase
//! x-height at `y = X_HEIGHT`, ascenders up to `CAP`, descenders down to
//! `DESCENT`. The pen advances by `advance` units between glyphs. All of this is
//! scaled to the requested cap height by [`super::stroke_text`].

/// Capital letter height in grid units (the layout unit that maps to the
/// requested text height).
pub const CAP: f32 = 14.0;
/// Lowercase x-height.
pub const X_HEIGHT: f32 = 9.0;
/// Lowest point of descenders (`g`, `j`, `p`, `q`, `y`).
pub const DESCENT: f32 = -4.0;

/// One glyph: a horizontal advance and its pen strokes.
pub struct Glyph {
    /// Horizontal advance in grid units (includes right side bearing).
    pub advance: f32,
    /// Pen-down polylines. An empty slice (e.g. space) draws nothing.
    pub strokes: &'static [&'static [(i8, i8)]],
}

/// Look up the glyph for a character, or `None` if unsupported (the caller
/// advances by a blank space rather than drawing a tofu box).
pub fn glyph(c: char) -> Option<&'static Glyph> {
    Some(match c {
        ' ' => &SPACE,
        '!' => &BANG,
        '"' => &DQUOTE,
        '#' => &HASH,
        '$' => &DOLLAR,
        '%' => &PERCENT,
        '&' => &AMP,
        '\'' => &QUOTE,
        '(' => &LPAREN,
        ')' => &RPAREN,
        '*' => &STAR,
        '+' => &PLUS,
        ',' => &COMMA,
        '-' => &HYPHEN,
        '.' => &DOT,
        '/' => &SLASH,
        '0' => &D0,
        '1' => &D1,
        '2' => &D2,
        '3' => &D3,
        '4' => &D4,
        '5' => &D5,
        '6' => &D6,
        '7' => &D7,
        '8' => &D8,
        '9' => &D9,
        ':' => &COLON,
        ';' => &SEMI,
        '<' => &LT,
        '=' => &EQ,
        '>' => &GT,
        '?' => &QUESTION,
        '@' => &AT,
        'A' => &UA,
        'B' => &UB,
        'C' => &UC,
        'D' => &UD,
        'E' => &UE,
        'F' => &UF,
        'G' => &UG,
        'H' => &UH,
        'I' => &UI,
        'J' => &UJ,
        'K' => &UK,
        'L' => &UL,
        'M' => &UM,
        'N' => &UN,
        'O' => &UO,
        'P' => &UP,
        'Q' => &UQ,
        'R' => &UR,
        'S' => &US,
        'T' => &UT,
        'U' => &UU,
        'V' => &UV,
        'W' => &UW,
        'X' => &UX,
        'Y' => &UY,
        'Z' => &UZ,
        '[' => &LBRACK,
        '\\' => &BACKSLASH,
        ']' => &RBRACK,
        '^' => &CARET,
        '_' => &UNDERSCORE,
        '`' => &BACKTICK,
        'a' => &LA,
        'b' => &LB,
        'c' => &LC,
        'd' => &LD,
        'e' => &LE,
        'f' => &LF,
        'g' => &LG,
        'h' => &LH,
        'i' => &LI,
        'j' => &LJ,
        'k' => &LK,
        'l' => &LL,
        'm' => &LM,
        'n' => &LN,
        'o' => &LO,
        'p' => &LP,
        'q' => &LQ,
        'r' => &LR,
        's' => &LS,
        't' => &LT_,
        'u' => &LU,
        'v' => &LV,
        'w' => &LW,
        'x' => &LX,
        'y' => &LY,
        'z' => &LZ,
        '{' => &LBRACE,
        '|' => &BAR,
        '}' => &RBRACE,
        '~' => &TILDE,
        _ => return None,
    })
}

// Helper to cut down on noise: each glyph is `Glyph { advance, strokes }`.
macro_rules! g {
    ($adv:expr, [ $( [ $( ($x:expr,$y:expr) ),* ] ),* $(,)? ]) => {
        Glyph { advance: $adv, strokes: &[ $( &[ $( ($x,$y) ),* ] ),* ] }
    };
}

// ---- space & punctuation ---------------------------------------------------

static SPACE: Glyph = g!(8.0, []);
static BANG: Glyph = g!(4.0, [[(2, 14), (2, 4)], [(2, 1), (2, 0)]]);
static DQUOTE: Glyph = g!(6.0, [[(2, 14), (2, 10)], [(4, 14), (4, 10)]]);
static HASH: Glyph = g!(
    12.0,
    [
        [(3, 0), (4, 14)],
        [(7, 0), (8, 14)],
        [(1, 5), (10, 5)],
        [(1, 9), (10, 9)],
    ]
);
static DOLLAR: Glyph = g!(
    10.0,
    [
        [
            (8, 12),
            (2, 12),
            (1, 9),
            (2, 7),
            (7, 7),
            (8, 5),
            (7, 2),
            (1, 2)
        ],
        [(5, 14), (5, 0)],
    ]
);
static PERCENT: Glyph = g!(
    12.0,
    [
        [(1, 0), (11, 14)],
        [(2, 14), (4, 14), (4, 11), (2, 11), (2, 14)],
        [(8, 3), (10, 3), (10, 0), (8, 0), (8, 3)],
    ]
);
static AMP: Glyph = g!(
    13.0,
    [[
        (11, 0),
        (4, 10),
        (4, 12),
        (6, 14),
        (8, 12),
        (8, 10),
        (1, 5),
        (1, 2),
        (3, 0),
        (6, 0),
        (10, 4)
    ],]
);
static QUOTE: Glyph = g!(4.0, [[(2, 14), (2, 10)]]);
static LPAREN: Glyph = g!(6.0, [[(4, 14), (1, 10), (1, 4), (4, 0)]]);
static RPAREN: Glyph = g!(6.0, [[(1, 14), (4, 10), (4, 4), (1, 0)]]);
static STAR: Glyph = g!(
    10.0,
    [[(5, 12), (5, 4)], [(2, 10), (8, 6)], [(8, 10), (2, 6)],]
);
static PLUS: Glyph = g!(12.0, [[(6, 11), (6, 3)], [(2, 7), (10, 7)]]);
static COMMA: Glyph = g!(5.0, [[(3, 1), (2, -1), (1, -2)]]);
static HYPHEN: Glyph = g!(10.0, [[(2, 7), (8, 7)]]);
static DOT: Glyph = g!(5.0, [[(2, 1), (2, 0)]]);
static SLASH: Glyph = g!(10.0, [[(1, 0), (9, 14)]]);
static COLON: Glyph = g!(5.0, [[(2, 9), (2, 8)], [(2, 1), (2, 0)]]);
static SEMI: Glyph = g!(5.0, [[(3, 9), (3, 8)], [(3, 1), (2, -1), (1, -2)]]);
static LT: Glyph = g!(11.0, [[(9, 13), (2, 7), (9, 1)]]);
static EQ: Glyph = g!(12.0, [[(2, 9), (10, 9)], [(2, 5), (10, 5)]]);
static GT: Glyph = g!(11.0, [[(2, 13), (9, 7), (2, 1)]]);
static QUESTION: Glyph = g!(
    10.0,
    [
        [(1, 11), (3, 14), (6, 14), (8, 11), (8, 9), (5, 6), (5, 4)],
        [(5, 1), (5, 0)],
    ]
);
static AT: Glyph = g!(
    14.0,
    [[
        (9, 5),
        (6, 5),
        (6, 9),
        (9, 9),
        (9, 4),
        (11, 4),
        (12, 7),
        (11, 11),
        (7, 14),
        (4, 14),
        (1, 11),
        (1, 4),
        (4, 0),
        (9, 0)
    ],]
);
static LBRACK: Glyph = g!(6.0, [[(4, 14), (2, 14), (2, 0), (4, 0)]]);
static BACKSLASH: Glyph = g!(10.0, [[(1, 14), (9, 0)]]);
static RBRACK: Glyph = g!(6.0, [[(2, 14), (4, 14), (4, 0), (2, 0)]]);
static CARET: Glyph = g!(10.0, [[(2, 10), (5, 14), (8, 10)]]);
static UNDERSCORE: Glyph = g!(12.0, [[(1, 0), (11, 0)]]);
static BACKTICK: Glyph = g!(5.0, [[(1, 14), (3, 11)]]);
static LBRACE: Glyph = g!(
    7.0,
    [[(5, 14), (3, 13), (3, 8), (1, 7), (3, 6), (3, 1), (5, 0)]]
);
static BAR: Glyph = g!(4.0, [[(2, 14), (2, -2)]]);
static RBRACE: Glyph = g!(
    7.0,
    [[(2, 14), (4, 13), (4, 8), (6, 7), (4, 6), (4, 1), (2, 0)]]
);
static TILDE: Glyph = g!(12.0, [[(2, 8), (4, 10), (8, 6), (10, 8)]]);

// ---- digits ----------------------------------------------------------------

static D0: Glyph = g!(
    11.0,
    [
        [
            (2, 2),
            (2, 12),
            (5, 14),
            (7, 14),
            (9, 12),
            (9, 2),
            (7, 0),
            (5, 0),
            (2, 2)
        ],
        [(2, 2), (9, 12)],
    ]
);
static D1: Glyph = g!(8.0, [[(2, 11), (5, 14), (5, 0)], [(2, 0), (8, 0)]]);
static D2: Glyph = g!(
    11.0,
    [[
        (2, 11),
        (4, 14),
        (7, 14),
        (9, 11),
        (9, 9),
        (2, 2),
        (2, 0),
        (9, 0)
    ],]
);
static D3: Glyph = g!(
    11.0,
    [
        [(2, 12), (4, 14), (7, 14), (9, 12), (9, 9), (7, 7), (4, 7)],
        [(7, 7), (9, 5), (9, 2), (7, 0), (4, 0), (2, 2)],
    ]
);
static D4: Glyph = g!(11.0, [[(7, 0), (7, 14), (2, 5), (10, 5)]]);
static D5: Glyph = g!(
    11.0,
    [[
        (9, 14),
        (2, 14),
        (2, 8),
        (6, 9),
        (9, 7),
        (9, 2),
        (7, 0),
        (4, 0),
        (2, 2)
    ],]
);
static D6: Glyph = g!(
    11.0,
    [[
        (9, 12),
        (7, 14),
        (4, 14),
        (2, 11),
        (2, 2),
        (4, 0),
        (7, 0),
        (9, 2),
        (9, 5),
        (7, 7),
        (4, 7),
        (2, 5)
    ],]
);
static D7: Glyph = g!(11.0, [[(2, 14), (9, 14), (5, 0)]]);
static D8: Glyph = g!(
    11.0,
    [[
        (4, 7),
        (2, 9),
        (2, 12),
        (4, 14),
        (7, 14),
        (9, 12),
        (9, 9),
        (7, 7),
        (4, 7),
        (2, 5),
        (2, 2),
        (4, 0),
        (7, 0),
        (9, 2),
        (9, 5),
        (7, 7)
    ],]
);
static D9: Glyph = g!(
    11.0,
    [[
        (2, 2),
        (4, 0),
        (7, 0),
        (9, 3),
        (9, 12),
        (7, 14),
        (4, 14),
        (2, 12),
        (2, 9),
        (4, 7),
        (7, 7),
        (9, 9)
    ],]
);

// ---- uppercase -------------------------------------------------------------

static UA: Glyph = g!(12.0, [[(1, 0), (5, 14), (9, 0)], [(3, 5), (7, 5)]]);
static UB: Glyph = g!(
    12.0,
    [
        [(2, 0), (2, 14), (7, 14), (9, 12), (9, 9), (7, 7), (2, 7)],
        [(7, 7), (9, 5), (9, 2), (7, 0), (2, 0)],
    ]
);
static UC: Glyph = g!(
    12.0,
    [[
        (10, 11),
        (7, 14),
        (4, 14),
        (2, 11),
        (2, 3),
        (4, 0),
        (7, 0),
        (10, 3)
    ],]
);
static UD: Glyph = g!(
    12.0,
    [[(2, 0), (2, 14), (6, 14), (9, 11), (9, 3), (6, 0), (2, 0)]]
);
static UE: Glyph = g!(11.0, [[(9, 14), (2, 14), (2, 0), (9, 0)], [(2, 7), (7, 7)]]);
static UF: Glyph = g!(11.0, [[(9, 14), (2, 14), (2, 0)], [(2, 7), (7, 7)]]);
static UG: Glyph = g!(
    13.0,
    [[
        (10, 11),
        (7, 14),
        (4, 14),
        (2, 11),
        (2, 3),
        (4, 0),
        (7, 0),
        (10, 3),
        (10, 6),
        (7, 6)
    ],]
);
static UH: Glyph = g!(
    12.0,
    [[(2, 0), (2, 14)], [(10, 0), (10, 14)], [(2, 7), (10, 7)]]
);
static UI: Glyph = g!(
    6.0,
    [[(1, 14), (5, 14)], [(3, 14), (3, 0)], [(1, 0), (5, 0)]]
);
static UJ: Glyph = g!(10.0, [[(8, 14), (8, 3), (6, 0), (3, 0), (1, 3)]]);
static UK: Glyph = g!(12.0, [[(2, 0), (2, 14)], [(10, 14), (2, 7), (10, 0)]]);
static UL: Glyph = g!(11.0, [[(2, 14), (2, 0), (9, 0)]]);
static UM: Glyph = g!(14.0, [[(2, 0), (2, 14), (6, 7), (10, 14), (10, 0)]]);
static UN: Glyph = g!(13.0, [[(2, 0), (2, 14), (10, 0), (10, 14)]]);
static UO: Glyph = g!(
    13.0,
    [[
        (4, 14),
        (2, 11),
        (2, 3),
        (4, 0),
        (8, 0),
        (10, 3),
        (10, 11),
        (8, 14),
        (4, 14)
    ],]
);
static UP: Glyph = g!(
    12.0,
    [[(2, 0), (2, 14), (7, 14), (9, 12), (9, 9), (7, 7), (2, 7)]]
);
static UQ: Glyph = g!(
    13.0,
    [
        [
            (4, 14),
            (2, 11),
            (2, 3),
            (4, 0),
            (8, 0),
            (10, 3),
            (10, 11),
            (8, 14),
            (4, 14)
        ],
        [(7, 4), (11, -1)],
    ]
);
static UR: Glyph = g!(
    12.0,
    [
        [(2, 0), (2, 14), (7, 14), (9, 12), (9, 9), (7, 7), (2, 7)],
        [(6, 7), (10, 0)],
    ]
);
static US: Glyph = g!(
    12.0,
    [[
        (10, 12),
        (7, 14),
        (4, 14),
        (2, 12),
        (2, 9),
        (4, 7),
        (8, 7),
        (10, 5),
        (10, 2),
        (8, 0),
        (5, 0),
        (2, 2)
    ],]
);
static UT: Glyph = g!(11.0, [[(1, 14), (10, 14)], [(5, 14), (5, 0)]]);
static UU: Glyph = g!(13.0, [[(2, 14), (2, 3), (4, 0), (8, 0), (10, 3), (10, 14)]]);
static UV: Glyph = g!(12.0, [[(1, 14), (6, 0), (11, 14)]]);
static UW: Glyph = g!(16.0, [[(1, 14), (4, 0), (8, 10), (12, 0), (15, 14)]]);
static UX: Glyph = g!(12.0, [[(2, 0), (10, 14)], [(10, 0), (2, 14)]]);
static UY: Glyph = g!(12.0, [[(1, 14), (6, 7), (11, 14)], [(6, 7), (6, 0)]]);
static UZ: Glyph = g!(12.0, [[(2, 14), (10, 14), (2, 0), (10, 0)]]);

// ---- lowercase -------------------------------------------------------------

static LA: Glyph = g!(
    11.0,
    [[
        (9, 0),
        (9, 9),
        (4, 9),
        (2, 7),
        (2, 2),
        (4, 0),
        (7, 0),
        (9, 2)
    ],]
);
static LB: Glyph = g!(
    11.0,
    [
        [(2, 14), (2, 0)],
        [
            (2, 2),
            (4, 0),
            (7, 0),
            (9, 2),
            (9, 7),
            (7, 9),
            (4, 9),
            (2, 7)
        ]
    ]
);
static LC: Glyph = g!(
    10.0,
    [[
        (9, 7),
        (7, 9),
        (4, 9),
        (2, 7),
        (2, 2),
        (4, 0),
        (7, 0),
        (9, 2)
    ]]
);
static LD: Glyph = g!(
    11.0,
    [
        [(9, 14), (9, 0)],
        [
            (9, 7),
            (7, 9),
            (4, 9),
            (2, 7),
            (2, 2),
            (4, 0),
            (7, 0),
            (9, 2)
        ]
    ]
);
static LE: Glyph = g!(
    11.0,
    [[
        (2, 5),
        (9, 5),
        (9, 7),
        (7, 9),
        (4, 9),
        (2, 7),
        (2, 2),
        (4, 0),
        (7, 0),
        (9, 2)
    ]]
);
static LF: Glyph = g!(7.0, [[(6, 14), (4, 14), (3, 12), (3, 0)], [(1, 9), (6, 9)]]);
static LG: Glyph = g!(
    11.0,
    [
        [(9, 9), (9, -2), (7, -4), (4, -4), (2, -2)],
        [
            (9, 7),
            (7, 9),
            (4, 9),
            (2, 7),
            (2, 2),
            (4, 0),
            (7, 0),
            (9, 2)
        ],
    ]
);
static LH: Glyph = g!(
    11.0,
    [[(2, 14), (2, 0)], [(2, 7), (4, 9), (7, 9), (9, 7), (9, 0)]]
);
static LI: Glyph = g!(4.0, [[(2, 13), (2, 12)], [(2, 9), (2, 0)]]);
static LJ: Glyph = g!(5.0, [[(3, 13), (3, 12)], [(3, 9), (3, -2), (1, -4)]]);
static LK: Glyph = g!(
    10.0,
    [[(2, 14), (2, 0)], [(8, 9), (2, 4)], [(4, 6), (9, 0)]]
);
static LL: Glyph = g!(4.0, [[(2, 14), (2, 0)]]);
static LM: Glyph = g!(
    15.0,
    [
        [(2, 0), (2, 9)],
        [(2, 7), (4, 9), (6, 7), (6, 0)],
        [(6, 7), (8, 9), (11, 9), (13, 7), (13, 0)],
    ]
);
static LN: Glyph = g!(
    11.0,
    [[(2, 0), (2, 9)], [(2, 7), (4, 9), (7, 9), (9, 7), (9, 0)]]
);
static LO: Glyph = g!(
    11.0,
    [[
        (4, 9),
        (2, 7),
        (2, 2),
        (4, 0),
        (7, 0),
        (9, 2),
        (9, 7),
        (7, 9),
        (4, 9)
    ]]
);
static LP: Glyph = g!(
    11.0,
    [
        [(2, -4), (2, 9)],
        [
            (2, 7),
            (4, 9),
            (7, 9),
            (9, 7),
            (9, 2),
            (7, 0),
            (4, 0),
            (2, 2)
        ]
    ]
);
static LQ: Glyph = g!(
    11.0,
    [
        [(9, -4), (9, 9)],
        [
            (9, 7),
            (7, 9),
            (4, 9),
            (2, 7),
            (2, 2),
            (4, 0),
            (7, 0),
            (9, 2)
        ]
    ]
);
static LR: Glyph = g!(8.0, [[(2, 0), (2, 9)], [(2, 6), (4, 9), (7, 9)]]);
static LS: Glyph = g!(
    10.0,
    [[
        (9, 7),
        (6, 9),
        (3, 9),
        (2, 7),
        (3, 5),
        (8, 4),
        (9, 2),
        (7, 0),
        (4, 0),
        (1, 2)
    ]]
);
static LT_: Glyph = g!(7.0, [[(3, 14), (3, 2), (5, 0)], [(1, 9), (6, 9)]]);
static LU: Glyph = g!(
    11.0,
    [[(2, 9), (2, 2), (4, 0), (7, 0), (9, 2)], [(9, 9), (9, 0)]]
);
static LV: Glyph = g!(10.0, [[(1, 9), (5, 0), (9, 9)]]);
static LW: Glyph = g!(14.0, [[(1, 9), (4, 0), (7, 7), (10, 0), (13, 9)]]);
static LX: Glyph = g!(10.0, [[(2, 9), (9, 0)], [(9, 9), (2, 0)]]);
static LY: Glyph = g!(
    10.0,
    [
        [(2, 9), (2, 2), (4, 0), (8, 0)],
        [(9, 9), (9, -2), (7, -4), (4, -4)]
    ]
);
static LZ: Glyph = g!(10.0, [[(2, 9), (9, 9), (2, 0), (9, 0)]]);
