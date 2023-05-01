use alloc::sync::Arc;
use riscv::register::{
    scause::{self, Exception, Interrupt},
    sstatus, stval,
};

use crate::{
    arch,
    boot::boot_pagetable_paddr,
    executor::{yield_future::yield_now},
    syscall::Syscall,
    trap::trap::run_user,
};

use super::lproc::{LightProcess, ProcessStatus};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use log::debug;

struct AutoSIE {}
static mut SIE_COUNT: i32 = 0;
impl AutoSIE {
    fn disable_interrupt_until_drop() -> Self {
        unsafe {
            if SIE_COUNT == 0 {
                sstatus::set_sie();
            }
            SIE_COUNT += 1;
        }

        Self {}
    }
}
impl Drop for AutoSIE {
    fn drop(&mut self) {
        unsafe {
            SIE_COUNT -= 1;
            if SIE_COUNT == 0 {
                sstatus::clear_sie();
            }
        }
    }
}

pub async fn userloop(lproc: Arc<LightProcess>) {
    loop {
        // TODO: 处理 HART 相关问题
        let auto_sie = AutoSIE::disable_interrupt_until_drop();
        let context = lproc.context();

        match lproc.status() {
            ProcessStatus::UNINIT => panic!("Uninitialized process should not enter userloop"),
            ProcessStatus::READY | ProcessStatus::RUNNING => {
                run_user(context);
            }
            ProcessStatus::ZOMBIE | ProcessStatus::STOPPED => {
                // 进程死掉了, 可以退出 userloop 了
                break;
            }
        }

        let scause = scause::read().cause();
        let stval = stval::read();

        drop(auto_sie);

        let mut is_exit = false;

        match scause {
            scause::Trap::Exception(e) => match e {
                Exception::UserEnvCall => {
                    debug!("Syscall, User SPEC: 0x{:x}", context.user_sepc);
                    is_exit = Syscall::new(context, &lproc).syscall().await;
                }
                Exception::InstructionPageFault
                | Exception::LoadPageFault
                | Exception::StorePageFault => {
                    // TODO: page fault
                    debug!(
                        "Pagefault, User SEPC: 0x{:x}, STVAL: 0x{:x}, SCAUSE: {:#?}, User sp: 0x{:x}",
                        context.user_sepc, stval, e, context.user_rx[2]
                    );
                    lproc.with_mut_memory(|m| m.handle_pagefault(stval.into()));
                    // is_exit = true;
                    // do_exit = trap_handler::page_fault(&thread, e, stval, context.user_sepc).await;
                }
                Exception::InstructionFault | Exception::IllegalInstruction => {
                    // user die
                    debug!(
                        "Invalid user programm, User SPEC: 0x{:x}, SCAUSE: {:#?}, STVAL: {:x}",
                        context.user_sepc, e, stval
                    );
                    is_exit = true;
                }
                _ => todo!(),
            },
            scause::Trap::Interrupt(i) => match i {
                Interrupt::SupervisorTimer => {
                    // TODO: timer, currently do nothing
                    // timer::tick();
                    if !is_exit {
                        yield_now().await;
                    }
                }
                _ => todo!(),
            },
        }

        if is_exit {
            break;
        }
    }

    if lproc.id() == 1 {
        // Preliminary stage have no init process, so allow pid 1 to exit
        // panic!("init process exit");
    }

    // TODO: 当最后一个线程去世时, 令进程去世 (消除 alive)
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
        // TODO: 关中断
        // TODO: 检查是否需要切换页表, 比如看看 hart 里的进程是不是当前进程
        // 切换页表
        let pg_paddr = this.lproc.with_memory(|m| m.page_table.root_paddr());
        arch::switch_page_table(pg_paddr.into());
        // TODO: 开中断
        // 再 poll 里边的 userloop
        let ret = unsafe { Pin::new_unchecked(&mut this.future).poll(cx) };
        // TODO: 优化, 如果下一个进程是当前进程, 就不用切回去了
        arch::switch_page_table(boot_pagetable_paddr());
        ret
    }
}
