# Phase 1 Implementation Blueprint — Robust 2D Geometry (v0.2)

> This is the **frozen coordination artifact** for the Phase 1 build. Multiple
> agents implement disjoint files against the API contract in §2. Do not deviate
> from the signatures in §2 — they are what makes the independently-written files
> link and compile. If a signature seems wrong, implement it as written and note
> the concern in your return; do not silently change it.

## 0. Decision (unanimous across 3 judge panels)

Build the boolean core as a **snap-rounded planar-overlay clipper with EXACT
integer (i128) topology**, winding-classified output loops, and keyhole-bridged
holed extrusion. Rationale: exact `i128` orientation/intersection removes all
float fragility from the combinatorial core (the classic source of boolean
bugs); the arrangement-of-atomic-edges model handles coincident/collinear/
overlapping edges and even self-intersecting input **uniformly**; it is the
lowest-LOC and most deterministic (byte-stable) of the candidates.

**Grafts** the judges required, folded into the contract below:

- **G1 (from the Vatti design):** an **independent geometric containment
  classifier** (`classify_loops`) is the *authoritative* source of truth for
  outer-vs-hole. The tracer's per-loop sign is only a hint, `debug_assert!`-ed to
  agree with containment. A nesting bug degrades to correct-but-slower, never
  wrong.
- **G2 (from the Vatti design):** an explicit **self-intersection / edge-balance
  guard**. If the kept directed-edge set is not balanced (every lattice vertex
  has equal in/out degree) or a trace cannot close, return
  `CadError::Geometry("non-manifold or self-intersecting boolean input")` — the
  roadmap's conservative fallback — rather than emitting junk.
- **G3:** drop any traced loop whose **signed-area sign disagrees with its
  depth parity** (only possible on degenerate slivers).
- **G4 (from the GH design):** thorough **manifold + Euler-characteristic +
  signed-volume** verification on extruded meshes; `transform()` clears the
  primitive hint on everything except pure translation / uniform scale.

## 1. File assignments (disjoint — safe to write in parallel)

| Agent | File(s) | Sections |
|---|---|---|
| A | `crates/cad-core/src/geom2d.rs` (full rewrite) | §2 geom2d, §5 winding, §7 fillet/chamfer, §8 exporter tags |
| B | `crates/cad-core/src/geom2d/boolean.rs` (new) | §2 boolean, §4 algorithm, §5 winding |
| C | `crates/cad-core/src/geom3d.rs` (full rewrite) | §2 geom3d, §6 holed extrude, §6 manifold |
| D | `crates/cad-core/src/export/dxf.rs` + `export/svg.rs` (rewrites) | §8 exporters |
| E | `crates/cad-core/src/script/mod.rs` (targeted edits) + `script/reference.txt` (edits) | §2 interpreter, §9 grammar |
| F | `crates/cad-core/tests/phase1.rs` (new) | §10 test plan |

Preserve every existing item not mentioned here. Each file may define its own
private point-in-polygon helper; do **not** depend on another file's private
helpers. Do not run `cargo` (sibling files land in parallel — the crate only
compiles once all are written; integration/fix is done by the orchestrator).

## 2. Frozen API contract

### `geom2d.rs`

