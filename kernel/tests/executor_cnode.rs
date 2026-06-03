mod support;

use kernel::{
    cap::{
        CNodeCap, CapError, Capability, CapabilityDescriptor, EndpointCap, FrameCap, MintParams,
        RetypeTarget, Rights, TcbCap, UntypedCap,
    },
    invocation::Invocation,
    object::{KernelObjectRef, ObjectTableError},
    state::{ExecutionOutcome, InvocationContext, KernelExecutionError},
    tcb::{Tcb, ThreadState},
};
use support::{cpu, thread};

fn cnode() -> Capability {
    Capability::CNode(CNodeCap::new(4))
}

fn endpoint(rights: Rights, badge: u64) -> Capability {
    Capability::Endpoint(EndpointCap { badge, rights })
}

fn untyped(size_bits: u8) -> Capability {
    Capability::Untyped(UntypedCap { size_bits })
}

fn cnode_state() -> (kernel::state::KernelState, CapabilityDescriptor) {
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let cnode = state
        .cspace_mut()
        .insert_initial_capability(cnode())
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
    let descriptor = state
        .cspace_mut()
        .insert_initial_capability(Capability::Tcb(TcbCap {
            rights: Rights::MANAGE,
        }))
        .unwrap();
    let object = state.cspace().lookup(descriptor).unwrap().object;
    state.objects_mut().insert_tcb(object).unwrap();
    state
        .insert_thread_object(object, Tcb::new(thread(id), cpu(0)))
        .unwrap();
    descriptor
}

#[test]
fn cnode_copy_commits_derived_capability_through_executor() {
    // Goal: CNode copy is a real executor path, not only a CapabilitySpace helper.
    // Scope: host integration through KernelState::execute_invocation.
    // Semantics: the copied cap preserves badge, reduces rights, and keeps source alive.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x42))
        .unwrap();
    let destination = kernel::cap::SlotId::from_raw(39);

    let copied = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeCopyInto {
                    source,
                    destination,
                    requested_rights: Rights::READ,
                },
            )
            .unwrap(),
        "CNode copy",
    );

    assert_eq!(
        state.cspace().lookup(copied).unwrap().capability,
        endpoint(Rights::READ, 0x42)
    );
    assert!(state.cspace().lookup(source).is_ok());
}

#[test]
fn cnode_copy_into_commits_to_requested_empty_slot() {
    // Goal: CNode copy uses the caller-selected destination slot like seL4 cteInsert.
    // Scope: host integration through explicit destination CNodeCopyInto invocation.
    // Semantics: the new descriptor lives exactly at the requested empty slot.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x42))
        .unwrap();
    let destination = kernel::cap::SlotId::from_raw(40);

    let copied = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeCopyInto {
                    source,
                    destination,
                    requested_rights: Rights::READ,
                },
            )
            .unwrap(),
        "CNode copy into",
    );

    assert_eq!(copied.slot, destination);
    assert_eq!(
        state.cspace().lookup(copied).unwrap().capability,
        endpoint(Rights::READ, 0x42)
    );
}

#[test]
fn cnode_copy_into_occupied_destination_fails_without_source_mutation() {
    // Goal: CNode copy validates destination emptiness before deriving source authority.
    // Scope: host integration of explicit CNodeCopyInto failure path.
    // Semantics: occupied destination fails and source remains live with no replacement.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x33))
        .unwrap();
    let occupied = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x44))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeCopyInto {
                source,
                destination: occupied.slot,
                requested_rights: Rights::READ,
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::SlotOccupied(occupied.slot))
        ))
    );

    assert_eq!(
        state.cspace().lookup(source).unwrap().capability,
        endpoint(Rights::READ, 0x33)
    );
    assert_eq!(
        state.cspace().lookup(occupied).unwrap().capability,
        endpoint(Rights::READ, 0x44)
    );
}

