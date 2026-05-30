const COM1: u16 = 0x3f8;
const LINE_STATUS_OFFSET: u16 = 5;
const TRANSMIT_EMPTY: u8 = 1 << 5;

pub fn early_println(message: &str) {
    for byte in message.bytes() {
        write_byte(byte);
    }
    write_byte(b'\n');
}

fn write_byte(byte: u8) {
    while unsafe { inb(COM1 + LINE_STATUS_OFFSET) } & TRANSMIT_EMPTY == 0 {}
    unsafe {
        outb(COM1, byte);
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
