# Building configuratins
ARCH := riscv64
TARGET := riscv64gc-unknown-none-elf
DEBUG := 0

KERNEL_PHYS_ADDR := 0x80200000

APP_NAME := hello_world
APP_PHYS_ADDR := 0x804000000

CARGO_BUILD_FLAGS += --target=$(TARGET)
QEMU_FLAGS += -machine virt -nographic -bios $(BOOTLOADER)

ifeq ($(DEBUG), 0)
	TARGET_DIR := target/$(TARGET)/release
	CARGO_BUILD_FLAGS += --release
else
	TARGET_DIR := target/$(TARGET)/debug
	QEMU_FLAGS += -S -gdb tcp::1234
endif

# Built artifacts
KERNEL_ELF := $(TARGET_DIR)/kernel
KERNEL_BIN := $(TARGET_DIR)/kernel.bin

APP_ELF := $(TARGET_DIR)/$(APP_NAME)
APP_BIN := $(TARGET_DIR)/$(APP_NAME).bin

BUNDLE_BIN := $(TARGET_DIR)/kernel_$(APP_NAME).bin

# External tools
RUSTSBI_QEMU_VERSION := Unreleased
RUSTSBI_QEMU := bootloader/rustsbi-qemu.bin
BOOTLOADER := $(RUSTSBI_QEMU)

OBJDUMP := rust-objdump --arch-name=$(ARCH)
OBJCOPY := rust-objcopy --binary-architecture=$(ARCH)

QEMU_SYSTEM := qemu-system-$(ARCH)
GDB := riscv-none-elf-gdb

kernel:
	cargo build $(CARGO_BUILD_FLAGS) --bin kernel
	$(OBJCOPY) --strip-all -O binary $(KERNEL_ELF) $(KERNEL_BIN)

kernel-qemu: $(BOOTLOADER) kernel

app:
	cargo build $(CARGO_BUILD_FLAGS) --bin $(APP_NAME)
	$(OBJCOPY) --strip-all -O binary $(APP_ELF) $(APP_BIN)

run-qemu: kernel-qemu
	$(QEMU_SYSTEM) $(QEMU_FLAGS) -device loader,file=$(KERNEL_BIN),addr=$(KERNEL_PHYS_ADDR)

run-app-qemu: kernel-qemu app
	@dd bs=1M count=2 of=$(BUNDLE_BIN) if=$(KERNEL_BIN) &>/dev/null
	@dd bs=1M count=2 of=$(BUNDLE_BIN) if=$(APP_BIN) seek=2 &>/dev/null
	$(QEMU_SYSTEM) $(QEMU_FLAGS) -device loader,file=$(BUNDLE_BIN),addr=$(KERNEL_PHYS_ADDR)

gdb-attach:
	$(GDB) \
		-ex 'file $(KERNEL_ELF)' \
		-ex 'set arch riscv:rv64' \
		-ex 'target remote localhost:1234' \
		-ex 'directory .'

$(RUSTSBI_QEMU):
	mkdir -p bootloader
	curl -fsSL -o bootloader/rustsbi-qemu.zip https://github.com/rustsbi/rustsbi-qemu/releases/download/$(RUSTSBI_QEMU_VERSION)/rustsbi-qemu-release.zip
	unzip -jo -d bootloader bootloader/rustsbi-qemu.zip '**/rustsbi-qemu.bin'

.PHONY: kernel kernel-qemu app run-qemu run-app-qemu gdb-attach
