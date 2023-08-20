.section .rodata
    .global __embed_dtb
    .type   __embed_dtb, @object

.balign 2097152
__embed_dtb:
    # .incbin "qemu.dtb"