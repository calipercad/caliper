//! Minimal linear-algebra kernel: 2D/3D vectors and a 4x4 affine transform.
//!
//! Everything here is `f64` and pure std. It is deliberately small — just what
//! the geometry kernel, tessellator and renderer need.

use std::ops::{Add, Div, Mul, Neg, Sub};

pub const EPS: f64 = 1e-9;
pub const TAU: f64 = std::f64::consts::TAU;
pub const PI: f64 = std::f64::consts::PI;

/// Convert degrees to radians.
#[inline]
pub fn rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

/// Convert radians to degrees.
#[inline]
pub fn deg(rad: f64) -> f64 {
    rad * 180.0 / PI
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    #[inline]
    pub fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }

    #[inline]
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// 2D cross product (z-component of the 3D cross).
    #[inline]
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }

    #[inline]
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }

    #[inline]
    pub fn len_sq(self) -> f64 {
        self.dot(self)
    }

    #[inline]
    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l < EPS {
            Vec2::ZERO
        } else {
            self / l
        }
    }

    #[inline]
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }

    #[inline]
    pub fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }

    #[inline]
    pub fn to3(self) -> Vec3 {
        Vec3::new(self.x, self.y, 0.0)
    }

    /// Angle of the vector in radians, in (-pi, pi].
    #[inline]
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }

    #[inline]
    pub fn from_angle(a: f64, r: f64) -> Vec2 {
        Vec2::new(a.cos() * r, a.sin() * r)
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    #[inline]
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    #[inline]
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl Div<f64> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn div(self, s: f64) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    #[inline]
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    #[inline]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }

    #[inline]
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    #[inline]
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    #[inline]
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }

    #[inline]
    pub fn normalized(self) -> Vec3 {
        let l = self.len();
        if l < EPS {
            Vec3::ZERO
        } else {
            self / l
        }
    }

    #[inline]
    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    #[inline]
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    #[inline]
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Div<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn div(self, s: f64) -> Vec3 {
        Vec3::new(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    #[inline]
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// Column-major 4x4 matrix used for affine transforms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    /// `m[col][row]`
    pub m: [[f64; 4]; 4],
}

impl Mat4 {
    pub fn identity() -> Mat4 {
        let mut m = [[0.0; 4]; 4];
        for i in 0..4 {
            m[i][i] = 1.0;
        }
        Mat4 { m }
    }

    pub fn translation(t: Vec3) -> Mat4 {
        let mut r = Mat4::identity();
        r.m[3][0] = t.x;
        r.m[3][1] = t.y;
        r.m[3][2] = t.z;
        r
    }

    pub fn scale(s: Vec3) -> Mat4 {
        let mut r = Mat4::identity();
        r.m[0][0] = s.x;
        r.m[1][1] = s.y;
        r.m[2][2] = s.z;
        r
    }

    pub fn rotation_x(a: f64) -> Mat4 {
        let (s, c) = a.sin_cos();
        let mut r = Mat4::identity();
        r.m[1][1] = c;
        r.m[1][2] = s;
        r.m[2][1] = -s;
        r.m[2][2] = c;
        r
    }

    pub fn rotation_y(a: f64) -> Mat4 {
        let (s, c) = a.sin_cos();
        let mut r = Mat4::identity();
        r.m[0][0] = c;
        r.m[0][2] = -s;
        r.m[2][0] = s;
        r.m[2][2] = c;
        r
    }

    pub fn rotation_z(a: f64) -> Mat4 {
        let (s, c) = a.sin_cos();
        let mut r = Mat4::identity();
        r.m[0][0] = c;
        r.m[0][1] = s;
        r.m[1][0] = -s;
        r.m[1][1] = c;
        r
    }

    /// `self * other` (apply `other` first, then `self`).
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for c in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.m[k][row] * o.m[c][k];
                }
                r[c][row] = sum;
            }
        }
        Mat4 { m: r }
    }

    /// Transform a point (w = 1), applying translation.
    pub fn apply_point(&self, p: Vec3) -> Vec3 {
        let x = self.m[0][0] * p.x + self.m[1][0] * p.y + self.m[2][0] * p.z + self.m[3][0];
        let y = self.m[0][1] * p.x + self.m[1][1] * p.y + self.m[2][1] * p.z + self.m[3][1];
        let z = self.m[0][2] * p.x + self.m[1][2] * p.y + self.m[2][2] * p.z + self.m[3][2];
        Vec3::new(x, y, z)
    }

    /// Transform a direction (w = 0), ignoring translation.
    pub fn apply_dir(&self, v: Vec3) -> Vec3 {
        let x = self.m[0][0] * v.x + self.m[1][0] * v.y + self.m[2][0] * v.z;
        let y = self.m[0][1] * v.x + self.m[1][1] * v.y + self.m[2][1] * v.z;
        let z = self.m[0][2] * v.x + self.m[1][2] * v.y + self.m[2][2] * v.z;
        Vec3::new(x, y, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec2_ops() {
        let a = Vec2::new(3.0, 4.0);
        assert!((a.len() - 5.0).abs() < EPS);
        assert!((a.normalized().len() - 1.0).abs() < EPS);
        assert!((Vec2::new(1.0, 0.0).cross(Vec2::new(0.0, 1.0)) - 1.0).abs() < EPS);
    }

    #[test]
    fn mat_translate_rotate() {
        let t = Mat4::translation(Vec3::new(1.0, 2.0, 3.0));
        let p = t.apply_point(Vec3::ZERO);
        assert_eq!(p, Vec3::new(1.0, 2.0, 3.0));

        let r = Mat4::rotation_z(rad(90.0));
        let p = r.apply_point(Vec3::new(1.0, 0.0, 0.0));
        assert!((p.x).abs() < 1e-9 && (p.y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn mat_compose() {
        let t = Mat4::translation(Vec3::new(10.0, 0.0, 0.0));
        let r = Mat4::rotation_z(rad(90.0));
        // translate then rotate: rotate(translate(p))
        let m = r.mul(&t);
        let p = m.apply_point(Vec3::ZERO);
        assert!((p.x).abs() < 1e-9 && (p.y - 10.0).abs() < 1e-9);
    }
}
