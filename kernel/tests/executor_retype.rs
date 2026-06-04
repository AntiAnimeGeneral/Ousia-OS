mod support;

use kernel::{
    cap::SlotId,
    cap::{CNodeCap, CNodePath, CapError, CapabilityDescriptor, MintParams, RetypeDestination},
    cap::{Capability, EndpointCap, FrameCap, RetypeTarget, Rights, TcbCap},
    invocation::InvocationError,
    invocation::{Invocation, RetypeDestinationPath},
    notification::NotificationState,
    object::{FrameObject, KernelObjectRef, ObjectTableError},
    state::{ExecutionOutcome, InvocationContext, KernelExecutionError},
    thread::action::ThreadAction,
};
use support::{cpu, state_with_untyped, thread};

fn retyped_descriptor(outcome: ExecutionOutcome, context: &str) -> CapabilityDescriptor {
    let ExecutionOutcome::Retyped { descriptors } = outcome else {
        panic!("{context}: expected retyped outcome");
    };

    let [descriptor] = descriptors.as_slice() else {
        panic!("{context}: expected one retyped descriptor");
    };
    *descriptor
}

fn capability_descriptor(outcome: ExecutionOutcome, context: &str) -> CapabilityDescriptor {
    let ExecutionOutcome::Capability { descriptor } = outcome else {
        panic!("{context}: expected capability outcome");
    };

    descriptor
}

fn planned_objects(
    state: &kernel::state::KernelState,
    source: CapabilityDescriptor,
    target: RetypeTarget,
) -> Vec<kernel::cap::ObjectId> {
    state
        .cspace()
        .plan_retype_untyped(source, target)
        .unwrap()
        .objects()
        .collect()
}

#[test]
fn untyped_retype_endpoint_creates_object_and_capability() {
    // Goal: endpoint retype creates one authoritative cap and one runtime object.
    // Scope: host integration through KernelState::execute_invocation.
    // Semantics: CSpace owns endpoint rights; ObjectTable owns endpoint presence.
    let (mut state, untyped) = state_with_untyped(12);

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Endpoint,
                },
            )
            .unwrap(),
        "endpoint retype",
    );
    let view = state.cspace().lookup(descriptor).unwrap();

    assert_eq!(
        view.capability,
        Capability::Endpoint(EndpointCap {
            badge: 0,
            rights: Rights::READ | Rights::WRITE | Rights::GRANT | Rights::GRANT_REPLY,
        })
    );
    assert_eq!(
        state.objects().get(view.object),
        Ok(KernelObjectRef::Endpoint)
    );
}

#[test]
fn untyped_retype_notification_creates_object_and_can_signal() {
    // Goal: notification retype produces an object usable through invocation.
    // Scope: host integration across retype, ObjectTable lookup, and signal.
    // Semantics: the retyped notification is initialized idle and can become active.
    let (mut state, untyped) = state_with_untyped(12);

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Notification,
                },
            )
            .unwrap(),
        "notification retype",
    );
    let notification_object = state.cspace().lookup(descriptor).unwrap().object;

    assert_eq!(
        state
            .objects()
            .notification(notification_object)
            .unwrap()
            .state(),
        NotificationState::Idle
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            descriptor,
            Invocation::NotificationSignal,
        ),
        Ok(ExecutionOutcome::Thread(ThreadAction::NoThread))
    );
    assert_eq!(
        state
            .objects()
            .notification(notification_object)
            .unwrap()
            .state(),
        NotificationState::Active
    );
}

#[test]
fn untyped_retype_frame_creates_object_and_capability() {
    // Goal: frame retype creates minimal runtime metadata without stealing rights.
    // Scope: host integration through the executor retype transaction.
    // Semantics: FrameCap owns rights; FrameObject only records runtime size metadata.
    let (mut state, untyped) = state_with_untyped(12);

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ | Rights::WRITE,
                    },
                },
            )
            .unwrap(),
        "frame retype",
    );
    let view = state.cspace().lookup(descriptor).unwrap();

    assert_eq!(
        view.capability,
        Capability::Frame(FrameCap {
            rights: Rights::READ | Rights::WRITE,
        })
    );
    assert_eq!(state.objects().frame(view.object), Ok(FrameObject::new(12)));
    assert_eq!(
        state.objects().get(view.object),
        Ok(KernelObjectRef::Frame { size_bits: 12 })
    );
}

