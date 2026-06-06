use core::panic::PanicInfo;

extern crate alloc;

use kernel::{
    handle::{HandleRights, HandleValue},
    object::ObjectKind,
    syscall::{Kernel, Syscall, SyscallContext, SyscallOutcome},
};
use ostd::boot::{early_println, wait_forever};

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    ostd::mm::heap::init_early_heap();
    run_ousia_native_kernel_smoke();
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

fn run_ousia_native_kernel_smoke() {
    let mut kernel = Kernel::new(16, 1).expect("kernel smoke state should initialize");
    let process = kernel
        .create_bootstrap_process(16, 16)
        .expect("bootstrap process should initialize");
    let context = SyscallContext::new(process);

    let event = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::WRITE,
                },
            )
            .expect("event creation should succeed during kernel smoke"),
    );
    kernel
        .lookup_handle(process, event, ObjectKind::Event, HandleRights::READ)
        .expect("event handle should be readable during kernel smoke");

    let SyscallOutcome::HandlePair { first, second } = kernel
        .execute(
            context,
            Syscall::CreateChannelPair {
                max_messages: 1,
                rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
            },
        )
        .expect("channel pair creation should succeed during kernel smoke")
    else {
        panic!("channel pair syscall returned wrong outcome during kernel smoke");
    };
    kernel
        .execute(
            context,
            Syscall::ChannelSend {
                channel: first,
                bytes: alloc::vec![1, 2, 3],
                handles: alloc::vec![],
            },
        )
        .expect("channel send should succeed during kernel smoke");
    let SyscallOutcome::Message { byte_len, .. } = kernel
        .execute(context, Syscall::ChannelRecv { channel: second })
        .expect("channel recv should succeed during kernel smoke")
    else {
        panic!("channel recv syscall returned wrong outcome during kernel smoke");
    };
    if byte_len != 3 {
        panic!("channel recv byte length mismatch during kernel smoke");
    }

    let memory = handle(
        kernel
            .execute(
                context,
                Syscall::CreateMemoryObject {
                    size_bytes: 0x2000,
                    rights: HandleRights::READ,
                },
            )
            .expect("memory object creation should succeed during kernel smoke"),
    );
    let address_space = handle(
        kernel
            .execute(
                context,
                Syscall::CreateAddressSpace {
                    rights: HandleRights::MANAGE,
                },
            )
            .expect("address space creation should succeed during kernel smoke"),
    );
    kernel
        .execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0x1000,
                size_bytes: 0x1000,
                memory_offset: 0,
                rights: HandleRights::READ,
            },
        )
        .expect("address space mapping should succeed during kernel smoke");
}

fn handle(outcome: SyscallOutcome) -> HandleValue {
    let SyscallOutcome::Handle { handle } = outcome else {
        panic!("handle syscall returned wrong outcome during kernel smoke");
    };
    handle
}

#[cfg(feature = "exception-smoke")]
fn trigger_exception_smoke_if_requested() {
    ostd::boot::trigger_diagnostic_exception_if_supported()
}

#[cfg(not(feature = "exception-smoke"))]
fn trigger_exception_smoke_if_requested() {}
