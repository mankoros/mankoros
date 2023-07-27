mod dma;
mod mmc;
mod registers;

use super::wait_for;

pub use mmc::MMC;

pub fn probe() -> Option<MMC> {
    None
}
