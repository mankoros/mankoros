//! Block device general traits

use core::ptr::NonNull;

use log::{info, warn};
use virtio_drivers::transport::{self, mmio::MmioTransport, Transport};

use crate::consts::address_space::K_SEG_DTB;
use crate::drivers::blk::virtio::as_dev_type;
use crate::drivers::{Device, DeviceType, VirtIoBlockDev};
use crate::memory::kernel_phys_dev_to_virt;
use crate::memory::pagetable::pte::PTEFlags;
use crate::{boot, memory};

use super::dw_mshc::{self, MMC};

pub fn probe_sdio_blk() -> Option<MMC> {
    dw_mshc::probe()
}

pub fn probe_virtio_blk() -> Option<VirtIoBlockDev> {
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    let node = device_tree.find_compatible(&["virtio,mmio"])?;
    let reg = node.reg()?.next()?;
    // First map memory, probe virtio device need to map it
    let mut kernel_page_table = memory::pagetable::pagetable::PageTable::new_with_paddr(
        (boot::boot_pagetable_paddr()).into(),
    );
    kernel_page_table.map_region(
        (kernel_phys_dev_to_virt(reg.starting_address as usize)).into(),
        (reg.starting_address as usize).into(),
        reg.size?,
        PTEFlags::R | PTEFlags::W | PTEFlags::A | PTEFlags::D,
    );
    let dev = probe_devices_common(
        DeviceType::Block,
        reg.starting_address as usize,
        reg.size?,
        |t| VirtIoBlockDev::try_new(t, reg.starting_address as usize, reg.size?).ok(),
    );
    kernel_page_table.unmap_region(
        (kernel_phys_dev_to_virt(reg.starting_address as usize)).into(),
        reg.size?,
    );
    // Avoid drop
    core::mem::forget(kernel_page_table);
    if dev.is_none() {
        warn!("No virtio block device found");
    }
    dev
}

fn probe_devices_common<D, F>(dev_type: DeviceType, base: usize, size: usize, ret: F) -> Option<D>
where
    D: Device,
    F: FnOnce(MmioTransport) -> Option<D>,
{
    if let Some(transport) = probe_mmio_device(
        kernel_phys_dev_to_virt(base) as *mut u8,
        size,
        Some(dev_type),
    ) {
        let dev = ret(transport)?;
        info!(
            "created a new {:?} device: {:?}",
            dev.device_type(),
            dev.name()
        );
        return Some(dev);
    }
    None
}

fn probe_mmio_device(
    reg_base: *mut u8,
    _reg_size: usize,
    type_match: Option<DeviceType>,
) -> Option<MmioTransport> {
    use transport::mmio::VirtIOHeader;

    let header = NonNull::new(reg_base as *mut VirtIOHeader).unwrap();
    if let Ok(transport) = unsafe { MmioTransport::new(header) } {
        if type_match.is_none() || as_dev_type(transport.device_type()) == type_match {
            info!(
                "Detected virtio MMIO device with vendor id: {:#X}, device type: {:?}, version: {:?}",
                transport.vendor_id(),
                transport.device_type(),
                transport.version(),
            );
            Some(transport)
        } else {
            None
        }
    } else {
        None
    }
}
