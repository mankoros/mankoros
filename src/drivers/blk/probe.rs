//! Block device general traits

use core::ptr::NonNull;

use log::{info, warn};
use virtio_drivers::transport::{self, mmio::MmioTransport, Transport};

use crate::drivers::blk::dev::as_dev_type;
use crate::drivers::{Device, DeviceType, VirtIoBlockDev};
use crate::{consts::platform, memory::kernel_phys_dev_to_virt};

pub fn probe() -> Option<VirtIoBlockDev> {
    probe_devices_common(DeviceType::Block, |t| {
        warn!("TODO: impl irq parsing");
        VirtIoBlockDev::try_new(t, platform::VIRTIO_MMIO_REGIONS[0].0, 4096).ok()
    })
}

fn probe_devices_common<D, F>(dev_type: DeviceType, ret: F) -> Option<D>
where
    D: Device,
    F: FnOnce(MmioTransport) -> Option<D>,
{
    for reg in platform::VIRTIO_MMIO_REGIONS {
        if let Some(transport) = probe_mmio_device(
            kernel_phys_dev_to_virt(reg.0.into()) as *mut u8,
            reg.1,
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
