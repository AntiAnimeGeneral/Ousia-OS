use core::ops::Deref;
use core::sync::atomic::{AtomicBool, Ordering};

use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::registers::{ReadOnly, ReadWrite, WriteOnly};
use tock_registers::{register_bitfields, register_structs};

const QEMU_VIRT_PL011_UART0: *mut RegisterBlock = 0x0900_0000 as *mut RegisterBlock;

register_structs! {
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x000 => DR: ReadWrite<u8>),
        (0x001 => _reserved0),
        (0x018 => FR: ReadOnly<u32, FR::Register>),
        (0x01c => _reserved1),
        (0x024 => IBRD: ReadWrite<u32>),
        (0x028 => FBRD: ReadWrite<u32>),
        (0x02c => LCRH: ReadWrite<u32, LCRH::Register>),
        (0x030 => CR: ReadWrite<u32, CR::Register>),
        (0x034 => _reserved2),
        (0x038 => IMSC: ReadWrite<u32>),
        (0x03c => _reserved3),
        (0x044 => ICR: WriteOnly<u32, ICR::Register>),
        (0x048 => @END),
    }
}

register_bitfields! {
    u32,

    FR [
        TXFF OFFSET(5) NUMBITS(1) [],
    ],

    LCRH [
        WLEN OFFSET(5) NUMBITS(2) [
            EightBit = 0b11,
        ],
        FEN OFFSET(4) NUMBITS(1) [],
    ],

    CR [
        RXE OFFSET(9) NUMBITS(1) [],
        TXE OFFSET(8) NUMBITS(1) [],
        UARTEN OFFSET(0) NUMBITS(1) [],
    ],

    ICR [
        ALL OFFSET(0) NUMBITS(11) [],
    ],
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn early_println(message: &str) {
    init_once();

    for byte in message.bytes() {
        write_byte(byte);
    }
    write_byte(b'\n');
}

fn init_once() {
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }

    uart().init();
}

fn write_byte(byte: u8) {
    uart().put_char(byte);
}

fn uart() -> Pl011 {
    unsafe { Pl011::new(QEMU_VIRT_PL011_UART0) }
}

struct Pl011 {
    registers: *mut RegisterBlock,
}

impl Pl011 {
    unsafe fn new(registers: *mut RegisterBlock) -> Self {
        Self { registers }
    }

    fn init(&self) {
        self.CR.set(0);
        self.ICR.write(ICR::ALL::SET);
        self.IBRD.set(1);
        self.FBRD.set(40);
        self.LCRH.write(LCRH::WLEN::EightBit + LCRH::FEN::SET);
        self.IMSC.set(0);
        self.CR.write(CR::UARTEN::SET + CR::TXE::SET + CR::RXE::SET);
    }

    fn put_char(&self, byte: u8) {
        while self.FR.matches_all(FR::TXFF::SET) {
            core::hint::spin_loop();
        }
        self.DR.set(byte);
    }
}

impl Deref for Pl011 {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.registers }
    }
}
