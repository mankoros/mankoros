//! Address type infrastructure
//!
//! 本模块包含了 `{Phys,Virt}{Addr,Addr4K,PageNum}` 共计 2 * 3 = 6 种地址类型，
//! 它们都是 usize 的包装，其中以 `Phys` 开头的用于表示物理地址，以 `Virt` 开头的用于表示虚拟地址 (用户地址空间中的地址).
//! 以 `Addr` 结尾的类型用于表示任意定位到字节的地址，而以 `Addr4K` 结尾的类型用于表示对齐到页的地址，`PageNum` 则表示页号。
//! 例如，地址 `0x87654321` 便是一个普通的地址; 如果其对齐到页，则是 `0x87654000` (4K 页情况下); 如果是页号，则是 `0x87654`.
//! 一般偏好使用 `Addr4K` 结尾的类型作为某些与页有关的数据的指代，除非使用 `PageNum` 更为合理 (比如页表项中或是需要页号相减获得页数量的时候).
//!
//! 下面是 `Addr`, `Addr4K`, `PageNum` 的转换关系：
//! - `Addr`
//!     - -> `Addr4K`: 可以分别通过 `round_up/round_down` 来向上/向下取整到页对齐的地址, 也可以通过 `assert_4k` 来强制转换 (在 Debug 模式下会进行 assert)
//!     - -> `PageNum`: 可以通过 `page_num_up/page_num_down` 来获得向上/向下取整的页号, 也可以通过 `.assert_4k().page_num()` 来转换到页对齐地址后再转换到页号
//!     - -> `usize`: 可以通过 `bits` 方法转换为 `usize`
//!     - <- `usize`: 可以通过 `usize::into` 转换为 `Addr`
//! - `Addr4K`
//!     - -> `Addr`: 可以通过 `into` 来转换
//!     - -> `PageNum`: 可以通过 `page_num` 来转换
//!     - -> `usize`: 可以通过 `bits` 方法转换为 `usize`
//!     - <- `usize`: 无。必须先转换为 `Addr`, 再从 `Addr` 显式指定如何转换为 `Addr4K`
//! - `PageNum`
//!     - -> `Addr`: 可以通过 `.addr().into()` 先转换到 `Addr4K` 再转换到 `Addr`
//!     - -> `Addr4K`: 可以通过 `.addr` 转换
//!     - -> `usize`: 可以通过 `bits` 方法转换为 `usize`
//!     - <- `usize`: 可以通过 `usize::into` 转换为 `PageNum`
//!
//! 同时这些类型里还有一些辅助方法，比如转换到 `u8` 指针的方法 (`as_ptr`/`as_mut_ptr`),
//! 转换到任意长度的 slice 的方法 (unsafe `as_slice`/`as_slice_mut`), 以及转换到以页为长度的 slice 的方法，
//! 可以按需取用。

use log::{debug, trace, warn};

use crate::consts;

mod phys;
pub use phys::*;

mod virt;
pub use virt::*;

/// Kernel Phy to Virt function
// Whenever kernel wants to access to a physical address
// it should use this function to translate physical address
// into virtual address.
#[inline]
pub fn kernel_phys_to_virt(addr: usize) -> usize {
    // Return if the address is obviously in HIGH address space
    if addr >= consts::address_space::K_SEG_BEG {
        warn!("Physical address 0x{:x} is in high address space", addr);
        return addr;
    }
    trace!("Kernel physical address 0x{:x} to virtual addr", addr);

    let offset = addr.checked_sub(unsafe { consts::device::PHYMEM_START });
    if offset.is_none() {
        panic!("Physical address 0x{:x} is out of range", addr);
    }
    let offset = offset.unwrap();
    let virt_addr = offset.checked_add(consts::address_space::K_SEG_PHY_MEM_BEG);
    if virt_addr.is_none() {
        debug!("phymem_start: 0x{:x}", unsafe {
            consts::device::PHYMEM_START
        });
        panic!("Physical memory offset 0x{:x} is out of range", offset);
    }
    virt_addr.unwrap()
}

/// Kernel Virt text to Phy address
#[inline]
pub fn kernel_virt_text_to_phys(addr: usize) -> usize {
    if addr < consts::address_space::K_SEG_DATA_BEG {
        warn!("Virtual address 0x{:x} is not in kernel text segment", addr);
        return addr;
    }
    addr - consts::address_space::K_SEG_DATA_BEG + unsafe { consts::device::PLATFORM_BOOT_PC }
}

// Kernel Virt to Phy function
#[inline]
pub fn kernel_virt_to_phys(addr: usize) -> usize {
    // Return if the address is obviously in HIGH address space
    if addr <= consts::address_space::K_SEG_BEG {
        warn!("Virtual address 0x{:x} is in low address space", addr);
        return addr;
    }
    trace!("Kernel virtual address 0x{:x} to physical addr", addr);

    addr - consts::address_space::K_SEG_PHY_MEM_BEG + unsafe { consts::device::PHYMEM_START }
}
// Kernel Phyical device addr to Virt function
//
#[inline]
pub fn kernel_phys_dev_to_virt(addr: usize) -> usize {
    // Return if the address is obviously in HIGH address space
    if addr >= consts::address_space::K_SEG_BEG {
        warn!("Physical address 0x{:x} is in high address space", addr);
        return addr;
    }
    trace!(
        "Kernel device physical address 0x{:x} translated to virtual addr",
        addr
    );

    addr - consts::device::DEVICE_START + consts::address_space::K_SEG_HARDWARE_BEG
}

// 为了减少代码量，我们使用宏来为各种地址的包装实现一些 trait
macro_rules! impl_arithmetic_with_usize {
    ($t:ty) => {
        impl const Add<usize> for $t {
            type Output = Self;
            #[inline]
            fn add(self, rhs: usize) -> Self {
                Self(self.0 + rhs)
            }
        }
        impl const AddAssign<usize> for $t {
            #[inline]
            fn add_assign(&mut self, rhs: usize) {
                *self = *self + rhs;
            }
        }
        impl const Sub<usize> for $t {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: usize) -> Self {
                Self(self.0 - rhs)
            }
        }
        impl const SubAssign<usize> for $t {
            #[inline]
            fn sub_assign(&mut self, rhs: usize) {
                *self = *self - rhs;
            }
        }
        impl const Sub<$t> for $t {
            type Output = usize;
            #[inline]
            fn sub(self, rhs: $t) -> usize {
                self.0 - rhs.0
            }
        }
    };
}

macro_rules! impl_fmt {
    ($t:ty, $prefix:expr) => {
        impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_fmt(format_args!("{}:{:#x}", $prefix, self.0))
            }
        }
        impl fmt::LowerHex for $t {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_fmt(format_args!("{}:{:#x}", $prefix, self.0))
            }
        }
        impl fmt::UpperHex for $t {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_fmt(format_args!("{}:{:#X}", $prefix, self.0))
            }
        }
    };
}

macro_rules! impl_usize_convert {
    ($t:ty) => {
        impl $t {
            #[inline]
            pub const fn bits(self) -> usize {
                self.0
            }
        }
        impl const From<usize> for $t {
            #[inline]
            fn from(bits: usize) -> Self {
                Self(bits)
            }
        }
    };
}

pub(self) use impl_arithmetic_with_usize;
pub(self) use impl_fmt;
pub(self) use impl_usize_convert;
