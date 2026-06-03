use core::panic::PanicInfo;

use kernel::cap::{Capability, CapabilitySpace, EndpointCap, ObjectId, Rights};
use kernel::invocation::{EndpointSendOp, Invocation, InvocationOutcome, invoke};
use kernel::state::KernelState;
use kernel::tcb::{CpuId, Tcb, ThreadId};
use ostd::boot::{early_println, wait_forever};

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    ostd::mm::heap::init_early_heap();
    run_alloc_smoke();
    run_multicore_kernel_state_smoke();
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
    let root = match cspace.insert_initial_capability(Capability::Endpoint(EndpointCap {
        badge: 1,
        rights: Rights::READ | Rights::WRITE | Rights::GRANT,
    })) {
        Ok(root) => root,
        Err(_) => panic!("initial capability insertion failed during alloc smoke"),
    };
    let child = match cspace.derive(root, Rights::READ) {
        Ok(child) => child,
        Err(_) => panic!("capability derivation failed during alloc smoke"),
    };

    if cspace.lookup(child).is_err() {
        panic!("capability lookup failed during alloc smoke");
    }

    match invoke(
        &cspace,
        root,
        Invocation::EndpointSend {
            message_words: 1,
            op: EndpointSendOp::Send,
        },
    ) {
        Ok(InvocationOutcome::SendIpcAuthorized(authorized)) if authorized.badge == 1 => {}
        Ok(_) | Err(_) => panic!("capability invocation failed during alloc smoke"),
    }
}

fn run_multicore_kernel_state_smoke() {
    let cpu0 = CpuId::new(0);
    let cpu1 = CpuId::new(1);
    let thread = ThreadId::new(1);
    let tcb_object = ObjectId::new(1);

    let mut state = match KernelState::new(&[cpu0, cpu1]) {
        Ok(state) => state,
        Err(_) => panic!("kernel state creation failed during multicore smoke"),
    };

    if state.objects_mut().insert_tcb(tcb_object).is_err() {
        panic!("TCB object insertion failed during multicore smoke");
    }
    if state
        .insert_thread_object(tcb_object, Tcb::new(thread, cpu1))
        .is_err()
    {
        panic!("thread insertion failed during multicore smoke");
    }
    if state.objects().tcb_thread(tcb_object) != Ok(thread) {
        panic!("TCB object binding failed during multicore smoke");
    }
    if state.threads().affinity(thread) != Some(cpu1) {
        panic!("thread affinity mismatch during multicore smoke");
    }
    if state.scheduler().placement(thread).is_some() {
        panic!("inactive thread was scheduled during multicore smoke");
    }
}

#[cfg(feature = "exception-smoke")]
fn trigger_exception_smoke_if_requested() {
    ostd::boot::trigger_diagnostic_exception_if_supported()
}

#[cfg(not(feature = "exception-smoke"))]
fn trigger_exception_smoke_if_requested() {}
