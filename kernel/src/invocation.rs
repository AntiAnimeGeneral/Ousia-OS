use crate::cap::{CapError, Capability, CapabilityDescriptor, CapabilitySpace, ObjectId, Rights};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    EndpointSend { message_words: usize },
    EndpointRecv,
    FrameMap { address_space: ObjectId },
    UntypedRetype { size_bits: u8 },
    TcbResume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationOutcome {
    EndpointSendQueued {
        endpoint: ObjectId,
        badge: u64,
        message_words: usize,
    },
    EndpointReceiveBlocked {
        endpoint: ObjectId,
    },
    FrameMapAuthorized {
        frame: ObjectId,
        address_space: ObjectId,
    },
    UntypedRetypeAuthorized {
        untyped: ObjectId,
        size_bits: u8,
    },
    TcbResumeAuthorized {
        tcb: ObjectId,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationTarget {
    Endpoint,
    Frame,
    Untyped,
    Tcb,
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
        Invocation::EndpointSend { message_words } => match view.capability {
            Capability::Endpoint(cap) => {
                require_rights(view.rights, Rights::WRITE)?;
                Ok(InvocationOutcome::EndpointSendQueued {
                    endpoint: view.object,
                    badge: cap.badge,
                    message_words,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Endpoint, actual)),
        },
        Invocation::EndpointRecv => match view.capability {
            Capability::Endpoint(_) => {
                require_rights(view.rights, Rights::READ)?;
                Ok(InvocationOutcome::EndpointReceiveBlocked {
                    endpoint: view.object,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Endpoint, actual)),
        },
        Invocation::FrameMap { address_space } => match view.capability {
            Capability::Frame(_) => {
                require_rights(view.rights, Rights::READ | Rights::WRITE)?;
                Ok(InvocationOutcome::FrameMapAuthorized {
                    frame: view.object,
                    address_space,
                })
            }
            actual => Err(wrong_capability(InvocationTarget::Frame, actual)),
        },
        Invocation::UntypedRetype { size_bits } => match view.capability {
            Capability::Untyped(cap) => {
                if size_bits > cap.size_bits {
                    return Err(InvocationError::InvalidRetypeSize {
                        requested: size_bits,
                        source: cap.size_bits,
                    });
                }
                Ok(InvocationOutcome::UntypedRetypeAuthorized {
                    untyped: view.object,
                    size_bits,
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
    use crate::cap::{EndpointCap, FrameCap, TcbCap, UntypedCap};

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

    #[test]
    fn endpoint_send_requires_write_rights_and_preserves_badge() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.create_object(endpoint(Rights::READ | Rights::WRITE, 0x2a));
        let endpoint = cspace.object_of(cap).unwrap();

        assert_eq!(
            invoke(&cspace, cap, Invocation::EndpointSend { message_words: 3 },),
            Ok(InvocationOutcome::EndpointSendQueued {
                endpoint,
                badge: 0x2a,
                message_words: 3,
            })
        );
    }

    #[test]
    fn endpoint_recv_requires_read_rights() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.create_object(endpoint(Rights::WRITE, 0));

        assert_eq!(
            invoke(&cspace, cap, Invocation::EndpointRecv),
            Err(InvocationError::MissingRights {
                required: Rights::READ,
                actual: Rights::WRITE,
            })
        );
    }

    #[test]
    fn wrong_capability_is_reported_explicitly() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.create_object(frame(Rights::READ | Rights::WRITE));

        assert_eq!(
            invoke(&cspace, cap, Invocation::EndpointSend { message_words: 1 },),
            Err(InvocationError::WrongCapability {
                expected: InvocationTarget::Endpoint,
                actual: frame(Rights::READ | Rights::WRITE),
            })
        );
    }

    #[test]
    fn frame_map_requires_read_and_write_rights() {
        let mut cspace = CapabilitySpace::new();
        let frame = cspace.create_object(frame(Rights::READ | Rights::WRITE));
        let address_space_cap = cspace.create_object(tcb(Rights::MANAGE));
        let address_space = cspace.object_of(address_space_cap).unwrap();
        let frame_object = cspace.object_of(frame).unwrap();

        assert_eq!(
            invoke(&cspace, frame, Invocation::FrameMap { address_space }),
            Ok(InvocationOutcome::FrameMapAuthorized {
                frame: frame_object,
                address_space,
            })
        );
    }

    #[test]
    fn untyped_retype_cannot_exceed_source_size() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.create_object(untyped(12));

        assert_eq!(
            invoke(&cspace, cap, Invocation::UntypedRetype { size_bits: 13 }),
            Err(InvocationError::InvalidRetypeSize {
                requested: 13,
                source: 12,
            })
        );
    }

    #[test]
    fn tcb_resume_requires_manage_rights() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.create_object(tcb(Rights::READ));

        assert_eq!(
            invoke(&cspace, cap, Invocation::TcbResume),
            Err(InvocationError::MissingRights {
                required: Rights::MANAGE,
                actual: Rights::READ,
            })
        );
    }
}
