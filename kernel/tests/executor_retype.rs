mod support;

use kernel::{
    cap::{CapError, CapabilityDescriptor, RetypeDestination},
    cap::{Capability, EndpointCap, FrameCap, RetypeTarget, Rights, TcbCap},
    invocation::Invocation,
    notification::NotificationState,
    object::{FrameObject, KernelObjectRef, ObjectTableError},
    state::{ExecutionOutcome, InvocationContext, KernelExecutionError},
};
use support::{cpu, state_with_untyped, thread};

fn retyped_descriptor(outcome: ExecutionOutcome, context: &str) -> CapabilityDescriptor {
    let ExecutionOutcome::Retyped { descriptor } = outcome else {
        panic!("{context}: expected retyped outcome");
    };

    descriptor
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
        Ok(ExecutionOutcome::Thread(
            kernel::thread_action::ThreadAction::NoThread
        ))
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
    let predicted_object = state
        .cspace()
        .preview_retype_untyped(
            untyped,
            &RetypeTarget::Frame {
                rights: Rights::READ,
            },
        )
        .unwrap();

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
fn untyped_capacity_failure_does_not_consume_next_object_or_watermark() {
    // Goal: capacity failures do not advance the CSpace allocation transaction.
    // Scope: host integration through executor preview, failure, and later commit.
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
    let predicted_object = state
        .cspace()
        .preview_retype_untyped(untyped, &endpoint_target)
        .unwrap();

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

    assert_eq!(state.objects().get(object), Ok(KernelObjectRef::CNode));
}

#[test]
fn untyped_retype_cnode_object_table_conflict_does_not_commit_cspace() {
    // Goal: object-table conflicts fail before CSpace consumes the next slot/object.
    // Scope: host integration of executor precheck ordering for CNode retype.
    // Semantics: after failure, a later retype observes the same predicted child object.
    let (mut state, untyped) = state_with_untyped(13);
    let target = RetypeTarget::CNode { radix: 4 };
    let predicted_object = state
        .cspace()
        .preview_retype_untyped(untyped, &target)
        .unwrap();
    state.objects_mut().insert_cnode(predicted_object).unwrap();

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

    let endpoint = state
        .cspace_mut()
        .retype_untyped(untyped, RetypeTarget::Endpoint)
        .unwrap();
    assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
    assert_eq!(
        state.cspace().lookup(endpoint).map(|view| view.object),
        Ok(predicted_object)
    );
}

#[test]
fn untyped_retype_frame_object_table_conflict_does_not_commit_cspace() {
    // Goal: Frame conflicts share the same failure-before-side-effect contract.
    // Scope: host integration of Frame retype precheck before CSpace commit.
    // Semantics: failed Frame retype leaves slot allocation and object lineage unchanged.
    let (mut state, untyped) = state_with_untyped(13);
    let target = RetypeTarget::Frame {
        rights: Rights::READ,
    };
    let predicted_object = state
        .cspace()
        .preview_retype_untyped(untyped, &target)
        .unwrap();
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

    let endpoint = state
        .cspace_mut()
        .retype_untyped(untyped, RetypeTarget::Endpoint)
        .unwrap();
    assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
    assert_eq!(
        state.cspace().lookup(endpoint).map(|view| view.object),
        Ok(predicted_object)
    );
}

#[test]
fn untyped_retype_object_table_conflict_does_not_consume_capacity() {
    // Goal: runtime object conflicts fail before Untyped watermark advances.
    // Scope: host integration across CSpace preview and ObjectTable precheck.
    // Semantics: after the conflict, the same Untyped can still commit through CSpace.
    let (mut state, untyped) = state_with_untyped(12);
    let target = RetypeTarget::Frame {
        rights: Rights::READ,
    };
    let predicted_object = state
        .cspace()
        .preview_retype_untyped(untyped, &target)
        .unwrap();
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
    let predicted_object = state
        .cspace()
        .preview_retype_untyped(untyped, &endpoint_target)
        .unwrap();

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
