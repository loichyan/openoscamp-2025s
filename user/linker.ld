OUTPUT_ARCH(riscv)
ENTRY(_start)
BASE_ADDRESS = 0x00400000;

SECTIONS {
    . = BASE_ADDRESS;
    sapp = .;

    .text : {
        stext = .;
        *(.text.entry)
        . = ALIGN(4K);
        etext = .;
    }

    .rodata : {
        srodata = .;
        *(.rodata .rodata.*)
        . = ALIGN(4K);
        erodata = .;
    }

    .data : {
        sdata = .;
        *(.data .data.*)
        . = ALIGN(4K);
        edata = .;
    }

    .bss : {
        sbss = .;
        *(.bss .bss.*)
        . = ALIGN(4K);
        ebss = .;
    }

    eapp = .;

    /DISCARD/ : {
        *(.comment) *(.eh_frame)
    }
}
