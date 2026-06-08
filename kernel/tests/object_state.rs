use kernel::{
    error::KernelError,
    handle::{HandleRights, HandleValue},
    memory::frame::FrameRange,
    object::{EventState, ObjectKind, ObjectPayload},
    syscall::{Kernel, Syscall, SyscallContext, SyscallOutcome},
    vm::{MappingPolicy, MemoryObject},
};

fn kernel(object_capacity: usize, process_capacity: usize) -> Kernel {
    Kernel::new(
        object_capacity,
        process_capacity,
        &[FrameRange::new(0x1000, 0x20000).unwrap()],
    )
    .unwrap()
}

fn handle(outcome: SyscallOutcome) -> HandleValue {
    let SyscallOutcome::Handle { handle } = outcome else {
        panic!("expected handle outcome");
    };
    handle
}

#[test]
fn event_object_owns_signal_state() {
    // Goal: Event has real object-manager-owned runtime state.
    // Scope: host integration through syscall creation and ObjectManager event operations.
    // Semantics: signal and clear mutate only the Event payload for the referenced object.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let event = handle(
        kernel
            .execute(
                SyscallContext::new(process),
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::WRITE,
                },
            )
            .unwrap(),
    );
    let view = kernel
        .lookup_handle(process, event, ObjectKind::Event, HandleRights::READ)
        .unwrap();

    assert_eq!(
        kernel.objects.event(view.object.id, view.object.generation),
        Ok(kernel::object::EventObject {
            state: EventState::Unsignaled,
        })
    );

    kernel
        .objects
        .signal_event(view.object.id, view.object.generation)
        .unwrap();
    assert_eq!(
        kernel.objects.event(view.object.id, view.object.generation),
        Ok(kernel::object::EventObject {
            state: EventState::Signaled,
        })
    );

    kernel
        .objects
        .clear_event(view.object.id, view.object.generation)
        .unwrap();
    assert_eq!(
        kernel.objects.event(view.object.id, view.object.generation),
        Ok(kernel::object::EventObject {
            state: EventState::Unsignaled,
        })
    );
}

#[test]
fn wrong_object_operation_does_not_mutate_payload() {
    // Goal: object-specific operations reject wrong kinds before mutation.
    // Scope: ObjectManager event operation against a MemoryObject.
    // Semantics: WrongObjectType leaves the original payload intact.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let memory = handle(
        kernel
            .execute(
                SyscallContext::new(process),
                Syscall::CreateMemoryObject {
                    size_bytes: 4096,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );
    let view = kernel
        .lookup_handle(
            process,
            memory,
            ObjectKind::MemoryObject,
            HandleRights::READ,
        )
        .unwrap();

    assert_eq!(
        kernel
            .objects
            .signal_event(view.object.id, view.object.generation),
        Err(KernelError::WrongObjectType)
    );
    assert_eq!(
        kernel
            .objects
            .memory_object(view.object.id, view.object.generation)
            .unwrap()
            .size_bytes,
        4096
    );
}

#[test]
fn memory_object_creation_records_size_in_payload() {
    // Goal: VM MemoryObject carries meaningful owner state, not just a kind tag.
    // Scope: host integration through Syscall::CreateMemoryObject and handle lookup.
    // Semantics: size metadata is owned by the VM payload and visible through object snapshot.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let memory = handle(
        kernel
            .execute(
                SyscallContext::new(process),
                Syscall::CreateMemoryObject {
                    size_bytes: 8192,
                    rights: HandleRights::READ | HandleRights::WRITE,
                },
            )
            .unwrap(),
    );

    let view = kernel
        .lookup_handle(
            process,
            memory,
            ObjectKind::MemoryObject,
            HandleRights::READ,
        )
        .unwrap();

    assert_eq!(view.object.kind(), ObjectKind::MemoryObject);
    assert_eq!(
        view.object.payload,
        ObjectPayload::MemoryObject(
            MemoryObject::new(
                8192,
                MappingPolicy::new(
                    HandleRights::READ | HandleRights::WRITE | HandleRights::EXECUTE
                ),
                FrameRange::new(0x1000, 0x3000).unwrap(),
            )
            .unwrap()
        )
    );
    assert_eq!(
        kernel
            .objects
            .memory_object(view.object.id, view.object.generation)
            .unwrap()
            .size_bytes,
        8192
    );
}

