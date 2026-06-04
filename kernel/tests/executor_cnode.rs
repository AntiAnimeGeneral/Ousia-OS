mod support;

use kernel::{
    cap::{
        CNodeCap, CNodePath, CapError, Capability, CapabilityDescriptor, EndpointCap, FrameCap,
        MintParams, RetypeTarget, Rights, TcbCap, UntypedCap,
    },
    invocation::{CNodePathTarget, Invocation},
    object::{KernelObjectRef, ObjectTableError},
    state::{ExecutionOutcome, InvocationContext, KernelExecutionError},
    thread::{
        action::ThreadAction,
        tcb::{CpuId, Tcb, ThreadState},
    },
};
use support::{cpu, thread};

fn root_cnode_cap() -> CNodeCap {
    CNodeCap::new(6)
}

fn target_slot(slot: kernel::cap::SlotId) -> CNodePathTarget {
    CNodePathTarget {
        capptr: slot.raw(),
        depth: 6,
    }
}

fn source_path(root: CapabilityDescriptor, slot: kernel::cap::SlotId) -> CNodePath {
    CNodePath {
        root,
        capptr: slot.raw(),
        depth: 6,
    }
}

fn guarded_cnode(radix: u8, guard: u64, guard_size: u8) -> CNodeCap {
    CNodeCap::with_guard(radix, guard, guard_size)
}

fn windowed_cnode(radix: u8, guard: u64, guard_size: u8) -> CNodeCap {
    CNodeCap::with_guard(radix, guard, guard_size)
}

fn endpoint(rights: Rights, badge: u64) -> Capability {
    Capability::Endpoint(EndpointCap { badge, rights })
}

fn frame(rights: Rights) -> Capability {
    Capability::Frame(FrameCap { rights })
}

fn untyped(size_bits: u8) -> Capability {
    Capability::Untyped(UntypedCap { size_bits })
}

fn cnode_state() -> (kernel::state::KernelState, CapabilityDescriptor) {
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let cnode = state
        .cspace
        .insert_initial_cnode_capability(root_cnode_cap(), kernel::cap::SlotId::new(0))
        .unwrap();
    (state, cnode)
}

fn capability_descriptor(outcome: ExecutionOutcome, context: &str) -> CapabilityDescriptor {
    let ExecutionOutcome::Capability { descriptor } = outcome else {
        panic!("{context}: expected capability outcome");
    };

    descriptor
}

fn retyped_descriptor(outcome: ExecutionOutcome, context: &str) -> CapabilityDescriptor {
    let ExecutionOutcome::Retyped { descriptors } = outcome else {
        panic!("{context}: expected retyped outcome");
    };

    let [descriptor] = descriptors.as_slice() else {
        panic!("{context}: expected one retyped descriptor");
    };
    *descriptor
}

fn configure_thread(state: &mut kernel::state::KernelState, id: u64) -> CapabilityDescriptor {
    configure_thread_on_cpu(state, id, cpu(0))
}

fn configure_thread_on_cpu(
    state: &mut kernel::state::KernelState,
    id: u64,
    affinity: CpuId,
) -> CapabilityDescriptor {
    let descriptor = state
        .cspace
        .insert_initial_capability(Capability::Tcb(TcbCap {
            rights: Rights::MANAGE,
        }))
        .unwrap();
    let object = state.cspace.lookup(descriptor).unwrap().object;
    state.objects.insert_tcb(object).unwrap();
    state
        .insert_thread_object(object, Tcb::new(thread(id), affinity))
        .unwrap();
    descriptor
}

