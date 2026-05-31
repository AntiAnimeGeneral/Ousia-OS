use core::panic::PanicInfo;

use kernel::cap::{Capability, CapabilitySpace, EndpointCap, Rights};
use ostd::boot::{early_println, wait_forever};

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    ostd::mm::heap::init_early_heap();
    run_alloc_smoke();
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

fn run_alloc_smoke() {
    let mut cspace = CapabilitySpace::new();
    let root = cspace.create_object(Capability::Endpoint(EndpointCap {
        badge: 1,
        rights: Rights::READ | Rights::GRANT,
    }));
    let child = match cspace.derive(root, Rights::READ) {
        Ok(child) => child,
        Err(_) => panic!("capability derivation failed during alloc smoke"),
    };

    if cspace.lookup(child).is_err() {
        panic!("capability lookup failed during alloc smoke");
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
