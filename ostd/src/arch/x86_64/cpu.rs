pub fn wait_forever() -> ! {
    loop {
        x86_64_crate::instructions::hlt();
    }
}
