//! Address type infrastructure
//!

use log::{trace, warn};

use crate::consts;

mod phy;
pub use phy::*;

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

    addr - unsafe { consts::device::PHYMEM_START } + consts::address_space::K_SEG_PHY_MEM_BEG
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
