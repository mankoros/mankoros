const FD_STDOUT: usize = 1;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    // TODO write a small user mode program to test write to stdout
    match fd {
        FD_STDOUT => {
            // 如果是往标准输出写, 就直接转发到 uart 去
            let mut uart = crate::UART0.lock();
            let buf = unsafe { core::slice::from_raw_parts(buf, len) };
            buf.iter().for_each(|&b| uart.send(b));
            len as isize
        }

        _ => panic!("unsupported fd, only stdout is supported yet"),
    }
}
