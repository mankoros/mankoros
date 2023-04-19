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

    addr - consts::PHYMEM_START + consts::address_space::K_SEG_PHY_MEM_BEG
}

/// Kernel Virt text to Phy address
#[inline]
pub fn kernel_virt_text_to_phys(addr: usize) -> usize {
    addr - consts::address_space::K_SEG_DATA_BEG + consts::PHYMEM_START
}