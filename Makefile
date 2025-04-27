# Building configuratins
ARCH := riscv64
TARGET := riscv64gc-unknown-none-elf
PROFILE := release

KERNEL_PHYS_ADDR := 0x80200000

# External tools
OBJDUMP := rust-objdump --arch-name=$(ARCH)
OBJCOPY := rust-objcopy --binary-architecture=$(ARCH)

RUSTSBI_QEMU_VERSION := Unreleased
RUSTSBI_QEMU := bootloader/rustsbi-qemu.bin
BOOTLOADER := $(RUSTSBI_QEMU)

# Built artifacts
KERNEL_ELF := target/$(TARGET)/$(PROFILE)/kernel
KERNEL_BIN := target/$(TARGET)/$(PROFILE)/kernel.bin

ifeq ($(PROFILE), release)
	CARGO_BUILD_FLAGS += --release
else ifneq ($(PROFILE), debug)
	CARGO_BUILD_FLAGS += --profile=$(PROFILE)
endif

export CARGO_BUILD_TARGET = $(TARGET)

kernel:
	cargo build $(CARGO_BUILD_FLAGS) --bin kernel

$(KERNEL_ELF): kernel
$(KERNEL_BIN): $(KERNEL_ELF)
	$(OBJCOPY) --strip-all -O binary $(KERNEL_ELF) $@

run-qemu: $(BOOTLOADER) $(KERNEL_BIN)
	qemu-system-$(ARCH) \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_PHYS_ADDR)

$(RUSTSBI_QEMU):
	mkdir -p bootloader
	curl -fsSL -o bootloader/rustsbi-qemu.zip https://github.com/rustsbi/rustsbi-qemu/releases/download/$(RUSTSBI_QEMU_VERSION)/rustsbi-qemu-release.zip
	unzip -jo -d bootloader bootloader/rustsbi-qemu.zip '**/rustsbi-qemu.bin'

.PHONY: kernel run-qemu
