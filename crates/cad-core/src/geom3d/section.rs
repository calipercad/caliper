// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Planar cross-section of a mesh: slice a solid by an axis-aligned plane and
//! chain the cut segments into closed 2D contours (a [`Shape`]).
//!
//! Axis-aligned planes keep the projection to 2D trivial and the arithmetic
//! robust. Each triangle crossing the plane contributes one segment (the classic
//! marching-triangles cut, using the crossing formula from the CSG splitter);
//! segment endpoints are welded on a grid and walked into rings. A clean slice of
//! a closed manifold yields closed loops; a dangling endpoint (an open or
//! non-manifold cut) is reported as an error rather than a broken contour.

use super::Mesh;
use crate::error::{CadError, Result};
use crate::geom2d::{Contour, Shape};
use crate::math::{Vec2, Vec3};
use std::collections::HashMap;

/// The slicing plane's normal axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn parse(s: &str) -> Option<Axis> {
        match s.to_lowercase().as_str() {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "z" => Some(Axis::Z),
            _ => None,
        }
    }
    /// Coordinate of `p` along this axis.
    fn coord(&self, p: Vec3) -> f64 {
        match self {
            Axis::X => p.x,
            Axis::Y => p.y,
            Axis::Z => p.z,
        }
    }
    /// Project `p` to the 2D plane coordinates (dropping this axis).
    fn project(&self, p: Vec3) -> Vec2 {
        match self {
            Axis::X => Vec2::new(p.y, p.z),
            Axis::Y => Vec2::new(p.x, p.z),
            Axis::Z => Vec2::new(p.x, p.y),
        }
    }
}

/// Slice `mesh` by the plane `axis = at`, returning the outline as a [`Shape`].
pub fn section(mesh: &Mesh, axis: Axis, at: f64) -> Result<Shape> {
    // Collect cut segments (pairs of 2D endpoints) from spanning triangles.
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for t in &mesh.tris {
        let v = [mesh.verts[t[0]], mesh.verts[t[1]], mesh.verts[t[2]]];
        let d = [
            axis.coord(v[0]) - at,
            axis.coord(v[1]) - at,
            axis.coord(v[2]) - at,
        ];
        // Marching convention: treat "on the plane" as the + side so each
        // spanning triangle yields exactly two edge crossings.
        let above = [d[0] >= 0.0, d[1] >= 0.0, d[2] >= 0.0];
        if above[0] == above[1] && above[1] == above[2] {
            continue; // entirely on one side
        }
        let mut hits: Vec<Vec2> = Vec::new();
        for e in 0..3 {
            let (i, j) = (e, (e + 1) % 3);
            if above[i] != above[j] {
                let t = d[i] / (d[i] - d[j]);
                hits.push(axis.project(v[i].lerp(v[j], t)));
            }
        }
        if hits.len() == 2 {
            segs.push((hits[0], hits[1]));
        }
    }
    if segs.is_empty() {
        return Err(CadError::Geometry(format!(
            "section plane {axis:?}={at} does not cross the mesh"
        )));
    }

    chain(segs)
}

/// Weld segment endpoints and walk them into closed contours.
fn chain(segs: Vec<(Vec2, Vec2)>) -> Result<Shape> {
    // Weld tolerance relative to the section's extent.
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (a, b) in &segs {
        for p in [a, b] {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    let ext = (hi.x - lo.x).max(hi.y - lo.y).max(1.0);
    let grid = ext * 1e-7;
    let key =
        |p: Vec2| -> (i64, i64) { ((p.x / grid).round() as i64, (p.y / grid).round() as i64) };

    // Weld to unique points; build an adjacency list of undirected edges.
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    let mut points: Vec<Vec2> = Vec::new();
    let mut adj: Vec<Vec<usize>> = Vec::new();
    let intern = |p: Vec2,
                  index: &mut HashMap<(i64, i64), usize>,
                  points: &mut Vec<Vec2>,
                  adj: &mut Vec<Vec<usize>>|
     -> usize {
        *index.entry(key(p)).or_insert_with(|| {
            points.push(p);
            adj.push(Vec::new());
            points.len() - 1
        })
    };
    for (a, b) in &segs {
        let ia = intern(*a, &mut index, &mut points, &mut adj);
        let ib = intern(*b, &mut index, &mut points, &mut adj);
        if ia != ib {
            adj[ia].push(ib);
            adj[ib].push(ia);
        }
    }

    // Walk rings: from an unused edge, always leave by an unused edge until we
    // return to the start. A clean manifold slice gives every node degree 2.
    // `used[i]` marks which of node i's adjacency slots are already traversed.
    let mut used: Vec<Vec<bool>> = adj.iter().map(|a| vec![false; a.len()]).collect();

    // Consume the undirected edge between `a` and `b` from both endpoints' slot
    // lists (first unused matching slot on each side).
    fn consume(used: &mut [Vec<bool>], adj: &[Vec<usize>], a: usize, b: usize) {
        for &(x, y) in &[(a, b), (b, a)] {
            if let Some(k) = (0..adj[x].len()).find(|&k| adj[x][k] == y && !used[x][k]) {
                used[x][k] = true;
            }
        }
    }

    let mut contours: Vec<Contour> = Vec::new();
    for start in 0..points.len() {
        while let Some(first) = (0..adj[start].len()).find(|&k| !used[start][k]) {
            let mut loop_pts: Vec<Vec2> = vec![points[start]];
            let mut cur = adj[start][first];
            consume(&mut used, &adj, start, cur);
            while cur != start {
                loop_pts.push(points[cur]);
                let nxt = match (0..adj[cur].len()).find(|&k| !used[cur][k]) {
                    Some(k) => adj[cur][k],
                    None => {
                        return Err(CadError::Geometry(
                            "section did not close (open or non-manifold cut)".into(),
                        ))
                    }
                };
                consume(&mut used, &adj, cur, nxt);
                cur = nxt;
            }
            if loop_pts.len() >= 3 {
                contours.push(Contour::new(loop_pts, true));
            }
        }
    }

    if contours.is_empty() {
        return Err(CadError::Geometry(
            "section produced no closed loops".into(),
        ));
    }
    let mut shape = Shape::new(contours);
    shape.normalize_windings();
    Ok(shape)
}