#[test]
fn cnode_copy_mint_and_move_path_commit_to_selected_slot() {
    // Goal: CNode mutations commit through the executor into the path-selected slot.
    // Scope: host integration through CNodeCopyPath, CNodeMintPath, and CNodeMovePath.
    // Semantics: each case preserves its authority semantics while resolving under the invoked CNode.
    struct Case {
        label: &'static str,
        source: Capability,
        destination: u64,
        invocation: fn(CNodePath, CNodePathTarget) -> Invocation,
        expected_capability: Capability,
        invalidates_source: bool,
    }

    let cases = [
        Case {
            label: "copy reduces rights and keeps source live",
            source: endpoint(Rights::READ | Rights::WRITE, 0x42),
            destination: 40,
            invocation: |source, destination| Invocation::CNodeCopyPath {
                source,
                destination,
                requested_rights: Rights::READ,
            },
            expected_capability: endpoint(Rights::READ, 0x42),
            invalidates_source: false,
        },
        Case {
            label: "mint badges without escalating rights",
            source: endpoint(Rights::READ | Rights::WRITE, 0),
            destination: 41,
            invocation: |source, destination| Invocation::CNodeMintPath {
                source,
                destination,
                requested_rights: Rights::READ,
                params: MintParams::badge(0x99),
            },
            expected_capability: endpoint(Rights::READ, 0x99),
            invalidates_source: false,
        },
        Case {
            label: "move transfers object authority and invalidates source",
            source: endpoint(Rights::READ, 0x22),
            destination: 42,
            invocation: |source, destination| Invocation::CNodeMovePath {
                source,
                destination,
            },
            expected_capability: endpoint(Rights::READ, 0x22),
            invalidates_source: true,
        },
    ];

    for case in cases {
        let (mut state, cnode) = cnode_state();
        let source = state.cspace.insert_initial_capability(case.source).unwrap();
        let source_object = state.cspace.lookup(source).unwrap().object;
        let destination = kernel::cap::SlotId::new(case.destination);

        let descriptor = capability_descriptor(
            state
                .execute_invocation(
                    InvocationContext::new(thread(1), cpu(0)),
                    cnode,
                    (case.invocation)(source_path(cnode, source.slot), target_slot(destination)),
                )
                .unwrap(),
            case.label,
        );

        assert_eq!(descriptor.slot, destination, "{}", case.label);
        let view = state.cspace.lookup(descriptor).unwrap();
        assert_eq!(view.capability, case.expected_capability, "{}", case.label);
        assert_eq!(view.object, source_object, "{}", case.label);
        if case.invalidates_source {
            assert!(
                matches!(state.cspace.lookup(source), Err(CapError::SlotNotFound(_))),
                "{}",
                case.label
            );
        } else {
            assert!(state.cspace.lookup(source).is_ok(), "{}", case.label);
        }
    }
}

#[test]
fn cnode_path_copy_mint_and_move_resolve_destination_window() {
    // Goal: path-based CNode mutations resolve guard/radix under the invoked CNode before commit.
    // Scope: host integration through CNodeCopyPath, CNodeMintPath, and CNodeMovePath.
    // Semantics: each case lands in the slot selected by the CNode path, not a raw slot argument.
    struct Case {
        label: &'static str,
        source: Capability,
        window_start: u64,
        capptr: u64,
        invocation: fn(CNodePath, CNodePathTarget) -> Invocation,
        expected_capability: Capability,
        invalidates_source: bool,
    }

    let cases = [
        Case {
            label: "copy path reduces rights and keeps source live",
            source: endpoint(Rights::READ | Rights::WRITE, 0x42),
            window_start: 32,
            capptr: 0b10_0110,
            invocation: |source, destination| Invocation::CNodeCopyPath {
                source,
                destination,
                requested_rights: Rights::READ,
            },
            expected_capability: endpoint(Rights::READ, 0x42),
            invalidates_source: false,
        },
        Case {
            label: "mint path applies badge after path resolution",
            source: endpoint(Rights::READ | Rights::WRITE, 0),
            window_start: 48,
            capptr: 0b10_0011,
            invocation: |source, destination| Invocation::CNodeMintPath {
                source,
                destination,
                requested_rights: Rights::READ,
                params: MintParams::badge(0x77),
            },
            expected_capability: endpoint(Rights::READ, 0x77),
            invalidates_source: false,
        },
        Case {
            label: "move path transfers authority after path resolution",
            source: endpoint(Rights::READ, 0x22),
            window_start: 64,
            capptr: 0b10_0010,
            invocation: |source, destination| Invocation::CNodeMovePath {
                source,
                destination,
            },
            expected_capability: endpoint(Rights::READ, 0x22),
            invalidates_source: true,
        },
    ];

    for case in cases {
        let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
        let cnode = state
            .cspace
            .insert_initial_cnode_capability(
                windowed_cnode(4, 0b10, 2),
                kernel::cap::SlotId::new(case.window_start),
            )
            .unwrap();
        let source_root = state
            .cspace
            .insert_initial_cnode_capability(root_cnode_cap(), kernel::cap::SlotId::new(0))
            .unwrap();
        let source = state.cspace.insert_initial_capability(case.source).unwrap();
        let source_object = state.cspace.lookup(source).unwrap().object;
        let target = CNodePathTarget {
            capptr: case.capptr,
            depth: 6,
        };

        let descriptor = capability_descriptor(
            state
                .execute_invocation(
                    InvocationContext::new(thread(1), cpu(0)),
                    cnode,
                    (case.invocation)(source_path(source_root, source.slot), target),
                )
                .unwrap(),
            case.label,
        );

        assert_eq!(
            descriptor.slot,
            kernel::cap::SlotId::new(case.window_start + (case.capptr & 0b1111)),
            "{}",
            case.label
        );
        let view = state.cspace.lookup(descriptor).unwrap();
        assert_eq!(view.capability, case.expected_capability, "{}", case.label);
        assert_eq!(view.object, source_object, "{}", case.label);
        if case.invalidates_source {
            assert!(
                matches!(state.cspace.lookup(source), Err(CapError::SlotNotFound(_))),
                "{}",
                case.label
            );
        } else {
            assert!(state.cspace.lookup(source).is_ok(), "{}", case.label);
        }
    }
}

