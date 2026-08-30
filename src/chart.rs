use std::{iter::Sum, ops::Add};

use crate::{
    classifier::{Tool, ToolCategory},
    proc::SystemInfo,
};

#[derive(Debug, Default, Clone)]
pub struct MemUsageByCategory {
    pub link: usize,
    pub compile: usize,
    pub build: usize,
    pub other: usize,
}

impl MemUsageByCategory {
    pub fn of_tool(value: usize, tool: &Tool) -> Self {
        let mut res = Self::default();
        match ToolCategory::of_tool(tool) {
            ToolCategory::Compiler => res.compile = value,
            ToolCategory::Linker => res.link = value,
            ToolCategory::Other => res.other = value,
            ToolCategory::Build => res.build = value,
        }
        res
    }
}

impl Sum for MemUsageByCategory {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Default::default(), |x, y| x + y)
    }
}

impl Add for MemUsageByCategory {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            build: self.build + rhs.build,
            compile: self.compile + rhs.compile,
            link: self.link + rhs.link,
            other: self.other + rhs.other,
        }
    }
}

pub const MAX_POINTS: usize = 32;
const DATA_ROW_COUNT: usize = 4;
type DataRow = [(f64, f64); MAX_POINTS];

#[derive(Debug, Default)]
pub struct MemChartDataset {
    link: DataRow,
    compile: DataRow,
    build: DataRow,
    other: DataRow,
    size: usize,
    next_offset: usize,
}

impl MemChartDataset {
    pub fn new_boxed() -> Box<Self> {
        Default::default()
    }
    pub fn add_point(&mut self, point: MemUsageByCategory, x: f64) {
        let points = [
            (x, point.link as f64),
            (x, point.compile as f64),
            (x, point.build as f64),
            (x, point.other as f64),
        ];
        if self.max_size() == self.next_offset {
            self.for_each(points, |slice, pt| {
                slice.copy_within(1.., 0);
                *slice.last_mut().unwrap() = pt;
            });
        } else {
            let offset = self.next_offset;
            self.for_each(points, move |slice, pt| slice[offset] = pt);
            self.next_offset += 1;
        }
    }
    #[must_use]
    pub fn link(&self) -> &[(f64, f64)] {
        &self.link[..self.len()]
    }
    #[must_use]
    pub fn compile(&self) -> &[(f64, f64)] {
        &self.compile[..self.len()]
    }
    #[must_use]
    pub fn build(&self) -> &[(f64, f64)] {
        &self.build[..self.len()]
    }
    #[must_use]
    pub fn other(&self) -> &[(f64, f64)] {
        &self.other[..self.len()]
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.next_offset
    }

    pub fn x_bounds(&self) -> [f64; 2] {
        [self.compile().first().copied().unwrap_or_default().0, self.compile.last().copied().unwrap_or_default().0]
    }
    pub fn y_bounds(&self) -> [f64; 2] {
        [0.0, SystemInfo::get().total_ram() as f64]
    }
    /// This function guarantees that the data rows are traversed in the following order:
    /// 1. `link`
    /// 2. `compile`
    /// 3. `build`
    /// 4. `other`
    #[inline(always)]
    fn for_each<T>(&mut self, args: [T; DATA_ROW_COUNT], mut f: impl FnMut(&mut DataRow, T)) {
        let rows = [
            &mut self.link,
            &mut self.compile,
            &mut self.build,
            &mut self.other,
        ];
        for (s, arg) in rows.into_iter().zip(args) {
            f(s, arg);
        }
    }
    fn max_size(&self) -> usize {
        MAX_POINTS
    }
}
