use kernel::{
    error::KernelError,
    handle::{HandleRights, HandleValue},
    object::{ObjectKind, ObjectState},
    syscall::{Kernel, Syscall, SyscallContext, SyscallOutcome},
};

fn handle(outcome: SyscallOutcome) -> HandleValue {
    let SyscallOutcome::Handle { handle } = outcome else {
        panic!("expected handle outcome");
    };
    handle
}

#[test]
fn bootstrap_process_installs_live_process_handle() {
    // Goal: bootstrap establishes the first Ousia-native process authority.
    // Scope: host integration through Kernel bootstrap and handle lookup.
    // Semantics: the process owns a live Process object handle with stable rights.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(4, 2).unwrap();

    let view = kernel
        .lookup_handle(
            process,
            HandleValue::new(0, kernel::handle::HandleGeneration::INITIAL),
            ObjectKind::Process,
            HandleRights::READ,
        )
        .unwrap();

    assert_eq!(view.object.kind(), ObjectKind::Process);
    assert_eq!(view.object.state, ObjectState::Live);
    assert!(view.entry.rights.contains(HandleRights::MANAGE));
}

#[test]
fn duplicate_handle_reduces_rights_and_keeps_source_live() {
    // Goal: handle duplication can only create a rights subset.
    // Scope: host integration through Syscall::DuplicateHandle.
    // Semantics: source authority remains live while derived handle has narrower rights.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let context = SyscallContext::new(process);
    let source = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::DUPLICATE,
                },
            )
            .unwrap(),
    );

    let derived = handle(
        kernel
            .execute(
                context,
                Syscall::DuplicateHandle {
                    source,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );

    assert!(
        kernel
            .lookup_handle(process, source, ObjectKind::Event, HandleRights::WRITE)
            .is_ok()
    );
    let derived_view = kernel
        .lookup_handle(process, derived, ObjectKind::Event, HandleRights::READ)
        .unwrap();
    assert_eq!(derived_view.entry.rights, HandleRights::READ);
    assert_eq!(derived_view.object.handle_count, 2);
}

#[test]
fn duplicate_handle_rejects_rights_expansion_without_state_change() {
    // Goal: rights derivation cannot expand authority.
    // Scope: host integration through Syscall::DuplicateHandle failure.
    // Semantics: failed duplication does not install a destination handle or alter handle counts.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let context = SyscallContext::new(process);
    let source = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::DUPLICATE,
                },
            )
            .unwrap(),
    );
    let before = kernel
        .lookup_handle(process, source, ObjectKind::Event, HandleRights::READ)
        .unwrap()
        .object;

    assert_eq!(
        kernel.execute(
            context,
            Syscall::DuplicateHandle {
                source,
                rights: HandleRights::READ | HandleRights::WRITE,
            },
        ),
        Err(KernelError::MissingRights)
    );

    let after = kernel
        .lookup_handle(process, source, ObjectKind::Event, HandleRights::READ)
        .unwrap()
        .object;
    assert_eq!(after.handle_count, before.handle_count);
    assert_eq!(kernel.objects.live_count(), 2);
}

#[test]
fn close_invalidates_only_named_handle() {
    // Goal: close removes one handle without destroying a still-referenced object.
    // Scope: host integration through duplicate and close syscalls.
    // Semantics: derived authority remains live; closed handle is no longer usable.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(4, 3).unwrap();
    let context = SyscallContext::new(process);
    let source = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::DUPLICATE,
                },
            )
            .unwrap(),
    );
    let derived = handle(
        kernel
            .execute(
                context,
                Syscall::DuplicateHandle {
                    source,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );

    assert_eq!(
        kernel.execute(context, Syscall::CloseHandle { handle: source }),
        Ok(SyscallOutcome::Closed)
    );

    assert_eq!(
        kernel.lookup_handle(process, source, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::InvalidHandle)
    );
    let view = kernel
        .lookup_handle(process, derived, ObjectKind::Event, HandleRights::READ)
        .unwrap();
    assert_eq!(view.object.handle_count, 1);
}

#[test]
fn destroyed_object_makes_existing_handle_dead() {
    // Goal: object generation is the single lifetime authority.
    // Scope: host integration through object manager destroy and handle lookup.
    // Semantics: an existing handle fails once its object generation changes.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(4, 2).unwrap();
    let context = SyscallContext::new(process);
    let handle = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );
    let view = kernel
        .lookup_handle(process, handle, ObjectKind::Event, HandleRights::READ)
        .unwrap();

    kernel
        .objects
        .destroy(view.object.id, view.object.generation)
        .unwrap();

    assert_eq!(
        kernel.lookup_handle(process, handle, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::DeadObject)
    );
}

