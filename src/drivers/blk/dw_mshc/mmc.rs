//! Copyright (c) 2023 MankorOS EastonMan
//!
//! driver for Synopsys DesignWare Mobile Storage Host Controller
//!

use core::cell::UnsafeCell;
use core::mem::size_of;

use byte_slice_cast::*;
use log::debug;
use log::info;
use log::warn;

use crate::arch;
use crate::drivers::AsyncBlockDevice;
use crate::drivers::Device;
use crate::memory::frame::alloc_frame;
use crate::memory::kernel_phys_to_virt;

use super::dma::Descriptor;
use super::registers::CtypeCardWidth;
use super::registers::BLKSIZ;
use super::registers::BMOD;
use super::registers::BYTCNT;
use super::registers::CDETECT;
use super::registers::CID;
use super::registers::CLKDIV;
use super::registers::CLKENA;
use super::registers::CTRL;
use super::registers::CTYPE;
use super::registers::DBADDRL;
use super::registers::DBADDRU;
use super::registers::IDSTS;
use super::registers::PWREN;
use super::registers::STATUS;
use super::registers::{CMD, RINSTS};
use super::registers::{CMDARG, RESP};
use super::wait_for;

#[derive(Debug)]
pub struct MMC {
    base_address: usize,
    fifo_offset: UnsafeCell<usize>,
}

unsafe impl Send for MMC {}
unsafe impl Sync for MMC {}

