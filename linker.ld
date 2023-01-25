/* Linker file */

/* Apply memory mapping to adapt to riscv OpenSBI */

/* Architecture */
OUTPUT_ARCH(riscv)

/* Entry Point */
ENTRY(_start)

/* OpenSBI transfer control to this address */
BASE_ADDRESS = 0x80200000;

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

    .bss : {
        *(.bss.stack)
        /* Actual BSS start after stack */
        bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }
    bss_end = .;

    . = ALIGN(4K);
    kernel_end = .;
}