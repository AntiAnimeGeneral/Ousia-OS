use core::panic::PanicInfo;

use ostd::boot::{early_println, wait_forever};

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    early_println(boot_message());
    wait_forever()
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    early_println("Ousia kernel panic");
    wait_forever()
}

fn boot_message() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "Ousia kernel booted on aarch64"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "Ousia kernel booted on amd64"
    }
}