#[test]
fn cnode_mint_sets_badge_without_escalating_rights() {
    // Goal: CNode mint commits cap-specific mint parameters through the executor.
    // Scope: host integration across invocation authorization and CSpace mutation.
    // Semantics: seL4 updateCapData sets a badge only on an unbadged endpoint cap.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0))
        .unwrap();
    let destination = kernel::cap::SlotId::from_raw(43);

    let minted = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMintInto {
                    source,
                    destination,
                    requested_rights: Rights::READ,
                    params: MintParams::badge(0x99),
                },
            )
            .unwrap(),
        "CNode mint",
    );

    assert_eq!(
        state.cspace().lookup(minted).unwrap().capability,
        endpoint(Rights::READ, 0x99)
    );
}

#[test]
fn cnode_mint_into_commits_badged_cap_to_requested_slot() {
    // Goal: CNode mint combines updateCapData with an explicit destination slot.
    // Scope: host integration through CNodeMintInto.
    // Semantics: badge minting succeeds only into the caller-selected empty slot.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0))
        .unwrap();
    let destination = kernel::cap::SlotId::from_raw(41);

    let minted = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMintInto {
                    source,
                    destination,
                    requested_rights: Rights::READ,
                    params: MintParams::badge(0x99),
                },
            )
            .unwrap(),
        "CNode mint into",
    );

    assert_eq!(minted.slot, destination);
    assert_eq!(
        state.cspace().lookup(minted).unwrap().capability,
        endpoint(Rights::READ, 0x99)
    );
}

#[test]
fn cnode_mint_into_occupied_destination_fails_without_source_mutation() {
    // Goal: CNode mint validates destination emptiness before minting cap data.
    // Scope: host integration of explicit CNodeMintInto failure path.
    // Semantics: occupied destination fails and source remains unbadged.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0))
        .unwrap();
    let occupied = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x44))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeMintInto {
                source,
                destination: occupied.slot,
                requested_rights: Rights::READ,
                params: MintParams::badge(0x99),
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::SlotOccupied(occupied.slot))
        ))
    );
    assert_eq!(
        state.cspace().lookup(source).unwrap().capability,
        endpoint(Rights::READ | Rights::WRITE, 0)
    );
}

#[test]
fn cnode_mint_rejects_rebadging_badged_endpoint() {
    // Goal: CNode mint follows seL4 updateCapData preserve rules for endpoint badges.
    // Scope: host integration across invocation authorization and CSpace mutation failure.
    // Semantics: a nonzero endpoint badge cannot be replaced by another badge.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x11))
        .unwrap();
    let destination = kernel::cap::SlotId::from_raw(44);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeMintInto {
                source,
                destination,
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
        state.cspace().lookup(source).unwrap().capability,
        endpoint(Rights::READ | Rights::WRITE, 0x11)
    );
}

#[test]
fn cnode_move_transfers_authority_and_invalidates_source_descriptor() {
    // Goal: CNode move reaches the CSpace owner through a real executor path.
    // Scope: host integration for slot transfer semantics.
    // Semantics: moved authority keeps the object but the old descriptor becomes stale.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x22))
        .unwrap();
    let source_object = state.cspace().lookup(source).unwrap().object;
    let destination = kernel::cap::SlotId::from_raw(45);

    let moved = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMoveInto {
                    source,
                    destination,
                },
            )
            .unwrap(),
        "CNode move",
    );

    assert_eq!(state.cspace().lookup(moved).unwrap().object, source_object);
    assert!(matches!(
        state.cspace().lookup(source),
        Err(CapError::SlotNotFound(_))
    ));
}

#[test]
fn cnode_move_into_transfers_authority_to_requested_slot() {
    // Goal: CNode move follows seL4 cteMove by moving into an explicit empty destination.
    // Scope: host integration through CNodeMoveInto.
    // Semantics: source becomes empty and destination receives the same object authority.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x22))
        .unwrap();
    let source_object = state.cspace().lookup(source).unwrap().object;
    let destination = kernel::cap::SlotId::from_raw(42);

    let moved = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMoveInto {
                    source,
                    destination,
                },
            )
            .unwrap(),
        "CNode move into",
    );

    assert_eq!(moved.slot, destination);
    assert_eq!(state.cspace().lookup(moved).unwrap().object, source_object);
    assert!(matches!(
        state.cspace().lookup(source),
        Err(CapError::SlotNotFound(_))
    ));
}

