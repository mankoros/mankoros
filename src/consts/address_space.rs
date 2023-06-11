//! User address space and kernel address space

// =========== 用户段 ===========
pub const U_SEG_BEG: usize = 0x0000_0000_0000_0000;
// 链接基地址
pub const U_SEG_LINK_ADDR: usize = 0x0000_0000_0001_0000;

// 数据段
pub const U_SEG_DATA_BEG: usize = 0x0000_0000_0001_0000;
pub const U_SEG_DATA_END: usize = 0x0000_0000_4000_0000;

// 堆段
pub const U_SEG_HEAP_BEG: usize = 0x0000_0000_4000_0000;
pub const U_SEG_HEAP_END: usize = 0x0000_0000_8000_0000;

// 线程栈段 (64 GiB)
pub const U_SEG_STACK_BEG: usize = 0x0000_0001_0000_0000;
pub const U_SEG_STACK_END: usize = 0x0000_0002_0000_0000;

// mmap 段 (128 GiB)
pub const U_SEG_FILE_BEG: usize = 0x0000_0002_0000_0000;
pub const U_SEG_FILE_END: usize = 0x0000_0004_0000_0000;

pub const U_SEG_END: usize = 0x0000_0004_0000_0000;

// =========== 内核段 ===========
pub const K_SEG_BEG: usize = 0xffff_ffc0_0000_0000;

// 虚拟内存映射 (64 GiB)
pub const K_SEG_VIRT_MEM_BEG: usize = 0xffff_ffc0_0000_0000;
pub const K_SEG_VIRT_MEM_END: usize = 0xffff_ffd0_0000_0000;

// 文件映射 (64 GiB)
pub const K_SEG_FILE_BEG: usize = 0xffff_ffd0_0000_0000;
pub const K_SEG_FILE_END: usize = 0xffff_ffe0_0000_0000;

// 物理内存直接映射区域 (62 GiB)
pub const K_SEG_PHY_MEM_BEG: usize = 0xffff_fff0_0000_0000;
pub const K_SEG_PHY_MEM_END: usize = 0xffff_ffff_8000_0000;

// 内核数据段 (1 GiB)
pub const K_SEG_DATA_BEG: usize = 0xffff_ffff_8000_0000;
pub const K_SEG_DATA_END: usize = 0xffff_ffff_c000_0000;

// 硬件 IO 地址 (750 MiB)
pub const K_SEG_HARDWARE_BEG: usize = 0xffff_ffff_c000_0000;
pub const K_SEG_HARDWARE_END: usize = 0xffff_ffff_f000_0000;

// DTB fixed mapping
pub const K_SEG_DTB: usize = 0xffff_ffff_f000_0000;

pub const K_SEG_END: usize = 0xffff_ffff_ffff_ffff;