```rust
// at the top of the file:
pub mod boolean;

#[derive(Clone, Debug)]
pub enum ContourKind {
    Polyline,
    Circle { c: Vec2, r: f64 },
    Arc { c: Vec2, r: f64, a0: f64, a1: f64 }, // a0,a1 in DEGREES
}

#[derive(Clone, Debug)]
pub struct Contour {
    pub points: Vec<Vec2>,
    pub closed: bool,
    pub kind: ContourKind,
}

impl Contour {
    pub fn new(points: Vec<Vec2>, closed: bool) -> Self;   // SIGNATURE UNCHANGED; sets kind = Polyline
    pub fn with_kind(points: Vec<Vec2>, closed: bool, kind: ContourKind) -> Self;
    pub fn signed_area(&self) -> f64;                      // keep existing shoelace
    pub fn area(&self) -> f64;                             // keep
    pub fn length(&self) -> f64;                           // keep
    pub fn ensure_ccw(&mut self);                          // keep
    pub fn is_ccw(&self) -> bool;                          // signed_area() > 0.0
    pub fn reversed(&self) -> Contour;                     // points reversed, kind = Polyline, same closed
    pub fn fillet(&self, r: f64, seg: usize) -> Contour;   // §7; kind = Polyline
    pub fn chamfer(&self, d: f64) -> Contour;              // §7; kind = Polyline
}

pub struct Region<'a> {
    pub outer: &'a Contour,
    pub holes: Vec<&'a Contour>,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub contours: Vec<Contour>,
}

impl Shape {
    // KEEP existing: new, from_contour, bbox, transform (SEE below), center_origin,
    //   rect, circle (SEE below), polygon, star, polyline, arc (SEE below), add_hole
    pub fn area(&self) -> f64;                 // REWRITE §5: clone+normalize, sum(+CCW areas) - sum(CW areas), .max(0.0)
    pub fn normalize_windings(&mut self);      // §5: solids CCW, holes CW, by nesting-depth parity
    pub fn regions(&self) -> Vec<Region<'_>>;  // §5: each hole under smallest-area containing shell
    pub fn is_empty(&self) -> bool;            // contours empty, OR every contour has < 3 points
    pub fn union(&self, o: &Shape) -> crate::error::Result<Shape>;         // delegate to boolean::union
    pub fn difference(&self, o: &Shape) -> crate::error::Result<Shape>;    // boolean::difference
    pub fn intersection(&self, o: &Shape) -> crate::error::Result<Shape>;  // boolean::intersection
    pub fn xor(&self, o: &Shape) -> crate::error::Result<Shape>;           // boolean::xor
    pub fn fillet(&self, r: f64, seg: usize) -> Shape;  // map Contour::fillet over contours, preserve winding
    pub fn chamfer(&self, d: f64) -> Shape;             // map Contour::chamfer over contours
}
```

- `Shape::circle(r, seg)` stamps `kind = Circle { c: Vec2::ZERO, r }` on its contour.
- `Shape::arc(r, start_deg, end_deg, seg)` stamps `kind = Arc { c: Vec2::ZERO, r, a0: start_deg, a1: end_deg }`.
- `Shape::rect`/`polygon`/`star`/`polyline` keep `kind = Polyline`.
- **`Shape::transform`** (kind-preservation rule, G4): after applying `m` to the
  points, decide each contour's new kind: if `m`'s linear 3×3 part is (within EPS)
  a pure translation → keep Circle/Arc, updating `c` by the translation; else if it
  is a uniform scale `s*I` (optionally with translation) → keep, scaling `r` by `s`
  and mapping `c`; **otherwise set `kind = Polyline`.** When unsure, downgrade to
  Polyline (conservative). A robust simple impl: read the transformed center
  `m.apply_point(c.to3())` for the new `c`, derive `s` from the transformed length
  of a unit X vector (`m.apply_dir(Vec3::X).xy().len()`), and keep the primitive
  only if the transform preserves shape (uniform scale, no shear/rotation for arcs;
  full circles tolerate rotation). Simplicity beats cleverness: it is acceptable to
  keep primitives only under pure translation + uniform scale and Polyline
  otherwise.

### `geom2d/boolean.rs` (new)

```rust
use crate::geom2d::Shape;
use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp { Union, Difference, Intersection, Xor }

pub fn boolean(a: &Shape, b: &Shape, op: BoolOp) -> Result<Shape>;
pub fn union(a: &Shape, b: &Shape) -> Result<Shape>;         // boolean(a,b,Union)
pub fn difference(a: &Shape, b: &Shape) -> Result<Shape>;    // boolean(a,b,Difference)  (a - b)
pub fn intersection(a: &Shape, b: &Shape) -> Result<Shape>;  // boolean(a,b,Intersection)
pub fn xor(a: &Shape, b: &Shape) -> Result<Shape>;           // boolean(a,b,Xor)
// private: IPt{x:i64,y:i64}, orient (i128), segment_intersection (i128),
//          winding_nonzero(p: Vec2, loops: &[&[Vec2]]) -> bool
```

