use super::as_dev_err;
/// Block device general traits
use super::BaseDriverOps;
use super::DevResult;
use super::DeviceType;
pub trait BlockDriverOps: BaseDriverOps {
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;

    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> DevResult;
    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> DevResult;
    fn flush(&mut self) -> DevResult;
}

use log::trace;
/// VirtIO blk driver
///
///
use virtio_drivers::{device::blk::VirtIOBlk as InnerDev, transport::Transport, Hal};
pub struct VirtIoBlkDev<H: Hal, T: Transport> {
    inner: InnerDev<H, T>,
    pos: u64,
}

unsafe impl<H: Hal, T: Transport> Send for VirtIoBlkDev<H, T> {}
unsafe impl<H: Hal, T: Transport> Sync for VirtIoBlkDev<H, T> {}

impl<H: Hal, T: Transport> VirtIoBlkDev<H, T> {
    pub fn try_new(transport: T) -> DevResult<Self> {
        Ok(Self {
            inner: InnerDev::new(transport).map_err(as_dev_err)?,
            pos: 0,
        })
    }
}

impl<H: Hal, T: Transport> const BaseDriverOps for VirtIoBlkDev<H, T> {
    fn device_name(&self) -> &str {
        "virtio-blk"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }
}

impl<H: Hal, T: Transport> BlockDriverOps for VirtIoBlkDev<H, T> {
    #[inline]
    fn num_blocks(&self) -> u64 {
        self.inner.capacity()
    }

    #[inline]
    fn block_size(&self) -> usize {
        virtio_drivers::device::blk::SECTOR_SIZE
    }

    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> DevResult {
        self.inner.read_block(block_id as _, buf).map_err(as_dev_err)
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> DevResult {
        self.inner.write_block(block_id as _, buf).map_err(as_dev_err)
    }

    fn flush(&mut self) -> DevResult {
        Ok(())
    }
}

/// FAT32 FS temp
/// TODO: implement HAL

impl<H: Hal, T: Transport> fatfs::IoBase for VirtIoBlkDev<H, T> {
    type Error = ();
}
impl<H: Hal, T: Transport> fatfs::Read for VirtIoBlkDev<H, T> {
    fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut read_len = 0;
        let mut cur_block_id = (self.pos + 0x800000) / self.block_size() as u64;
        let mut cur_block_offset = (self.pos as usize + 0x800000) % self.block_size();
        while !buf.is_empty() {
            trace!(
                "Reading position {} with buffer length {}",
                self.pos,
                buf.len()
            );
            if buf.len() < self.block_size() || cur_block_offset != 0 {
                // Partial Block
                let mut data = [0u8; 512];
                let start = cur_block_offset;
                let count = buf.len().min(self.block_size() - cur_block_offset);
                self.read_block(cur_block_id, &mut data).expect("Error reading block");
                buf[..count].copy_from_slice(&data[start..start + count]);
                read_len += count;
                buf = &mut buf[count..];
            } else {
                match self.read_block(cur_block_id, buf) {
                    Ok(_) => {
                        let tmp = buf;
                        buf = &mut tmp[self.block_size()..];
                        read_len += self.block_size();
                    }
                    Err(_) => return Err(()),
                }
                cur_block_id += 1;
            }
        }
        self.pos += read_len as u64;
        Ok(read_len)
    }
}

impl<H: Hal, T: Transport> fatfs::Write for VirtIoBlkDev<H, T> {
    fn write(&mut self, mut buf: &[u8]) -> Result<usize, Self::Error> {
        let mut write_len = 0;
        let mut cur_block_id = self.pos / self.block_size() as u64;
        while !buf.is_empty() {
            match self.write_block(cur_block_id, buf) {
                Ok(_) => {
                    buf = &buf[self.block_size()..];
                    write_len += self.block_size();
                }
                Err(_) => return Err(()),
            }
            cur_block_id += 1;
        }
        self.pos += write_len as u64;
        Ok(write_len)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<H: Hal, T: Transport> fatfs::Seek for VirtIoBlkDev<H, T> {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        match pos {
            fatfs::SeekFrom::Start(off) => self.pos = off,
            fatfs::SeekFrom::Current(off) => self.pos = (self.pos as i64 + off) as u64,
            fatfs::SeekFrom::End(off) => self.pos = (self.inner.capacity() as i64 + off) as u64,
        }
        Ok(self.pos)
    }
}
