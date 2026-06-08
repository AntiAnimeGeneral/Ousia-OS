use kernel::{
    error::KernelError,
    handle::{HandleRights, HandleValue},
    memory::frame::FrameRange,
    object::ObjectKind,
    syscall::{Kernel, Syscall, SyscallContext},
};

fn kernel(object_capacity: usize, process_capacity: usize) -> Kernel {
    Kernel::new(
        object_capacity,
        process_capacity,
        &[FrameRange::new(0x1000, 0x20000).unwrap()],
    )
    .unwrap()
}

#[test]
fn handle_table_capacity_failure_leaves_object_manager_unchanged() {
    // Goal: fixed handle table capacity is checked before object publication.
    // Scope: host integration through Syscall::CreateObject on a full process table.
    // Semantics: NoCapacity leaves object manager, handle table, and quota state unchanged.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(1, 4).unwrap();
    let before_objects = kernel.objects.live_count();
    let before_quota = kernel
        .processes
        .get(process)
        .unwrap()
        .budget
        .remaining_objects();

    assert_eq!(
        kernel.execute(
            SyscallContext::new(process),
            Syscall::CreateObject {
                kind: ObjectKind::Event,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::NoCapacity)
    );

    assert_eq!(kernel.objects.live_count(), before_objects);
    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), before_quota);
}

#[test]
fn quota_failure_leaves_object_manager_and_handles_unchanged() {
    // Goal: process resource budget is a preflight boundary.
    // Scope: host integration through Syscall::CreateObject after bootstrap consumes quota.
    // Semantics: QuotaExceeded happens before handle installation or object creation.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 1).unwrap();
    let before_objects = kernel.objects.live_count();

    assert_eq!(
        kernel.execute(
            SyscallContext::new(process),
            Syscall::CreateObject {
                kind: ObjectKind::Event,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::QuotaExceeded)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.objects.live_count(), before_objects);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), 0);
    assert!(
        kernel
            .lookup_handle(
                process,
                HandleValue::new(0, kernel::handle::HandleGeneration::INITIAL),
                ObjectKind::Process,
                HandleRights::READ,
            )
            .is_ok()
    );
}

#[test]
fn object_capacity_failure_rolls_back_process_budget() {
    // Goal: object table capacity failure does not consume process quota.
    // Scope: host integration through Syscall::CreateObject with no object slots left.
    // Semantics: NoCapacity from object manager leaves process budget and handles unchanged.
    let mut kernel = kernel(1, 1);
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let before_quota = kernel
        .processes
        .get(process)
        .unwrap()
        .budget
        .remaining_objects();

    assert_eq!(
        kernel.execute(
            SyscallContext::new(process),
            Syscall::CreateObject {
                kind: ObjectKind::Event,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::NoCapacity)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.objects.live_count(), 1);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), before_quota);
}

#[test]
fn memory_object_quota_failure_leaves_object_manager_and_handles_unchanged() {
    // Goal: MemoryObject creation uses the same process preflight boundary as generic objects.
    // Scope: host integration through Syscall::CreateMemoryObject after bootstrap consumes quota.
    // Semantics: QuotaExceeded happens before MemoryObject publication or handle installation.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 1).unwrap();
    let before_objects = kernel.objects.live_count();

    assert_eq!(
        kernel.execute(
            SyscallContext::new(process),
            Syscall::CreateMemoryObject {
                size_bytes: 4096,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::QuotaExceeded)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.objects.live_count(), before_objects);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), 0);
}

#[test]
fn address_space_quota_failure_leaves_object_manager_and_handles_unchanged() {
    // Goal: AddressSpace creation uses the same process quota preflight as other object creation.
    // Scope: host integration through Syscall::CreateAddressSpace after bootstrap consumes quota.
    // Semantics: QuotaExceeded happens before AddressSpace publication or handle installation.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 1).unwrap();
    let before_objects = kernel.objects.live_count();

    assert_eq!(
        kernel.execute(
            SyscallContext::new(process),
            Syscall::CreateAddressSpace {
                rights: HandleRights::MANAGE,
            },
        ),
        Err(KernelError::QuotaExceeded)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.objects.live_count(), before_objects);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), 0);
}

#[test]
fn memory_object_handle_capacity_failure_leaves_object_manager_unchanged() {
    // Goal: MemoryObject publication is blocked by handle capacity preflight.
    // Scope: host integration through Syscall::CreateMemoryObject with a full handle table.
    // Semantics: NoCapacity leaves object manager, handle table, and quota state unchanged.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(1, 4).unwrap();
    let before_objects = kernel.objects.live_count();
    let before_quota = kernel
        .processes
        .get(process)
        .unwrap()
        .budget
        .remaining_objects();

    assert_eq!(
        kernel.execute(
            SyscallContext::new(process),
            Syscall::CreateMemoryObject {
                size_bytes: 4096,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::NoCapacity)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.objects.live_count(), before_objects);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), before_quota);
}

#[test]
fn address_space_handle_capacity_failure_leaves_object_manager_unchanged() {
    // Goal: AddressSpace creation consumes the shared object/handle reservation path.
    // Scope: host integration through Syscall::CreateAddressSpace with a full handle table.
    // Semantics: NoCapacity leaves object manager, handle table, and quota state unchanged.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(1, 4).unwrap();
    let before_objects = kernel.objects.live_count();
    let before_quota = kernel
        .processes
        .get(process)
        .unwrap()
        .budget
        .remaining_objects();

    assert_eq!(
        kernel.execute(
            SyscallContext::new(process),
            Syscall::CreateAddressSpace {
                rights: HandleRights::MANAGE,
            },
        ),
        Err(KernelError::NoCapacity)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.objects.live_count(), before_objects);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), before_quota);
}
