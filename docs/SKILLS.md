# Caliper — Skill library

**Skills** are named parametric generators: one line builds a complete part from
a handful of dimensions. Invoke with `skill <name> [key=value ...]`; every
parameter has a sensible default, so a bare `skill <name>` always produces
something. Run `skills` in the REPL for this list with live usage, and add your
own as `.cad` files in `~/.caliper/skills/` (no code needed).

All dimensions are in the document's current units (`units mm|cm|in`). 2D skills
emit cut/score/engrave on separate layers (see [DESIGN](DESIGN.md)); mechanical
skills with a `thickness=` become solids.

```
skill gridfinity units_x=3 units_y=2 name=bin
render bin mode=hlr           # preview as a solid
export stl bin.stl name=bin   # or: export svg plate.svg  (2D)
```

## Foundation (v0.1)

| Skill | What it makes | Usage |
|---|---|---|
| `gear` | Involute spur gear (2D or solid) | `gear teeth=<n> module=<mm> [pressure=20] [bore=] [thickness=]` |
| `boxjoint` | Finger-jointed box panels, laid flat | `boxjoint width= depth= height= thickness= [finger=] [lid] [gap=10]` |
| `honeycomb` | Plate with a hex-grid perforation | `honeycomb width= height= [cell=10] [wall=2] [margin=5]` |
| `washer` | Flat annulus, or a solid tube | `washer outer= inner= [thickness=] [seg=64]` |
| `standoff` | Bored cylindrical spacer (3D) | `standoff outer= bore= height= [seg=48]` |

## Mechanical (accurate profiles)

| Skill | What it makes | Usage |
|---|---|---|
| `flange` | Bolt-circle flange disc (2D or solid) | `flange [outer=60] [bore=20] [bolt_circle=45] [bolt_dia=5] [bolts=4] [thickness=0]` |
| `sprocket` | ANSI roller-chain sprocket (seating+topping tooth form) | `sprocket [teeth=17] [pitch=12.7] [roller=7.95] [bore=10] [thickness=0]` |
| `rack` | Spur-gear rack, trapezoidal teeth | `rack [teeth=10] [module=2] [pressure=20] [height=10] [thickness=0]` |
| `pulley` | Timing-belt pulley (GT2 / HTD groove) | `pulley [teeth=20] [belt=GT2\|HTD3M\|HTD5M] [bore=5] [thickness=0]` |
| `cam` | Radial disc cam (harmonic / cycloidal) | `cam [base=20] [lift=10] [motion=harmonic\|cycloidal] [rise_deg=120] [dwell_deg=60] [bore=6]` |

## Maker / 3D

| Skill | What it makes | Usage |
|---|---|---|
| `gridfinity` | Gridfinity bin (42 mm grid, closed manifold) | `gridfinity [units_x=2] [units_y=1] [height_u=3]` |
| `enclosure` | Two-part project box (base + optional lid/standoffs) | `enclosure [width=80] [depth=60] [height=30] [wall=2] [lid] [standoffs]` |
| `standoff_grid` | Grid of bored standoff tubes (+ optional base) | `standoff_grid [nx=3] [ny=2] [pitch=20] [outer=6] [bore=3] [height=10] [base]` |
| `knob` | Fluted control knob with an axial/D-shaft bore | `knob [diameter=30] [height=15] [flutes=12] [bore=6] [d_flat=0]` |
| `cable_clip` | Snap-in C-clip with a screw-down foot | `cable_clip [cable=6] [thickness=3] [depth=8] [screw=3]` |

## Craft / laser 2D

| Skill | What it makes | Usage |
|---|---|---|
| `monogram` | Single-stroke initials in a cut frame | `monogram [text=AB] [size=30] [frame=circle\|rect\|none]` |
| `mandala` | Polar petal motif for laser/vinyl | `mandala [petals=12] [rings=2] [r=40]` |
| `card` | Folding card net (cut outline + score fold) | `card [width=100] [height=70] [fold=left\|top] [window=0]` |
| `box_template` | Flat tuck-box net (cut + fold scores) | `box_template [width=60] [depth=40] [height=30] [tab=10]` |
| `living_hinge` | Laser living-hinge panel (staggered flex slots) | `living_hinge [width=80] [height=40] [slot=15] [gap=3] [rows=0]` |

## Previews

`skill mandala petals=10 rings=3` then `render`:

```
⠀⠀⠀⠀⠀⠀⢀⣀⠤⠤⡀⠀⠀⠀⠀⠈⠉⡹⢄⡀⠀⠀⠀⡠⠼⡊⠉⠀⠀⠀⠀⠀⡠⠤⢄⣀⠀
⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⠈⡆⠀⠀⠀⠀⡸⠀⠀⠈⢢⢠⠊⠀⠀⠸⡀⠀⠀⠀⠀⡎⠀⠀⠀⡸⠀
⠀⠀⠀⢀⠤⠊⠉⠢⣀⢀⡠⠊⠉⠢⢄⢀⡸⡊⣙⣖⣗⣿⣗⣝⡉⡺⡀⢀⠔⠊⠉⠢⡀⢀⠤⠊⠉
⠀⠀⠀⠑⠢⡀⠀⡠⠔⠉⠢⡀⠀⣀⠔⠉⢢⡊⢒⢞⣿⣟⣟⣖⠊⣢⠚⠑⠤⡀⠀⡠⠚⠑⠢⡀⠀
⠀⠀⠀⠀⠀⠀⠀⡠⠤⠤⠤⡼⠤⣀⡠⠜⢀⠔⠉⠑⡄⠀⡔⠉⠒⢄⠘⠤⣀⡠⠼⡤⠤⠤⠤⡀⠀
```

`skill gridfinity units_x=3 units_y=2` then `render mode=wire`:

```
⠀⠀⠀⠀⠀⠀⠀⢀⣀⡤⠴⠖⠛⠉⠉⠀⠀⠀⠀⠀⠀⠀⠀⣀⣀⣤⣴⠶⠲⢦⣄⡀⠀⠀⠀⠈⠛⠦⣄⡀
⠀⣀⡠⠤⠖⠒⠉⠉⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣤⣴⠶⠞⠛⠋⠉⠀⠀⠀⠀⠀⠈⠙⠲⣄⡀⠀⠀⠀⠈⢙⣳⡄
⠀⠉⠲⣤⣀⠀⠀⠀⠀⢀⣀⣤⣤⠶⠾⠛⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣭⠷⢶⡚⠋⠉⠀
⠀⠀⢀⣀⣩⣿⠶⣾⡛⠋⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣠⠤⠖⠚⠋⠉⠀⠀⠀⠀⠈⠓⢦⣀⡀
```