#[test]
fn cnode_copy_path_resolves_source_under_explicit_source_root() {
    // Goal: source authority is resolved through its own CNode path, not a raw descriptor.
    // Scope: host integration with distinct source and destination CNode roots.
    // Semantics: the invoked CNode selects the destination while the source root selects source authority.
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let destination_root = state
        .cspace
        .insert_initial_cnode_capability(windowed_cnode(4, 0, 0), kernel::cap::SlotId::new(80))
        .unwrap();
    let source_root = state
        .cspace
        .insert_initial_cnode_capability(root_cnode_cap(), kernel::cap::SlotId::new(0))
        .unwrap();
    let source = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x31))
        .unwrap();
    let source_object = state.cspace.lookup(source).unwrap().object;

    let copied = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                destination_root,
                Invocation::CNodeCopyPath {
                    source: source_path(source_root, source.slot),
                    destination: CNodePathTarget {
                        capptr: 0b0011,
                        depth: 4,
                    },
                    requested_rights: Rights::READ,
                },
            )
            .unwrap(),
        "CNode copy from explicit source root",
    );

    assert_eq!(copied.slot, kernel::cap::SlotId::new(83));
    let copied_view = state.cspace.lookup(copied).unwrap();
    assert_eq!(copied_view.object, source_object);
    assert_eq!(copied_view.capability, endpoint(Rights::READ, 0x31));
}

#[test]
fn cnode_copy_path_rejects_non_cnode_source_root_without_destination_mutation() {
    // Goal: source path root must be CNode authority before CSpace commit.
    // Scope: host integration through source-path lookup failure.
    // Semantics: an invalid source root does not occupy the destination selected by the invoked CNode.
    let (mut state, cnode) = cnode_state();
    let source_root = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x11))
        .unwrap();
    let destination = kernel::cap::SlotId::new(52);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeCopyPath {
                source: source_path(source_root, source_root.slot),
                destination: target_slot(destination),
                requested_rights: Rights::READ,
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::WrongCapability {
                expected: kernel::cap::ObjectKind::CNode,
                actual: kernel::cap::ObjectKind::Endpoint,
            })
        ))
    );

    assert_eq!(
        state.cspace.descriptor_for_live_slot(destination),
        Err(CapError::SlotNotFound(destination))
    );
}

#[test]
fn cnode_copy_path_guard_mismatch_fails_without_source_mutation() {
    // Goal: path lookup faults are preflight failures for CNode copy.
    // Scope: host integration through KernelState::execute_invocation.
    // Semantics: guard mismatch does not derive source authority or occupy the selected slot.
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let cnode = state
        .cspace
        .insert_initial_cnode_capability(guarded_cnode(4, 0b10, 2), kernel::cap::SlotId::new(0))
        .unwrap();
    let source_root = state
        .cspace
        .insert_initial_cnode_capability(root_cnode_cap(), kernel::cap::SlotId::new(0))
        .unwrap();
    let source = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x42))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeCopyPath {
                source: source_path(source_root, source.slot),
                destination: CNodePathTarget {
                    capptr: 0b11_0110,
                    depth: 6,
                },
                requested_rights: Rights::READ,
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::CNodeGuardMismatch {
                expected_guard: 0b10,
                actual_guard: 0b11,
                bits_remaining: 6,
                guard_size: 2,
            })
        ))
    );
    assert_eq!(
        state.cspace.lookup(source).unwrap().capability,
        endpoint(Rights::READ | Rights::WRITE, 0x42)
    );
    assert_eq!(
        state.cspace.lookup(CapabilityDescriptor {
            slot: kernel::cap::SlotId::new(0b0110),
            slot_generation: 1,
        }),
        Err(CapError::SlotNotFound(kernel::cap::SlotId::new(0b0110)))
    );
}

