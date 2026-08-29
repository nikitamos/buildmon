use std::fmt::Display;

#[derive(Copy, Clone, Debug)]
pub enum MemUnit {
    B(f64),
    KiB(f64),
    MiB(f64),
    GiB(f64),
    TiB(f64),
}

impl MemUnit {
    pub fn from_bytes(bytes: usize) -> Self {
        use MemUnit::*;
        const KIB: usize = 1024;
        const MIB: usize = KIB * 1024;
        const GIB: usize = MIB * 1024;
        const TIB: usize = GIB * 1024;

        let b = bytes as f64;
        match bytes {
            0..KIB => B(b),
            KIB..MIB => KiB(b / KIB as f64),
            MIB..GIB => MiB(b / MIB as f64),
            GIB..TIB => GiB(b / GIB as f64),
            _ => TiB(b / TIB as f64),
        }
    }
}

impl Display for MemUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (val, unit) = match self {
            MemUnit::B(b) => (b, "b"),
            MemUnit::KiB(k) => (k, "K"),
            MemUnit::MiB(m) => (m, "M"),
            MemUnit::GiB(g) => (g, "G"),
            MemUnit::TiB(t) => (t, "T"),
        };
        write!(f, "{val:.2}{unit}",)
    }
}

#[deprecated]
pub fn convert_mem_units(bytes: usize) -> MemUnit {
    MemUnit::from_bytes(bytes)
}

#[derive(Debug, Default, Clone)]
pub struct Time {
    pub ms: u16,
    pub s: u16,
    pub m: u16,
    pub h: u16,
    pub d: u64,
}

impl Time {
    pub fn from_ms(ms: u64) -> Self {
        let s = ms / 1000;
        let m = s / 60;
        let h = m / 60;
        let d = h / 24;
        Self {
            ms: (ms % 1000) as u16,
            s: (s % 60) as u16,
            m: (m % 60) as u16,
            h: (h % 24) as u16,
            d,
        }
    }
}

impl Display for Time {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.d == 0 {
            if self.h == 0 {
                if self.m == 0 {
                    write!(f, "{}.{:03}s", self.s, self.ms)
                } else {
                    write!(f, "{}m{:02}s", self.m, self.s)
                }
            } else {
                write!(f, "{}h{:02}m", self.h, self.m)
            }
        } else {
            write!(f, "{}d{}", self.d, self.h)
        }
    }
}
