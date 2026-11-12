// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Parametric "coded skills": named generators that build geometry from a
//! handful of parameters. This is the toolkit's extensible design library.
//!
//! # Extending
//!
//! Implement [`Skill`] for your generator and register it in
//! [`SkillRegistry::with_builtins`] (or call [`SkillRegistry::register`] at
//! runtime). Script-based skills — parametric `.cad` files dropped in the user
//! skills directory — are handled by the interpreter and need no code.

use crate::args::Args;
use crate::error::Result;
use crate::scene::Scene;

pub mod craft;
pub mod finger_box;
pub mod gear;
pub mod hardware;
pub mod honeycomb;
pub mod maker;
pub mod mechanical;

/// A parametric generator. `run` mutates the scene and returns a short summary
/// (typically the created entity name) for display.
pub trait Skill {
    fn name(&self) -> &'static str;
    fn summary(&self) -> &'static str;
    /// One-line usage string, e.g. `gear teeth=<n> module=<mm> [thickness=<mm>]`.
    fn usage(&self) -> &'static str;
    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String>;
}

/// Registry of available Rust skills.
pub struct SkillRegistry {
    skills: Vec<Box<dyn Skill>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        SkillRegistry { skills: Vec::new() }
    }

    /// Registry preloaded with every built-in skill.
    pub fn with_builtins() -> Self {
        let mut r = SkillRegistry::new();
        r.register(Box::new(gear::GearSkill));
        r.register(Box::new(finger_box::FingerBoxSkill));
        r.register(Box::new(honeycomb::HoneycombSkill));
        r.register(Box::new(hardware::WasherSkill));
        r.register(Box::new(hardware::StandoffSkill));
        // ---- Phase 5 library ----
        r.register(Box::new(mechanical::FlangeSkill));
        r.register(Box::new(mechanical::SprocketSkill));
        r.register(Box::new(mechanical::RackSkill));
        r.register(Box::new(mechanical::PulleySkill));
        r.register(Box::new(mechanical::CamSkill));
        r.register(Box::new(maker::GridfinitySkill));
        r.register(Box::new(maker::EnclosureSkill));
        r.register(Box::new(maker::StandoffGridSkill));
        r.register(Box::new(maker::KnobSkill));
        r.register(Box::new(maker::CableClipSkill));
        r.register(Box::new(craft::MonogramSkill));
        r.register(Box::new(craft::MandalaSkill));
        r.register(Box::new(craft::CardSkill));
        r.register(Box::new(craft::BoxTemplateSkill));
        r.register(Box::new(craft::LivingHingeSkill));
        r
    }

    pub fn register(&mut self, skill: Box<dyn Skill>) {
        self.skills.push(skill);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Skill> {
        self.skills
            .iter()
            .find(|s| s.name() == name)
            .map(|b| b.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Skill> {
        self.skills.iter().map(|b| b.as_ref())
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        SkillRegistry::with_builtins()
    }
}
