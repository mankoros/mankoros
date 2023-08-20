.global mcount
    .section .text
mcount:
    li a0, 0xffffffc000000000
    bltu sp, a0, __do_nothing
    call mcount_impl
__do_nothing:
    ret

mcount_impl:
    # MCOUNT_CNT++
    la a1, MCOUNT_CNT
    ld a0, 0(a1)
    addi a0, a0, 1
    sd a0, 0(a1)
    ret

.data
MCOUNT_CNT:
    .word 0x0000000000000000