use core::arch::{asm, global_asm};
use core::ptr;

use crate::arch::aarch64::{console, cpu};

const NUM_EXCEPTION_REGISTERS: usize = 32;

#[used]
#[unsafe(no_mangle)]
static mut __ousia_exception_registers: [u64; NUM_EXCEPTION_REGISTERS] =
    [0; NUM_EXCEPTION_REGISTERS];

global_asm!(
    r#"
    .section .text.exception, "ax"
    .balign 2048
    .global __ousia_aarch64_vector_table
__ousia_aarch64_vector_table:
    .balign 128
    b __ousia_exception_entry_0
    .balign 128
    b __ousia_exception_entry_1
    .balign 128
    b __ousia_exception_entry_2
    .balign 128
    b __ousia_exception_entry_3
    .balign 128
    b __ousia_exception_entry_4
    .balign 128
    b __ousia_exception_entry_5
    .balign 128
    b __ousia_exception_entry_6
    .balign 128
    b __ousia_exception_entry_7
    .balign 128
    b __ousia_exception_entry_8
    .balign 128
    b __ousia_exception_entry_9
    .balign 128
    b __ousia_exception_entry_10
    .balign 128
    b __ousia_exception_entry_11
    .balign 128
    b __ousia_exception_entry_12
    .balign 128
    b __ousia_exception_entry_13
    .balign 128
    b __ousia_exception_entry_14
    .balign 128
    b __ousia_exception_entry_15

    .macro ousia_exception_entry id
        stp x15, x16, [sp, #-32]!
        str x17, [sp, #16]
        mov x15, #\id
        b __ousia_exception_entry_common
    .endm

__ousia_exception_entry_0:
    ousia_exception_entry 0
__ousia_exception_entry_1:
    ousia_exception_entry 1
__ousia_exception_entry_2:
    ousia_exception_entry 2
__ousia_exception_entry_3:
    ousia_exception_entry 3
__ousia_exception_entry_4:
    ousia_exception_entry 4
__ousia_exception_entry_5:
    ousia_exception_entry 5
__ousia_exception_entry_6:
    ousia_exception_entry 6
__ousia_exception_entry_7:
    ousia_exception_entry 7
__ousia_exception_entry_8:
    ousia_exception_entry 8
__ousia_exception_entry_9:
    ousia_exception_entry 9
__ousia_exception_entry_10:
    ousia_exception_entry 10
__ousia_exception_entry_11:
    ousia_exception_entry 11
__ousia_exception_entry_12:
    ousia_exception_entry 12
__ousia_exception_entry_13:
    ousia_exception_entry 13
__ousia_exception_entry_14:
    ousia_exception_entry 14
__ousia_exception_entry_15:
    ousia_exception_entry 15

__ousia_exception_entry_common:
    adrp x16, __ousia_exception_registers
    add x16, x16, :lo12:__ousia_exception_registers
    stp x0, x1, [x16, #16 * 0]
    stp x2, x3, [x16, #16 * 1]
    stp x4, x5, [x16, #16 * 2]
    stp x6, x7, [x16, #16 * 3]
    stp x8, x9, [x16, #16 * 4]
    stp x10, x11, [x16, #16 * 5]
    stp x12, x13, [x16, #16 * 6]
    stp x14, x15, [x16, #16 * 7]
    ldr x17, [sp, #0]
    str x17, [x16, #8 * 15]
    ldr x17, [sp, #8]
    str x17, [x16, #8 * 16]
    ldr x17, [sp, #16]
    str x17, [x16, #8 * 17]
    add sp, sp, #32
    stp x18, x19, [x16, #16 * 9]
    stp x20, x21, [x16, #16 * 10]
    stp x22, x23, [x16, #16 * 11]
    stp x24, x25, [x16, #16 * 12]
    stp x26, x27, [x16, #16 * 13]
    stp x28, x29, [x16, #16 * 14]
    str x30, [x16, #8 * 30]
    mov x17, sp
    str x17, [x16, #8 * 31]
    mov x0, x15
    b __ousia_aarch64_exception_handler
"#,
);

unsafe extern "C" {
    static __ousia_aarch64_vector_table: u8;
}

#[unsafe(no_mangle)]
pub extern "C" fn __ousia_aarch64_install_exception_vector() {
    let table = ptr::addr_of!(__ousia_aarch64_vector_table) as u64;
    let current_el = current_exception_level();

    unsafe {
        asm!("dsb sy", options(nostack, preserves_flags));
        match current_el {
            2 => {
                asm!("msr vbar_el2, {table}", table = in(reg) table, options(nostack, preserves_flags))
            }
            _ => {
                asm!("msr vbar_el1, {table}", table = in(reg) table, options(nostack, preserves_flags))
            }
        }
        asm!("isb", options(nostack, preserves_flags));
    }
}

#[cfg(feature = "exception-smoke")]
pub fn trigger_diagnostic_exception() -> ! {
    unsafe {
        asm!("brk #0x111", options(nomem, nostack));
    }
    cpu::wait_forever()
}

#[unsafe(no_mangle)]
extern "C" fn __ousia_aarch64_exception_handler(vector_index: usize) -> ! {
    let registers = unsafe { ptr::addr_of!(__ousia_exception_registers).read_volatile() };
    let snapshot = ExceptionSnapshot::read(vector_index, registers);

    console::early_print(format_args!(
        "Ousia AArch64 exception\nvector: {}\ncurrent_el: EL{}\nelr: {:#018x}\nesr: {:#018x}\nfar: {:#018x}\nspsr: {:#018x}\n",
        vector_name(snapshot.vector_index),
        snapshot.current_el,
        snapshot.elr,
        snapshot.esr,
        snapshot.far,
        snapshot.spsr,
    ));

    for (index, value) in snapshot.registers.iter().enumerate() {
        console::early_print(format_args!("x{index}: {value:#018x}\n"));
    }

    cpu::wait_forever()
}

struct ExceptionSnapshot {
    vector_index: usize,
    current_el: u64,
    elr: u64,
    esr: u64,
    far: u64,
    spsr: u64,
    registers: [u64; NUM_EXCEPTION_REGISTERS],
}

impl ExceptionSnapshot {
    fn read(vector_index: usize, registers: [u64; NUM_EXCEPTION_REGISTERS]) -> Self {
        let current_el = current_exception_level();
        let (elr, esr, far, spsr) = read_exception_registers(current_el);

        Self {
            vector_index,
            current_el,
            elr,
            esr,
            far,
            spsr,
            registers,
        }
    }
}

fn current_exception_level() -> u64 {
    let current_el: u64;
    unsafe {
        asm!("mrs {current_el}, CurrentEL", current_el = out(reg) current_el, options(nomem, nostack, preserves_flags));
    }
    (current_el >> 2) & 0b11
}

fn read_exception_registers(current_el: u64) -> (u64, u64, u64, u64) {
    let elr: u64;
    let esr: u64;
    let far: u64;
    let spsr: u64;

    unsafe {
        match current_el {
            2 => {
                asm!("mrs {elr}, elr_el2", elr = out(reg) elr, options(nomem, nostack, preserves_flags));
                asm!("mrs {esr}, esr_el2", esr = out(reg) esr, options(nomem, nostack, preserves_flags));
                asm!("mrs {far}, far_el2", far = out(reg) far, options(nomem, nostack, preserves_flags));
                asm!("mrs {spsr}, spsr_el2", spsr = out(reg) spsr, options(nomem, nostack, preserves_flags));
            }
            _ => {
                asm!("mrs {elr}, elr_el1", elr = out(reg) elr, options(nomem, nostack, preserves_flags));
                asm!("mrs {esr}, esr_el1", esr = out(reg) esr, options(nomem, nostack, preserves_flags));
                asm!("mrs {far}, far_el1", far = out(reg) far, options(nomem, nostack, preserves_flags));
                asm!("mrs {spsr}, spsr_el1", spsr = out(reg) spsr, options(nomem, nostack, preserves_flags));
            }
        }
    }

    (elr, esr, far, spsr)
}

fn vector_name(vector_index: usize) -> &'static str {
    match vector_index {
        0 => "sync current EL with SP0",
        1 => "IRQ current EL with SP0",
        2 => "FIQ current EL with SP0",
        3 => "SError current EL with SP0",
        4 => "sync current EL with SPx",
        5 => "IRQ current EL with SPx",
        6 => "FIQ current EL with SPx",
        7 => "SError current EL with SPx",
        8 => "sync lower EL AArch64",
        9 => "IRQ lower EL AArch64",
        10 => "FIQ lower EL AArch64",
        11 => "SError lower EL AArch64",
        12 => "sync lower EL AArch32",
        13 => "IRQ lower EL AArch32",
        14 => "FIQ lower EL AArch32",
        15 => "SError lower EL AArch32",
        _ => "corrupted vector index",
    }
}