#[test]
fn cnode_delete_path_invalidates_resolved_target() {
    // Goal: path-based CNode delete resolves a live descriptor before mutation.
    // Scope: host integration through KernelState::execute_invocation.
    // Semantics: deleting a resolved target slot leaves sibling slots intact.
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let cnode = state
        .cspace
        .insert_initial_cnode_capability(windowed_cnode(4, 0b10, 2), kernel::cap::SlotId::new(80))
        .unwrap();
    let source = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x1))
        .unwrap();
    let target = state
        .cspace
        .copy_into(source, kernel::cap::SlotId::new(80 + 0b0101), Rights::READ)
        .unwrap();
    let sibling = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x2))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeDeletePath {
                target: CNodePathTarget {
                    capptr: 0b10_0101,
                    depth: 6,
                },
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace.lookup(target),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.cspace.lookup(sibling).unwrap().capability,
        endpoint(Rights::READ, 0x2)
    );
}

#[test]
fn cnode_revoke_path_removes_descendants_but_keeps_resolved_target() {
    // Goal: path-based CNode revoke resolves target authority before descendant traversal.
    // Scope: host integration through KernelState::execute_invocation.
    // Semantics: revoke keeps the resolved target slot and removes MDB descendants.
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    // The transitional descriptor facade allocates the invoked CNode at slot 1;
    // this window starts at the next initial slot so the path resolves to root.
    let cnode = state
        .cspace
        .insert_initial_cnode_capability(windowed_cnode(4, 0b10, 2), kernel::cap::SlotId::new(2))
        .unwrap();
    let root = state
        .cspace
        .insert_initial_capability(frame(Rights::READ | Rights::WRITE))
        .unwrap();
    let child = state.cspace.copy(root, Rights::READ).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: CNodePathTarget {
                    capptr: 0b10_0000,
                    depth: 6,
                },
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(state.cspace.lookup(root).is_ok());
    assert!(matches!(
        state.cspace.lookup(child),
        Err(CapError::SlotNotFound(_))
    ));
}

#[test]
fn cnode_copy_and_mint_path_to_occupied_destination_preserve_source() {
    // Goal: derivation-style CNode operations validate destination emptiness before source mutation.
    // Scope: host integration of CNodeCopyPath and CNodeMintPath failure paths.
    // Semantics: occupied destination fails, source authority remains unchanged, and occupied cap survives.
    struct Case {
        label: &'static str,
        source: Capability,
        invocation: fn(CNodePath, CNodePathTarget) -> Invocation,
    }

    let cases = [
        Case {
            label: "copy does not derive into an occupied slot",
            source: endpoint(Rights::READ, 0x33),
            invocation: |source, destination| Invocation::CNodeCopyPath {
                source,
                destination,
                requested_rights: Rights::READ,
            },
        },
        Case {
            label: "mint does not badge into an occupied slot",
            source: endpoint(Rights::READ | Rights::WRITE, 0),
            invocation: |source, destination| Invocation::CNodeMintPath {
                source,
                destination,
                requested_rights: Rights::READ,
                params: MintParams::badge(0x99),
            },
        },
    ];

    for case in cases {
        let (mut state, cnode) = cnode_state();
        let source = state
            .cspace
            .insert_initial_capability(case.source.clone())
            .unwrap();
        let occupied = state
            .cspace
            .insert_initial_capability(endpoint(Rights::READ, 0x44))
            .unwrap();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                (case.invocation)(source_path(cnode, source.slot), target_slot(occupied.slot)),
            ),
            Err(KernelExecutionError::Invocation(
                kernel::invocation::InvocationError::Cap(CapError::SlotOccupied(occupied.slot))
            )),
            "{}",
            case.label
        );
        assert_eq!(
            state.cspace.lookup(source).unwrap().capability,
            case.source,
            "{}",
            case.label
        );
        assert_eq!(
            state.cspace.lookup(occupied).unwrap().capability,
            endpoint(Rights::READ, 0x44),
            "{}",
            case.label
        );
    }
}

#[test]
fn cnode_mint_rejects_rebadging_badged_endpoint() {
    // Goal: CNode mint follows seL4 updateCapData preserve rules for endpoint badges.
    // Scope: host integration across invocation authorization and CSpace mutation failure.
    // Semantics: a nonzero endpoint badge cannot be replaced by another badge.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x11))
        .unwrap();
    let destination = kernel::cap::SlotId::new(44);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeMintPath {
                source: source_path(cnode, source.slot),
                destination: target_slot(destination),
                requested_rights: Rights::READ,
                params: MintParams::badge(0x99),
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::CapabilityNotMintable {
                parent: source.slot,
                capability: endpoint(Rights::READ | Rights::WRITE, 0x11),
                params: MintParams::badge(0x99),
            })
        ))
    );
    assert_eq!(
        state.cspace.lookup(source).unwrap().capability,
        endpoint(Rights::READ | Rights::WRITE, 0x11)
    );
}

