/// Number of entries in a page table.
pub const PT_LENGTH: usize = 512;
/// Width of a page directory.
pub const PD_WIDTH: usize = 9;
/// Width of the page offset.
pub const PAGE_WIDTH: usize = 12;

pub const VADDR_WIDTH: usize = 39;
/// Offset of the kernel memory region in virtual memory space.
pub const KADDR_OFFSET: usize = 0xFFFF_FFC0_0000_0000;

/// Start of the physical memory region.
pub const PADDR_START: usize = 0x8000_0000;
/// End of the physical memory region (8MB in total).
pub const PADDR_END: usize = 0x8080_0000;

pub const KERNEL_STACK_SIZE: usize = 4096 * 2; // 8KB
pub const USER_STACK_SIZE: usize = 4096 * 2; // 8KB