impl MMC {
    pub fn new(base_address: usize) -> MMC {
        MMC {
            base_address,
            fifo_offset: UnsafeCell::new(0x600), // See snps manual
        }
    }
    pub fn card_init(&self) {
        info!("====================== SDIO Init START ========================");

        info!("Card detect: {:b}", self.card_detect());
        info!("Power enable: {:b}", self.power_enable().power_enable());
        info!("Clock enable: {:b}", self.clock_enable().cclk_enable());
        info!("Card 0 width: {:?}", self.card_width(0));
        info!("Control register: {:?}", self.control_reg());
        info!("DMA enabled: {}", self.dma_enabled());
        info!(
            "Descriptor base address: {:x}",
            self.descriptor_base_address()
        );

        let card_idx = 0;
        // 0xAA is check pattern, see https://elixir.bootlin.com/linux/v6.4-rc7/source/drivers/mmc/core/sd_ops.c#L162
        const TEST_PATTERN: u32 = 0xAA;

        // Read clock divider
        info!("Read clock divider");
        let base = self.base_address as *mut CLKDIV;
        let clkdiv = unsafe { base.byte_add(CLKDIV::offset()).read_volatile() };
        info!("Clock divider: {:?}", clkdiv.clks());

        self.reset_clock();
        self.reset_fifo();
        self.set_controller_bus_width(card_idx, CtypeCardWidth::Width1);
        self.set_dma(false); // Disable DMA

        let cmd = CMD::reset_cmd0(0);
        self.send_cmd(cmd, CMDARG::empty(), None);

        // SDIO Check
        // info!("SDIO Check");
        // // CMD5
        // let cmd = CMD::no_data_cmd(card_idx, 5);
        // let cmdarg = CMDARG::empty();
        // if self.send_cmd(cmd, cmdarg).is_none() {
        //     info!("No response from card, not SDIO");
        // }

        // Voltage check and SDHC 2.0 check
        info!("Voltage Check");
        // CMD8
        let cmd = CMD::no_data_cmd(card_idx, 8);
        let cmdarg = CMDARG::from((1 << 8) | TEST_PATTERN);
        let resp = self.send_cmd(cmd, cmdarg, None).expect("Error sending command");
        if (resp.resp(0) & TEST_PATTERN) == 0 {
            warn!("Card {} unusable", card_idx);
        }

        // If card responses, consider it SD

        // Send ACMD41 to power up
        loop {
            // Send CMD55 before ACMD
            let cmd = CMD::no_data_cmd(card_idx, 55);
            let cmdarg = CMDARG::empty();
            self.send_cmd(cmd, cmdarg, None);
            let cmd = CMD::no_data_cmd_no_crc(card_idx, 41); // CRC is all 1 bit by design
            let cmdarg = CMDARG::from((1 << 30) | (1 << 24) | 0xFF8000);
            if let Some(resp) = self.send_cmd(cmd, cmdarg, None) {
                if resp.ocr() & (1 << 31) != 0 {
                    info!("Card {} powered up", card_idx);
                    if resp.ocr() & (1 << 30) != 0 {
                        info!("Card {} is high capacity", card_idx);
                    }
                    break;
                }
            }
            arch::spin(100000); // Wait for card to power up
        }

        // CMD2, get CID
        let cmd = CMD::no_data_cmd_no_crc(card_idx, 2).with_response_length(true); // R2 response, no CRC
        if let Some(resp) = self.send_cmd(cmd, CMDARG::empty(), None) {
            let cid = CID::from(resp.resps_u128());
            info!("CID: {:x?}", cid);
            info!(
                "Card Name: {}",
                core::str::from_utf8(cid.name().to_be_bytes().as_byte_slice()).unwrap()
            );
        }

        // CMD3, get RCA
        let cmd = CMD::no_data_cmd(card_idx, 3);
        let resp = self.send_cmd(cmd, CMDARG::empty(), None).expect("Error executing CMD3");
        let rca = resp.resp(0) >> 16; // RCA[31:16]
        info!("Card status: {:x?}", resp.resp(0) & 0xFFFF);

        // CMD9, get CSD
        let cmd = CMD::no_data_cmd_no_crc(card_idx, 9).with_response_length(true); // R2 response, no CRC
        let cmdarg = CMDARG::from(rca << 16);
        self.send_cmd(cmd, cmdarg, None);

        // CMD7 select card
        let cmd = CMD::no_data_cmd(card_idx, 7);
        let cmdarg = CMDARG::from(rca << 16);
        let resp = self.send_cmd(cmd, cmdarg, None).expect("Error executing CMD7");

        info!("Current FIFO count: {}", self.fifo_filled_cnt());

        // ACMD51 get bus width
        // Send CMD55 before ACMD
        let cmd = CMD::no_data_cmd(card_idx, 55);
        let cmdarg = CMDARG::from(rca << 16);
        self.send_cmd(cmd, cmdarg, None); // RCA is required
        self.set_size(8, 8); // Set transfer size
        let cmd = CMD::data_cmd(card_idx, 51);
        let mut buffer: [usize; 64] = [0; 64]; // 512B
        self.send_cmd(cmd, CMDARG::empty(), Some(&mut buffer));
        info!("Current FIFO count: {}", self.fifo_filled_cnt());
        let resp = u64::from_be(self.read_fifo::<u64>());
        info!("Bus width supported: {:b}", (resp >> 48) & 0xF);

        // CMD16 set block length
        // let cmd = CMD::no_data_cmd(card_idx, 16);
        // let cmdarg = CMDARG::from(512);
        // self.send_cmd(cmd, cmdarg);

        info!("Current FIFO count: {}", self.fifo_filled_cnt());

        // Read one block
        self.set_size(512, 512);
        let cmd = CMD::data_cmd(card_idx, 17);
        let cmdarg = CMDARG::empty();
        let resp = self.send_cmd(cmd, cmdarg, Some(&mut buffer)).expect("Error sending command");

        info!("Current FIFO count: {}", self.fifo_filled_cnt());

        let cmdarg = CMDARG::from(0x200);
        let resp = self.send_cmd(cmd, cmdarg, Some(&mut buffer)).expect("Error sending command");
        debug!("Magic: 0x{:x}", buffer[0]);
        info!("Current FIFO count: {}", self.fifo_filled_cnt());

        // Try DMA

        // Allocate a page for descriptor table
        let descriptor_page_paddr: usize =
            alloc_frame().expect("Error allocating descriptor page").bits();
        let descriptor_page_vaddr = kernel_phys_to_virt(descriptor_page_paddr);
        const descriptor_cnt: usize = 2;
        let mut buffer_page_paddr: [usize; descriptor_cnt] = [0; descriptor_cnt];
        for i in 0..descriptor_cnt {
            buffer_page_paddr[i] = alloc_frame().expect("Error allocating buffer page").bits();
        }
        let descriptor_table = unsafe {
            core::slice::from_raw_parts_mut(
                descriptor_page_vaddr as *mut Descriptor,
                descriptor_cnt,
            )
        };

        // Build chain descriptor
        for idx in 0..descriptor_cnt {
            descriptor_table[idx] = Descriptor::new(
                512,
                buffer_page_paddr[idx],
                descriptor_page_paddr + (idx + 1) % descriptor_cnt * 16, // 16B for each
            );
        }
        // Set descriptor base address
        self.set_descript_base_address(descriptor_page_paddr);

        // Enable DMA
        self.set_dma(true);

        // Read one block
        let buffer = unsafe {
            core::slice::from_raw_parts_mut(
                kernel_phys_to_virt(buffer_page_paddr[0]) as *mut usize,
                64,
            )
        };
        debug!("Magic before: 0x{:x}", buffer[0]);
        let cmdarg = CMDARG::from(0x200);
        let resp = self.send_cmd(cmd, cmdarg, None).expect("Error sending command");

        debug!("Magic after: 0x{:x}", buffer[0]);

        info!("======================= SDIO Init END ========================");
    }