#[test]
fn cnode_move_path_to_occupied_destination_fails_before_source_lookup() {
    // Goal: CNode move follows seL4 decode ordering by checking destination emptiness first.
    // Scope: host integration of CNodeMovePath failure path.
    // Semantics: occupied destination is reported even if the source path names a deleted slot.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x22))
        .unwrap();
    let occupied = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x44))
        .unwrap();
    state.cspace.delete(source).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeMovePath {
                source: source_path(cnode, source.slot),
                destination: target_slot(occupied.slot),
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::SlotOccupied(occupied.slot))
        ))
    );
    assert_eq!(
        state.cspace.lookup(occupied).unwrap().capability,
        endpoint(Rights::READ, 0x44)
    );
}

#[test]
fn cnode_delete_invalidates_target_without_touching_sibling() {
    // Goal: CNode delete removes exactly the target slot through executor dispatch.
    // Scope: host integration of delete mutation and sibling preservation.
    // Semantics: deleting one cap does not revoke unrelated caps or mutate ObjectTable.
    let (mut state, cnode) = cnode_state();
    let target = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x1))
        .unwrap();
    let sibling = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x2))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeDeletePath {
                target: target_slot(target.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace.lookup(target),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.cspace.lookup(sibling).unwrap().capability,
        endpoint(Rights::READ, 0x2)
    );
}

#[test]
fn cnode_revoke_removes_descendants_but_keeps_target() {
    // Goal: CNode revoke exposes descendant revocation through the executor.
    // Scope: host integration for CSpace lineage mutation.
    // Semantics: descendants are removed, while the revoked slot itself remains usable.
    let (mut state, cnode) = cnode_state();
    let root = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x7))
        .unwrap();
    let child = state.cspace.copy(root, Rights::READ).unwrap();
    let grandchild = state.cspace.copy(child, Rights::READ).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: target_slot(root.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(state.cspace.lookup(root).is_ok());
    assert!(matches!(
        state.cspace.lookup(child),
        Err(CapError::SlotNotFound(_))
    ));
    assert!(matches!(
        state.cspace.lookup(grandchild),
        Err(CapError::SlotNotFound(_))
    ));
}

#[test]
fn cnode_revoke_untyped_descendants_recovers_capacity() {
    // Goal: CNode revoke reaches Untyped capacity reset through the executor.
    // Scope: host integration for CSpace lineage and Untyped watermark mutation.
    // Semantics: revoked descendants disappear and the parent Untyped can retype again.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    let frame = state
        .cspace
        .retype_untyped(
            root,
            RetypeTarget::Frame {
                rights: Rights::READ,
            },
        )
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: target_slot(root.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace.lookup(frame),
        Err(CapError::SlotNotFound(_))
    ));
    let recycled = state
        .cspace
        .retype_untyped(
            root,
            RetypeTarget::Frame {
                rights: Rights::READ,
            },
        )
        .unwrap();
    assert_eq!(
        state.cspace.lookup(recycled).unwrap().capability,
        Capability::Frame(FrameCap {
            rights: Rights::READ,
        })
    );
}

#[test]
fn cnode_revoke_untyped_descendants_removes_unreachable_runtime_object() {
    // Goal: seL4-style CNode revoke over an Untyped parent tears down unreachable
    // runtime object state created by retype.
    // Scope: host integration across CSpace lineage, Untyped capacity reset, and ObjectTable cleanup.
    // Semantics: once the revoked Frame cap has no live aliases, ObjectTable no longer exposes it.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    let frame = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                root,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            )
            .unwrap(),
        "Frame retype",
    );
    let frame_object = state.cspace.lookup(frame).unwrap().object;

    assert_eq!(
        state.objects.get(frame_object),
        Ok(KernelObjectRef::Frame { size_bits: 12 })
    );
    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: target_slot(root.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace.lookup(frame),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.objects.get(frame_object),
        Err(ObjectTableError::ObjectNotFound {
            object: frame_object,
        })
    );
    let recycled = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                root,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            )
            .unwrap(),
        "Frame retype after revoke",
    );
    let recycled_object = state.cspace.lookup(recycled).unwrap().object;
    assert_eq!(
        state.objects.get(recycled_object),
        Ok(KernelObjectRef::Frame { size_bits: 12 })
    );
}

