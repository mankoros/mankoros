use riscv::register::{scause};

use super::timer;

#[no_mangle]
pub fn kernel_default_interrupt() {
    let interrupt = match scause::read().cause() {
        scause::Trap::Interrupt(i) => i,
        scause::Trap::Exception(e) => {
            panic!("should kernel_interrupt but {:?}", e);
        }
    };

    match interrupt {
        scause::Interrupt::UserSoft => todo!(),
        scause::Interrupt::SupervisorSoft => {
            todo!()
        }
        scause::Interrupt::UserTimer => todo!(),
        scause::Interrupt::SupervisorTimer => timer::timer_handler(),
        scause::Interrupt::UserExternal => todo!(),
        scause::Interrupt::SupervisorExternal => todo!(),
        _ => {
            // Anything else is unexpected
            todo!()
        }
    }
}
