use kernel::{
    error::KernelError,
    handle::{HandleRights, HandleValue},
    object::{EventState, ObjectKind, ObjectPayload},
    syscall::{Kernel, Syscall, SyscallContext, SyscallOutcome},
    vm::{MappingPolicy, MemoryObject},
};

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
    let mut kernel = Kernel::new(4, 1).unwrap();
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
    let mut kernel = Kernel::new(4, 1).unwrap();
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
    let mut kernel = Kernel::new(4, 1).unwrap();
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
        ObjectPayload::MemoryObject(MemoryObject::new(
            8192,
            MappingPolicy::new(HandleRights::READ | HandleRights::WRITE | HandleRights::EXECUTE),
        ))
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