#[test]
fn cnode_revoke_typed_descendants_keeps_target_runtime_object() {
    // Goal: CNode revoke follows seL4 descendant semantics without deleting the target cap's object.
    // Scope: host integration across Endpoint retype, CNode mint, CNode revoke, and ObjectTable cleanup.
    // Semantics: revocable badged descendants disappear, but the target Endpoint remains live.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    let endpoint = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                root,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Endpoint,
                },
            )
            .unwrap(),
        "Endpoint retype",
    );
    let endpoint_object = state.cspace.lookup(endpoint).unwrap().object;
    let alias = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMintPath {
                    source: source_path(cnode, endpoint.slot),
                    destination: target_slot(kernel::cap::SlotId::new(46)),
                    requested_rights: Rights::READ,
                    params: MintParams::badge(0x77),
                },
            )
            .unwrap(),
        "Endpoint badged alias mint",
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: target_slot(endpoint.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(state.cspace.lookup(endpoint).is_ok());
    assert!(matches!(
        state.cspace.lookup(alias),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.objects.get(endpoint_object),
        Ok(KernelObjectRef::Endpoint)
    );
}

#[test]
fn cnode_revoke_untyped_endpoint_finalises_runtime_object() {
    // Goal: CNode revoke reaches seL4-style final cap cleanup for Endpoint objects.
    // Scope: host integration across Untyped retype, CNode revoke, and ObjectTable finalisation.
    // Semantics: the final Endpoint cap disappears and its runtime object is removed.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    let endpoint = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                root,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Endpoint,
                },
            )
            .unwrap(),
        "Endpoint retype",
    );
    let endpoint_object = state.cspace.lookup(endpoint).unwrap().object;

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: target_slot(root.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace.lookup(endpoint),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.objects.get(endpoint_object),
        Err(ObjectTableError::ObjectNotFound {
            object: endpoint_object,
        })
    );
}