#[test]
fn reused_slot_rejects_old_handle_generation() {
    // Goal: handle generation rejects ABA after a table slot is reused.
    // Scope: host integration through close, create, and lookup.
    // Semantics: old handle values cannot name a new object installed in the same slot.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(2, 3).unwrap();
    let context = SyscallContext::new(process);
    let old = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );
    kernel
        .execute(context, Syscall::CloseHandle { handle: old })
        .unwrap();
    let new = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );

    assert_eq!(old.index(), new.index());
    assert_ne!(old.generation(), new.generation());
    assert_eq!(
        kernel.lookup_handle(process, old, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::StaleHandle)
    );
    assert!(
        kernel
            .lookup_handle(process, new, ObjectKind::Event, HandleRights::READ)
            .is_ok()
    );
}

#[test]
fn stale_handle_cannot_duplicate_or_close_reused_slot() {
    // Goal: stale generation protects every handle-consuming boundary.
    // Scope: host integration through duplicate and close after slot reuse.
    // Semantics: old handle values cannot mutate authority for a newly installed object.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(2, 4).unwrap();
    let context = SyscallContext::new(process);
    let old = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::DUPLICATE,
                },
            )
            .unwrap(),
    );
    kernel
        .execute(context, Syscall::CloseHandle { handle: old })
        .unwrap();
    let new = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::DUPLICATE,
                },
            )
            .unwrap(),
    );

    assert_eq!(
        kernel.execute(
            context,
            Syscall::DuplicateHandle {
                source: old,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::StaleHandle)
    );
    assert_eq!(
        kernel.execute(context, Syscall::CloseHandle { handle: old }),
        Err(KernelError::StaleHandle)
    );
    assert!(
        kernel
            .lookup_handle(process, new, ObjectKind::Event, HandleRights::READ)
            .is_ok()
    );
}

#[test]
fn close_dead_object_fails_without_mutating_handle_slot() {
    // Goal: close preflights object lifetime before mutating the handle table.
    // Scope: host integration after object manager destroys the target object.
    // Semantics: DeadObject failure leaves the handle slot observable as the same dead handle.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(2, 3).unwrap();
    let context = SyscallContext::new(process);
    let target = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );
    let view = kernel
        .lookup_handle(process, target, ObjectKind::Event, HandleRights::READ)
        .unwrap();
    kernel
        .objects
        .destroy(view.object.id, view.object.generation)
        .unwrap();

    assert_eq!(
        kernel.execute(context, Syscall::CloseHandle { handle: target }),
        Err(KernelError::DeadObject)
    );
    assert_eq!(
        kernel.lookup_handle(process, target, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::DeadObject)
    );
}

#[test]
fn revoke_descendants_removes_derived_handles_but_keeps_root() {
    // Goal: process-local revoke removes derived authority without deleting the root handle.
    // Scope: host integration through duplicate lineage and RevokeDescendants.
    // Semantics: descendants lose access, root authority and object handle count remain valid.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(4, 4).unwrap();
    let context = SyscallContext::new(process);
    let root = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::DUPLICATE,
                },
            )
            .unwrap(),
    );
    let child = handle(
        kernel
            .execute(
                context,
                Syscall::DuplicateHandle {
                    source: root,
                    rights: HandleRights::READ | HandleRights::DUPLICATE,
                },
            )
            .unwrap(),
    );
    let grandchild = handle(
        kernel
            .execute(
                context,
                Syscall::DuplicateHandle {
                    source: child,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );

    assert_eq!(
        kernel.execute(context, Syscall::RevokeDescendants { root }),
        Ok(SyscallOutcome::Revoked { count: 2 })
    );

    let root_view = kernel
        .lookup_handle(process, root, ObjectKind::Event, HandleRights::READ)
        .unwrap();
    assert_eq!(root_view.object.handle_count, 1);
    assert_eq!(
        kernel.lookup_handle(process, child, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::InvalidHandle)
    );
    assert_eq!(
        kernel.lookup_handle(process, grandchild, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::InvalidHandle)
    );
}
