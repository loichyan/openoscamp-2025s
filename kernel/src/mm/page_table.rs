use crate::config::*;
use bitflags::bitflags;

pub type RawPageTable = [PageTableEntry; PT_LENGTH];

bitflags! {
    pub struct PteFlags: u8 {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct PageTableEntry {
    pub i: usize,
}

impl PageTableEntry {
    pub const fn new(addr: usize, flags: PteFlags) -> Self {
        assert!(addr.trailing_zeros() >= 12, "address unaligned");
        Self {
            i: ((addr >> PAGE_WIDTH) << 10) | flags.bits() as usize,
        }
    }

    pub const fn empty() -> Self {
        Self { i: 0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PageLocation {
    /// Upper page directory.
    pub pt3: usize,
    /// Middle page directory.
    pub pt2: usize,
    /// Page directory.
    pub pt1: usize,
}

impl PageLocation {
    pub const fn new(mut addr: usize) -> Self {
        const PT_MASK: usize = !(usize::MAX << PD_WIDTH);
        Self {
            pt1: {
                addr >>= PAGE_WIDTH;
                addr & PT_MASK
            },
            pt2: {
                addr >>= PD_WIDTH;
                addr & PT_MASK
            },
            pt3: {
                addr >>= PD_WIDTH;
                addr & PT_MASK
            },
        }
    }
}

macro_rules! define_addr {
    ($(#[doc = $doc:literal])+
     $(#[$attr:meta])*
     $vis:vis struct $name:ident {
        $ivis:vis $i:ident: $itype:ty,
     }) => {
        $(#[doc = $doc])*
        $(#[$attr])*
        $vis struct $name {
            $ivis $i: $itype,
        }
        $(#[doc = $doc])*
        #[allow(non_snake_case)]
        $vis const fn $name($i: $itype) -> $name {
            $name { $i }
        }
    };
}