### `geom3d.rs`

```rust
pub fn extrude(shape: &Shape, height: f64) -> Result<Mesh>;  // SIGNATURE UNCHANGED; body rewritten §6
pub fn triangulate_with_holes(outer: &[Vec2], holes: &[&[Vec2]]) -> Result<(Vec<Vec2>, Vec<[usize; 3]>)>; // §6
pub fn triangulate(pts: &[Vec2]) -> Result<Vec<[usize; 3]>>; // KEEP existing (reused by the merged-ring ear clip)
pub fn revolve(profile: &[Vec2], seg: usize) -> Result<Mesh>; // KEEP existing
impl Mesh {
    // KEEP all existing: new,bbox,transform,center_origin,append,face_normal,edges,
    //   feature_edges,triangle_count,cuboid,cylinder,cone,sphere
    pub fn is_closed_manifold(&self) -> bool;   // §6
    pub fn euler_characteristic(&self) -> i64;  // §6: V - E + F over snap-deduped verts / unique undirected edges
}
```

### `script/mod.rs` (interpreter)

Add these arms to the `match cmd` block **above** the io/render early-return
arms (so they flow through the existing `if result.is_ok() { history.push }`
rule — they are deterministic, unlike `ai`):

```rust
"union"     => self.op_bool(&args, BoolOp::Union),
"subtract"  => self.op_bool(&args, BoolOp::Difference),
"intersect" => self.op_bool(&args, BoolOp::Intersection),
"xor"       => self.op_bool(&args, BoolOp::Xor),
"fillet"    => self.op_fillet(&args),
"chamfer"   => self.op_chamfer(&args),
```

Add `use crate::geom2d::boolean::BoolOp;` to the imports. Implement:

- `op_bool(&mut self, args, op) -> Result<String>`: read two positional operand
  names `a`, `b`; resolve BOTH as `Geometry::Shape` (clone them out first to end
  the borrow, error `"boolean needs two 2D shapes"` on a mesh/missing); call
  `boolean(&a_shape, &b_shape, op)`; if `result.is_empty()` return
  `Err(CadError::Geometry("empty result".into()))`; else add a NEW entity named
  `args.name()` or, for subtract, `format!("{a}_sub_{b}")`, else `auto_name("bool")`.
  **Operands are NOT consumed** (non-destructive).
- `op_fillet(&mut self, args)`: `r = pos_f64(0)`, `seg = usize_or("seg",16)`,
  optional `target=`; `resolve_mut` the target; require `Geometry::Shape`
  (error on mesh); replace its geom with `shape.fillet(r, seg)`.
- `op_chamfer(&mut self, args)`: `d = pos_f64(0)`, optional `target=`; likewise
  with `shape.chamfer(d)`.

## 3. (reserved)

## 4. Boolean algorithm (agent B — verbatim design)

Core idea: build the planar arrangement of both shapes' edges on a shared
integer lattice, keep only the atomic edges that separate "inside result" from
"outside result", trace those into loops, and classify loops by nesting depth.
Everything topological is decided by EXACT integer arithmetic on the lattice;
only in/out membership consults the original float loops.

`boolean(a, b, op)`:

1. Normalize both inputs: clone, `normalize_windings()` so every CCW contour is
   a solid outer and every CW contour a hole. The "filled region" of a Shape is
   defined by the nonzero/even-odd rule over its loops (the arrangement
   re-derives it either way, self-healing self-intersections).
