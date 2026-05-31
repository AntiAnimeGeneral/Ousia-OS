use core::panic::PanicInfo;

use ostd::boot::{early_println, wait_forever};

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    early_println(boot_message());
    trigger_exception_smoke_if_requested();
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

#[cfg(all(
    feature = "exception-smoke",
    target_os = "none",
    target_arch = "aarch64"
))]
fn trigger_exception_smoke_if_requested() {
    ostd::boot::trigger_diagnostic_exception()
}

#[cfg(not(all(
    feature = "exception-smoke",
    target_os = "none",
    target_arch = "aarch64"
)))]
fn trigger_exception_smoke_if_requested() {}
