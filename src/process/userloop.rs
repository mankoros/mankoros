use alloc::sync::Arc;
use riscv::register::{
    scause::{self, Exception, Interrupt},
    sstatus, stval,
};

use crate::{
    arch,
    executor::{self, yield_future::yield_now},
    syscall::Syscall,
    trap::trap::run_user,
};

use super::process::{ProcessInfo, ThreadInfo};
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

async fn userloop(thread: Arc<ThreadInfo>) {
    loop {
        // TODO: 处理 HART 相关问题
        let auto_sie = AutoSIE::disable_interrupt_until_drop();
        let context = thread.context();

        match thread.process.with_alive_or_dead(|_| {}) {
            Some(_) => {
                // enter user mode
                // never return until an exception/interrupt occurs in user mode
                run_user(context);
            }
            // 进程死掉了, 可以退出 userloop 了
            None => break,
        }

        let scause = scause::read().cause();
        let stval = stval::read();

        drop(auto_sie);

        let mut is_exit = false;

        match scause {
            scause::Trap::Exception(e) => match e {
                Exception::UserEnvCall => {
                    debug!("Syscall, User SPEC: 0x{:x}", context.user_sepc);
                    is_exit = Syscall::new(context, &thread, &thread.process).syscall().await;
                }
                Exception::InstructionPageFault
                | Exception::LoadPageFault
                | Exception::StorePageFault => {
                    // TODO: page fault
                    debug!(
                        "Pagefault, User SEPC: 0x{:x}, STVAL: 0x{:x}, SCAUSE: {:#?}, User sp: 0x{:x}",
                        context.user_sepc, stval, e, context.user_rx[2]
                    );
                    thread.process.with_alive(|a| a.handle_pagefault(stval.into()));
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

    if thread.process.pid() == 1 {
        // Preliminary stage have no init process, so allow pid 1 to exit
        // panic!("init process exit");
    }

    // TODO: 当最后一个线程去世时, 令进程去世 (消除 alive)
}

pub fn spawn(thread: Arc<ThreadInfo>) {
    let future = OutermostFuture::new(thread.process.clone(), userloop(thread));
    let (r, t) = executor::spawn(future);
    r.run();
    t.detach();
}

struct OutermostFuture<F: Future + Send + 'static> {
    process: Arc<ProcessInfo>,
    future: F,
}
impl<F: Future + Send + 'static> OutermostFuture<F> {
    #[inline]
    pub fn new(process: Arc<ProcessInfo>, future: F) -> Self {
        Self { process, future }
    }
}

impl<F: Future + Send + 'static> Future for OutermostFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        // TODO: 关中断
        // TODO: 检查是否需要切换页表, 比如看看 hart 里的进程是不是当前进程
        // 切换页表
        let pg_paddr = this.process.get_page_table_addr();
        arch::switch_page_table(pg_paddr.into());
        // TODO: 开中断
        // 再 poll 里边的 userloop
        let ret = unsafe { Pin::new_unchecked(&mut this.future).poll(cx) };
        ret
    }
}
