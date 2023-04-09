    .section .text
    .global __kernel_to_user
    .global __user_trap_entry
    .global __kernel_trap_vector

# 常量：表示每个寄存器占的字节数，由于是64位，都是8字节
.equ XLENB, 8

// recover register from UKContext
.macro RECOVER_UKCX reg, offset_in_byte 
    ld \reg, \offset_in_byte * XLENB(a0)
.endm

// save register to UKContext
.macro SAVE_UKCX reg, offset_in_byte 
    sd \reg, \offset_in_byte * XLENB(a0)
.endm

__kernel_to_user:
    // a0: *mut UKContext

    // save all kernel register to kernel_xx in UKContext
    // kernel_sx
    SAVE_UKCX s0, 34
    SAVE_UKCX s1, 35
    SAVE_UKCX s2, 36
    SAVE_UKCX s3, 37
    SAVE_UKCX s4, 38
    SAVE_UKCX s5, 39
    SAVE_UKCX s6, 40
    SAVE_UKCX s7, 41
    SAVE_UKCX s8, 42
    SAVE_UKCX s9, 43
    SAVE_UKCX s10, 44
    SAVE_UKCX s11, 45
    // kernel_ra, kernel_sp, kernel_tp
    SAVE_UKCX ra, 46
    SAVE_UKCX sp, 47
    SAVE_UKCX tp, 48

    // put *mut UKContext to sscratch for next trap
    csrw sscratch, a0

    // recover sepc and scause from UKContext
    RECOVER_UKCX t0, 32
    csrw sepc, t0
    RECOVER_UKCX t0, 33
    csrw scause, t0

    // recover all user register from user_xx in UKContext
    // user_rx
    RECOVER_UKCX x1, 1
    RECOVER_UKCX x2, 2
    RECOVER_UKCX x3, 3
    RECOVER_UKCX x4, 4
    RECOVER_UKCX x5, 5
    RECOVER_UKCX x6, 6
    RECOVER_UKCX x7, 7
    RECOVER_UKCX x8, 8
    RECOVER_UKCX x9, 9
    // x10 == a0, which is holding *mut UKContext now
    // recover it later
    RECOVER_UKCX x11, 11
    RECOVER_UKCX x12, 12
    RECOVER_UKCX x13, 13
    RECOVER_UKCX x14, 14
    RECOVER_UKCX x15, 15
    RECOVER_UKCX x16, 16
    RECOVER_UKCX x17, 17
    RECOVER_UKCX x18, 18
    RECOVER_UKCX x19, 19
    RECOVER_UKCX x20, 20
    RECOVER_UKCX x21, 21
    RECOVER_UKCX x22, 22
    RECOVER_UKCX x23, 23
    RECOVER_UKCX x24, 24
    RECOVER_UKCX x25, 25
    RECOVER_UKCX x26, 26
    RECOVER_UKCX x27, 27
    RECOVER_UKCX x28, 28
    RECOVER_UKCX x29, 29
    RECOVER_UKCX x30, 30
    RECOVER_UKCX x31, 31
    // now we don't need *mut UKContext anymore
    // so we can recover x10
    RECOVER_UKCX x10, 10

    sret
    // run in user mode now


// 实际上就是 __user_to_kernel, 但是为了凸显它是一个 Direct Mode 的 
// Trap Vector, 所以叫这个名字
__user_trap_entry:
    // 交换 a0 和 sscratch 的值
    // 我们需要使用 a0 来放 *mut UKContext, 但是也不能直接把 a0 原来的值丢掉
    // 所以我们利用 sscratch 来保存 a0 原来的值
    csrrw a0, sscratch, a0

    // save all user register to user_xx in UKContext
    // user_rx
    SAVE_UKCX x1, 1
    SAVE_UKCX x2, 2
    SAVE_UKCX x3, 3
    SAVE_UKCX x4, 4
    SAVE_UKCX x5, 5
    SAVE_UKCX x6, 6
    SAVE_UKCX x7, 7
    SAVE_UKCX x8, 8
    SAVE_UKCX x9, 9
    // x10 == a0, which is holding *mut UKContext now
    // we can get the original a0 from sscratch
    // use x9 to temporarily hold the original value of x10
    csrr x9, sscratch
    SAVE_UKCX x9, 10
    SAVE_UKCX x11, 11
    SAVE_UKCX x12, 12
    SAVE_UKCX x13, 13
    SAVE_UKCX x14, 14
    SAVE_UKCX x15, 15
    SAVE_UKCX x16, 16
    SAVE_UKCX x17, 17
    SAVE_UKCX x18, 18
    SAVE_UKCX x19, 19
    SAVE_UKCX x20, 20
    SAVE_UKCX x21, 21
    SAVE_UKCX x22, 22
    SAVE_UKCX x23, 23
    SAVE_UKCX x24, 24
    SAVE_UKCX x25, 25
    SAVE_UKCX x26, 26
    SAVE_UKCX x27, 27
    SAVE_UKCX x28, 28
    SAVE_UKCX x29, 29
    SAVE_UKCX x30, 30
    SAVE_UKCX x31, 31
    // user_sepc, user_scause
    csrr t0, sepc
    SAVE_UKCX t0, 32
    csrr t0, scause
    SAVE_UKCX t0, 33

    // recover all kernel register from kernel_xx in UKContext
    // kernel_sx
    RECOVER_UKCX s0, 34
    RECOVER_UKCX s1, 35
    RECOVER_UKCX s2, 36
    RECOVER_UKCX s3, 37
    RECOVER_UKCX s4, 38
    RECOVER_UKCX s5, 39
    RECOVER_UKCX s6, 40
    RECOVER_UKCX s7, 41
    RECOVER_UKCX s8, 42
    RECOVER_UKCX s9, 43
    RECOVER_UKCX s10, 44
    RECOVER_UKCX s11, 45
    // kernel_ra, kernel_sp, kernel_tp
    RECOVER_UKCX ra, 46
    RECOVER_UKCX sp, 47
    RECOVER_UKCX tp, 48

    ret

