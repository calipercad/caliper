//! The scene: a named collection of 2D and 3D entities plus document units.

use crate::bbox::{Aabb2, Aabb3};
use crate::error::{CadError, Result};
use crate::geom2d::Shape;
use crate::geom3d::Mesh;
use crate::math::Mat4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Mm,
    Cm,
    In,
}

impl Units {
    /// Human-readable suffix.
    pub fn suffix(&self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Cm => "cm",
            Units::In => "in",
        }
    }

    /// Millimetres per unit — used by exporters that must emit real-world sizes.
    pub fn to_mm(&self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Cm => 10.0,
            Units::In => 25.4,
        }
    }

    pub fn parse(s: &str) -> Option<Units> {
        match s.to_lowercase().as_str() {
            "mm" => Some(Units::Mm),
            "cm" => Some(Units::Cm),
            "in" | "inch" | "inches" => Some(Units::In),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Geometry {
    Shape(Shape),
    Mesh(Mesh),
}

impl Geometry {
    pub fn kind(&self) -> &'static str {
        match self {
            Geometry::Shape(_) => "2d",
            Geometry::Mesh(_) => "3d",
        }
    }

    pub fn transform(&mut self, m: &Mat4) {
        match self {
            Geometry::Shape(s) => s.transform(m),
            Geometry::Mesh(me) => me.transform(m),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub name: String,
    pub geom: Geometry,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub entities: Vec<Entity>,
    pub units: Units,
    /// Name of the most recently created/selected entity.
    pub selected: Option<String>,
    auto_counter: usize,
}

impl Scene {
    pub fn new() -> Self {
        Scene {
            entities: Vec::new(),
            units: Units::Mm,
            selected: None,
            auto_counter: 0,
        }
    }

    /// Generate a fresh auto name with the given prefix (e.g. `circle3`).
    pub fn auto_name(&mut self, prefix: &str) -> String {
        self.auto_counter += 1;
        format!("{prefix}{}", self.auto_counter)
    }

    /// Insert or replace an entity, marking it selected. Returns its name.
    pub fn add(&mut self, name: impl Into<String>, geom: Geometry) -> String {
        let name = name.into();
        if let Some(e) = self.entities.iter_mut().find(|e| e.name == name) {
            e.geom = geom;
        } else {
            self.entities.push(Entity {
                name: name.clone(),
                geom,
            });
        }
        self.selected = Some(name.clone());
        name
    }

    pub fn get(&self, name: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.name == name)
    }

    /// Resolve a target name, defaulting to the current selection.
    pub fn resolve<'a>(&'a self, target: Option<&'a str>) -> Result<&'a Entity> {
        let name = target
            .map(|s| s.to_string())
            .or_else(|| self.selected.clone())
            .ok_or_else(|| CadError::NotFound("no target and no selection".into()))?;
        self.get(&name)
            .ok_or_else(|| CadError::NotFound(name.clone()))
    }

    pub fn resolve_mut(&mut self, target: Option<&str>) -> Result<&mut Entity> {
        let name = target
            .map(|s| s.to_string())
            .or_else(|| self.selected.clone())
            .ok_or_else(|| CadError::NotFound("no target and no selection".into()))?;
        self.entities
            .iter_mut()
            .find(|e| e.name == name)
            .ok_or_else(|| CadError::NotFound(name))
    }

    pub fn delete(&mut self, name: &str) -> Result<()> {
        let before = self.entities.len();
        self.entities.retain(|e| e.name != name);
        if self.entities.len() == before {
            return Err(CadError::NotFound(name.into()));
        }
        if self.selected.as_deref() == Some(name) {
            self.selected = self.entities.last().map(|e| e.name.clone());
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.entities.clear();
        self.selected = None;
        self.auto_counter = 0;
    }

    /// Bounding box over all 2D entities.
    pub fn bbox2(&self) -> Aabb2 {
        let mut b = Aabb2::empty();
        for e in &self.entities {
            if let Geometry::Shape(s) = &e.geom {
                let sb = s.bbox();
                if !sb.is_empty() {
                    b.expand(sb.min);
                    b.expand(sb.max);
                }
            }
        }
        b
    }

    /// Bounding box over all 3D entities.
    pub fn bbox3(&self) -> Aabb3 {
        let mut b = Aabb3::empty();
        for e in &self.entities {
            if let Geometry::Mesh(m) = &e.geom {
                let mb = m.bbox();
                if !mb.is_empty() {
                    b.expand(mb.min);
                    b.expand(mb.max);
                }
            }
        }
        b
    }
}

impl Default for Scene {
    fn default() -> Self {
        Scene::new()
    }
}
