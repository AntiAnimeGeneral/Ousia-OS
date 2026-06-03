use crate::cap::{
    CapError, Capability, CapabilityDescriptor, CapabilitySpace, MintParams, ObjectId,
    RetypeDestination, RetypeTarget, Rights, SlotId,
};
use crate::tcb::{CpuId, ThreadId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    EndpointSend {
        message_words: usize,
        blocking: bool,
        is_call: bool,
    },
    EndpointRecv {
        blocking: bool,
    },
    FrameMap {
        address_space: ObjectId,
        vm_rights: Rights,
    },
    UntypedRetype {
        target: RetypeTarget,
    },
    UntypedRetypeInto {
        target: RetypeTarget,
        destination: RetypeDestination,
    },
    CNodeCopyInto {
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
    },
    CNodeMintInto {
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
        params: MintParams,
    },
    CNodeMoveInto {
        source: CapabilityDescriptor,
        destination: SlotId,
    },
    CNodeDelete {
        target: CapabilityDescriptor,
    },
    CNodeRevoke {
        target: CapabilityDescriptor,
    },
    TcbResume,
    TcbConfigure {
        thread: ThreadId,
        affinity: CpuId,
    },
    NotificationSignal,
    NotificationWait {
        blocking: bool,
    },
    Reply {
        target: ObjectId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationOutcome {
    SendIpcAuthorized {
        endpoint: ObjectId,
        badge: u64,
        message_words: usize,
        blocking: bool,
        is_call: bool,
        can_grant: bool,
        can_grant_reply: bool,
    },
    ReceiveIpcAuthorized {
        endpoint: ObjectId,
        blocking: bool,
        can_grant: bool,
    },
    FrameMapAuthorized {
        frame: ObjectId,
        address_space: ObjectId,
        vm_rights: Rights,
    },
    UntypedRetypeAuthorized {
        untyped: ObjectId,
        target: RetypeTarget,
        destination: Option<RetypeDestination>,
    },
    CNodeCopyAuthorized {
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
    },
    CNodeMintAuthorized {
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
        params: MintParams,
    },
    CNodeMoveAuthorized {
        source: CapabilityDescriptor,
        destination: SlotId,
    },
    CNodeDeleteAuthorized {
        target: CapabilityDescriptor,
    },
    CNodeRevokeAuthorized {
        target: CapabilityDescriptor,
    },
    TcbResumeAuthorized {
        tcb: ObjectId,
    },
    TcbConfigureAuthorized {
        tcb: ObjectId,
        thread: ThreadId,
        affinity: CpuId,
    },
    NotificationSignalAuthorized {
        notification: ObjectId,
        badge: u64,
    },
    NotificationReceiveAuthorized {
        notification: ObjectId,
        blocking: bool,
    },
    ReplyAuthorized {
        reply: ObjectId,
        caller: ObjectId,
        target: ObjectId,
        can_grant: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationError {
    Cap(CapError),
    WrongCapability {
        expected: InvocationTarget,
        actual: Capability,
    },
    MissingRights {
        required: Rights,
        actual: Rights,
    },
    InvalidRetypeSize {
        requested: u8,
        source: u8,
    },
    ReplyTargetMismatch {
        expected: ObjectId,
        actual: ObjectId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationTarget {
    Endpoint,
    Frame,
    CNode,
    Untyped,
    Tcb,
    Notification,
    Reply,
}

impl From<CapError> for InvocationError {
    fn from(error: CapError) -> Self {
        Self::Cap(error)
    }
}

pub fn invoke(
    cspace: &CapabilitySpace,
    descriptor: CapabilityDescriptor,
    invocation: Invocation,
) -> Result<InvocationOutcome, InvocationError> {
    let view = cspace.lookup(descriptor)?;

    match invocation {
        Invocation::EndpointSend {
            message_words,
            blocking,
            is_call,
        } => match view.capability {
            Capability::Endpoint(cap) => {
                if !cap.can_send() {
                    return Err(InvocationError::MissingRights {
                        required: Rights::WRITE,
                        actual: view.rights,
                    });
                }
                if is_call && !(cap.can_grant() || cap.can_grant_reply()) {
                    return Err(InvocationError::MissingRights {
                        required: Rights::GRANT | Rights::GRANT_REPLY,
                        actual: view.rights,
                    });
                }
                Ok(InvocationOutcome::SendIpcAuthorized {
                    endpoint: view.object,
                    badge: cap.badge,
                    message_words,
                    blocking,
                    is_call,
                    can_grant: cap.can_grant(),
                    can_grant_reply: cap.can_grant_reply(),
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Endpoint, actual)),
        },
        Invocation::EndpointRecv { blocking } => match view.capability {
            Capability::Endpoint(cap) => {
                if !cap.can_receive() {
                    return Err(InvocationError::MissingRights {
                        required: Rights::READ,
                        actual: view.rights,
                    });
                }
                Ok(InvocationOutcome::ReceiveIpcAuthorized {
                    endpoint: view.object,
                    blocking,
                    can_grant: cap.can_grant(),
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Endpoint, actual)),
        },
        Invocation::FrameMap {
            address_space,
            vm_rights,
        } => match view.capability {
            Capability::Frame(_) => {
                let vm_rights =
                    vm_rights & view.rights & (Rights::READ | Rights::WRITE | Rights::EXECUTE);
                Ok(InvocationOutcome::FrameMapAuthorized {
                    frame: view.object,
                    address_space,
                    vm_rights,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Frame, actual)),
        },
        Invocation::UntypedRetype { target } => match view.capability {
            Capability::Untyped(cap) => {
                target.validate_rights()?;
                let requested_size = target.minimum_size_bits();
                if requested_size > cap.size_bits {
                    return Err(InvocationError::InvalidRetypeSize {
                        requested: requested_size,
                        source: cap.size_bits,
                    });
                }
                Ok(InvocationOutcome::UntypedRetypeAuthorized {
                    untyped: view.object,
                    target,
                    destination: None,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Untyped, actual)),
        },
        Invocation::UntypedRetypeInto {
            target,
            destination,
        } => match view.capability {
            Capability::Untyped(cap) => {
                target.validate_rights()?;
                let requested_size = target.minimum_size_bits();
                if requested_size > cap.size_bits {
                    return Err(InvocationError::InvalidRetypeSize {
                        requested: requested_size,
                        source: cap.size_bits,
                    });
                }
                Ok(InvocationOutcome::UntypedRetypeAuthorized {
                    untyped: view.object,
                    target,
                    destination: Some(destination),
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Untyped, actual)),
        },
        Invocation::CNodeCopyInto {
            source,
            destination,
            requested_rights,
        } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeCopyAuthorized {
                source,
                destination,
                requested_rights,
            }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeMintInto {
            source,
            destination,
            requested_rights,
            params,
        } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeMintAuthorized {
                source,
                destination,
                requested_rights,
                params,
            }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeMoveInto {
            source,
            destination,
        } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeMoveAuthorized {
                source,
                destination,
            }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeDelete { target } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeDeleteAuthorized { target }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeRevoke { target } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeRevokeAuthorized { target }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::TcbResume => match view.capability {
            Capability::Tcb(_) => {
                require_rights(view.rights, Rights::MANAGE)?;
                Ok(InvocationOutcome::TcbResumeAuthorized { tcb: view.object })
            }
            actual => Err(wrong_capability(InvocationTarget::Tcb, actual)),
        },
        Invocation::TcbConfigure { thread, affinity } => match view.capability {
            Capability::Tcb(_) => {
                require_rights(view.rights, Rights::MANAGE)?;
                Ok(InvocationOutcome::TcbConfigureAuthorized {
                    tcb: view.object,
                    thread,
                    affinity,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Tcb, actual)),
        },
        Invocation::NotificationSignal => match view.capability {
            Capability::Notification(cap) => {
                if !cap.can_send() {
                    return Err(InvocationError::MissingRights {
                        required: Rights::WRITE,
                        actual: view.rights,
                    });
                }
                Ok(InvocationOutcome::NotificationSignalAuthorized {
                    notification: view.object,
                    badge: cap.badge,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Notification, actual)),
        },
        Invocation::NotificationWait { blocking } => match view.capability {
            Capability::Notification(cap) => {
                if !cap.can_receive() {
                    return Err(InvocationError::MissingRights {
                        required: Rights::READ,
                        actual: view.rights,
                    });
                }
                Ok(InvocationOutcome::NotificationReceiveAuthorized {
                    notification: view.object,
                    blocking,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Notification, actual)),
        },
        Invocation::Reply { target } => match view.capability {
            Capability::Reply(cap) => {
                if !cap.can_reply(target) {
                    return Err(InvocationError::ReplyTargetMismatch {
                        expected: cap.target,
                        actual: target,
                    });
                }
                Ok(InvocationOutcome::ReplyAuthorized {
                    reply: view.object,
                    caller: cap.caller,
                    target: cap.target,
                    can_grant: cap.can_grant,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Reply, actual)),
        },
    }
}

fn require_rights(actual: Rights, required: Rights) -> Result<(), InvocationError> {
    if required.is_subset_of(actual) {
        return Ok(());
    }

    Err(InvocationError::MissingRights { required, actual })
}

fn wrong_capability(expected: InvocationTarget, actual: Capability) -> InvocationError {
    InvocationError::WrongCapability { expected, actual }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cap::{
        CNodeCap, EndpointCap, FrameCap, NotificationCap, ReplyCap, TcbCap, UntypedCap,
    };

    fn endpoint(rights: Rights, badge: u64) -> Capability {
        Capability::Endpoint(EndpointCap { badge, rights })
    }

    fn frame(rights: Rights) -> Capability {
        Capability::Frame(FrameCap { rights })
    }

    fn untyped(size_bits: u8) -> Capability {
        Capability::Untyped(UntypedCap { size_bits })
    }

    fn tcb(rights: Rights) -> Capability {
        Capability::Tcb(TcbCap { rights })
    }

    fn notification(rights: Rights, badge: u64) -> Capability {
        Capability::Notification(NotificationCap { badge, rights })
    }

    fn cnode() -> Capability {
        Capability::CNode(CNodeCap::new(4))
    }

    #[test]
    fn endpoint_send_requires_write_rights_and_preserves_badge() {
        // Goal: invocation authorization exports endpoint send facts without queue side effects.
        // Scope: unit test for the capability invocation boundary.
        // Semantics: Endpoint rights and badge are read from CSpace; delivery is owned elsewhere.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(endpoint(
                Rights::READ | Rights::WRITE | Rights::GRANT_REPLY,
                0x2a,
            ))
            .unwrap();
        let endpoint = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::EndpointSend {
                    message_words: 3,
                    blocking: true,
                    is_call: true,
                },
            ),
            Ok(InvocationOutcome::SendIpcAuthorized {
                endpoint,
                badge: 0x2a,
                message_words: 3,
                blocking: true,
                is_call: true,
                can_grant: false,
                can_grant_reply: true,
            })
        );
    }

    #[test]
    fn endpoint_call_requires_grant_or_grant_reply() {
        // Goal: call setup cannot be authorized without grant or grant-reply authority.
        // Scope: unit test for endpoint invocation rights.
        // Semantics: WRITE permits send, but call reply authority needs an explicit grant bit.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(endpoint(Rights::WRITE, 0x2a))
            .unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::EndpointSend {
                    message_words: 0,
                    blocking: true,
                    is_call: true,
                },
            ),
            Err(InvocationError::MissingRights {
                required: Rights::GRANT | Rights::GRANT_REPLY,
                actual: Rights::WRITE,
            })
        );
    }

    #[test]
    fn endpoint_recv_requires_read_rights() {
        // Goal: receive authorization depends on endpoint receive rights.
        // Scope: unit test for invocation rights at the CSpace boundary.
        // Semantics: WRITE-only endpoint caps cannot authorize receive-side IPC.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(endpoint(Rights::WRITE, 0))
            .unwrap();

        assert_eq!(
            invoke(&cspace, cap, Invocation::EndpointRecv { blocking: true }),
            Err(InvocationError::MissingRights {
                required: Rights::READ,
                actual: Rights::WRITE,
            })
        );
    }

    #[test]
    fn endpoint_invocation_exports_grant_flags() {
        // Goal: endpoint invocation exports grant facts for later IPC/reply handling.
        // Scope: unit test for authorization output shape, not endpoint queue behavior.
        // Semantics: grant flags are authority metadata, while scheduling side effects happen later.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(endpoint(
                Rights::READ | Rights::WRITE | Rights::GRANT | Rights::GRANT_REPLY,
                0x2a,
            ))
            .unwrap();
        let endpoint = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::EndpointSend {
                    message_words: 1,
                    blocking: false,
                    is_call: false,
                },
            ),
            Ok(InvocationOutcome::SendIpcAuthorized {
                endpoint,
                badge: 0x2a,
                message_words: 1,
                blocking: false,
                is_call: false,
                can_grant: true,
                can_grant_reply: true,
            })
        );

        assert_eq!(
            invoke(&cspace, cap, Invocation::EndpointRecv { blocking: true }),
            Ok(InvocationOutcome::ReceiveIpcAuthorized {
                endpoint,
                blocking: true,
                can_grant: true,
            })
        );
    }

    #[test]
    fn wrong_capability_is_reported_explicitly() {
        // Goal: invocation boundary rejects object-type mismatch before rights checks.
        // Scope: unit test for target object discrimination.
        // Semantics: a valid cap for the wrong object kind is not interchangeable authority.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE))
            .unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::EndpointSend {
                    message_words: 1,
                    blocking: true,
                    is_call: false,
                },
            ),
            Err(InvocationError::WrongCapability {
                expected: InvocationTarget::Endpoint,
                actual: frame(Rights::READ | Rights::WRITE),
            })
        );
    }

    #[test]
    fn frame_map_masks_requested_vm_rights_by_cap_rights() {
        // Goal: FrameMap authorization cannot escalate VM rights beyond the frame cap.
        // Scope: unit test for invocation-level authority clipping.
        // Semantics: mapping state is not created here; only clipped rights are authorized.
        let mut cspace = CapabilitySpace::new();
        let frame = cspace
            .insert_initial_capability(frame(Rights::READ))
            .unwrap();
        let address_space_cap = cspace
            .insert_initial_capability(tcb(Rights::MANAGE))
            .unwrap();
        let address_space = cspace.object_of(address_space_cap).unwrap();
        let frame_object = cspace.object_of(frame).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                frame,
                Invocation::FrameMap {
                    address_space,
                    vm_rights: Rights::READ | Rights::WRITE,
                },
            ),
            Ok(InvocationOutcome::FrameMapAuthorized {
                frame: frame_object,
                address_space,
                vm_rights: Rights::READ,
            })
        );
    }

    #[test]
    fn untyped_retype_cannot_exceed_source_size() {
        // Goal: retype authorization rejects targets larger than the source Untyped.
        // Scope: unit test for invocation-level size guard.
        // Semantics: CSpace lineage and ObjectTable entries must not be changed by authorization.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(untyped(12)).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Untyped { size_bits: 13 },
                },
            ),
            Err(InvocationError::InvalidRetypeSize {
                requested: 13,
                source: 12,
            })
        );
    }

    #[test]
    fn untyped_retype_authorizes_target_object() {
        // Goal: Untyped retype authorization exports the source object and target request.
        // Scope: unit test for authorization output before executor commit.
        // Semantics: actual child object creation is tested through KernelState integration.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(untyped(12)).unwrap();
        let untyped = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ | Rights::WRITE,
                    },
                },
            ),
            Ok(InvocationOutcome::UntypedRetypeAuthorized {
                untyped,
                target: RetypeTarget::Frame {
                    rights: Rights::READ | Rights::WRITE,
                },
                destination: None,
            })
        );
    }

    #[test]
    fn untyped_retype_rejects_target_rights_outside_object_policy() {
        // Goal: target-specific rights policy is enforced before retype commit.
        // Scope: unit test for target validation at invocation authorization.
        // Semantics: invalid target rights do not reach CSpace or ObjectTable mutation.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(untyped(12)).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ | Rights::GRANT_REPLY,
                    },
                },
            ),
            Err(InvocationError::Cap(CapError::InvalidRights {
                object: crate::cap::ObjectKind::Frame,
                requested_rights: Rights::READ | Rights::GRANT_REPLY,
                allowed_rights: Rights::READ | Rights::WRITE | Rights::EXECUTE,
            }))
        );
    }

    #[test]
    fn untyped_retype_checks_target_minimum_size() {
        // Goal: fixed-size target objects use their minimum object size at authorization.
        // Scope: unit test for target metadata consumed by invocation.
        // Semantics: Frame size policy is checked before any child object is allocated.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(untyped(11)).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                },
            ),
            Err(InvocationError::InvalidRetypeSize {
                requested: 12,
                source: 11,
            })
        );
    }

    #[test]
    fn cnode_copy_authorizes_source_and_destination() {
        // Goal: CNode copy authorization exposes seL4-style source and destination slots.
        // Scope: unit test for CNode invocation authorization before CSpace mutation.
        // Semantics: source and destination slot mutation is owned by CapabilitySpace after authorization.
        let mut cspace = CapabilitySpace::new();
        let cnode_cap = cspace.insert_initial_capability(cnode()).unwrap();
        let source = cspace
            .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x55))
            .unwrap();
        let destination = SlotId::from_raw(30);

        assert_eq!(
            invoke(
                &cspace,
                cnode_cap,
                Invocation::CNodeCopyInto {
                    source,
                    destination,
                    requested_rights: Rights::READ,
                },
            ),
            Ok(InvocationOutcome::CNodeCopyAuthorized {
                source,
                destination,
                requested_rights: Rights::READ,
            })
        );

        assert!(
            invoke(
                &cspace,
                cnode_cap,
                Invocation::CNodeCopyInto {
                    source,
                    destination,
                    requested_rights: Rights::READ,
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn cnode_invocation_rejects_wrong_invoking_capability() {
        // Goal: CNode operations cannot be authorized by non-CNode capabilities.
        // Scope: unit test for CNode target discrimination.
        // Semantics: source slot checks are not reached when the invoking cap has the wrong kind.
        let mut cspace = CapabilitySpace::new();
        let endpoint_cap = cspace
            .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0))
            .unwrap();

        assert_eq!(
            invoke(
                &cspace,
                endpoint_cap,
                Invocation::CNodeDelete {
                    target: endpoint_cap,
                },
            ),
            Err(InvocationError::WrongCapability {
                expected: InvocationTarget::CNode,
                actual: endpoint(Rights::READ | Rights::WRITE, 0),
            })
        );
    }

    #[test]
    fn tcb_resume_requires_manage_rights() {
        // Goal: TCB resume cannot be authorized without manage rights.
        // Scope: unit test for TCB invocation rights.
        // Semantics: thread state and scheduler placement are owned by executor paths.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(tcb(Rights::NONE)).unwrap();

        assert_eq!(
            invoke(&cspace, cap, Invocation::TcbResume),
            Err(InvocationError::MissingRights {
                required: Rights::MANAGE,
                actual: Rights::NONE,
            })
        );
    }

    #[test]
    fn tcb_configure_requires_manage_rights_and_exports_configuration() {
        // Goal: TCB configure authorization exports thread identity and affinity only after rights check.
        // Scope: unit test for TCB invocation authorization output.
        // Semantics: object binding, thread creation, and CPU validation happen in KernelState.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(tcb(Rights::MANAGE))
            .unwrap();
        let object = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::TcbConfigure {
                    thread: ThreadId::new(10),
                    affinity: CpuId::new(1),
                },
            ),
            Ok(InvocationOutcome::TcbConfigureAuthorized {
                tcb: object,
                thread: ThreadId::new(10),
                affinity: CpuId::new(1),
            })
        );

        let read_only = cspace.insert_initial_capability(tcb(Rights::NONE)).unwrap();
        assert_eq!(
            invoke(
                &cspace,
                read_only,
                Invocation::TcbConfigure {
                    thread: ThreadId::new(10),
                    affinity: CpuId::new(1),
                },
            ),
            Err(InvocationError::MissingRights {
                required: Rights::MANAGE,
                actual: Rights::NONE,
            })
        );
    }

    #[test]
    fn notification_signal_requires_write_and_preserves_badge() {
        // Goal: notification signal authorization preserves cap badge authority.
        // Scope: unit test for notification invocation boundary.
        // Semantics: waiter wakeup and badge accumulation are Notification/ThreadAction concerns.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(notification(Rights::WRITE, 0x55))
            .unwrap();
        let object = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(&cspace, cap, Invocation::NotificationSignal),
            Ok(InvocationOutcome::NotificationSignalAuthorized {
                notification: object,
                badge: 0x55,
            })
        );
    }

    #[test]
    fn notification_wait_requires_read_rights() {
        // Goal: notification wait authorization requires receive rights.
        // Scope: unit test for notification invocation rights.
        // Semantics: blocking and waiter queue side effects are handled after authorization.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(notification(Rights::WRITE, 0))
            .unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::NotificationWait { blocking: false },
            ),
            Err(InvocationError::MissingRights {
                required: Rights::READ,
                actual: Rights::WRITE,
            })
        );
    }

    #[test]
    fn reply_requires_matching_target() {
        // Goal: reply caps are one-target authority and cannot reply to another endpoint.
        // Scope: unit test for reply invocation metadata.
        // Semantics: wakeup and reply-cap consumption remain executor responsibilities.
        let mut cspace = CapabilitySpace::new();
        let caller = ObjectId::new(100);
        let target = ObjectId::new(200);
        let cap = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller,
                target,
                can_grant: true,
            })
            .unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::Reply {
                    target: ObjectId::new(201),
                },
            ),
            Err(InvocationError::ReplyTargetMismatch {
                expected: target,
                actual: ObjectId::new(201),
            })
        );
    }

    #[test]
    fn reply_invocation_authorizes_reply_object() {
        // Goal: valid reply invocation exports caller, target, and grant metadata.
        // Scope: unit test for reply authorization output.
        // Semantics: this does not consume the reply cap or mutate thread state.
        let mut cspace = CapabilitySpace::new();
        let caller = ObjectId::new(100);
        let target = ObjectId::new(200);
        let cap = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller,
                target,
                can_grant: true,
            })
            .unwrap();
        let object = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(&cspace, cap, Invocation::Reply { target }),
            Ok(InvocationOutcome::ReplyAuthorized {
                reply: object,
                caller,
                target,
                can_grant: true,
            })
        );
    }
}
