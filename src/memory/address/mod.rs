//! Address type infrastructure
//!

use log::{debug, trace, warn};

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

    let offset = addr.checked_sub(unsafe { consts::device::PHYMEM_START });
    if offset.is_none() {
        panic!("Physical address 0x{:x} is out of range", addr);
    }
    let offset = offset.unwrap();
    let virt_addr = offset.checked_add(consts::address_space::K_SEG_PHY_MEM_BEG);
    if virt_addr.is_none() {
        panic!("Physical address 0x{:x} is out of range", addr);
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
