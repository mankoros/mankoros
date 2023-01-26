TARGET      := riscv64gc-unknown-none-elf
MODE        := debug
KERNEL_FILE := target/$(TARGET)/$(MODE)/mankoros
BIN_FILE    := kernel.bin

OBJDUMP     := rust-objdump --arch-name=riscv64
OBJCOPY     := rust-objcopy --binary-architecture=riscv64

CPUS		:= 4

MAX_BUILD_JOBS	:= 8

# Build args
CARGO_BUILD_ARGS := -j $(MAX_BUILD_JOBS)

ifeq ($(MODE), release)
CARGO_BUILD_ARGS += --release
endif

.PHONY: doc kernel build clean qemu run

build: kernel $(BIN_FILE)

doc:
	@cargo doc --document-private-items
kernel:
	@cargo build $(CARGO_BUILD_ARGS)

$(BIN_FILE): kernel
	@$(OBJCOPY) $(KERNEL_FILE) --strip-all -O binary $@

asm:
	@$(OBJDUMP) -d $(KERNEL_FILE) | less

clean:
	@cargo clean
	@rm -rf $(BIN_FILE)

# launch qemu
# -kernel will give control to 0x80200000
qemu: build
	@qemu-system-riscv64 		\
            -machine virt 		\
            -nographic 			\
            -bios default 		\
			-smp $(CPUS) 		\
			-kernel $(BIN_FILE)

debug: build
	@qemu-system-riscv64 		\
            -machine virt 		\
            -nographic 			\
            -bios default 		\
			-smp $(CPUS) 		\
			-kernel $(BIN_FILE) \
			-s -S

# build and run
run: build qemu