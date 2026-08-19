use std::fmt::{Display, write};

use crossterm::style::{ContentStyle, StyledContent, Stylize};

use crate::classifier::Tool::Heresy;

use super::*;

pub enum ProcessKind {
    Compiler,
    Linker,
    Tool,
    Build,
}

#[derive(Debug, Clone)]
pub enum Tool<'a> {
    /// A C compiler
    CC,
    /// A C++ compiler
    CXX,
    Rust,
    /// Zig compiler itself (NOT linker and NOT `zig build`)
    Zig,

    /// Any linker (ld, lld, mold, zig link, etc.)
    Linker,

    /// Any shell (sh, bash, zsh, etc.)
    Shell,

    CMake,
    Ninja,
    Make,
    Cargo,
    /// `zig build` process
    ZigBuild,

    Heresy(Cow<'a, str>),
}

impl<'a> Display for Tool<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tool::CC => write!(f, "   CC"),
            Tool::CXX => write!(f, "  CXX"),
            Tool::Rust => write!(f, "RUSTC"),
            Tool::Zig => write!(f, "  ZIG"),
            Tool::Linker => write!(f, " LINK"),
            Tool::CMake => write!(f, "CMAKE"),
            Tool::Ninja => write!(f, "NINJA"),
            Tool::Make => write!(f, " MAKE"),
            Tool::Cargo => write!(f, "CARGO"),
            Tool::ZigBuild => write!(f, "ZIGBD"),
            Tool::Shell => write!(f, "SHELL"),
            Heresy(_) => write!(f, "OTHER"),
        }
    }
}

impl<'a> Stylize for Tool<'a> {
    type Styled = StyledContent<Self>;

    fn stylize(self) -> Self::Styled {
        StyledContent::new(ContentStyle::default(), self)
    }
}

pub struct ProcessClassification<'proc> {
    pub tool: Tool<'proc>,
    pub artifact: Option<&'proc str>,
}

pub trait ProcessClassifier {
    fn classify<'proc>(
        &self,
        proc: &'proc CompilationProcess,
    ) -> Result<ProcessClassification<'proc>, ProcessError>;
}

pub struct ClassifierImpl;

impl ProcessClassifier for ClassifierImpl {
    fn classify<'proc>(
        &self,
        proc: &'proc CompilationProcess,
    ) -> Result<ProcessClassification<'proc>, ProcessError> {
        let basename = proc.executable_basename().unwrap_or_default();
        let tool = match basename.as_ref() {
            "clang++" | "g++" | "c++" | "cc1plus" => Tool::CXX, // TODO:
            "clang" | "gcc" | "cc" => Tool::CC,
            "rustc" => Tool::Rust,
            "zig" => Tool::Zig,
            "cmake" => Tool::CMake,
            "ninja" => Tool::Ninja,
            "make" => Tool::Make,
            "cargo" => Tool::Cargo,
            "sh" | "bash" | "csh" | "zsh" | "fish" => Tool::Shell,
            "ld" | "lld" | "ld.lld" | "mold" | "gold" => Tool::Linker,
            _ => Heresy(basename),
        };
        Ok(ProcessClassification {
            tool,
            artifact: None,
        })
    }
}