#[test]
fn cnode_move_into_occupied_destination_fails_before_source_lookup() {
    // Goal: CNode move follows seL4 decode ordering by checking destination emptiness first.
    // Scope: host integration of explicit CNodeMoveInto failure path.
    // Semantics: occupied destination is reported even if the source descriptor is stale.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x22))
        .unwrap();
    let occupied = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x44))
        .unwrap();
    state.cspace_mut().delete(source).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeMoveInto {
                source,
                destination: occupied.slot,
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::SlotOccupied(occupied.slot))
        ))
    );
    assert_eq!(
        state.cspace().lookup(occupied).unwrap().capability,
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
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x1))
        .unwrap();
    let sibling = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x2))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeDelete { target },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace().lookup(target),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.cspace().lookup(sibling).unwrap().capability,
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
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x7))
        .unwrap();
    let child = state.cspace_mut().copy(root, Rights::READ).unwrap();
    let grandchild = state.cspace_mut().copy(child, Rights::READ).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevoke { target: root },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(state.cspace().lookup(root).is_ok());
    assert!(matches!(
        state.cspace().lookup(child),
        Err(CapError::SlotNotFound(_))
    ));
    assert!(matches!(
        state.cspace().lookup(grandchild),
        Err(CapError::SlotNotFound(_))
    ));
}

#[test]
fn cnode_revoke_untyped_descendants_recovers_capacity() {
    // Goal: CNode revoke reaches Untyped capacity reset through the executor.
    // Scope: host integration for CSpace lineage and Untyped watermark mutation.
    // Semantics: revoked descendants disappear and the parent Untyped can retype again.
    let (mut state, cnode) = cnode_state();
    let root = state
        .cspace_mut()
        .insert_initial_capability(untyped(12))
        .unwrap();
    let frame = state
        .cspace_mut()
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
            Invocation::CNodeRevoke { target: root },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace().lookup(frame),
        Err(CapError::SlotNotFound(_))
    ));
    let recycled = state
        .cspace_mut()
        .retype_untyped(
            root,
            RetypeTarget::Frame {
                rights: Rights::READ,
            },
        )
        .unwrap();
    assert_eq!(
        state.cspace().lookup(recycled).unwrap().capability,
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
    let root = state
        .cspace_mut()
        .insert_initial_capability(untyped(12))
        .unwrap();
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
    let frame_object = state.cspace().lookup(frame).unwrap().object;

    assert_eq!(
        state.objects().get(frame_object),
        Ok(KernelObjectRef::Frame { size_bits: 12 })
    );
    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevoke { target: root },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace().lookup(frame),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.objects().get(frame_object),
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
    let recycled_object = state.cspace().lookup(recycled).unwrap().object;
    assert_eq!(
        state.objects().get(recycled_object),
        Ok(KernelObjectRef::Frame { size_bits: 12 })
    );
}