#[test]
fn untyped_retype_capacity_allows_only_one_full_size_frame() {
    // Goal: Frame retype consumes Untyped capacity instead of only checking object size.
    // Scope: host integration through KernelState::execute_invocation.
    // Semantics: a second full-size object from the same Untyped fails without CSpace drift.
    let (mut state, untyped) = state_with_untyped(12);

    let first = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            )
            .unwrap(),
        "first frame retype",
    );
    let next_slot = first.slot.raw() + 1;

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::UntypedCapacityExhausted {
                parent: untyped.slot,
                requested: 12,
                source: 12,
            })
        ))
    );
    assert_eq!(
        state.cspace().lookup(first).unwrap().descriptor.slot.raw(),
        first.slot.raw()
    );
    assert_eq!(next_slot, untyped.slot.raw() + 2);
}

#[test]
fn untyped_retype_into_occupied_destination_fails_without_side_effects() {
    // Goal: seL4-style UntypedRetype validates the destination slot window before mutation.
    // Scope: host integration through explicit UntypedRetypeInto invocation.
    // Semantics: an occupied destination slot fails before Untyped capacity or ObjectTable changes.
    let (mut state, untyped) = state_with_untyped(13);
    let occupied = state
        .cspace_mut()
        .insert_initial_capability(Capability::Endpoint(EndpointCap {
            badge: 0,
            rights: Rights::READ,
        }))
        .unwrap();
    let [predicted_object] = planned_objects(
        &state,
        untyped,
        RetypeTarget::Frame {
            rights: Rights::READ,
        },
    )[..] else {
        panic!("single frame retype plan must contain one object");
    };

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetypeInto {
                target: RetypeTarget::Frame {
                    rights: Rights::READ,
                },
                destination: RetypeDestination::single(occupied.slot),
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::SlotOccupied(occupied.slot))
        ))
    );
    assert_eq!(
        state.cspace().lookup(occupied).unwrap().capability,
        Capability::Endpoint(EndpointCap {
            badge: 0,
            rights: Rights::READ,
        })
    );

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            )
            .unwrap(),
        "frame retype after occupied destination failure",
    );
    assert_eq!(
        state.cspace().lookup(descriptor).map(|view| view.object),
        Ok(predicted_object)
    );
}

#[test]
fn untyped_retype_into_window_creates_all_runtime_objects() {
    // Goal: executor commit covers every object in an UntypedRetype destination window.
    // Scope: host integration through explicit UntypedRetypeInto invocation.
    // Semantics: CSpace descriptors and ObjectTable runtime entries are created for the full window.
    let (mut state, untyped) = state_with_untyped(13);

    let outcome = state
        .execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetypeInto {
                target: RetypeTarget::Frame {
                    rights: Rights::READ,
                },
                destination: RetypeDestination {
                    start: kernel::cap::SlotId::new(30),
                    count: 2,
                },
            },
        )
        .unwrap();
    let ExecutionOutcome::Retyped { descriptors } = outcome else {
        panic!("window retype must return retyped descriptors");
    };

    assert_eq!(descriptors.len(), 2);
    for descriptor in descriptors {
        let view = state.cspace().lookup(descriptor).unwrap();
        assert_eq!(
            view.capability,
            Capability::Frame(FrameCap {
                rights: Rights::READ,
            })
        );
        assert_eq!(
            state.objects().get(view.object),
            Ok(KernelObjectRef::Frame { size_bits: 12 })
        );
    }
}

