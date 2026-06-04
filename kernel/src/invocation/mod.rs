use crate::cap::{
    CNodePath, CapError, Capability, CapabilityDescriptor, CapabilitySpace, MintParams, ObjectId,
    RetypeDestination, RetypeTarget, Rights, SlotId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CNodePathTarget {
    pub capptr: u64,
    pub depth: u8,
}

impl CNodePathTarget {
    pub const fn under_root(self, root: CapabilityDescriptor) -> CNodePath {
        CNodePath {
            root,
            capptr: self.capptr,
            depth: self.depth,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetypeDestinationPath {
    pub start: CNodePath,
    pub count: usize,
}
use crate::thread::tcb::{CpuId, ThreadId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    EndpointSend {
        message_words: usize,
        op: EndpointSendOp,
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
    UntypedRetypePath {
        target: RetypeTarget,
        destination: RetypeDestinationPath,
    },
    CNodeCopyInto {
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
    },
    CNodeCopyPath {
        source: CapabilityDescriptor,
        destination: CNodePathTarget,
        requested_rights: Rights,
    },
    CNodeMintInto {
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
        params: MintParams,
    },
    CNodeMintPath {
        source: CapabilityDescriptor,
        destination: CNodePathTarget,
        requested_rights: Rights,
        params: MintParams,
    },
    CNodeMoveInto {
        source: CapabilityDescriptor,
        destination: SlotId,
    },
    CNodeMovePath {
        source: CapabilityDescriptor,
        destination: CNodePathTarget,
    },
    CNodeDelete {
        target: CapabilityDescriptor,
    },
    CNodeDeletePath {
        target: CNodePathTarget,
    },
    CNodeRevoke {
        target: CapabilityDescriptor,
    },
    CNodeRevokePath {
        target: CNodePathTarget,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointSendOp {
    Send,
    NBSend,
    Call,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationOutcome {
    SendIpcAuthorized(EndpointSendAuthorized),
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
    CNodeCopyPathAuthorized {
        source: CapabilityDescriptor,
        destination: CNodePath,
        requested_rights: Rights,
    },
    CNodeMintAuthorized {
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
        params: MintParams,
    },
    CNodeMintPathAuthorized {
        source: CapabilityDescriptor,
        destination: CNodePath,
        requested_rights: Rights,
        params: MintParams,
    },
    CNodeMoveAuthorized {
        source: CapabilityDescriptor,
        destination: SlotId,
    },
    CNodeMovePathAuthorized {
        source: CapabilityDescriptor,
        destination: CNodePath,
    },
    CNodeDeleteAuthorized {
        target: CapabilityDescriptor,
    },
    CNodeDeletePathAuthorized {
        target: CNodePath,
    },
    CNodeRevokeAuthorized {
        target: CapabilityDescriptor,
    },
    CNodeRevokePathAuthorized {
        target: CNodePath,
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
pub struct EndpointSendAuthorized {
    pub endpoint: ObjectId,
    pub badge: u64,
    pub message_words: usize,
    pub op: EndpointSendOp,
    pub can_grant: bool,
    pub can_grant_reply: bool,
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

impl EndpointSendOp {
    pub const fn is_blocking(self) -> bool {
        matches!(self, Self::Send | Self::Call)
    }

    pub const fn is_call(self) -> bool {
        matches!(self, Self::Call)
    }
}

pub fn invoke(
    cspace: &CapabilitySpace,
    descriptor: CapabilityDescriptor,
    invocation: Invocation,
) -> Result<InvocationOutcome, InvocationError> {
    let view = cspace.lookup(descriptor)?;

    match invocation {
        Invocation::EndpointSend { message_words, op } => match view.capability {
            Capability::Endpoint(cap) => {
                if !cap.can_send() {
                    return Err(InvocationError::MissingRights {
                        required: Rights::WRITE,
                        actual: view.rights,
                    });
                }
                Ok(InvocationOutcome::SendIpcAuthorized(
                    EndpointSendAuthorized {
                        endpoint: view.object,
                        badge: cap.badge,
                        message_words,
                        op,
                        can_grant: cap.can_grant(),
                        can_grant_reply: cap.can_grant_reply(),
                    },
                ))
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
        Invocation::UntypedRetypePath {
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
                let lookup = cspace.lookup_cnode_window(destination.start)?;
                if destination.count > lookup.slots_remaining {
                    return Err(CapError::RetypeWindowExceedsCNode {
                        start: lookup.slot,
                        requested: destination.count,
                        available: lookup.slots_remaining,
                    }
                    .into());
                }
                Ok(InvocationOutcome::UntypedRetypeAuthorized {
                    untyped: view.object,
                    target,
                    destination: Some(RetypeDestination {
                        start: lookup.slot,
                        count: destination.count,
                    }),
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
        Invocation::CNodeCopyPath {
            source,
            destination,
            requested_rights,
        } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeCopyPathAuthorized {
                source,
                destination: destination.under_root(descriptor),
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
        Invocation::CNodeMintPath {
            source,
            destination,
            requested_rights,
            params,
        } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeMintPathAuthorized {
                source,
                destination: destination.under_root(descriptor),
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
        Invocation::CNodeMovePath {
            source,
            destination,
        } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeMovePathAuthorized {
                source,
                destination: destination.under_root(descriptor),
            }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeDelete { target } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeDeleteAuthorized { target }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeDeletePath { target } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeDeletePathAuthorized {
                target: target.under_root(descriptor),
            }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeRevoke { target } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeRevokeAuthorized { target }),
            actual => Err(wrong_capability(InvocationTarget::CNode, actual)),
        },
        Invocation::CNodeRevokePath { target } => match view.capability {
            Capability::CNode(_) => Ok(InvocationOutcome::CNodeRevokePathAuthorized {
                target: target.under_root(descriptor),
            }),
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
    use rstest::rstest;

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

    fn configure_thread() -> ThreadId {
        ThreadId::new(10)
    }

    fn configure_affinity() -> CpuId {
        CpuId::new(1)
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
                    op: EndpointSendOp::Call,
                },
            ),
            Ok(InvocationOutcome::SendIpcAuthorized(
                EndpointSendAuthorized {
                    endpoint,
                    badge: 0x2a,
                    message_words: 3,
                    op: EndpointSendOp::Call,
                    can_grant: false,
                    can_grant_reply: true,
                }
            ))
        );
    }

    #[test]
    fn endpoint_call_without_grant_authorizes_without_reply_transfer_rights() {
        // Goal: endpoint call authorization follows seL4 and does not require grant rights.
        // Scope: unit test for endpoint invocation rights.
        // Semantics: WRITE permits call; later IPC/reply transition consumes grant facts.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(endpoint(Rights::WRITE, 0x2a))
            .unwrap();
        let endpoint = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::EndpointSend {
                    message_words: 0,
                    op: EndpointSendOp::Call,
                },
            ),
            Ok(InvocationOutcome::SendIpcAuthorized(
                EndpointSendAuthorized {
                    endpoint,
                    badge: 0x2a,
                    message_words: 0,
                    op: EndpointSendOp::Call,
                    can_grant: false,
                    can_grant_reply: false,
                }
            ))
        );
    }

    #[rstest]
    #[case::endpoint_recv_requires_read(
        endpoint(Rights::WRITE, 0),
        Invocation::EndpointRecv { blocking: true },
        Rights::READ,
        Rights::WRITE
    )]
    #[case::notification_wait_requires_read(
        notification(Rights::WRITE, 0),
        Invocation::NotificationWait { blocking: false },
        Rights::READ,
        Rights::WRITE
    )]
    #[case::tcb_resume_requires_manage(
        tcb(Rights::NONE),
        Invocation::TcbResume,
        Rights::MANAGE,
        Rights::NONE
    )]
    #[case::tcb_configure_requires_manage(
        tcb(Rights::NONE),
        Invocation::TcbConfigure {
            thread: configure_thread(),
            affinity: configure_affinity(),
        },
        Rights::MANAGE,
        Rights::NONE
    )]
    fn invocation_rejects_missing_capability_rights(
        #[case] capability: Capability,
        #[case] invocation: Invocation,
        #[case] required: Rights,
        #[case] actual: Rights,
    ) {
        // Goal: invocation authorization rejects insufficient rights at the CSpace boundary.
        // Scope: unit test for typed capabilities with the right object kind but missing rights.
        // Semantics: missing rights fail before executor-owned endpoint, notification, or TCB side effects.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(capability).unwrap();

        assert_eq!(
            invoke(&cspace, cap, invocation),
            Err(InvocationError::MissingRights { required, actual })
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
                    op: EndpointSendOp::NBSend,
                },
            ),
            Ok(InvocationOutcome::SendIpcAuthorized(
                EndpointSendAuthorized {
                    endpoint,
                    badge: 0x2a,
                    message_words: 1,
                    op: EndpointSendOp::NBSend,
                    can_grant: true,
                    can_grant_reply: true,
                }
            ))
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
                    op: EndpointSendOp::Send,
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
        let destination = SlotId::new(30);

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
    fn cnode_copy_path_targets_invoked_cnode_root() {
        // Goal: target CNode paths cannot name an arbitrary root distinct from the invoked cap.
        // Scope: unit test for CNode invocation authorization before CSpace mutation.
        // Semantics: the authorized path root is always the invoked CNode descriptor.
        let mut cspace = CapabilitySpace::new();
        let cnode_cap = cspace.insert_initial_capability(cnode()).unwrap();
        let source = cspace
            .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE, 0x55))
            .unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cnode_cap,
                Invocation::CNodeCopyPath {
                    source,
                    destination: CNodePathTarget {
                        capptr: 0b10_0110,
                        depth: 6,
                    },
                    requested_rights: Rights::READ,
                },
            ),
            Ok(InvocationOutcome::CNodeCopyPathAuthorized {
                source,
                destination: CNodePath {
                    root: cnode_cap,
                    capptr: 0b10_0110,
                    depth: 6,
                },
                requested_rights: Rights::READ,
            })
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
    fn tcb_configure_exports_configuration_after_rights_check() {
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
