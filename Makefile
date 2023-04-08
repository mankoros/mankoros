TARGET      := riscv64gc-unknown-none-elf
MODE        := debug
KERNEL_FILE := target/$(TARGET)/$(MODE)/mankoros
BIN_FILE    := kernel.bin

OBJDUMP     := rust-objdump --arch-name=riscv64
OBJCOPY     := rust-objcopy --binary-architecture=riscv64
ADDR2LINE 	:= llvm-addr2line

TARGET_CC	:= clang
TARGET_CXX	:= clang++

CPUS		:= 4
MEM_SIZE	:= 1G

MAX_BUILD_JOBS	:= 8

# Build args
CARGO_BUILD_ARGS := -j $(MAX_BUILD_JOBS)

ifeq ($(MODE), release)
CARGO_BUILD_ARGS += --release
endif

.PHONY: doc kernel build clean qemu run
.EXPORT_ALL_VARIABLES:

build: kernel $(BIN_FILE)

doc:
	@cargo doc --document-private-items
kernel:
	@cargo build $(CARGO_BUILD_ARGS)

ifeq ($(ADDR),)
addr2line:
	@echo "Usage: make addr2line ADDR=<addr>"
else 
addr2line:
	$(ADDR2LINE) -e $(KERNEL_FILE) $(ADDR)
endif

$(BIN_FILE): kernel
	@$(OBJCOPY) $(KERNEL_FILE) -O binary $@

asm: build
	@$(OBJDUMP) -d $(KERNEL_FILE) > asm

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
			-m $(MEM_SIZE)		\
			-smp $(CPUS) 		\
			-kernel $(BIN_FILE)

debug: build
	@qemu-system-riscv64 		\
            -machine virt 		\
            -nographic 			\
            -bios default 		\
			-m $(MEM_SIZE)		\
			-smp $(CPUS) 		\
			-kernel $(BIN_FILE) \
			-s -d int

# build and run
run: build qemu