//! Address type infrastructure
//!

use log::{trace, warn};

use crate::consts;

mod phys;
pub use phys::*;

mod virt;
pub use virt::*;

use crate::consts::platform;

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

    addr - consts::PHYMEM_START + consts::address_space::K_SEG_PHY_MEM_BEG
}

/// Kernel Virt text to Phy address
#[inline]
pub fn kernel_virt_text_to_phys(addr: usize) -> usize {
    if addr < consts::address_space::K_SEG_DATA_BEG {
        warn!("Virtual address 0x{:x} is not in kernel text segment", addr);
        return addr;
    }
    addr - consts::address_space::K_SEG_DATA_BEG + consts::PHYMEM_START
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

    addr - consts::address_space::K_SEG_PHY_MEM_BEG + consts::PHYMEM_START
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

    addr - platform::DEVICE_START + consts::address_space::K_SEG_HARDWARE_BEG
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
    }
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
            #[inline]
            pub const fn from(bits: usize) -> Self {
                Self(bits)
            }
        }
    };
}

pub(self) use impl_arithmetic_with_usize;
pub(self) use impl_fmt; 
pub(self) use impl_usize_convert;
