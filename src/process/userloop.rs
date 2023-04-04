use alloc::sync::Arc;
use riscv::register::{
    scause::{self, Exception, Interrupt},
    sstatus, stval,
};

use crate::executor;

use super::process::ThreadInfo;

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
                // TODO: run user
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
                    // TODO: syscall
                    // do_exit = Syscall::new(context, &thread, &thread.process).syscall().await;
                }
                Exception::InstructionPageFault
                | Exception::LoadPageFault
                | Exception::StorePageFault => {
                    // TODO: page fault
                    // do_exit = trap_handler::page_fault(&thread, e, stval, context.user_sepc).await;
                }
                Exception::InstructionFault | Exception::IllegalInstruction => {
                    // user die
                    is_exit = true;
                }
                _ => todo!(),
            },
            scause::Trap::Interrupt(i) => match i {
                Interrupt::SupervisorTimer => {
                    // TODO: timer
                    // timer::tick();
                    // if !do_exit {
                    //     thread::yield_now().await;
                    // }
                }
                _ => todo!(),
            },
        }

        if is_exit {
            break;
        }
    }

    if thread.process.pid().is_init() {
        panic!("init process exit");
    }

    // TODO: 当最后一个线程去世时, 令进程去世 (消除 alive)
}

pub fn spawn(thread: Arc<ThreadInfo>) {
    let future = userloop(thread);
    let (r, t) = executor::spawn(future);
    r.run();
    t.detach();
}