#[test]
fn memory_object_creation_rejects_invalid_size_without_publication() {
    // Goal: MemoryObject size obeys VM page-granularity before object publication.
    // Scope: host integration through Syscall::CreateMemoryObject.
    // Semantics: invalid sizes fail before object manager, handle table, or quota state changes.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let context = SyscallContext::new(process);
    let before_objects = kernel.objects.live_count();
    let before_quota = kernel
        .processes
        .get(process)
        .unwrap()
        .budget
        .remaining_objects();

    for size_bytes in [0, 1, 4097] {
        assert_eq!(
            kernel.execute(
                context,
                Syscall::CreateMemoryObject {
                    size_bytes,
                    rights: HandleRights::READ,
                },
            ),
            Err(KernelError::InvalidArgument)
        );
        let process_state = kernel.processes.get(process).unwrap();
        assert_eq!(kernel.objects.live_count(), before_objects);
        assert_eq!(process_state.handles.live_count(), 1);
        assert_eq!(process_state.budget.remaining_objects(), before_quota);
    }
}

#[test]
fn memory_object_frame_exhaustion_leaves_public_state_unchanged() {
    // Goal: MemoryObject creation reserves frame backing before object publication.
    // Scope: host integration through Syscall::CreateMemoryObject with too few runtime frames.
    // Semantics: NoMemory leaves frame metadata, object manager, handle table, and quota unchanged.
    let mut kernel = Kernel::new(4, 1, &[FrameRange::new(0x1000, 0x2000).unwrap()]).unwrap();
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let context = SyscallContext::new(process);
    let before_objects = kernel.objects.live_count();
    let before_quota = kernel
        .processes
        .get(process)
        .unwrap()
        .budget
        .remaining_objects();

    assert_eq!(
        kernel.execute(
            context,
            Syscall::CreateMemoryObject {
                size_bytes: 0x2000,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::NoMemory)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.frames.free_count(), 1);
    assert_eq!(kernel.objects.live_count(), before_objects);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), before_quota);
}

#[test]
fn closing_last_memory_object_handle_reclaims_unmapped_frames() {
    // Goal: MemoryObject frame backing is reclaimed when the last unmapped handle closes.
    // Scope: host integration through CreateMemoryObject and CloseHandle.
    // Semantics: close destroys the unreferenced MemoryObject and frees its runtime frames.
    let mut kernel = Kernel::new(4, 1, &[FrameRange::new(0x1000, 0x3000).unwrap()]).unwrap();
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let context = SyscallContext::new(process);
    let memory = handle(
        kernel
            .execute(
                context,
                Syscall::CreateMemoryObject {
                    size_bytes: 0x2000,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );
    assert_eq!(kernel.frames.free_count(), 0);

    assert_eq!(
        kernel.execute(context, Syscall::CloseHandle { handle: memory }),
        Ok(SyscallOutcome::Closed)
    );

    assert_eq!(kernel.frames.free_count(), 2);
    assert_eq!(kernel.objects.live_count(), 1);
    assert_eq!(
        kernel.lookup_handle(
            process,
            memory,
            ObjectKind::MemoryObject,
            HandleRights::READ
        ),
        Err(KernelError::InvalidHandle)
    );
}

#[test]
fn generic_memory_object_creation_is_not_supported() {
    // Goal: MemoryObject creation requires a size descriptor and cannot use generic object create.
    // Scope: host integration through Syscall::CreateObject with ObjectKind::MemoryObject.
    // Semantics: Unsupported leaves object manager, handle table, and quota state unchanged.
    let mut kernel = kernel(4, 1);
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let context = SyscallContext::new(process);
    let before_objects = kernel.objects.live_count();
    let before_quota = kernel
        .processes
        .get(process)
        .unwrap()
        .budget
        .remaining_objects();

    assert_eq!(
        kernel.execute(
            context,
            Syscall::CreateObject {
                kind: ObjectKind::MemoryObject,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::Unsupported)
    );

    let process_state = kernel.processes.get(process).unwrap();
    assert_eq!(kernel.objects.live_count(), before_objects);
    assert_eq!(process_state.handles.live_count(), 1);
    assert_eq!(process_state.budget.remaining_objects(), before_quota);
}
