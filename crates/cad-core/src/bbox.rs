// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Axis-aligned bounding boxes in 2D and 3D.

use crate::math::{Vec2, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct Aabb2 {
    pub min: Vec2,
    pub max: Vec2,
}

impl Aabb2 {
    pub fn empty() -> Self {
        Aabb2 {
            min: Vec2::new(f64::INFINITY, f64::INFINITY),
            max: Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub fn expand(&mut self, p: Vec2) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }

    pub fn width(&self) -> f64 {
        (self.max.x - self.min.x).max(0.0)
    }

    pub fn height(&self) -> f64 {
        (self.max.y - self.min.y).max(0.0)
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// Whether two boxes overlap, allowing a small negative `slack` to treat a
    /// bare touch as non-overlapping (broad-phase for interference tests).
    pub fn intersects(&self, o: &Aabb2, slack: f64) -> bool {
        self.min.x <= o.max.x - slack
            && self.max.x >= o.min.x + slack
            && self.min.y <= o.max.y - slack
            && self.max.y >= o.min.y + slack
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb3 {
    pub fn empty() -> Self {
        Aabb3 {
            min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub fn expand(&mut self, p: Vec3) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.min.z = self.min.z.min(p.z);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
        self.max.z = self.max.z.max(p.z);
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Whether two boxes overlap, allowing a small negative `slack` to treat a
    /// bare touch as non-overlapping (broad-phase for interference tests).
    pub fn intersects(&self, o: &Aabb3, slack: f64) -> bool {
        self.min.x <= o.max.x - slack
            && self.max.x >= o.min.x + slack
            && self.min.y <= o.max.y - slack
            && self.max.y >= o.min.y + slack
            && self.min.z <= o.max.z - slack
            && self.max.z >= o.min.z + slack
    }
}
