use core::arch::global_asm;

global_asm!(
    r#"
    .section .text.boot, "ax"
    .global _start
_start:
    bl __ousia_enable_fp_simd
    ldr x1, =__ousia_boot_stack_end
    mov sp, x1
    bl __ousia_aarch64_install_exception_vector
    bl kernel_main

1:
    wfe
    b 1b

__ousia_enable_fp_simd:
    mrs x0, CurrentEL
    lsr x0, x0, #2
    cmp x0, #1
    b.eq 2f
    cmp x0, #2
    b.eq 3f
    ret

2:
    mrs x0, CPACR_EL1
    orr x0, x0, #(0b11 << 20)
    msr CPACR_EL1, x0
    isb
    ret

3:
    mrs x0, CPTR_EL2
    bic x0, x0, #(1 << 10)
    msr CPTR_EL2, x0
    isb
    ret

    .section .bss.stack, "aw", %nobits
    .balign 16
__ousia_boot_stack:
    .skip 65536
    .global __ousia_boot_stack_end
__ousia_boot_stack_end:
"#,
);
