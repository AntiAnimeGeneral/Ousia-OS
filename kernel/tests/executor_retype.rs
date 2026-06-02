mod support;

use kernel::{
    cap::{Capability, EndpointCap, RetypeTarget, Rights, TcbCap},
    invocation::Invocation,
    notification::NotificationState,
    object::{KernelObjectRef, ObjectTableError},
    state::{ExecutionOutcome, InvocationContext, KernelExecutionError, UnsupportedInvocation},
};
use support::{cpu, state_with_untyped, thread};

#[test]
fn untyped_retype_endpoint_creates_object_and_capability() {
    let (mut state, untyped) = state_with_untyped(12);

    let outcome = state
        .execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::Endpoint,
            },
        )
        .unwrap();
    let ExecutionOutcome::Retyped { descriptor } = outcome else {
        panic!("untyped endpoint retype must return a new capability descriptor");
    };
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
    let (mut state, untyped) = state_with_untyped(12);

    let outcome = state
        .execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::Notification,
            },
        )
        .unwrap();
    let ExecutionOutcome::Retyped { descriptor } = outcome else {
        panic!("untyped notification retype must return a new capability descriptor");
    };
    let notification_object = state.cspace().lookup(descriptor).unwrap().object;

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
fn unsupported_frame_retype_does_not_commit_cspace_or_objects() {
    let (mut state, untyped) = state_with_untyped(12);

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
        Ok(ExecutionOutcome::Unsupported(
            UnsupportedInvocation::UntypedRetype
        ))
    );

    let endpoint = state
        .cspace_mut()
        .retype_untyped(untyped, RetypeTarget::Endpoint)
        .unwrap();
    let endpoint_object = state.cspace().lookup(endpoint).unwrap().object;
    assert_eq!(endpoint.slot.raw(), untyped.slot.raw() + 1);
    assert_eq!(
        state.objects().get(endpoint_object),
        Err(ObjectTableError::ObjectNotFound {
            object: endpoint_object,
        })
    );
}

#[test]
fn untyped_retype_cnode_creates_object_and_capability() {
    let (mut state, untyped) = state_with_untyped(12);

    let outcome = state
        .execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::CNode {
                    rights: Rights::MANAGE,
                },
            },
        )
        .unwrap();
    let ExecutionOutcome::Retyped { descriptor } = outcome else {
        panic!("CNode retype must return a new capability descriptor");
    };
    let object = state.cspace().lookup(descriptor).unwrap().object;

    assert_eq!(state.objects().get(object), Ok(KernelObjectRef::CNode));
}

#[test]
fn untyped_retype_cnode_object_table_conflict_does_not_commit_cspace() {
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
fn untyped_retype_tcb_creates_unbound_tcb_object() {
    let (mut state, untyped) = state_with_untyped(12);

    let outcome = state
        .execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            untyped,
            Invocation::UntypedRetype {
                target: RetypeTarget::Tcb {
                    rights: Rights::MANAGE,
                },
            },
        )
        .unwrap();
    let ExecutionOutcome::Retyped { descriptor } = outcome else {
        panic!("TCB retype must return a new capability descriptor");
    };
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
fn oversized_nested_untyped_retype_does_not_commit_cspace() {
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