#[test]
fn cnode_revoke_typed_descendants_keeps_target_runtime_object() {
    // Goal: CNode revoke follows seL4 descendant semantics without deleting the target cap's object.
    // Scope: host integration across Endpoint retype, CNode mint, CNode revoke, and ObjectTable cleanup.
    // Semantics: revocable badged descendants disappear, but the target Endpoint remains live.
    let (mut state, cnode) = cnode_state();
    let root = state
        .cspace_mut()
        .insert_initial_capability(untyped(12))
        .unwrap();
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
    let endpoint_object = state.cspace().lookup(endpoint).unwrap().object;
    let alias = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMintInto {
                    source: endpoint,
                    destination: kernel::cap::SlotId::from_raw(46),
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
            Invocation::CNodeRevoke { target: endpoint },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(state.cspace().lookup(endpoint).is_ok());
    assert!(matches!(
        state.cspace().lookup(alias),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.objects().get(endpoint_object),
        Ok(KernelObjectRef::Endpoint)
    );
}

#[test]
fn cnode_revoke_untyped_endpoint_finalises_runtime_object() {
    // Goal: CNode revoke reaches seL4-style final cap cleanup for Endpoint objects.
    // Scope: host integration across Untyped retype, CNode revoke, and ObjectTable finalisation.
    // Semantics: the final Endpoint cap disappears and its runtime object is removed.
    let (mut state, cnode) = cnode_state();
    let root = state
        .cspace_mut()
        .insert_initial_capability(untyped(12))
        .unwrap();
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
    let endpoint_object = state.cspace().lookup(endpoint).unwrap().object;

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevoke { target: root },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert!(matches!(
        state.cspace().lookup(endpoint),
        Err(CapError::SlotNotFound(_))
    ));
    assert_eq!(
        state.objects().get(endpoint_object),
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
    let root = state
        .cspace_mut()
        .insert_initial_capability(untyped(12))
        .unwrap();
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
    let endpoint_object = state.cspace().lookup(endpoint).unwrap().object;
    let sender_tcb = configure_thread(&mut state, 7);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(7), cpu(0)),
            sender_tcb,
            Invocation::TcbResume,
        ),
        Ok(ExecutionOutcome::Thread(
            kernel::thread_action::ThreadAction::Resumed {
                thread: thread(7),
                cpu: cpu(0),
                scheduler: kernel::scheduler::SchedulerAction::Enqueued {
                    thread: thread(7),
                    cpu: cpu(0),
                },
            }
        ))
    );
    state.scheduler_mut().schedule_next(cpu(0)).unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(7), cpu(0)),
            endpoint,
            Invocation::EndpointSend {
                message_words: 0,
                blocking: true,
                is_call: false,
            },
        ),
        Ok(ExecutionOutcome::Thread(
            kernel::thread_action::ThreadAction::Blocked {
                thread: thread(7),
                cpu: cpu(0),
            }
        ))
    );
    assert_eq!(
        state.threads().state(thread(7)),
        Some(ThreadState::BlockedOnSend {
            endpoint: endpoint_object,
            badge: 0,
            can_grant: true,
            can_grant_reply: true,
            is_call: false,
        })
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeRevoke { target: root },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert_eq!(state.threads().state(thread(7)), Some(ThreadState::Restart));
    assert_eq!(
        state.scheduler().placement(thread(7)),
        Some(kernel::scheduler::ThreadPlacement::Ready { cpu: cpu(0) })
    );
    assert_eq!(
        state.objects().get(endpoint_object),
        Err(ObjectTableError::ObjectNotFound {
            object: endpoint_object,
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
    let tcb_object = state.cspace().lookup(tcb).unwrap().object;

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(8), cpu(0)),
            tcb,
            Invocation::TcbResume,
        ),
        Ok(ExecutionOutcome::Thread(
            kernel::thread_action::ThreadAction::Resumed {
                thread: thread(8),
                cpu: cpu(0),
                scheduler: kernel::scheduler::SchedulerAction::Enqueued {
                    thread: thread(8),
                    cpu: cpu(0),
                },
            }
        ))
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeDelete { target: tcb },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert_eq!(state.threads().get(thread(8)), None);
    assert_eq!(state.scheduler().placement(thread(8)), None);
    assert_eq!(
        state.objects().get(tcb_object),
        Err(ObjectTableError::ObjectNotFound { object: tcb_object })
    );
}

#[test]
fn cnode_delete_blocked_tcb_removes_endpoint_queue_entry() {
    // Goal: TCB finalisation performs seL4-style cancelIPC before removing the TCB runtime object.
    // Scope: host integration across Endpoint queue state, ThreadTable, Scheduler, and CNode delete.
    // Semantics: deleting a TCB blocked on an Endpoint removes its queued sender while keeping the Endpoint live.
    let (mut state, cnode) = cnode_state();
    let root = state
        .cspace_mut()
        .insert_initial_capability(untyped(12))
        .unwrap();
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
    let endpoint_object = state.cspace().lookup(endpoint).unwrap().object;
    let sender_tcb = configure_thread(&mut state, 9);
    let sender_object = state.cspace().lookup(sender_tcb).unwrap().object;

    state
        .execute_invocation(
            InvocationContext::new(thread(9), cpu(0)),
            sender_tcb,
            Invocation::TcbResume,
        )
        .unwrap();
    state.scheduler_mut().schedule_next(cpu(0)).unwrap();
    state
        .execute_invocation(
            InvocationContext::new(thread(9), cpu(0)),
            endpoint,
            Invocation::EndpointSend {
                message_words: 0,
                blocking: true,
                is_call: false,
            },
        )
        .unwrap();

    assert_eq!(
        state
            .objects()
            .endpoint(endpoint_object)
            .unwrap()
            .queued_senders(),
        1
    );
    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeDelete { target: sender_tcb },
        ),
        Ok(ExecutionOutcome::CapabilityMutation)
    );

    assert_eq!(
        state
            .objects()
            .endpoint(endpoint_object)
            .unwrap()
            .queued_senders(),
        0
    );
    assert_eq!(state.threads().get(thread(9)), None);
    assert_eq!(state.scheduler().placement(thread(9)), None);
    assert_eq!(
        state.objects().get(sender_object),
        Err(ObjectTableError::ObjectNotFound {
            object: sender_object
        })
    );
    assert_eq!(
        state.objects().get(endpoint_object),
        Ok(KernelObjectRef::Endpoint)
    );
}