    /// Internal function to send a command to the card
    fn send_cmd(&self, cmd: CMD, arg: CMDARG, buffer: Option<&mut [usize]>) -> Option<RESP> {
        let base = self.base_address as *mut u32;

        // Sanity check
        if cmd.data_expected() && !self.dma_enabled() {
            debug_assert!(buffer.is_some())
        }

        let mut buffer_offset = 0;

        // Wait for can send cmd
        wait_for!({
            let cmd: CMD = unsafe { base.byte_add(CMD::offset()).read_volatile() }.into();
            cmd.can_send_cmd()
        });
        // Wait for card not busy if data is required
        if cmd.data_expected() {
            wait_for!({
                let status: STATUS =
                    unsafe { base.byte_add(STATUS::offset()).read_volatile() }.into();
                !status.data_busy()
            })
        }
        unsafe {
            // Set CMARG
            base.byte_add(CMDARG::offset()).write_volatile(arg.into());
            // Send CMD
            base.byte_add(CMD::offset()).write_volatile(cmd.into());
        }

        // Wait for cmd accepted
        wait_for!({
            let cmd: CMD = unsafe { base.byte_add(CMD::offset()).read_volatile() }.into();
            cmd.can_send_cmd()
        });

        // Wait for command done if response expected
        if cmd.response_expected() {
            wait_for!({
                let rinsts: RINSTS =
                    unsafe { base.byte_add(RINSTS::offset()).read_volatile() }.into();
                rinsts.command_done()
            });
        }

        // Wait for data transfer complete if data expected
        if cmd.data_expected() {
            let buffer = // TODO: dirty
                buffer.unwrap_or(unsafe { core::slice::from_raw_parts_mut(0 as *mut usize, 64) });
            wait_for!({
                let rinsts: RINSTS =
                    unsafe { base.byte_add(RINSTS::offset()).read_volatile() }.into();
                if rinsts.receive_data_request() && !self.dma_enabled() {
                    while self.fifo_filled_cnt() >= 2 {
                        buffer[buffer_offset] = self.read_fifo::<usize>();
                        buffer_offset += 1;
                    }
                }
                rinsts.data_transfer_over() || !rinsts.no_error()
            });
        }

        // Check for error
        let rinsts: RINSTS = unsafe { base.byte_add(RINSTS::offset()).read_volatile() }.into();
        // Clear interrupt by writing 1
        unsafe {
            // Just clear all for now
            base.byte_add(RINSTS::offset()).write_volatile(rinsts.into());
        }

        // Check response
        let base = self.base_address as *mut RESP;
        let resp = unsafe { base.byte_add(RESP::offset()).read_volatile() };
        if rinsts.no_error() && !rinsts.command_conflict() {
            // No return for clock command
            if cmd.update_clock_register_only() {
                info!("Clock cmd done");
                return None;
            }
            info!(
                "CMD{} done: {:?}, dma: {:?}",
                cmd.cmd_index(),
                rinsts.status(),
                self.dma_enabled()
            );
            Some(resp)
        } else {
            warn!("CMD{} error: {:?}", cmd.cmd_index(), rinsts.status());
            warn!("Dumping response");
            warn!("Response: {:x?}", resp);
            None
        }
    }