#[test]
fn untyped_retype_path_creates_all_runtime_objects_in_resolved_window() {
    // Goal: Untyped retype can target a CNode path instead of a raw SlotId window.
    // Scope: host integration through explicit UntypedRetypePath invocation.
    // Semantics: the CNode path resolves the first destination slot before CSpace/Object mutation.
    let (mut state, untyped) = state_with_untyped(13);
    let target_root = state
        .cspace_mut()
        .insert_initial_capability(Capability::CNode(CNodeCap::with_window(
            4,
            0b10,
            2,
            kernel::cap::SlotId::new(120),
        )))
        .unwrap();

    let outcome = state
        .execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetypePath {
                target: RetypeTarget::Frame {
                    rights: Rights::READ,
                },
                destination: RetypeDestinationPath {
                    start: CNodePath {
                        root: target_root,
                        capptr: 0b10_0010,
                        depth: 6,
                    },
                    count: 2,
                },
            },
        )
        .unwrap();
    let ExecutionOutcome::Retyped { descriptors } = outcome else {
        panic!("path window retype must return retyped descriptors");
    };

    assert_eq!(descriptors.len(), 2);
    assert_eq!(descriptors[0].slot, kernel::cap::SlotId::new(120 + 0b0010));
    assert_eq!(descriptors[1].slot, kernel::cap::SlotId::new(120 + 0b0011));
    for descriptor in descriptors {
        let view = state.cspace().lookup(descriptor).unwrap();
        assert_eq!(
            view.capability,
            Capability::Frame(FrameCap {
                rights: Rights::READ,
            })
        );
        assert_eq!(
            state.objects().get(view.object),
            Ok(KernelObjectRef::Frame { size_bits: 12 })
        );
    }
}

#[test]
fn untyped_retype_path_failures_do_not_consume_capacity_or_target_slots() {
    // Goal: destination CNode lookup/window faults are Untyped retype preflight failures.
    // Scope: host integration through explicit UntypedRetypePath invocation.
    // Semantics: each failure leaves Untyped capacity and target slots uncommitted.
    struct Case {
        label: &'static str,
        cnode: CNodeCap,
        path_capptr: u64,
        path_depth: u8,
        count: usize,
        expected_error: CapError,
        empty_slot: Option<SlotId>,
    }

    let cases = [
        Case {
            label: "guard mismatch fails before creating the resolved slot",
            cnode: CNodeCap::with_window(4, 0b10, 2, SlotId::new(140)),
            path_capptr: 0b11_0010,
            path_depth: 6,
            count: 1,
            expected_error: CapError::CNodeGuardMismatch {
                expected_guard: 0b10,
                actual_guard: 0b11,
                bits_remaining: 6,
                guard_size: 2,
            },
            empty_slot: Some(SlotId::new(140 + 0b0010)),
        },
        Case {
            label: "window overflow fails before consuming untyped capacity",
            cnode: CNodeCap::with_window(2, 0b10, 2, SlotId::new(160)),
            path_capptr: 0b10_11,
            path_depth: 4,
            count: 2,
            expected_error: CapError::RetypeWindowExceedsCNode {
                start: SlotId::new(160 + 0b11),
                requested: 2,
                available: 1,
            },
            empty_slot: Some(SlotId::new(160 + 0b11)),
        },
    ];

    for case in cases {
        let (mut state, untyped) = state_with_untyped(13);
        let target_root = state
            .cspace_mut()
            .insert_initial_capability(Capability::CNode(case.cnode))
            .unwrap();
        let frame_target = RetypeTarget::Frame {
            rights: Rights::READ,
        };
        let predicted_object = state
            .cspace()
            .plan_retype_untyped(untyped, frame_target.clone())
            .unwrap()
            .objects()
            .next()
            .unwrap();

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetypePath {
                    target: frame_target,
                    destination: RetypeDestinationPath {
                        start: CNodePath {
                            root: target_root,
                            capptr: case.path_capptr,
                            depth: case.path_depth,
                        },
                        count: case.count,
                    },
                },
            ),
            Err(KernelExecutionError::Invocation(InvocationError::Cap(
                case.expected_error
            ))),
            "{}",
            case.label
        );
        if let Some(slot) = case.empty_slot {
            assert_eq!(
                state.cspace().lookup(CapabilityDescriptor {
                    slot,
                    slot_generation: 1,
                }),
                Err(CapError::SlotNotFound(slot)),
                "{}",
                case.label
            );
        }

        let descriptor = retyped_descriptor(
            state
                .execute_invocation(
                    InvocationContext::new(thread(1), cpu(0)),
                    untyped,
                    Invocation::UntypedRetype {
                        target: RetypeTarget::Frame {
                            rights: Rights::READ,
                        },
                    },
                )
                .unwrap(),
            case.label,
        );
        assert_eq!(
            state.cspace().lookup(descriptor).map(|view| view.object),
            Ok(predicted_object),
            "{}",
            case.label
        );
    }
}

