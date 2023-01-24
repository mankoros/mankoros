# MankorOS

> MankorOS is named after the three main contributers
> namely man, kong and luo, so "MankorOS" is essentially "man-ko-ro-s"

Zesty-Core is a [RISC-V](https://riscv.org/) kernel written in [Rust](https://www.rust-lang.org/)

## RoadMap
- [ ] Mutex
    - [x] simple spinlock (2023-01-24)
    - [ ] disable interrupt
- [ ] Console
    - [x] UART driver (2023-01-24)
    - [x] print! and panic! macro (2023-01-24)
    - [ ] initialize using device tree
    - [ ] UART input
- [ ] Interrupt
    - [ ] interrupt infra
    - [ ] interrupt handler
    - [ ] timer interrupt
- [ ] Memory management
    - [ ] device tree parsing
    - [ ] physical memory management
    - [ ] enable paging
    - [ ] global allocator
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