#![cfg_attr(all(target_arch = "aarch64", target_os = "none"), no_std)]
#![cfg_attr(all(target_arch = "aarch64", target_os = "none"), no_main)]

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
use core::arch::global_asm;
#[cfg(all(target_arch = "aarch64", target_os = "none"))]
use core::panic::PanicInfo;
#[cfg(all(target_arch = "aarch64", target_os = "none"))]
use ostd::boot::{early_println, wait_forever};

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
global_asm!(
    r#"
    .section .text.boot, "ax"
    .global _start
_start:
    ldr x1, =__ousia_boot_stack_end
    mov sp, x1
    bl kernel_main

1:
    wfe
    b 1b

    .section .bss.stack, "aw", %nobits
    .balign 16
__ousia_boot_stack:
    .skip 65536
    .global __ousia_boot_stack_end
__ousia_boot_stack_end:
"#,
);

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    early_println("Ousia kernel booted on arm64");
    wait_forever()
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    early_println("Ousia kernel panic");
    wait_forever()
}

#[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
fn main() {}
