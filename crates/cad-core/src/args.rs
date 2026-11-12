// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Argument parsing shared by script commands and skills.
//!
//! Tokens are split into *positional* values and `key=value` *options*. Helpers
//! coerce to numbers with clear errors so skills stay terse.

use crate::error::{CadError, Result};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub positional: Vec<String>,
    pub options: HashMap<String, String>,
}

impl Args {
    /// Parse already-tokenized arguments (quotes handled by the lexer).
    pub fn from_tokens(tokens: &[String]) -> Args {
        let mut a = Args::default();
        for t in tokens {
            if let Some((k, v)) = t.split_once('=') {
                a.options.insert(k.to_string(), v.to_string());
            } else {
                a.positional.push(t.clone());
            }
        }
        a
    }

    fn parse_num(key: &str, raw: &str) -> Result<f64> {
        raw.parse::<f64>()
            .map_err(|_| CadError::Arg(format!("`{key}`='{raw}' is not a number")))
    }

    /// Required option as f64.
    pub fn f64(&self, key: &str) -> Result<f64> {
        let raw = self
            .options
            .get(key)
            .ok_or_else(|| CadError::Arg(format!("missing required `{key}=`")))?;
        Self::parse_num(key, raw)
    }

    /// Optional option as f64 with a default.
    pub fn f64_or(&self, key: &str, default: f64) -> Result<f64> {
        match self.options.get(key) {
            Some(raw) => Self::parse_num(key, raw),
            None => Ok(default),
        }
    }

    pub fn usize_or(&self, key: &str, default: usize) -> Result<usize> {
        match self.options.get(key) {
            Some(raw) => raw
                .parse::<usize>()
                .map_err(|_| CadError::Arg(format!("`{key}`='{raw}' is not a whole number"))),
            None => Ok(default),
        }
    }

    pub fn str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.options.get(key).map(|s| s.as_str()).unwrap_or(default)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(|s| s.as_str())
    }

    /// A boolean flag. True when written as a bare word (`closed`), or as
    /// `key=true/1/yes/on`. Bare flags arrive as positionals, so check both.
    pub fn flag(&self, key: &str) -> bool {
        match self.options.get(key) {
            Some(v) => matches!(v.as_str(), "" | "true" | "1" | "yes" | "on"),
            None => self.positional.iter().any(|p| p == key),
        }
    }

    /// Positional value as f64.
    pub fn pos_f64(&self, i: usize) -> Result<f64> {
        let raw = self
            .positional
            .get(i)
            .ok_or_else(|| CadError::Arg(format!("missing positional argument #{}", i + 1)))?;
        Self::parse_num(&format!("#{}", i + 1), raw)
    }

    pub fn pos_f64_or(&self, i: usize, default: f64) -> Result<f64> {
        match self.positional.get(i) {
            Some(raw) => Self::parse_num(&format!("#{}", i + 1), raw),
            None => Ok(default),
        }
    }

    pub fn pos_str(&self, i: usize) -> Option<&str> {
        self.positional.get(i).map(|s| s.as_str())
    }

    /// A `name=` option, falling back to a positional if given, else `None`.
    pub fn name(&self) -> Option<String> {
        self.options.get("name").cloned()
    }
}