#[test]
fn cnode_copy_rejects_untyped_with_children_without_recovering_capacity() {
    // Goal: seL4 deriveCap rejects copying an Untyped cap while it has children.
    // Scope: host integration across Untyped retype, CNode copy, and capacity state.
    // Semantics: copy fails before creating an alias and leaves Untyped capacity consumed.
    let (mut state, cnode) = cnode_state();
    let root = state
        .cspace_mut()
        .insert_initial_capability(untyped(12))
        .unwrap();
    state
        .cspace_mut()
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
            Invocation::CNodeCopyInto {
                source: root,
                destination: kernel::cap::SlotId::from_raw(47),
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
        state
            .cspace_mut()
            .retype_untyped(root, RetypeTarget::Endpoint),
        Err(CapError::UntypedCapacityExhausted {
            parent: root.slot,
            requested: 4,
            source: 12,
        })
    );
}

#[test]
fn cnode_copy_rights_failure_does_not_consume_new_slot() {
    // Goal: failed CNode copy leaves the explicit destination reusable and source authority unchanged.
    // Scope: host integration of CapabilitySpace failure-before-side-effect via executor.
    // Semantics: rights escalation is rejected before a child slot is inserted.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x33))
        .unwrap();
    let destination = kernel::cap::SlotId::from_raw(48);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeCopyInto {
                source,
                destination,
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
                Invocation::CNodeCopyInto {
                    source,
                    destination,
                    requested_rights: Rights::READ,
                },
            )
            .unwrap(),
        "later CNode copy",
    );
    assert_eq!(later.slot, destination);
    assert!(state.cspace().lookup(source).is_ok());
}

#[test]
fn cnode_operation_requires_cnode_capability_without_mutating_source() {
    // Goal: invoking CNode authority is checked before the source slot is touched.
    // Scope: host integration of authorization failure before CSpace mutation.
    // Semantics: a non-CNode cap cannot mutate source or target slots.
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let invoking_endpoint = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x43))
        .unwrap();
    let target = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x44))
        .unwrap();
    let cases = [
        Invocation::CNodeCopyInto {
            source: target,
            destination: kernel::cap::SlotId::from_raw(49),
            requested_rights: Rights::READ,
        },
        Invocation::CNodeMintInto {
            source: target,
            destination: kernel::cap::SlotId::from_raw(50),
            requested_rights: Rights::READ,
            params: MintParams::badge(0x45),
        },
        Invocation::CNodeMoveInto {
            source: target,
            destination: kernel::cap::SlotId::from_raw(51),
        },
        Invocation::CNodeDelete { target },
        Invocation::CNodeRevoke { target },
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
            state.cspace().lookup(target).unwrap().capability,
            endpoint(Rights::READ, 0x44)
        );
    }
}