#[test]
fn cnode_revoke_endpoint_restarts_blocked_sender_before_removing_object() {
    // Goal: Endpoint finalisation follows seL4 cancelAllIPC semantics, not only ObjectTable removal.
    // Scope: host integration across TCB scheduling, Endpoint send blocking, and CNode revoke.
    // Semantics: a sender blocked on the final Endpoint cap is restarted and requeued when revoke finalises it.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    let endpoint = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                root,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Endpoint,
                },
            )
            .unwrap(),
        "Endpoint retype",
    );
    let endpoint_object = state.cspace.lookup(endpoint).unwrap().object;
    let sender_tcb = configure_thread(&mut state, 7);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(7), cpu(0)),
            sender_tcb,
            Invocation::TcbResume,
        ),
        Ok(ExecutionOutcome::Thread(ThreadAction::Resumed {
            thread: thread(7),
            cpu: cpu(0),
            scheduler: kernel::scheduler::SchedulerAction::Enqueued {
                thread: thread(7),
                cpu: cpu(0),
            },
        }))
    );
    state.scheduler.schedule_next(cpu(0)).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(7), cpu(0)),
            endpoint,
            Invocation::EndpointSend {
                message_words: 0,
                op: kernel::invocation::EndpointSendOp::Send,
            },
        ),
        Ok(ExecutionOutcome::Thread(ThreadAction::Blocked {
            thread: thread(7),
            cpu: cpu(0),
        }))
    );
    assert_eq!(
        state.threads.state(thread(7)),
        Some(ThreadState::BlockedOnSend {
            endpoint: endpoint_object,
            sender_cpu: cpu(0),
            badge: 0,
            can_grant: true,
            can_grant_reply: true,
            is_call: false,
            payload: kernel::ipc::IpcPayload::empty(),
        })
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: target_slot(root.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert_eq!(state.threads.state(thread(7)), Some(ThreadState::Restart));
    assert_eq!(
        state.scheduler.placement(thread(7)),
        Some(kernel::scheduler::ThreadPlacement::Ready { cpu: cpu(0) })
    );
    assert_eq!(
        state.objects.get(endpoint_object),
        Err(ObjectTableError::ObjectNotFound {
            object: endpoint_object,
        })
    );
}

#[test]
fn cnode_revoke_notification_restarts_waiter_from_tcb_blocked_cpu() {
    // Goal: Notification finalisation consumes waiter CPU metadata from TCB state.
    // Scope: host integration across Notification wait blocking, ThreadTable state, and CNode revoke.
    // Semantics: a waiter blocked on a final Notification cap is restarted on its recorded receiver CPU.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    let notification = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                root,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Notification,
                },
            )
            .unwrap(),
        "Notification retype",
    );
    let notification_object = state.cspace.lookup(notification).unwrap().object;
    let waiter_tcb = configure_thread_on_cpu(&mut state, 11, cpu(1));

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(11), cpu(1)),
            waiter_tcb,
            Invocation::TcbResume,
        ),
        Ok(ExecutionOutcome::Thread(ThreadAction::Resumed {
            thread: thread(11),
            cpu: cpu(1),
            scheduler: kernel::scheduler::SchedulerAction::Enqueued {
                thread: thread(11),
                cpu: cpu(1),
            },
        }))
    );
    state.scheduler.schedule_next(cpu(1)).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(11), cpu(1)),
            notification,
            Invocation::NotificationWait { blocking: true },
        ),
        Ok(ExecutionOutcome::Thread(ThreadAction::Blocked {
            thread: thread(11),
            cpu: cpu(1),
        }))
    );
    assert_eq!(
        state.threads.state(thread(11)),
        Some(ThreadState::BlockedOnNotification {
            notification: notification_object,
            receiver_cpu: cpu(1),
        })
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevokePath {
                target: target_slot(root.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert_eq!(state.threads.state(thread(11)), Some(ThreadState::Restart));
    assert_eq!(
        state.scheduler.placement(thread(11)),
        Some(kernel::scheduler::ThreadPlacement::Ready { cpu: cpu(1) })
    );
    assert_eq!(
        state.objects.get(notification_object),
        Err(ObjectTableError::ObjectNotFound {
            object: notification_object,
        })
    );
}

#[test]
fn cnode_delete_final_tcb_cap_removes_thread_scheduler_and_runtime_object() {
    // Goal: CNode delete reaches the seL4 thread finalisation boundary for a final TCB cap.
    // Scope: host integration across CSpace delete, ObjectTable TCB binding, ThreadTable, and Scheduler.
    // Semantics: deleting the final TCB cap suspends/removes thread state and clears scheduler placement.
    let (mut state, cnode) = cnode_state();
    let tcb = configure_thread(&mut state, 8);
    let tcb_object = state.cspace.lookup(tcb).unwrap().object;

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(8), cpu(0)),
            tcb,
            Invocation::TcbResume,
        ),
        Ok(ExecutionOutcome::Thread(ThreadAction::Resumed {
            thread: thread(8),
            cpu: cpu(0),
            scheduler: kernel::scheduler::SchedulerAction::Enqueued {
                thread: thread(8),
                cpu: cpu(0),
            },
        }))
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeDeletePath {
                target: target_slot(tcb.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert_eq!(state.threads.get(thread(8)), None);
    assert_eq!(state.scheduler.placement(thread(8)), None);
    assert_eq!(
        state.objects.get(tcb_object),
        Err(ObjectTableError::ObjectNotFound { object: tcb_object })
    );
}

#[test]
fn cnode_delete_blocked_tcb_removes_endpoint_queue_entry() {
    // Goal: TCB finalisation performs seL4-style cancelIPC before removing the TCB runtime object.
    // Scope: host integration across Endpoint queue state, ThreadTable, Scheduler, and CNode delete.
    // Semantics: deleting a TCB blocked on an Endpoint removes its queued sender while keeping the Endpoint live.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    let endpoint = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                root,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Endpoint,
                },
            )
            .unwrap(),
        "Endpoint retype",
    );
    let endpoint_object = state.cspace.lookup(endpoint).unwrap().object;
    let sender_tcb = configure_thread(&mut state, 9);
    let sender_object = state.cspace.lookup(sender_tcb).unwrap().object;

    state
        .execute_invocation(
            InvocationContext::new(thread(9), cpu(0)),
            sender_tcb,
            Invocation::TcbResume,
        )
        .unwrap();
    state.scheduler.schedule_next(cpu(0)).unwrap();
    state
        .execute_invocation(
            InvocationContext::new(thread(9), cpu(0)),
            endpoint,
            Invocation::EndpointSend {
                message_words: 0,
                op: kernel::invocation::EndpointSendOp::Send,
            },
        )
        .unwrap();

    assert_eq!(
        state
            .objects
            .endpoint(endpoint_object)
            .unwrap()
            .queued_senders(),
        1
    );
    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeDeletePath {
                target: target_slot(sender_tcb.slot),
            },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert_eq!(
        state
            .objects
            .endpoint(endpoint_object)
            .unwrap()
            .queued_senders(),
        0
    );
    assert_eq!(state.threads.get(thread(9)), None);
    assert_eq!(state.scheduler.placement(thread(9)), None);
    assert_eq!(
        state.objects.get(sender_object),
        Err(ObjectTableError::ObjectNotFound {
            object: sender_object
        })
    );
    assert_eq!(
        state.objects.get(endpoint_object),
        Ok(KernelObjectRef::Endpoint)
    );
}

