//! Stable kernel-facing error codes.
//!
//! Internal subsystems may keep richer typed errors for model tests and
//! diagnostics. Syscall and invocation boundaries should collapse those details
//! into a small, seL4-like set of error codes.

use crate::cap::CapError;
use crate::invocation::InvocationError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelErrorCode {
    NoError = 0,
    InvalidArgument = 1,
    InvalidCapability = 2,
    IllegalOperation = 3,
    RangeError = 4,
    AlignmentError = 5,
    FailedLookup = 6,
    TruncatedMessage = 7,
    DeleteFirst = 8,
    RevokeFirst = 9,
    NotEnoughMemory = 10,
}

impl KernelErrorCode {
    pub const fn raw(self) -> u32 {
        self as u32
    }
}

impl CapError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            // A missing or dead slot failed CSpace lookup. A descriptor that
            // still names a slot but no longer matches its object/slot state is
            // an invalid capability at the syscall boundary.
            Self::SlotNotFound(_) => KernelErrorCode::FailedLookup,
            Self::ObjectNotFound(_) | Self::ObjectDestroyed(_) | Self::StaleDescriptor { .. } => {
                KernelErrorCode::InvalidCapability
            }
            Self::RightsEscalation { .. }
            | Self::CapabilityNotDerivable { .. }
            | Self::CapabilityNotMintable { .. }
            | Self::InvalidRights { .. } => KernelErrorCode::IllegalOperation,
            Self::InvalidInitialCapability { .. } => KernelErrorCode::InvalidArgument,
            Self::WrongCapability { .. } => KernelErrorCode::InvalidCapability,
            Self::InvalidRetypeSize { .. } => KernelErrorCode::RangeError,
        }
    }
}

impl InvocationError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            Self::Cap(error) => error.error_code(),
            Self::WrongCapability { .. } => KernelErrorCode::InvalidCapability,
            Self::MissingRights { .. } | Self::ReplyTargetMismatch { .. } => {
                KernelErrorCode::IllegalOperation
            }
            Self::InvalidRetypeSize { .. } => KernelErrorCode::RangeError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cap::{
        Capability, CapabilitySpace, EndpointCap, FrameCap, ObjectId, RetypeTarget, Rights,
        UntypedCap,
    };
    use crate::invocation::{Invocation, invoke};

    fn endpoint(rights: Rights) -> Capability {
        Capability::Endpoint(EndpointCap { badge: 0, rights })
    }

    fn frame(rights: Rights) -> Capability {
        Capability::Frame(FrameCap { rights })
    }

    fn untyped(size_bits: u8) -> Capability {
        Capability::Untyped(UntypedCap { size_bits })
    }

    #[test]
    fn error_code_values_match_sel4_ordering() {
        assert_eq!(KernelErrorCode::NoError.raw(), 0);
        assert_eq!(KernelErrorCode::InvalidArgument.raw(), 1);
        assert_eq!(KernelErrorCode::InvalidCapability.raw(), 2);
        assert_eq!(KernelErrorCode::IllegalOperation.raw(), 3);
        assert_eq!(KernelErrorCode::RangeError.raw(), 4);
        assert_eq!(KernelErrorCode::AlignmentError.raw(), 5);
        assert_eq!(KernelErrorCode::FailedLookup.raw(), 6);
        assert_eq!(KernelErrorCode::TruncatedMessage.raw(), 7);
        assert_eq!(KernelErrorCode::DeleteFirst.raw(), 8);
        assert_eq!(KernelErrorCode::RevokeFirst.raw(), 9);
        assert_eq!(KernelErrorCode::NotEnoughMemory.raw(), 10);
    }

    #[test]
    fn cap_lookup_failure_collapses_to_failed_lookup_code() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        cspace.delete(cap).unwrap();

        assert_eq!(
            cspace.lookup(cap).unwrap_err().error_code(),
            KernelErrorCode::FailedLookup
        );
    }

    #[test]
    fn stale_descriptor_collapses_to_invalid_capability_code() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        let object = cspace.object_of(cap).unwrap();
        cspace.bump_generation(object).unwrap();

        assert_eq!(
            cspace.lookup(cap).unwrap_err().error_code(),
            KernelErrorCode::InvalidCapability
        );
    }

    #[test]
    fn rights_and_policy_failures_collapse_to_illegal_operation_code() {
        let mut cspace = CapabilitySpace::new();
        let endpoint = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();

        assert_eq!(
            cspace
                .derive(endpoint, Rights::READ | Rights::WRITE)
                .unwrap_err()
                .error_code(),
            KernelErrorCode::IllegalOperation
        );

        assert_eq!(
            cspace
                .insert_initial_capability(frame(Rights::READ | Rights::GRANT_REPLY))
                .unwrap_err()
                .error_code(),
            KernelErrorCode::IllegalOperation
        );
    }

    #[test]
    fn cap_retype_size_failure_collapses_to_range_error_code() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(untyped(11)).unwrap();

        assert_eq!(
            cspace
                .retype_untyped(
                    cap,
                    RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                )
                .unwrap_err()
                .error_code(),
            KernelErrorCode::RangeError
        );
    }

    #[test]
    fn invocation_errors_collapse_to_boundary_error_codes() {
        let mut cspace = CapabilitySpace::new();
        let endpoint = cspace
            .insert_initial_capability(endpoint(Rights::WRITE))
            .unwrap();
        let frame = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE))
            .unwrap();
        let untyped = cspace.insert_initial_capability(untyped(11)).unwrap();

        assert_eq!(
            invoke(
                &cspace,
                endpoint,
                Invocation::EndpointRecv { blocking: true }
            )
            .unwrap_err()
            .error_code(),
            KernelErrorCode::IllegalOperation
        );
        assert_eq!(
            invoke(&cspace, frame, Invocation::EndpointRecv { blocking: true })
                .unwrap_err()
                .error_code(),
            KernelErrorCode::InvalidCapability
        );
        assert_eq!(
            invoke(
                &cspace,
                untyped,
                Invocation::UntypedRetype {
                    target: RetypeTarget::Untyped { size_bits: 12 },
                },
            )
            .unwrap_err()
            .error_code(),
            KernelErrorCode::RangeError
        );
    }

    #[test]
    fn reply_target_mismatch_collapses_to_illegal_operation_code() {
        let mut cspace = CapabilitySpace::new();
        let cap = cspace
            .insert_reply_capability_for_test(crate::cap::ReplyCap {
                caller: ObjectId::new(1),
                target: ObjectId::new(2),
                can_grant: false,
            })
            .unwrap();

        assert_eq!(
            invoke(
                &cspace,
                cap,
                Invocation::Reply {
                    target: ObjectId::new(3),
                },
            )
            .unwrap_err()
            .error_code(),
            KernelErrorCode::IllegalOperation
        );
    }
}
