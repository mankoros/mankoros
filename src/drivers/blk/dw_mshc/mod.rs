mod dma;
mod mmc;
mod registers;

use crate::consts::address_space::K_SEG_DTB;

use super::wait_for;

use log::{info, warn};
pub use mmc::MMC;

pub fn probe() -> Option<MMC> {
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };

    // Parse SD Card Host Controller
    if let Some(sdhci) = device_tree.find_node("/soc/sdio1@16020000") {
        let base_address =
            sdhci.reg().unwrap().into_iter().next().unwrap().starting_address as usize;
        let sdcard = MMC::new(base_address);
        info!("SD Card Host Controller found at 0x{:x}", base_address);
        return Some(sdcard);
    }
    warn!("SD Card Host Controller not found");
    None
}