#[test]
fn cnode_copy_rejects_untyped_with_children_without_recovering_capacity() {
    // Goal: seL4 deriveCap rejects copying an Untyped cap while it has children.
    // Scope: host integration across Untyped retype, CNode copy, and capacity state.
    // Semantics: copy fails before creating an alias and leaves Untyped capacity consumed.
    let (mut state, cnode) = cnode_state();
    let root = state.cspace.insert_initial_capability(untyped(12)).unwrap();
    state
        .cspace
        .retype_untyped(
            root,
            RetypeTarget::Frame {
                rights: Rights::READ,
            },
        )
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeCopyPath {
                source: source_path(cnode, root.slot),
                destination: target_slot(kernel::cap::SlotId::new(47)),
                requested_rights: Rights::NONE,
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::CapabilityNotDerivable {
                parent: root.slot,
                capability: untyped(12),
            })
        ))
    );

    assert_eq!(
        state.cspace.retype_untyped(root, RetypeTarget::Endpoint),
        Err(CapError::UntypedCapacityExhausted {
            parent: root.slot,
            requested: 4,
            source: 12,
        })
    );
}

#[test]
fn cnode_copy_rights_failure_does_not_consume_new_slot() {
    // Goal: failed CNode copy leaves the path-selected destination reusable and source authority unchanged.
    // Scope: host integration of CapabilitySpace failure-before-side-effect via executor.
    // Semantics: rights escalation is rejected before a child slot is inserted.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x33))
        .unwrap();
    let destination = kernel::cap::SlotId::new(48);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeCopyPath {
                source: source_path(cnode, source.slot),
                destination: target_slot(destination),
                requested_rights: Rights::READ | Rights::WRITE,
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::RightsEscalation {
                parent: source.slot,
                parent_rights: Rights::READ,
                requested_rights: Rights::READ | Rights::WRITE,
            })
        ))
    );

    let later = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeCopyPath {
                    source: source_path(cnode, source.slot),
                    destination: target_slot(destination),
                    requested_rights: Rights::READ,
                },
            )
            .unwrap(),
        "later CNode copy",
    );
    assert_eq!(later.slot, destination);
    assert!(state.cspace.lookup(source).is_ok());
}

#[test]
fn cnode_operation_requires_cnode_capability_without_mutating_source() {
    // Goal: invoking CNode authority is checked before the source slot is touched.
    // Scope: host integration of authorization failure before CSpace mutation.
    // Semantics: a non-CNode cap cannot mutate source or target slots.
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let invoking_endpoint = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x43))
        .unwrap();
    let target = state
        .cspace
        .insert_initial_capability(endpoint(Rights::READ, 0x44))
        .unwrap();
    let cases = [
        Invocation::CNodeCopyPath {
            source: source_path(invoking_endpoint, target.slot),
            destination: target_slot(kernel::cap::SlotId::new(49)),
            requested_rights: Rights::READ,
        },
        Invocation::CNodeMintPath {
            source: source_path(invoking_endpoint, target.slot),
            destination: target_slot(kernel::cap::SlotId::new(50)),
            requested_rights: Rights::READ,
            params: MintParams::badge(0x45),
        },
        Invocation::CNodeMovePath {
            source: source_path(invoking_endpoint, target.slot),
            destination: target_slot(kernel::cap::SlotId::new(51)),
        },
        Invocation::CNodeDeletePath {
            target: target_slot(target.slot),
        },
        Invocation::CNodeRevokePath {
            target: target_slot(target.slot),
        },
    ];

    for invocation in cases {
        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                invoking_endpoint,
                invocation,
            ),
            Err(KernelExecutionError::Invocation(
                kernel::invocation::InvocationError::WrongCapability {
                    expected: kernel::invocation::InvocationTarget::CNode,
                    actual: endpoint(Rights::READ, 0x43),
                }
            ))
        );
        assert_eq!(
            state.cspace.lookup(target).unwrap().capability,
            endpoint(Rights::READ, 0x44)
        );
    }
}