2. Pick a snap grid. `let ext = union_bbox(a,b).max_extent(); let grid = (ext * 1e-9).max(1e-9);`
   `inv = 1/grid`, origin = bbox.min. Snap every point:
   `IPt{ x: ((p.x-origin.x)*inv).round() as i64, y: ((p.y-origin.y)*inv).round() as i64 }`.
   Snapping collapses near-coincident vertices to identical lattice points,
   coincident edges become identical, tangencies become shared vertices.
   **Invariant to preserve for i128 safety:** `grid = ext*1e-9` bounds snapped
   integers to ~2^30, so `orient` (products ~2^61) and the intersection numerator
   (~2^91) fit in `i128`. Document this in a comment.
3. Gather all closed-loop edges from `a` and `b` as directed integer segments
   (skip zero-length after snap). Build the arrangement: for every unordered pair
   of edges compute intersections with exact `i128` orientation tests; collect,
   per edge, the set of lattice split points = its own endpoints ∪ every proper
   crossing ∪ every endpoint of the other shape that lies ON this edge (exact
   collinear + between test). Sort each edge's split points along the edge and
   emit consecutive "atomic" sub-edges. Collinear/overlapping edges are split at
   all shared endpoints, so overlaps reduce to byte-identical atomic edges. O(n²)
   pairwise — fine for our loop sizes (tens–hundreds of verts).
4. Dedup atomic edges by unordered endpoint pair. Drop zero-length.
5. Classify each atomic edge as a result-boundary edge. Its interior has NO
   crossing (that is the point of "atomic"), so its left and right faces each have
   constant membership. Take the float midpoint `m`, the unit edge normal `nrm`,
   and probe `pL = m + nrm*(grid*0.5)`, `pR = m - nrm*(grid*0.5)`. Evaluate
   `inA = winding_nonzero(pL, a_loops)`, `inB = winding_nonzero(pL, b_loops)` on the
   ORIGINAL float loops (ray-cast winding — immune to snap), and likewise for `pR`.
   Reduce with the op predicate:
   - union: `inA || inB`
   - intersection: `inA && inB`
   - difference (a−b): `inA && !inB`
   - xor: `inA ^ inB`
   Keep the edge iff `resultL != resultR`; orient it so the result-interior is on
   its LEFT (reverse if interior is on the right).
6. Trace loops. Kept directed edges form a balanced set (in-degree == out-degree
   at every lattice vertex — **G2: if not balanced, return the conservative
   `CadError::Geometry` above**). Repeatedly: pick an unused directed edge, then at
   each arrival vertex choose the next unused outgoing edge by rotational order —
   the one whose direction is the first clockwise turn from the reversed incoming
   direction (standard "peel one face at a time" rule for interior-on-left
   tracing, using exact `orient` for the angular comparison). Mark edges used;
   close the loop when back to start. If a loop cannot close, return the
   conservative error (G2).
7. Map lattice vertices back to float: keep a `HashMap<IPt,Vec2>` seeded from
   original input points (so exact input coords survive) and filled with
   `origin + ipt*grid` for computed intersections. Build a `Contour` per loop,
   dropping loops with `|signed_area| < grid*grid` (slivers).
8. Classify loops into outer/hole (§5) and pack all resulting contours (CCW
   outers + CW holes) into ONE `Shape`. Empty result → `Shape { contours: vec![] }`
   (`is_empty()` true); the command layer turns that into a graceful error.

Intersection detail (step 3): `segment_intersection` works on `i64` endpoints,
computing denominators/numerators in `i128`. If parallel (`den == 0`): if
collinear, resolve the overlap purely by adding each segment's endpoints as split
points on the other (no new vertex). If crossing, compute the rational
intersection and round to the nearest lattice point deterministically; because
both edges are split at that same rounded point, the arrangement stays consistent
(edges meet exactly at a shared `IPt`).

`winding_nonzero(p, loops)`: standard ray-cast / signed-crossing winding number on
`f64` loops; `> 0` (nonzero rule) ⇒ inside. Use the same rule for both operands.

## 5. Winding classification (agents A + B)

Two-stage, exact-then-nesting.