#[test]
fn untyped_retype_into_runtime_conflict_fails_before_cspace_commit() {
    // Goal: executor validates the whole runtime destination set before committing CSpace.
    // Scope: host integration through explicit UntypedRetypeInto with an ObjectTable conflict.
    // Semantics: no descriptor in the requested window becomes live after a later runtime conflict.
    let (mut state, untyped) = state_with_untyped(13);
    state
        .objects_mut()
        .insert_frame(kernel::cap::ObjectId::new(2), FrameObject::new(12))
        .unwrap();
    let start = kernel::cap::SlotId::new(40);

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetypeInto {
                target: RetypeTarget::Frame {
                    rights: Rights::READ,
                },
                destination: RetypeDestination { start, count: 2 },
            },
        ),
        Err(KernelExecutionError::Object(
            ObjectTableError::ObjectIdAlreadyBound {
                object: kernel::cap::ObjectId::new(2),
            }
        ))
    );

    assert_eq!(
        state.cspace().lookup(CapabilityDescriptor {
            slot: start,
            slot_generation: 1,
        }),
        Err(CapError::SlotNotFound(start))
    );
    let next = kernel::cap::SlotId::new(start.raw() + 1);
    assert_eq!(
        state.cspace().lookup(CapabilityDescriptor {
            slot: next,
            slot_generation: 1,
        }),
        Err(CapError::SlotNotFound(next))
    );
}

#[test]
fn untyped_capacity_failure_does_not_consume_next_object_or_watermark() {
    // Goal: capacity failures do not advance the CSpace allocation transaction.
    // Scope: host integration through executor planning, failure, and later commit.
    // Semantics: a failed aligned large retype leaves the next object available.
    let (mut state, untyped) = state_with_untyped(13);

    retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            )
            .unwrap(),
        "initial frame retype",
    );
    let endpoint_target = RetypeTarget::Endpoint;
    let [predicted_object] = planned_objects(&state, untyped, endpoint_target.clone())[..] else {
        panic!("single endpoint retype plan must contain one object");
    };

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::Untyped { size_bits: 13 },
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::UntypedCapacityExhausted {
                parent: untyped.slot,
                requested: 13,
                source: 13,
            })
        ))
    );

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: endpoint_target,
                },
            )
            .unwrap(),
        "endpoint retype after capacity failure",
    );
    assert_eq!(
        state.cspace().lookup(descriptor).map(|view| view.object),
        Ok(predicted_object)
    );
}

#[test]
fn untyped_retype_cnode_creates_object_and_capability() {
    // Goal: CNode retype creates a runtime object at the executor boundary.
    // Scope: host integration through KernelState, not direct CSpace retype.
    // Semantics: CSpace creates authority; ObjectTable records CNode presence.
    let (mut state, untyped) = state_with_untyped(12);

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::CNode { radix: 4 },
                },
            )
            .unwrap(),
        "CNode retype",
    );
    let object = state.cspace().lookup(descriptor).unwrap().object;

    assert_eq!(
        state.objects().get(object),
        Ok(KernelObjectRef::CNode {
            radix: 4,
            slots: 16,
            window_start: kernel::cap::SlotId::new(descriptor.slot.raw() + 1),
        })
    );
}

