# MankorOS

> MankorOS is named after the three main contributers
> namely man, kong and luo, so "MankorOS" is essentially "man-ko-ro-s"

MankorOS is a [RISC-V](https://riscv.org/) kernel written in [Rust](https://www.rust-lang.org/)

## RoadMap
- [ ] Mutex
    - [x] simple spinlock (2023-01-24 EastonMan)
    - [ ] disable interrupt
- [ ] Console
    - [x] UART driver (2023-01-24 EastonMan)
    - [x] print! and panic! macro (2023-01-24 EastonMan)
    - [x] logging system
        - [x] info!, warn! and error! (2023-01-25 EastonMan)
        - [x] colorful output (2023-01-25 EastonMan)
        - [x] log level support (2023-01-25 EastonMan)
        - [x] timestamp (2023-04-09 EastonMan)
    - [ ] initialize using device tree
    - [ ] UART input
- [ ] Interrupt
    - [x] interrupt infra (2023-02-22 EastonMan)
    - [x] interrupt handler (2023-02-22 EastonMan)
    - [x] timer interrupt
        - [x] global TICK (2023-02-22 EastonMan)
        - [ ] configurable HZ value
- [ ] Memory management
    - [ ] device tree parsing
    - [x] physical memory management (2023-01-26 EastonMan)
    - [x] enable paging
    - [x] global allocator (2023-01-26 Origami404)
    - [ ] auto growing kernel heap
- [ ] Process
    - [x] rCore-like, process/thread infra (very early Origami404, 2023-05-01 deprecated)
    - [x] linux-like, unified `task_struct` infra (2023-05-01 Origami404)
    - [ ] scheduler
    - [x] smp boot (2023-04-09 EastonMan)
- [ ] Syscall
    - [ ] syscall infra
    - [ ] POSIX
- [ ] Device
    - [x] VirtIO driver (2023-04-15 EastonMan)
- [ ] Filesystem
    - [x] VFS (2023-04-21 SoraShu)
    - [x] FAT32 (2023-04-17 EastonMan)
- [ ] Userspace
    - [ ] user program loading
    - [ ] dynamic linking

## License

This project is licensed under GPLv2 or later verion of GPL.
