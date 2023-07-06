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
MEM_SIZE	:= 4G

MAX_BUILD_JOBS	:= 8

# Build args
CARGO_BUILD_ARGS := -j $(MAX_BUILD_JOBS)

ifeq ($(MODE), release)
CARGO_BUILD_ARGS += --release
endif

SDCARD_IMG		:= final.img
QEMU_DEVICES	:= -drive file=$(SDCARD_IMG),format=raw,id=hd0 -device virtio-blk-device,drive=hd0

# QEMU cmdline
QEMU_CMD		:= qemu-system-riscv64 		\
						-machine virt		\
						-nographic 			\
						-bios default 		\
						-m $(MEM_SIZE)		\
						-smp $(CPUS) 		\
						$(QEMU_DEVICES)		\
						-kernel $(BIN_FILE) 

.PHONY: doc kernel build clean qemu run release all release-qemu qemu-dtb
.EXPORT_ALL_VARIABLES:

build: kernel $(BIN_FILE)

doc:
	@cargo doc --document-private-items
kernel:
	@cargo build $(CARGO_BUILD_ARGS)

# addr2line hack
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


# launch qemu
# -kernel will give control to 0x80200000
qemu: build
	@$(QEMU_CMD)

debug: build
	@$(QEMU_CMD)	\
			-s -d int

release-qemu: release
	@$(QEMU_CMD)

preliminary-qemu: SDCARD_IMG = fs.img
preliminary-qemu: QEMU_DEVICES = -drive file=$(SDCARD_IMG),format=raw,id=hd0 -device virtio-blk-device,drive=hd0
preliminary-qemu: QEMU_CMD = qemu-system-riscv64 		\
						-machine virt		\
						-nographic 			\
						-bios default 		\
						-m $(MEM_SIZE)		\
						-smp $(CPUS) 		\
						$(QEMU_DEVICES)		\
						-kernel $(BIN_FILE)
preliminary-qemu: release
	$(QEMU_CMD)
	
# First set release mode
release: MODE = release
release: CARGO_BUILD_ARGS += --release
release: KERNEL_FILE = target/$(TARGET)/$(MODE)/mankoros
# Then build
release: build
	cp $(BIN_FILE) kernel-qemu

# build and run
run: build qemu

# Genrate current QEMU dtb
# Also generate human readable dts
qemu-dtb:
	@$(QEMU_CMD)	\
		-machine dumpdtb=qemu.dtb
	@dtc -o qemu.dts -O dts -I dtb qemu.dtb

# Make a u-boot bootable uImage
uImage: build
	mkimage -A riscv -O linux -C none -T kernel -a 0x40200000 -e 0x40200000 -n MankorOS -d $(BIN_FILE) uImage
	cp uImage /srv/tftp/

# Make a u-boot gzip compressed image
# Load to normal address, leave a space for unzipped
zImage: build
	gzip -f $(BIN_FILE)
	mkimage -A riscv -O linux -C gzip -T kernel -a 0x40400000 -e 0x40400000 -n MankorOS -d $(BIN_FILE).gz zImage
	cp zImage /srv/tftp/

clean:
	@cargo clean
	@rm -rf $(BIN_FILE)
	@rm -rf qemu.dts
	@rm -rf kernel-qemu
	@rm -rf qemu.dtb

# Compatible with OS competition
all: build