.align 6
__kernel_default_exception_entry:
    // Using current stack
    addi sp, sp, -17*8
    sd  ra,  1*8(sp)
    sd  t0,  2*8(sp)
    sd  t1,  3*8(sp)
    sd  t2,  4*8(sp)
    sd  t3,  5*8(sp)
    sd  t4,  6*8(sp)
    sd  t5,  7*8(sp)
    sd  t6,  8*8(sp)
    sd  a0,  9*8(sp)
    sd  a1, 10*8(sp)
    sd  a2, 11*8(sp)
    sd  a3, 12*8(sp)
    sd  a4, 13*8(sp)
    sd  a5, 14*8(sp)
    sd  a6, 15*8(sp)
    sd  a7, 16*8(sp)
    call kernel_default_exception
    ld  ra,  1*8(sp)
    ld  t0,  2*8(sp)
    ld  t1,  3*8(sp)
    ld  t2,  4*8(sp)
    ld  t3,  5*8(sp)
    ld  t4,  6*8(sp)
    ld  t5,  7*8(sp)
    ld  t6,  8*8(sp)
    ld  a0,  9*8(sp)
    ld  a1, 10*8(sp)
    ld  a2, 11*8(sp)
    ld  a3, 12*8(sp)
    ld  a4, 13*8(sp)
    ld  a5, 14*8(sp)
    ld  a6, 15*8(sp)
    ld  a7, 16*8(sp)
    addi sp, sp, 17*8
    sret

.align 6
__kernel_default_interrupt_entry:
    // Using current stack
    addi sp, sp, -17*8
    sd  ra,  1*8(sp)
    sd  t0,  2*8(sp)
    sd  t1,  3*8(sp)
    sd  t2,  4*8(sp)
    sd  t3,  5*8(sp)
    sd  t4,  6*8(sp)
    sd  t5,  7*8(sp)
    sd  t6,  8*8(sp)
    sd  a0,  9*8(sp)
    sd  a1, 10*8(sp)
    sd  a2, 11*8(sp)
    sd  a3, 12*8(sp)
    sd  a4, 13*8(sp)
    sd  a5, 14*8(sp)
    sd  a6, 15*8(sp)
    sd  a7, 16*8(sp)
    call kernel_default_interrupt
    ld  ra,  1*8(sp)
    ld  t0,  2*8(sp)
    ld  t1,  3*8(sp)
    ld  t2,  4*8(sp)
    ld  t3,  5*8(sp)
    ld  t4,  6*8(sp)
    ld  t5,  7*8(sp)
    ld  t6,  8*8(sp)
    ld  a0,  9*8(sp)
    ld  a1, 10*8(sp)
    ld  a2, 11*8(sp)
    ld  a3, 12*8(sp)
    ld  a4, 13*8(sp)
    ld  a5, 14*8(sp)
    ld  a6, 15*8(sp)
    ld  a7, 16*8(sp)
    addi sp, sp, 17*8
    sret

// 实际上就是 __kernel_to_kernel, 但是为了凸显它是一个 **Vector Mode** 的
// Trap Vector, 所以叫这个名字
.align 8
__kernel_trap_vector:
    j __kernel_default_exception_entry
    .rept 16
    .align 2
    j __kernel_default_interrupt_entry
    .endr
    unimp