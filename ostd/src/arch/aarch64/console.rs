use core::sync::atomic::{AtomicBool, Ordering};

const QEMU_VIRT_PL011_UART0: usize = 0x0900_0000;
const UART_DR: usize = 0x00;
const UART_IBRD: usize = 0x24;
const UART_FBRD: usize = 0x28;
const UART_LCRH: usize = 0x2c;
const UART_CR: usize = 0x30;
const UART_IMSC: usize = 0x38;
const UART_ICR: usize = 0x44;

const UART_CR_UARTEN: u32 = 1 << 0;
const UART_CR_TXE: u32 = 1 << 8;
const UART_CR_RXE: u32 = 1 << 9;
const UART_LCRH_WLEN_8: u32 = 0b11 << 5;
const UART_LCRH_FEN: u32 = 1 << 4;

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

    write_u32(UART_CR, 0);
    write_u32(UART_ICR, u32::MAX);
    write_u32(UART_IBRD, 1);
    write_u32(UART_FBRD, 40);
    write_u32(UART_LCRH, UART_LCRH_WLEN_8 | UART_LCRH_FEN);
    write_u32(UART_IMSC, 0);
    write_u32(UART_CR, UART_CR_UARTEN | UART_CR_TXE | UART_CR_RXE);
}

fn write_byte(byte: u8) {
    write_u32(UART_DR, byte as u32);
}

fn write_u32(offset: usize, value: u32) {
    let ptr = (QEMU_VIRT_PL011_UART0 + offset) as *mut u32;
    unsafe { ptr.write_volatile(value) }
}
