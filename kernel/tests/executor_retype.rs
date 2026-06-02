mod support;

use kernel::{
    cap::CapabilityDescriptor,
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
                    target: RetypeTarget::CNode {
                        rights: Rights::MANAGE,
                    },
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
    let (mut state, untyped) = state_with_untyped(12);
    let target = RetypeTarget::CNode {
        rights: Rights::MANAGE,
    };
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
