// SPDX-License-Identifier: MPL-2.0

//! NS16550A UART.
//!
//! Adapted from Asterinas `ostd/src/console/uart_ns16650a.rs`.
//! The original source is distributed under MPL-2.0.

use core::fmt;

use bitflags::bitflags;

/// Registers of a NS16550A UART.
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum Ns16550aRegister {
    /// Receive/Transmit Data Register or Divisor Latch Low.
    DataOrDivisorLo,
    /// Interrupt Enable Register or Divisor Latch High.
    IntEnOrDivisorHi,
    /// FIFO Control Register.
    FifoCtrl,
    /// Line Control Register.
    LineCtrl,
    /// Modem Control Register.
    ModemCtrl,
    /// Line Status Register.
    LineStat,
    /// Modem Status Register.
    ModemStat,
}

/// A trait that provides methods to access NS16550A registers.
pub trait Ns16550aAccess {
    /// Reads from an NS16550A register.
    fn read(&self, reg: Ns16550aRegister) -> u8;

    /// Writes to an NS16550A register.
    fn write(&mut self, reg: Ns16550aRegister, val: u8);
}

/// An NS16550A UART.
#[derive(Debug)]
pub struct Ns16550aUart<A: Ns16550aAccess> {
    access: A,
}

bitflags! {
    struct LineStat: u8 {
        /// Data ready (DR).
        const DR    = 1 << 0;
        /// Transmitter holding register empty (THRE).
        const THRE  = 1 << 5;
    }
}

impl<A: Ns16550aAccess> Ns16550aUart<A> {
    /// Creates a new instance.
    pub const fn new(access: A) -> Self {
        Self { access }
    }

    /// Initializes the device.
    pub fn init(&mut self) {
        const DLAB: u8 = 0x80;

        self.access.write(Ns16550aRegister::LineCtrl, DLAB);
        self.access.write(Ns16550aRegister::DataOrDivisorLo, 0x01);
        self.access.write(Ns16550aRegister::IntEnOrDivisorHi, 0x00);

        self.access.write(Ns16550aRegister::LineCtrl, 0x03);
        self.access.write(Ns16550aRegister::FifoCtrl, 0x00);
        self.access.write(Ns16550aRegister::ModemCtrl, 0x0B);
        self.access.write(Ns16550aRegister::IntEnOrDivisorHi, 0x01);
    }

    /// Sends a byte.
    pub fn send(&mut self, data: u8) {
        while !self.line_stat().contains(LineStat::THRE) {
            core::hint::spin_loop();
        }

        self.access.write(Ns16550aRegister::DataOrDivisorLo, data);
    }

    /// Receives a byte.
    pub fn recv(&mut self) -> Option<u8> {
        if !self.line_stat().contains(LineStat::DR) {
            return None;
        }

        Some(self.access.read(Ns16550aRegister::DataOrDivisorLo))
    }

    fn line_stat(&self) -> LineStat {
        LineStat::from_bits_truncate(self.access.read(Ns16550aRegister::LineStat))
    }
}

impl<A: Ns16550aAccess> fmt::Write for Ns16550aUart<A> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.as_bytes() {
            if *c == b'\n' {
                self.send(b'\r');
            }
            self.send(*c);
        }
        Ok(())
    }
}
