#![allow(dead_code)]

use std::{
    borrow::Cow,
    path::Path,
};


#[inline(always)]
pub(crate) fn minmax<T: Ord>(a: T, b: T) -> (T, T) {
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => (b, a),
        _ => (a, b),
    }
}

#[derive(Debug)]
pub enum ProcessError {
    Dead,
    UnknownClass,
    Io(std::io::Error),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<std::io::Error> for ProcessError {
    fn from(value: std::io::Error) -> Self {
        ProcessError::Io(value)
    }
}
impl std::error::Error for ProcessError {}

pub type Result<T, E = ProcessError> = std::result::Result<T, E>;

pub mod classifier;
pub mod component;
pub mod std_streams;
pub mod tree;

pub use proc::CompilationProcess;
pub use proc::ProcStat;

pub mod proc;
