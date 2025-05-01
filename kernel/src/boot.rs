use crate::asm::*;
use crate::config::*;
use crate::mm::{PageLocation, PageTableEntry, PteFlags, RawPageTable};
use core::arch::naked_asm;

#[unsafe(link_section = ".data.pagetable")]
static mut BOOT_PAGE_TABLE: RawPageTable = [PageTableEntry::empty(); PT_LENGTH];

#[unsafe(link_section = ".bss.stack")]
static mut BOOT_STACK: [u8; KERNEL_BOOT_STACK_SIZE] = [0; KERNEL_BOOT_STACK_SIZE];

#[unsafe(link_section = ".text.entry")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn _start() -> ! {
    // RWX permission is dangerous, but we will switch to a more safe page table
    // for the kernel space after booting.
    const PTE_VRMX: PteFlags = PteFlags::V
        .union(PteFlags::R)
        .union(PteFlags::W)
        .union(PteFlags::X);

    /// `BOOT_PAGE_TABLE[$i] = $v`
    macro_rules! set_kpgtbl {
        ($i:ident, $v:ident) => {
            concat_asm!(
                "lla t0, {kpgtbl}",
                concat!("li t1, {", stringify!($i), "}*{pte_size}"), // Convert to byte index
                "add t0, t0, t1",
                concat!("li t1, {", stringify!($v), "}"),
                save!(t1, t0[0]),
            )
        };
    }

    naked_asm!(
        // Create a minimal boot page table to start the kernel using
        // handwritten position-independent-code (PIE).
        //
        // Identically map the physical memory region so that we can move to the
        // next instruction after activating the MMU.
        set_kpgtbl!(pm_vpn, pm_pte),
        // Map the physical memory region to the kernel memory region so that we
        // can access the kernel in the VMS.
        set_kpgtbl!(vm_vpn, pm_pte),
        // Activate the MMU (SV39)
        "
        lla  t0, {kpgtbl}
        srli t0, t0, {page_width}
        li   t1, {sv39_flag}
        or   t0, t0, t1
        csrw satp, t0
        sfence.vma
        ",
        // Set the kernel stack
        "
        lla sp, {kstack}
        li  t1, {kstack_size}
        add sp, sp, t1        # Move to the stack top
        li  t1, {koffset}
        add sp, sp, t1        # Relocate to the VMS
        ",
        // Call the entrypoint
        "
        lla t0, {rust_entry}
        li  t1, {koffset}
        add t0, t0, t1        # Relocate to the VMS
        jr  t0
        ",
        kpgtbl      = sym   BOOT_PAGE_TABLE,
        pte_size    = const core::mem::size_of::<PageTableEntry>(),
        sv39_flag   = const 8usize << 60,
        page_width  = const PAGE_WIDTH,

        pm_vpn      = const PageLocation::new(PADDR_START).pt3,
        vm_vpn      = const PageLocation::new(PADDR_START + KADDR_OFFSET).pt3,
        pm_pte      = const PageTableEntry::new(PADDR_START, PTE_VRMX).i,

        koffset     = const KADDR_OFFSET,
        kstack      = sym   BOOT_STACK,
        kstack_size = const KERNEL_BOOT_STACK_SIZE,

        rust_entry  = sym   rust_entry,
    );
}

#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn rust_entry() -> ! {
    unsafe extern "C" {
        fn sbss();
        fn ebss();
    }
    // Clear the .bss segment
    unsafe {
        let sbss = sbss as usize;
        let ebss = ebss as usize;
        core::slice::from_raw_parts_mut::<u8>(sbss as *mut u8, ebss - sbss).fill(0);
    }
    crate::main();
}
