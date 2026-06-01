use crate::cap::{
    CapError, Capability, CapabilityDescriptor, CapabilitySpace, ObjectId, RetypeTarget, Rights,
};

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
    TcbResume,
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
    },
    TcbResumeAuthorized {
        tcb: ObjectId,
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
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Untyped, actual)),
        },
        Invocation::TcbResume => match view.capability {
            Capability::Tcb(_) => {
                require_rights(view.rights, Rights::MANAGE)?;
                Ok(InvocationOutcome::TcbResumeAuthorized { tcb: view.object })
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
    use crate::cap::{EndpointCap, FrameCap, NotificationCap, ReplyCap, TcbCap, UntypedCap};

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

    #[test]
    fn endpoint_send_requires_write_rights_and_preserves_badge() {
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
            })
        );
    }

    #[test]
    fn untyped_retype_rejects_target_rights_outside_object_policy() {
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
    fn untyped_retype_outcome_feeds_cspace_retype() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(untyped(12)).unwrap();

        let outcome = invoke(
            &cspace,
            cap,
            Invocation::UntypedRetype {
                target: RetypeTarget::Frame {
                    rights: Rights::READ | Rights::WRITE,
                },
            },
        )
        .unwrap();

        let InvocationOutcome::UntypedRetypeAuthorized { target, .. } = outcome else {
            panic!("expected untyped retype authorization");
        };
        let frame_cap = cspace.retype_untyped(cap, target).unwrap();

        assert_eq!(
            cspace.lookup(frame_cap).unwrap().capability,
            frame(Rights::READ | Rights::WRITE)
        );
    }

    #[test]
    fn untyped_retype_checks_target_minimum_size() {
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
    fn tcb_resume_requires_manage_rights() {
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
    fn notification_signal_requires_write_and_preserves_badge() {
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