- **Stage 1 (falls out of tracing, agent B):** loops traced "result-interior on
  the left" get the correct sign automatically — a solid shell comes out CCW
  (`signed_area > 0`), a hole CW. So `signed_area().signum()` is a cheap first
  classification.
- **Stage 2 (authoritative — G1):** for each loop `L` pick a strictly-interior
  test point (just inside one edge, offset by a small epsilon along the inward
  normal). Its depth = number of OTHER loops that strictly contain it (ray-cast
  point-in-polygon, made robust by the offset so the point is off every boundary).
  Even depth (0,2,…) ⇒ solid shell; odd depth ⇒ hole. This even-odd nesting rule
  handles arbitrarily nested results (an island inside a bore).
- **Cross-check (G3):** for well-formed output Stage-1 sign and Stage-2 parity
  agree; if they disagree (degenerate sliver) **drop the loop**. In debug builds,
  `debug_assert!` agreement before dropping.

`Shape::normalize_windings()` (agent A): classify every contour by Stage-2
nesting parity against the other contours, then force solids CCW and holes CW
(reverse points whose current sign disagrees with their class). Legacy skill
shapes (CCW outer + CCW holes) are thereby repaired: a hole at depth 1 is
reclassified and flipped to CW — this is what makes existing gear/washer bores
into true through-holes on extrude.

`Shape::regions()` (agent A): sort shells (CCW contours) by ascending `|area|`;
assign each hole (CW contour) to the smallest-area shell whose polygon contains
the hole's interior test point. A hole with no containing shell is dropped (with a
`debug_assert!`). Each shell + its holes is one `Region`. Nested solids become
their own `Region`.

## 6. Holed extrusion + manifold check (agent C)

Rewrite `geom3d::extrude` around `Shape::regions()` and hole-bridging, and fix
the existing **weld bug** (today the caps build a SEPARATE vertex ring from the
walls, so the mesh is non-manifold even for a plain rect).

`extrude(shape, height)`:

1. `let mut s = shape.clone(); s.normalize_windings();` (solids CCW, holes CW).
   Error if `s.is_empty()` or the outer has < 3 points (keep current messages).
2. For each `Region { outer, holes }` in `s.regions()`:
   a. `let (ring, tris2d) = triangulate_with_holes(&outer.points, &holes_slices)?;`
      — `ring` is a flat `Vec<Vec2>` = outer verts followed by each hole's verts
      (in the CW order normalize produced); `tris2d` are CCW index triples into
      `ring`. Bridges exist only as triangulation topology, NOT as extra vertices.
   b. Allocate ONE bottom ring and ONE top ring from `ring` — the SHARED vertex
      set both caps and walls index into (the weld fix):
      `let b0 = m.verts.len();` push all `ring` pts at `z=0`;
      `let t0 = m.verts.len();` push all `ring` pts at `z=height`.
   c. Caps from `tris2d`: bottom face (−Z) reversed winding `[b0+i0, b0+i2, b0+i1]`;
      top face (+Z) `[t0+i0, t0+i1, t0+i2]`.
   d. Side walls, driven by the ORIGINAL loop boundaries (outer, then each hole),
      each a contiguous span `[start,len)` inside `ring`: for `i in 0..len`,
      `ni=(i+1)%len`, quads `[b0+start+i, b0+start+ni, t0+start+ni]` and
      `[b0+start+i, t0+start+ni, t0+start+i]`. Because outer is CCW this yields
      OUTWARD normals; because holes are CW the SAME formula yields INWARD normals
      — a bore's wall automatically faces into the hole. No per-loop special-casing.

