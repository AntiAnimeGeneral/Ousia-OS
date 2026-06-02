mod support;

use kernel::{
    cap::{CNodeCap, CapError, Capability, CapabilityDescriptor, EndpointCap, MintParams, Rights},
    invocation::Invocation,
    state::{ExecutionOutcome, InvocationContext, KernelExecutionError},
};
use support::{cpu, thread};

fn cnode(rights: Rights) -> Capability {
    Capability::CNode(CNodeCap { rights })
}

fn endpoint(rights: Rights, badge: u64) -> Capability {
    Capability::Endpoint(EndpointCap { badge, rights })
}

fn cnode_state() -> (kernel::state::KernelState, CapabilityDescriptor) {
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let cnode = state
        .cspace_mut()
        .insert_initial_capability(cnode(Rights::MANAGE))
        .unwrap();
    (state, cnode)
}

fn capability_descriptor(outcome: ExecutionOutcome, context: &str) -> CapabilityDescriptor {
    let ExecutionOutcome::Capability { descriptor } = outcome else {
        panic!("{context}: expected capability outcome");
    };

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

    let copied = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeCopy {
                    source,
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
fn cnode_mint_sets_badge_without_escalating_rights() {
    // Goal: CNode mint commits cap-specific mint parameters through the executor.
    // Scope: host integration across invocation authorization and CSpace mutation.
    // Semantics: badge changes are allowed for endpoint caps, while rights still shrink.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x11))
        .unwrap();

    let minted = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMint {
                    source,
                    requested_rights: Rights::READ,
                    params: MintParams::Badge(0x99),
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

    let moved = capability_descriptor(
        state
            .execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                cnode,
                Invocation::CNodeMove { source },
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
fn cnode_copy_rights_failure_does_not_consume_new_slot() {
    // Goal: failed CNode copy leaves future slot allocation and source authority unchanged.
    // Scope: host integration of CapabilitySpace failure-before-side-effect via executor.
    // Semantics: rights escalation is rejected before a child slot is inserted.
    let (mut state, cnode) = cnode_state();
    let source = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x33))
        .unwrap();

    assert_eq!(
        state.execute_invocation(
            InvocationContext::new(thread(1), cpu(0)),
            cnode,
            Invocation::CNodeCopy {
                source,
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
                Invocation::CNodeCopy {
                    source,
                    requested_rights: Rights::READ,
                },
            )
            .unwrap(),
        "later CNode copy",
    );
    assert_eq!(later.slot.raw(), source.slot.raw() + 1);
    assert!(state.cspace().lookup(source).is_ok());
}

#[test]
fn cnode_operation_requires_manage_rights_without_mutating_source() {
    // Goal: invoking CNode rights are checked before the source slot is touched.
    // Scope: host integration of authorization failure before CSpace mutation.
    // Semantics: a read-only CNode cannot mutate source or target slots.
    let mut state = kernel::state::KernelState::new(&[cpu(0), cpu(1)]).unwrap();
    let read_only_cnode = state
        .cspace_mut()
        .insert_initial_capability(cnode(Rights::NONE))
        .unwrap();
    let target = state
        .cspace_mut()
        .insert_initial_capability(endpoint(Rights::READ, 0x44))
        .unwrap();
    let cases = [
        Invocation::CNodeCopy {
            source: target,
            requested_rights: Rights::READ,
        },
        Invocation::CNodeMint {
            source: target,
            requested_rights: Rights::READ,
            params: MintParams::Badge(0x45),
        },
        Invocation::CNodeMove { source: target },
        Invocation::CNodeDelete { target },
        Invocation::CNodeRevoke { target },
    ];

    for invocation in cases {
        assert_eq!(
            state.execute_invocation(
                InvocationContext::new(thread(1), cpu(0)),
                read_only_cnode,
                invocation,
            ),
            Err(KernelExecutionError::Invocation(
                kernel::invocation::InvocationError::MissingRights {
                    required: Rights::MANAGE,
                    actual: Rights::NONE,
                }
            ))
        );
        assert_eq!(
            state.cspace().lookup(target).unwrap().capability,
            endpoint(Rights::READ, 0x44)
        );
    }
}
