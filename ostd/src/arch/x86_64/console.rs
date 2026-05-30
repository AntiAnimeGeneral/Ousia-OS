// SPDX-License-Identifier: MPL-2.0

//! Console output for x86_64.

use core::fmt::Write;

use spin::{Mutex, Once};

use crate::console::uart_ns16650a::{Ns16550aAccess, Ns16550aRegister, Ns16550aUart};

/// The primary serial port, which serves as an early console.
static SERIAL_PORT: Once<Mutex<Ns16550aUart<SerialAccess>>> = Once::new();

/// Prints a line to the early console.
pub fn early_println(message: &str) {
    let serial = SERIAL_PORT.call_once(|| {
        Mutex::new(Ns16550aUart::new(
            // SAFETY:
            // 1. QEMU `q35` exposes the legacy 16550-compatible port at `0x3F8`.
            // 2. The port is only used by the boot console in this single-threaded stage.
            unsafe { SerialAccess::new(0x3F8) },
        ))
    });

    let mut serial = serial.lock();
    serial.init();
    let _ = serial.write_str(message);
    let _ = serial.write_str("\n");
}

/// Access to serial registers via I/O ports in x86.
#[derive(Debug)]
pub struct SerialAccess {
    base: u16,
}

impl SerialAccess {
    /// # Safety
    ///
    /// The caller must ensure that the base port is a valid serial base port and that it has
    /// exclusive ownership of the serial registers.
    const unsafe fn new(port: u16) -> Self {
        Self { base: port }
    }

    fn port(&self, reg: Ns16550aRegister) -> u16 {
        self.base + reg as u16
    }
}

impl Ns16550aAccess for SerialAccess {
    fn read(&self, reg: Ns16550aRegister) -> u8 {
        unsafe { inb(self.port(reg)) }
    }

    fn write(&mut self, reg: Ns16550aRegister, val: u8) {
        unsafe { outb(self.port(reg), val) }
    }
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags),
        );
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags),
        );
    }
    value
}
