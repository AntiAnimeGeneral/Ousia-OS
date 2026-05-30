pub fn wait_forever() -> ! {
    loop {
        aarch64_cpu::asm::wfe();
    }
}