#[test]
fn untyped_retype_cnode_creates_usable_slot_window() {
    // Goal: CNode retype creates a CNode cap and runtime object that agree on the owned slot window.
    // Scope: host integration across CSpace retype, ObjectTable metadata, and CNode path copy.
    // Semantics: the retyped CNode resolves path operations into its own reserved CTE window.
    let (mut state, untyped) = state_with_untyped(13);

    let cnode = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::CNode { radix: 2 },
                },
            )
            .unwrap(),
        "CNode retype",
    );
    let cnode_view = state.cspace().lookup(cnode).unwrap();
    let Capability::CNode(cnode_cap) = cnode_view.capability else {
        panic!("retype must install a CNode cap");
    };
    assert_eq!(
        state.objects().get(cnode_view.object),
        Ok(KernelObjectRef::CNode {
            radix: 2,
            slots: 4,
            window_start: cnode_cap.window_start,
        })
    );

    let source = state
        .cspace_mut()
        .insert_initial_capability(Capability::Endpoint(EndpointCap {
            badge: 0,
            rights: Rights::READ | Rights::WRITE,
        }))
        .unwrap();
    let copied = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMintPath {
                    source,
                    destination: kernel::invocation::CNodePathTarget {
                        capptr: 0b10,
                        depth: 2,
                    },
                    requested_rights: Rights::READ,
                    params: MintParams::badge(0x55),
                },
            )
            .unwrap(),
        "CNode mint path into retyped window",
    );

    assert_eq!(
        copied.slot,
        SlotId::new(cnode_cap.window_start.raw() + 0b10)
    );
    assert_eq!(
        state.cspace().lookup(copied).unwrap().capability,
        Capability::Endpoint(EndpointCap {
            badge: 0x55,
            rights: Rights::READ,
        })
    );
}

#[test]
fn untyped_retype_object_table_conflicts_do_not_commit_cspace() {
    // Goal: runtime object conflicts fail before CSpace consumes the next slot/object.
    // Scope: host integration of executor precheck ordering for typed Untyped retype targets.
    // Semantics: after each conflict, a later retype observes the same predicted child object.
    struct Case {
        label: &'static str,
        target: RetypeTarget,
        install_conflict: fn(&mut kernel::state::KernelState, kernel::cap::ObjectId),
    }

    let cases = [
        Case {
            label: "CNode conflict leaves CSpace transaction uncommitted",
            target: RetypeTarget::CNode { radix: 4 },
            install_conflict: |state, object| {
                state
                    .objects_mut()
                    .insert_cnode(
                        object,
                        kernel::object::CNodeObject::new(4, kernel::cap::SlotId::new(99)),
                    )
                    .unwrap();
            },
        },
        Case {
            label: "Frame conflict leaves CSpace transaction uncommitted",
            target: RetypeTarget::Frame {
                rights: Rights::READ,
            },
            install_conflict: |state, object| {
                state
                    .objects_mut()
                    .insert_frame(object, FrameObject::new(12))
                    .unwrap();
            },
        },
    ];

    for case in cases {
        let (mut state, untyped) = state_with_untyped(13);
        let predicted_object = state
            .cspace()
            .plan_retype_untyped(untyped, case.target.clone())
            .unwrap()
            .objects()
            .next()
            .unwrap();
        (case.install_conflict)(&mut state, predicted_object);

        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: case.target,
                },
            ),
            Err(KernelExecutionError::Object(
                ObjectTableError::ObjectIdAlreadyBound {
                    object: predicted_object,
                }
            )),
            "{}",
            case.label
        );

        let endpoint = state
            .cspace_mut()
            .retype_untyped(untyped, RetypeTarget::Endpoint)
            .unwrap();
        assert_eq!(
            endpoint.slot.raw(),
            untyped.slot.raw() + 1,
            "{}",
            case.label
        );
        assert_eq!(
            state.cspace().lookup(endpoint).map(|view| view.object),
            Ok(predicted_object),
            "{}",
            case.label
        );
    }
}

#[test]
fn untyped_retype_object_table_conflict_does_not_consume_capacity() {
    // Goal: runtime object conflicts fail before Untyped watermark advances.
    // Scope: host integration across CSpace planning and ObjectTable precheck.
    // Semantics: after the conflict, the same Untyped can still commit through CSpace.
    let (mut state, untyped) = state_with_untyped(12);
    let target = RetypeTarget::Frame {
        rights: Rights::READ,
    };
    let [predicted_object] = planned_objects(&state, untyped, target.clone())[..] else {
        panic!("single frame retype plan must contain one object");
    };
    state
        .objects_mut()
        .insert_frame(predicted_object, FrameObject::new(12))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype { target },
        ),
        Err(KernelExecutionError::Object(
            ObjectTableError::ObjectIdAlreadyBound {
                object: predicted_object,
            }
        ))
    );

    let descriptor = state
        .cspace_mut()
        .retype_untyped(
            untyped,
            RetypeTarget::Frame {
                rights: Rights::READ,
            },
        )
        .unwrap();
    assert_eq!(
        state.cspace().lookup(descriptor).map(|view| view.object),
        Ok(predicted_object)
    );
}

