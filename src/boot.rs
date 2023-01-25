/// Copyright (c) 2023 Easton Man
///
/// Boot time things
///
///
use crate::println;

const BOOT_MSG: &str = r"
 __  __             _               ___  ____  
|  \/  | __ _ _ __ | | _____  _ __ / _ \/ ___| 
| |\/| |/ _` | '_ \| |/ / _ \| '__| | | \___ \ 
| |  | | (_| | | | |   < (_) | |  | |_| |___) |
|_|  |_|\__,_|_| |_|_|\_\___/|_|   \___/|____/ 

";

pub fn print_boot_msg() {
    println!("{}", BOOT_MSG);
}

/// Clear BSS segment at start up
///
///
pub fn clear_bss() {
    // linker.ld symbols
    extern "C" {
        fn bss_start();
        fn bss_end();
    }
    (bss_start as usize..bss_end as usize)
        .for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}

/// Get currect HART status
///
/// Return the hart amount
pub fn get_hart_status() -> usize {
    let mut hart_cnt = 0;
    let mut hart_id = 0;
    loop {
        if sbi_rt::hart_get_status(hart_id).is_ok() {
            hart_cnt += 1;
            hart_id += 1;
        } else {
            break;
        }
    }
    hart_cnt
}
