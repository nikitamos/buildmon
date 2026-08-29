use std::fmt::Display;

use crossterm::style::{ContentStyle, StyledContent, Stylize};

use crate::classifier::Tool::Heresy;

use super::*;

#[derive(Debug, Clone)]
pub enum ToolCategory {
    Compiler,
    Linker,
    Other,
    Build,
}

impl ToolCategory {
    pub fn of_tool(tool: &Tool) -> Self {
        match tool {
            Tool::CC | Tool::CXX | Tool::Rust | Tool::Zig | Tool::Asm => ToolCategory::Compiler,
            Tool::Linker => ToolCategory::Linker,
            Tool::CMake | Tool::Ninja | Tool::Make | Tool::Cargo | Tool::ZigBuild => {
                ToolCategory::Build
            }
            Tool::Shell | Heresy(_) => ToolCategory::Other,
        }
    }
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
    /// Assembler
    Asm,

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

impl<'a> Tool<'a> {
    pub fn short_repr(&self) -> &'a str {
        match self {
            Tool::CC => "CC   ",
            Tool::CXX => "CXX  ",
            Tool::Rust => "RUSTC",
            Tool::Zig => "ZIG  ",
            Tool::Linker => "LINK ",
            Tool::CMake => "CMAKE",
            Tool::Ninja => "NINJA",
            Tool::Make => "MAKE ",
            Tool::Cargo => "CARGO",
            Tool::ZigBuild => "ZIGBD",
            Tool::Shell => "SHELL",
            Tool::Asm => "ASM  ",
            Heresy(_) => "OTHER",
        }
    }
}

impl<'a> Display for Tool<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short_repr())
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

pub fn classify_tool<'p>(proc: &'p CompilationProcess) -> ProcessClassification<'p> {
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
        "ld" | "lld" | "ld.lld" | "mold" | "gold" | "collect2" | "rust-lld" => Tool::Linker,
        "as" | "llvm-as" => Tool::Asm,
        _ => Heresy(basename),
    };

    // For now we will assume, that path to the output artifact
    // is given after -o, as it is a rather common convention
    let mut iter = proc.args();
    let artifact = loop {
        if let Some(arg) = iter.next() {
            if arg == "-o" {
                break iter
                    .next()
                    .and_then(|p| Path::new(p).file_name().map(|s| s.to_str().unwrap_or(p)));
            }
        } else {
            break None;
        }
    };

    ProcessClassification { tool, artifact }
}
