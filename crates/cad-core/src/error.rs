// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Crate-wide error type.

use std::fmt;

#[derive(Debug)]
pub enum CadError {
    /// A command or skill received bad arguments.
    Arg(String),
    /// A named entity was not found in the scene.
    NotFound(String),
    /// Geometry could not be produced (e.g. degenerate input).
    Geometry(String),
    /// Parsing a script line failed.
    Parse(String),
    /// I/O failure while reading/writing files.
    Io(String),
    /// The AI bridge failed.
    Ai(String),
}

impl fmt::Display for CadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CadError::Arg(m) => write!(f, "argument error: {m}"),
            CadError::NotFound(m) => write!(f, "not found: {m}"),
            CadError::Geometry(m) => write!(f, "geometry error: {m}"),
            CadError::Parse(m) => write!(f, "parse error: {m}"),
            CadError::Io(m) => write!(f, "io error: {m}"),
            CadError::Ai(m) => write!(f, "ai error: {m}"),
        }
    }
}

impl std::error::Error for CadError {}

impl From<std::io::Error> for CadError {
    fn from(e: std::io::Error) -> Self {
        CadError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CadError>;
