use crate::config::*;
use crate::mm::{PageLocation, PageTableEntry, PteFlags};
use core::arch::naked_asm;

#[unsafe(link_section = ".data.pagetable")]
static mut BOOT_PAGE_TABLE: [usize; PT_LENGTH] = [0; PT_LENGTH];

#[unsafe(link_section = ".bss.stack")]
static mut KERNEL_STACK: [u8; KERNEL_STACK_SIZE] = [0; KERNEL_STACK_SIZE];

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
    const SV39_FLAG: usize = 8 << 60;

    const PM_VPN: usize = PageLocation::new(PADDR_START).pt3;
    const VM_VPN: usize = PageLocation::new(PADDR_START + KADDR_OFFSET).pt3;
    const PM_PTE: usize = PageTableEntry::new(PADDR_START, PTE_VRMX).i;

    naked_asm!(
        "
        // Create a minimal boot page table to start the kernel using
        // handwritten position-independent-code (PIE).

        // Identically map the physical memory region so that we can move to the
        // next instruction after activating the MMU.
        li   t0, {pm_vpn}
        li   t1, {pm_pte}
        call {set_boot_pt}

        // Map the physical memory region to the kernel memory region so that we
        // can access the kernel in the VMS.
        li   t0, {vm_vpn}
        li   t1, {pm_pte}
        call {set_boot_pt}

        // Activate the MMU (SV39)
        lla  t0, {boot_pt}
        srli t0, t0, {page_width}
        li   t1, {sv39_flag}
        or   t0, t0, t1
        csrw satp, t0
        sfence.vma

        // Set the kernel stack
        lla sp, {kstack}
        li  t1, {kstack_size}
        add sp, sp, t1        // Move to the stack top
        li  t1, {koffset}
        add sp, sp, t1        // Relocate to the VMS

        // Call the entrypoint
        lla t0, {rust_entry}
        li  t1, {koffset}
        add t0, t0, t1        // Relocate to the VMS
        jr  t0
        ",
        boot_pt     = sym   BOOT_PAGE_TABLE,
        set_boot_pt = sym   set_boot_page_table,
        sv39_flag   = const SV39_FLAG,
        page_width  = const PAGE_WIDTH,

        pm_vpn      = const PM_VPN,
        vm_vpn      = const VM_VPN,
        pm_pte      = const PM_PTE,

        koffset     = const KADDR_OFFSET,
        kstack      = sym   KERNEL_STACK,
        kstack_size = const KERNEL_STACK_SIZE,

        rust_entry  = sym   rust_entry,
    );
}

/// `BOOT_PAGE_TABLE[%t0] = %t1`
#[unsafe(link_section = ".text.entry")]
#[unsafe(naked)]
unsafe extern "C" fn set_boot_page_table() {
    naked_asm!(
        "
        li  t3, {pte_size}
        mul t0, t0, t3     // Convert to byte index
        lla t3, {boot_pt}
        add t3, t3, t0
        sd  t1, (t3)       // Set a huge page of 1GB
        ret
        ",
        boot_pt  = sym   BOOT_PAGE_TABLE,
        pte_size = const core::mem::size_of::<PageTableEntry>(),
    );
}

#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn rust_entry() -> ! {
    // Clear the .bss segment
    unsafe {
        unsafe extern "C" {
            fn sbss();
            fn ebss();
        }
        let sbss = sbss as usize;
        let ebss = ebss as usize;
        core::slice::from_raw_parts_mut::<u8>(sbss as *mut u8, ebss - sbss).fill(0);
    }
    crate::main();
}
