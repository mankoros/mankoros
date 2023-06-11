/* Linker file */

/* Apply memory mapping to adapt to riscv OpenSBI */

/* Architecture */
OUTPUT_ARCH(riscv)

/* Entry Point */
ENTRY(_start)

/* Kernel will run at this address once paging is enabled */
BASE_ADDRESS = 0xFFFFFFFF80000000;

SECTIONS
{
    . = BASE_ADDRESS;
    kernel_start = .;

    text_start = .;
    .text : {
        *(.text.entry)

        /* All text section off object file */
        *(.text .text.*)
    }
    text_end = .;
    . = ALIGN(4K);


    rodata_start = .;
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }
    . = ALIGN(4K);

    data_start = .;
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }
    . = ALIGN(4K);

    init_kstack_start = .;
    .bss : {
        *(.bss.stack)
        /* Actual BSS start after stack */
        bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }
    bss_end = .;

    . = ALIGN(4K);
    __eh_frame = .;
    .eh_frame : { 
        *(.eh_frame.*) 
    }
    .eh_frame_hdr : {
        *(.eh_frame_hdr.*)
    }
    . = ALIGN(2M);
    kernel_end = .;

    __eh_frame_hdr_start = ADDR(.eh_frame_hdr);
    __eh_frame_hdr_end = ADDR(.eh_frame_hdr) + SIZEOF(.eh_frame_hdr);
    __eh_frame_start = ADDR(.eh_frame);
    __eh_frame_end = ADDR(.eh_frame) + SIZEOF(.eh_frame);
}