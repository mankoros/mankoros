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
    - [ ] logging system
        - [x] info!, warn! and error! (2023-01-25 EastonMan)
        - [x] colorful output (2023-01-25 EastonMan)
        - [x] log level support (2023-01-25 EastonMan)
        - [ ] timestamp
    - [ ] initialize using device tree
    - [ ] UART input
- [ ] Interrupt
    - [ ] interrupt infra
    - [ ] interrupt handler
    - [ ] timer interrupt
        - [ ] global TICK and HZ
- [ ] Memory management
    - [ ] device tree parsing
    - [x] physical memory management (2023-01-26 EastonMan)
    - [ ] enable paging
    - [x] global allocator (2023-01-26 Origami404)
    - [ ] auto growing kernel heap
- [ ] Process
    - [ ] process infra
    - [ ] scheduler
    - [ ] smp setup
- [ ] Syscall
    - [ ] syscall infra
    - [ ] POSIX
- [ ] Filesystem
    - [ ] VFS
    - [ ] FAT32
- [ ] Userspace
    - [ ] user program loading
    - [ ] dynamic linking

## License

This project is licensed under GPLv2 or later verion of GPL.