`triangulate_with_holes(outer, holes)`: classic ear-clipping-with-holes bridge
(Eberly). Sort holes by descending rightmost-x. For each hole, take its rightmost
vertex `M`, cast a ray +X to the nearest outer edge intersection, pick the
mutually-visible outer vertex `P` (the intersection edge's endpoint with largest
x, or, if a reflex vertex blocks, the reflex vertex inside triangle `(M, hit, P)`
with smallest angle to +X). Splice the hole loop into the working polygon via the
two-way bridge `M<->P` (duplicate `M` and `P` around the inserted hole run),
producing ONE simple CCW polygon. After merging all holes, ear-clip the merged
index list and remap ears back to `ring` indices. Bridges appear as interior
diagonals used by two triangles and cancel, so the cap is watertight. Keep the
existing `triangulate` fan fallback reachable so it can never hang/panic.

`Mesh::is_closed_manifold()`: build a `HashMap<(usize,usize), i32>` over DIRECTED
edges `(a,b),(b,c),(c,a)` of every triangle. Closed orientable 2-manifold iff for
every undirected pair `{u,v}` the directed edge `(u,v)` occurs exactly once AND
its reverse `(v,u)` occurs exactly once. (Dedup verts by rounding to a grid so the
shared ring vertices count once.)

`Mesh::euler_characteristic()`: `V - E + F` where `V` = unique vertices
(snap-dedup by rounding to ~1e-6 of the extent), `E` = unique undirected edges,
`F` = tris. For a closed manifold this equals `2 - 2·genus`. Tests derive genus
and assert it equals the number of bored through-holes (0 for a plain solid, 1 for
one bore, 3 for three bores). Normal orientation is verified by summing signed
tetra volumes `Σ a·(b×c)/6 > 0` ⇒ all faces wound outward.

## 7. Fillet / chamfer (agent A, on `Contour`)

Per closed contour, operate on straight-segment joints; produce a new
`Contour` (`kind = Polyline`).

For vertex `v` with prev `p`, next `q`: `u = (v-p).normalized()`,
`w = (q-v).normalized()`. Skip near-collinear joints (`|cross(u,w)| < eps`) —
this leaves a finely-tessellated circle untouched.

- **FILLET(r, seg):** interior half-angle `phi = (PI - theta)/2` where `theta` is
  the turn angle between `(v-p)` and `(q-v)`. Setback `t = r / tan(phi)`, clamped
  `t = t.min(0.5*|v-p|).min(0.5*|v-q|)` (never cross a corner); recompute effective
  `r` from clamped `t`. Tangent points `a = v - u*t`, `b = v + w*t`; arc center
  `= a + n_a*r` where `n_a` is the inward normal of edge `(p->v)` toward the
  corner interior (sign from `cross(u,w)`). Emit the arc polyline (`seg` steps)
  from center→a to center→b in place of `v`. Convex and reflex corners both handled
  by the interior-side sign.
- **CHAMFER(d):** replace `v` with two points `a = v - u*d`, `b = v + w*d`
  (straight bevel), same clamp `d <= 0.5*min(edge lengths)`.

Open contours leave their two endpoints unmodified; only interior joints are
processed. `Shape::fillet`/`chamfer` map over every contour, preserving winding.

## 8. Exporter fidelity (agents A + D)

`ContourKind` records when tessellated points came from a known primitive; the
`points` remain the single source of truth for render/extrude/boolean. Any
point-mutating op (boolean, fillet, chamfer, non-rigid transform) resets
`kind = Polyline` (§2 transform rule).

- **DXF** (`dxf.rs`), per contour `match &c.kind`:
  - `Circle { c, r }` → `push(0,"CIRCLE"); (8,"0"); (10, c.x*mm); (20, c.y*mm); (40, r*mm)`.
  - `Arc { c, r, a0, a1 }` → `push(0,"ARC"); (8,"0"); (10,c.x*mm);(20,c.y*mm);(40,r*mm);(50,a0);(51,a1)`
    — DXF arcs are CCW start→end; normalize so `a1 > a0` (add 360 if needed).
  - `Polyline` → existing `LWPOLYLINE` path.
- **SVG** (`svg.rs`), per contour `match &c.kind`:
  - `Circle { c, r }` → `<circle cx=.. cy=.. r=.. fill="none" stroke="black" stroke-width="0.2"/>`
    using the same y-flip map for the center and `r*mm` for radius.
  - `Arc` / `Polyline` → existing `<path>` (arcs stay polylines — avoids the
    y-flip sweep-flag bug; conservative per the judges).