    /// Read data from FIFO
    fn read_fifo<T>(&self) -> T {
        let base = self.base_address as *mut T;
        let result = unsafe { base.byte_add(*self.fifo_offset.get()).read_volatile() };
        unsafe { *self.fifo_offset.get() += size_of::<T>() };
        result
    }

    /// Reset FIFO
    fn reset_fifo(&self) {
        let base = self.base_address as *mut CTRL;
        let ctrl = self.control_reg().with_fifo_reset(true);
        unsafe { base.byte_add(*self.fifo_offset.get()).write_volatile(ctrl) }
    }

    /// Set transaction size
    ///
    /// block_size: size of transfer
    /// byte_cnt: number of bytes to transfer
    fn set_size(&self, block_size: usize, byte_cnt: usize) {
        let blksiz = BLKSIZ::new().with_block_size(block_size);
        let bytcnt = BYTCNT::new().with_byte_count(byte_cnt);
        let base = self.base_address as *mut BLKSIZ;
        unsafe { base.byte_add(BLKSIZ::offset()).write_volatile(blksiz.into()) };
        let base = self.base_address as *mut BYTCNT;
        unsafe { base.byte_add(BYTCNT::offset()).write_volatile(bytcnt.into()) };
    }

    fn set_controller_bus_width(&self, card_index: usize, width: CtypeCardWidth) {
        let ctype = CTYPE::set_card_width(card_index, width);
        let base = self.base_address as *mut CTYPE;
        unsafe { base.byte_add(CTYPE::offset()).write_volatile(ctype) }
    }

    fn last_response_command_index(&self) -> usize {
        let base = self.base_address as *mut STATUS;
        let status = unsafe { base.byte_add(STATUS::offset()).read_volatile() };
        status.response_index()
    }

    fn fifo_filled_cnt(&self) -> usize {
        self.status().fifo_count()
    }
    fn status(&self) -> STATUS {
        let base = self.base_address as *mut STATUS;
        let status = unsafe { base.byte_add(STATUS::offset()).read_volatile() };
        status
    }

    fn card_detect(&self) -> usize {
        let base = self.base_address as *mut CDETECT;
        let cdetect = unsafe { base.byte_add(CDETECT::offset()).read_volatile() };
        !cdetect.card_detect_n() & 0xFFFF
    }

    fn power_enable(&self) -> PWREN {
        let base = self.base_address as *mut PWREN;
        let pwren = unsafe { base.byte_add(PWREN::offset()).read_volatile() };
        pwren
    }

    fn clock_enable(&self) -> CLKENA {
        let base = self.base_address as *mut CLKENA;
        let clkena = unsafe { base.byte_add(CLKENA::offset()).read_volatile() };
        clkena
    }

    fn set_dma(&self, enable: bool) {
        let base = self.base_address as *mut BMOD;
        let bmod = unsafe { base.byte_add(BMOD::offset()).read_volatile() };
        let bmod = bmod.with_idmac_enable(enable).with_software_reset(true);
        unsafe { base.byte_add(BMOD::offset()).write_volatile(bmod) };

        // Also reset the dma controller
        let base = self.base_address as *mut CTRL;
        let ctrl = unsafe { base.byte_add(CTRL::offset()).read_volatile() };
        let ctrl = ctrl.with_dma_reset(true).with_use_internal_dmac(enable);
        unsafe { base.byte_add(CTRL::offset()).write_volatile(ctrl) };
    }

    fn dma_enabled(&self) -> bool {
        let base = self.base_address as *mut BMOD;
        let bmod = unsafe { base.byte_add(BMOD::offset()).read_volatile() };
        bmod.idmac_enable()
    }