#[test]
fn untyped_retype_tcb_creates_unbound_tcb_object() {
    // Goal: TCB retype creates an object but does not bootstrap a running thread.
    // Scope: host integration across CSpace, ObjectTable, ThreadTable, and Scheduler.
    // Semantics: no thread or scheduler placement appears before explicit TCB configure/resume.
    let (mut state, untyped) = state_with_untyped(12);

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Tcb {
                        rights: Rights::MANAGE,
                    },
                },
            )
            .unwrap(),
        "TCB retype",
    );
    let view = state.cspace().lookup(descriptor).unwrap();

    assert_eq!(
        view.capability,
        Capability::Tcb(TcbCap {
            rights: Rights::MANAGE,
        })
    );
    assert_eq!(
        state.objects().tcb_thread(view.object),
        Err(ObjectTableError::TcbObjectUnbound {
            object: view.object,
        })
    );
    assert_eq!(state.threads().get(thread(2)), None);
    assert_eq!(state.scheduler().placement(thread(2)), None);
}

#[test]
fn nested_untyped_retype_commits_cspace_without_object_table_entry() {
    // Goal: nested Untyped retype extends CSpace lineage but has no runtime object entry.
    // Scope: host integration through executor retype commit.
    // Semantics: nested Untyped is capability lineage only until a typed object is retyped from it.
    let (mut state, untyped) = state_with_untyped(12);

    let descriptor = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Untyped { size_bits: 10 },
                },
            )
            .unwrap(),
        "nested Untyped retype",
    );
    let child_view = state.cspace().lookup(descriptor).unwrap();
    let child_object = child_view.object;

    assert_eq!(
        child_view.capability,
        Capability::Untyped(kernel::cap::UntypedCap { size_bits: 10 })
    );
    assert_eq!(
        state.objects().get(child_object),
        Err(ObjectTableError::ObjectNotFound {
            object: child_object,
        })
    );
}

#[test]
fn nested_untyped_retype_consumes_parent_and_has_own_capacity() {
    // Goal: nested Untyped consumes parent capacity and becomes an independent source.
    // Scope: host integration through parent and child Untyped retype operations.
    // Semantics: parent cannot create a second same-size child, while child can create a frame.
    let (mut state, untyped) = state_with_untyped(12);

    let child = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Untyped { size_bits: 12 },
                },
            )
            .unwrap(),
        "nested Untyped retype",
    );

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::Untyped { size_bits: 12 },
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::Cap(CapError::UntypedCapacityExhausted {
                parent: untyped.slot,
                requested: 12,
                source: 12,
            })
        ))
    );

    let frame = retyped_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                child,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            )
            .unwrap(),
        "child frame retype",
    );
    let frame_object = state.cspace().lookup(frame).unwrap().object;

    assert_eq!(
        state.objects().frame(frame_object),
        Ok(FrameObject::new(12))
    );
}

#[test]
fn oversized_nested_untyped_retype_does_not_commit_cspace() {
    // Goal: invalid nested Untyped size is rejected before CSpace mutation.
    // Scope: host integration through executor authorization and commit prechecks.
    // Semantics: a failed retype does not consume the next child object or reusable slot.
    let (mut state, untyped) = state_with_untyped(12);
    let endpoint_target = RetypeTarget::Endpoint;
    let [predicted_object] = planned_objects(&state, untyped, endpoint_target.clone())[..] else {
        panic!("single endpoint retype plan must contain one object");
    };

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::Untyped { size_bits: 13 },
            },
        ),
        Err(KernelExecutionError::Invocation(
            kernel::invocation::InvocationError::InvalidRetypeSize {
                requested: 13,
                source: 12,
            }
        ))
    );

    let endpoint = state
        .cspace_mut()
        .retype_untyped(untyped, endpoint_target)
        .unwrap();
    assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
    assert_eq!(
        state.cspace().lookup(endpoint).map(|view| view.object),
        Ok(predicted_object)
    );
}
