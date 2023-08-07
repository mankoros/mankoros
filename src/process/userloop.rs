use alloc::sync::Arc;
use riscv::register::{
    scause::{self, Exception, Interrupt},
    stval,
};

use crate::{
    arch, drivers,
    executor::{
        hart_local::{set_curr_lproc, AutoSIE},
        util_futures::yield_now,
    },
    memory::address::VirtAddr,
    process::user_space::user_area::PageFaultAccessType,
    syscall::Syscall,
    trap::trap::run_user,
};

use super::lproc::{LightProcess, ProcessStatus};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use log::{debug, info, warn};

pub async fn userloop(lproc: Arc<LightProcess>) {
    loop {
        debug!("enter userspace: {:x?}", lproc.id());
        // TODO: 处理 HART 相关问题
        let auto_sie = AutoSIE::new();
        let context = lproc.context();
        let timer = lproc.timer();

        match lproc.status() {
            ProcessStatus::UNINIT => panic!("Uninitialized process should not enter userloop"),
            ProcessStatus::READY => {
                timer.lock(here!()).switch_into();
                timer.lock(here!()).kernel_to_user();
                run_user(context);
                timer.lock(here!()).user_to_kernel();
            }
            ProcessStatus::RUNNING => {
                timer.lock(here!()).kernel_to_user();
                run_user(context);
                timer.lock(here!()).user_to_kernel();
            }
            ProcessStatus::ZOMBIE | ProcessStatus::STOPPED => {
                // 进程死掉了, 可以退出 userloop 了
                timer.lock(here!()).switch_out();
                break;
            }
        }
        debug!("exit userspace: {:x?}", lproc.id());

        let scause = scause::read().cause();
        let stval = stval::read();

        drop(auto_sie);

        let mut is_exit = false;

        match scause {
            scause::Trap::Exception(e) => match e {
                Exception::UserEnvCall => {
                    debug!("Syscall, User SPEC: 0x{:x}", context.user_sepc);
                    is_exit = Syscall::new(context, lproc.clone()).syscall().await;
                }
                Exception::InstructionPageFault
                | Exception::LoadPageFault
                | Exception::StorePageFault => {
                    debug!("Pagefault, User SPEC: 0x{:x}", context.user_sepc);
                    let access_type = match e {
                        Exception::InstructionPageFault => PageFaultAccessType::RX,
                        Exception::LoadPageFault => PageFaultAccessType::RO,
                        Exception::StorePageFault => PageFaultAccessType::RW,
                        _ => unreachable!(),
                    };

                    let result = lproc.with_mut_memory(|m| {
                        m.handle_pagefault(VirtAddr::from(stval), access_type)
                    });
                    if let Err(e) = result {
                        warn!("Pagefault failed: {:?}, process killed", e);
                        is_exit = true;
                    }
                }
                Exception::InstructionFault | Exception::IllegalInstruction => {
                    // user die
                    warn!(
                        "Invalid user programm, User SPEC: 0x{:x}, SCAUSE: {:#?}, STVAL: {:x}",
                        context.user_sepc, e, stval
                    );
                    is_exit = true;
                }
                e => panic!("Unknown user exception: {:?}", e),
            },
            scause::Trap::Interrupt(i) => match i {
                Interrupt::SupervisorTimer => {
                    // TODO: timer, currently do nothing
                    // timer::tick();
                    if !is_exit {
                        debug!(
                            "Timer interrupt, User SEPC: 0x{:x}, STVAL: 0x{:x}",
                            context.user_sepc, stval
                        );
                        yield_now().await;
                    }
                }
                Interrupt::SupervisorExternal => {
                    drivers::get_device_manager_mut().interrupt_handler()
                }
                _ => todo!(),
            },
        }

        if is_exit {
            break;
        }
    }

    info!("Process {:?} exited", lproc.id());

    if lproc.id() == 1 {
        // Preliminary stage have no init process, so allow pid 1 to exit
        // panic!("init process exit");
    }
    // Do exit clean up, ownership moved
    lproc.do_exit();
}

pub struct OutermostFuture<F: Future + Send + 'static> {
    lproc: Arc<LightProcess>,
    future: F,
}
impl<F: Future + Send + 'static> OutermostFuture<F> {
    #[inline]
    pub fn new(lproc: Arc<LightProcess>, future: F) -> Self {
        Self { lproc, future }
    }
}

impl<F: Future + Send + 'static> Future for OutermostFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        set_curr_lproc(this.lproc.clone());
        // TODO: 关中断
        // TODO: 检查是否需要切换页表, 比如看看 hart 里的进程是不是当前进程
        // 切换页表
        let pg_paddr = this.lproc.with_memory(|m| m.page_table.root_paddr());
        let old_pgtbl = arch::switch_page_table(pg_paddr.bits());
        // TODO: 开中断
        // 再 poll 里边的 userloop
        let ret = unsafe { Pin::new_unchecked(&mut this.future).poll(cx) };
        // TODO: 优化, 如果下一个进程是当前进程, 就不用切回去了
        arch::switch_page_table(old_pgtbl);
        ret
    }
}