    fn dma_status(&self) -> IDSTS {
        let base = self.base_address as *mut IDSTS;
        let idsts = unsafe { base.byte_add(IDSTS::offset()).read_volatile() };
        idsts
    }

    fn card_width(&self, index: usize) -> CtypeCardWidth {
        let base = self.base_address as *mut CTYPE;
        let ctype = unsafe { base.byte_add(CTYPE::offset()).read_volatile() };
        ctype.card_width(index)
    }

    fn control_reg(&self) -> CTRL {
        let base = self.base_address as *mut CTRL;
        let ctrl = unsafe { base.byte_add(CTRL::offset()).read_volatile() };
        ctrl
    }

    fn descriptor_base_address(&self) -> usize {
        let base = self.base_address as *mut DBADDRL;
        let dbaddrl = unsafe { base.byte_add(DBADDRL::offset()).read_volatile() };
        let base = self.base_address as *mut DBADDRU;
        let dbaddru = unsafe { base.byte_add(DBADDRU::offset()).read_volatile() };
        usize::from(dbaddru.addr()) << 32 | usize::from(dbaddrl.addr())
    }

    fn set_descript_base_address(&self, addr: usize) {
        let base = self.base_address as *mut u32;
        unsafe { base.byte_add(DBADDRL::offset()).write_volatile(addr as u32) };
        unsafe { base.byte_add(DBADDRU::offset()).write_volatile((addr >> 32) as u32) };
    }

    fn reset_clock(&self) {
        // Disable clock
        info!("Disable clock");
        let base = self.base_address as *mut CLKENA;
        let clkena = CLKENA::new().with_cclk_enable(0);
        unsafe { base.byte_add(CLKENA::offset()).write_volatile(clkena) };
        let cmd = CMD::clock_cmd();
        self.send_cmd(cmd, CMDARG::empty(), None);

        // Set clock divider
        info!("Set clock divider");
        let base = self.base_address as *mut CLKDIV;
        let clkdiv = CLKDIV::new().with_clk_divider0(4); // Magic, supposedly set to 400KHz
        unsafe { base.byte_add(CLKDIV::offset()).write_volatile(clkdiv) };

        // Re enable clock
        info!("Renable clock");
        let base = self.base_address as *mut CLKENA;
        let clkena = CLKENA::new().with_cclk_enable(1);
        unsafe { base.byte_add(CLKENA::offset()).write_volatile(clkena) };

        let cmd = CMD::clock_cmd();
        self.send_cmd(cmd, CMDARG::empty(), None);
    }
}

impl Device for MMC {
    fn name(&self) -> &str {
        "snps,dw_mshc"
    }

    fn mmio_base(&self) -> usize {
        self.base_address
    }

    fn mmio_size(&self) -> usize {
        0x1000 // TODO: Hard coded for now
    }

    fn device_type(&self) -> crate::drivers::DeviceType {
        crate::drivers::DeviceType::Block
    }

    fn interrupt_number(&self) -> Option<usize> {
        todo!()
    }

    fn interrupt_handler(&self) {
        todo!()
    }

    fn init(&self) {
        self.card_init()
    }

    fn as_blk(
        self: alloc::sync::Arc<Self>,
    ) -> Option<alloc::sync::Arc<dyn crate::drivers::BlockDevice>> {
        None
    }

    fn as_char(
        self: alloc::sync::Arc<Self>,
    ) -> Option<alloc::sync::Arc<dyn crate::drivers::CharDevice>> {
        None
    }

    fn as_async_blk(
        self: alloc::sync::Arc<Self>,
    ) -> Option<alloc::sync::Arc<dyn AsyncBlockDevice>> {
        Some(self)
    }
}

impl AsyncBlockDevice for MMC {
    fn num_blocks(&self) -> u64 {
        todo!()
    }

    fn block_size(&self) -> usize {
        todo!()
    }

    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> crate::drivers::ADevResult {
        todo!()
    }

    fn write_block(&self, block_id: u64, buf: &[u8]) -> crate::drivers::ADevResult {
        todo!()
    }

    fn flush(&self) -> crate::drivers::ADevResult {
        todo!()
    }
}