Keep the `<g id=...>` grouping and the existing header/viewBox math in SVG, and
the HEADER/ENTITIES framing in DXF, exactly as they are.

## 9. Grammar additions (agent E — append to `reference.txt` OPERATIONS)

```
  union <a> <b> [name=]                            a ∪ b  (2 shapes -> new shape)
  subtract <a> <b> [name=]                         a − b
  intersect <a> <b> [name=]                        a ∩ b
  xor <a> <b> [name=]                              a ⊕ b
  fillet <r> [seg=16] [target=]                    round contour corners
  chamfer <d> [target=]                            bevel contour corners
```

## 10. Test plan (agent F — `crates/cad-core/tests/phase1.rs`)

Use the public API (`cad_core::geom2d::{Shape, Contour, ...}`,
`cad_core::geom2d::boolean::{union, difference, intersection, xor}` and/or the
`Shape` methods, `cad_core::geom3d::{extrude}`, `Mesh` methods). For the
checkpoint end-to-end test, drive `cad_core::script::Interpreter` and pull the
resulting `Geometry::Mesh` from the scene. Tolerances ±1e-6 unless noted.

- `boolean_truth_table`: A = square `[0,2]²`, B = square `[1,3]²`.
  `union.area()==7`, `intersection.area()==1`, `difference(A,B).area()==3`,
  `xor.area()==6`. (Build squares via `Shape::polyline` of 4 CCW corners or
  `Shape::rect` translated.)
- `subtract_circle_from_rect`: rect 40×40 centered, minus circle r8 centered ⇒
  one Shape, `regions().len()==1` with exactly 1 hole, `area ≈ 1600 - π·64`.
- `coincident_and_shared_edge`: union of two identical rects == that rect (1
  loop, same area); union of two rects sharing one full edge ⇒ single merged rect.
- `disjoint_split`: subtract a central bar spanning a rect ⇒ 2 disjoint shells;
  `regions().len()==2`.
- `hole_is_manifold` (**checkpoint**): rect 40 − circle 8, `extrude 5` ⇒
  `mesh.is_closed_manifold()==true` AND `euler_characteristic()==0` (genus 1); all
  face normals outward (signed volume > 0).
- `multi_hole_genus`: rect with 3 circular bores, extrude ⇒ `euler_characteristic()==-4` (genus 3).
- `extrude_weld_regression`: plain rect extrude ⇒ `is_closed_manifold()==true`
  (guards the fixed cap/wall weld bug).
- `fillet_square`: 40×40 square, `fillet(5, 8)` ⇒ 4 rounded corners
  (`4*(seg+1)` verts), each arc point within 1e-6 of `r` from the computed center;
  `area ≈ square − 4·(1−π/4)·r²`.
- `chamfer_square` ⇒ 8 vertices, each corner a straight bevel of the requested leg.
- `fillet_clamp`: fillet `r` larger than half the shortest edge ⇒ `t` clamped,
  result simple (`signed_area > 0`, no self-intersection).
- `fillet_skips_smooth`: fillet on a 64-seg circle leaves it (near-)unchanged.
- `exporter_primitives`: circle ⇒ DXF contains `"CIRCLE"` and radius group; SVG
  contains `"<circle"`; arc ⇒ DXF `"ARC"` with `50`/`51`; a boolean (Polyline)
  result still emits `LWPOLYLINE` / `<path>`.
- `empty_result_graceful`: intersection of two disjoint shapes, and
  `difference(A,A)` ⇒ `is_empty()`; the `intersect`/`subtract` command returns a
  clean error (no panic, no junk file).
- `determinism`: running the same boolean twice yields identical contour point
  lists.
- `inclusion_exclusion` (property): over a handful of overlapping rects/circles,
  `union.area() + intersection.area() ≈ a.area() + b.area()`.
