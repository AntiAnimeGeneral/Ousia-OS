const QEMU_VIRT_PL011_UART0: usize = 0x0900_0000;
const UART_FR: usize = 0x18;
const UART_DR: usize = 0x00;
const UART_FR_TXFF: u32 = 1 << 5;

pub fn early_println(message: &str) {
    for byte in message.bytes() {
        write_byte(byte);
    }
    write_byte(b'\n');
}

fn write_byte(byte: u8) {
    while read_reg(UART_FR) & UART_FR_TXFF != 0 {}
    write_reg(UART_DR, byte as u32);
}

fn read_reg(offset: usize) -> u32 {
    let ptr = (QEMU_VIRT_PL011_UART0 + offset) as *const u32;
    unsafe { ptr.read_volatile() }
}

fn write_reg(offset: usize, value: u32) {
    let ptr = (QEMU_VIRT_PL011_UART0 + offset) as *mut u32;
    unsafe { ptr.write_volatile(value) }
}
