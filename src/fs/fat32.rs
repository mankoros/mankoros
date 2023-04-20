/// Implementation of the fatfs glue code
/// FAT32 FS is supposed to work upon a partition
///
use super::partition::Partition;
use fatfs;
use log::warn;

// Implementation for Partition
impl fatfs::IoBase for Partition {
    type Error = ();
}
impl fatfs::Read for Partition {
    fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut read_len = 0;
        while !buf.is_empty() {
            match self.read_one(buf) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                    read_len += n;
                }
                Err(_) => return Err(()),
            }
        }
        Ok(read_len)
    }
}

impl fatfs::Write for Partition {
    fn write(&mut self, mut buf: &[u8]) -> Result<usize, Self::Error> {
        let mut write_len = 0;
        while !buf.is_empty() {
            match self.write_one(buf) {
                Ok(0) => break,
                Ok(n) => {
                    buf = &buf[n..];
                    write_len += n;
                }
                Err(_) => return Err(()),
            }
        }
        Ok(write_len)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl fatfs::Seek for Partition {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let size = self.size();
        let new_pos = match pos {
            fatfs::SeekFrom::Start(pos) => Some(pos),
            fatfs::SeekFrom::Current(off) => self.position().checked_add_signed(off),
            fatfs::SeekFrom::End(off) => size.checked_add_signed(off),
        }
        .ok_or(())?;
        if new_pos > size {
            warn!("Seek beyond the end of the block device");
        }
        self.set_position(new_pos);
        Ok(new_pos)
    }
}
