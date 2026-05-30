use core::arch::global_asm;

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
