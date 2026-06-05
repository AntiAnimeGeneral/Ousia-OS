//! Stable kernel-facing error codes.
//!
//! Internal subsystems may keep richer typed errors for model tests and
//! diagnostics. Syscall and invocation boundaries should collapse those details
//! into a small, seL4-like set of error codes.

use crate::cap::CapError;
use crate::invocation::InvocationError;
use crate::ipc::IpcError;
use crate::object::ObjectTableError;
use crate::reply::ReplyError;
use crate::scheduler::SchedulerError;
use crate::state::KernelExecutionError;
use crate::thread::action::ThreadActionError;

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
            Self::SlotNotFound(_) | Self::InvalidCteReference { .. } => {
                KernelErrorCode::FailedLookup
            }
            Self::ObjectNotFound(_) | Self::ObjectDestroyed(_) | Self::StaleDescriptor { .. } => {
                KernelErrorCode::InvalidCapability
            }
            Self::RightsEscalation { .. }
            | Self::CapabilityNotDerivable { .. }
            | Self::CapabilityNotMintable { .. }
            | Self::InvalidRights { .. }
            | Self::SlotOccupied(_) => KernelErrorCode::IllegalOperation,
            Self::InvalidInitialCapability { .. } | Self::EmptyRetypeWindow => {
                KernelErrorCode::InvalidArgument
            }
            Self::WrongCapability { .. } => KernelErrorCode::InvalidCapability,
            Self::InvalidRetypeSize { .. } => KernelErrorCode::RangeError,
            Self::UntypedCapacityExhausted { .. } | Self::CapacityExhausted => {
                KernelErrorCode::NotEnoughMemory
            }
            Self::InvalidCNodeDepth { .. }
            | Self::RetypeWindowExceedsCNode { .. }
            | Self::SlotWindowOverflow { .. } => KernelErrorCode::RangeError,
            Self::CNodeGuardMismatch { .. }
            | Self::CNodeDepthMismatch { .. }
            | Self::CNodeLookupUnresolved { .. } => KernelErrorCode::FailedLookup,
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

impl IpcError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            Self::TooManyMessageWords { .. } => KernelErrorCode::TruncatedMessage,
        }
    }
}

impl ReplyError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            Self::AlreadyPending { .. } | Self::NoPendingCaller => {
                KernelErrorCode::IllegalOperation
            }
        }
    }
}

impl SchedulerError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            Self::NotEnoughCpus { .. }
            | Self::DuplicateCpu { .. }
            | Self::UnknownCpu { .. }
            | Self::ThreadAffinityMismatch { .. } => KernelErrorCode::InvalidArgument,
            Self::ThreadNotRunnable { .. }
            | Self::ThreadAlreadyScheduled { .. }
            | Self::CpuAlreadyHasCurrent { .. } => KernelErrorCode::IllegalOperation,
            Self::ReadyQueueFull { .. } | Self::RunQueueCapacityExhausted { .. } => {
                KernelErrorCode::NotEnoughMemory
            }
        }
    }
}

impl ObjectTableError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            Self::ObjectNotFound { .. } | Self::ThreadObjectNotFound { .. } => {
                KernelErrorCode::FailedLookup
            }
            Self::WrongObjectType { .. } => KernelErrorCode::InvalidCapability,
            Self::ObjectTableFull { .. } => KernelErrorCode::NotEnoughMemory,
            Self::ObjectIdAlreadyBound { .. }
            | Self::TcbObjectUnbound { .. }
            | Self::ThreadObjectAlreadyBound { .. } => KernelErrorCode::IllegalOperation,
        }
    }
}

impl ThreadActionError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            Self::UnknownThread { .. } => KernelErrorCode::FailedLookup,
            Self::WrongCpu { .. } => KernelErrorCode::InvalidArgument,
            Self::ThreadNotCurrent { .. }
            | Self::UnexpectedThreadState { .. }
            | Self::NotWaitingOnBoundNotification { .. }
            | Self::MissingReplyObject { .. }
            | Self::MissingCallerObject { .. }
            | Self::ReceiveCallTransactionUnsupported { .. }
            | Self::ReplyAlreadyPending
            | Self::ThreadNotResumable { .. } => KernelErrorCode::IllegalOperation,
            Self::ThreadTableFull { .. } => KernelErrorCode::NotEnoughMemory,
            Self::Reply(error) => error.error_code(),
            Self::Scheduler(error) => error.error_code(),
        }
    }
}

impl KernelExecutionError {
    pub fn error_code(&self) -> KernelErrorCode {
        match self {
            Self::Invocation(error) => error.error_code(),
            Self::Object(error) => error.error_code(),
            Self::Thread(error) => error.error_code(),
            Self::Scheduler(error) => error.error_code(),
            Self::MissingReplyObject { .. }
            | Self::ReplyObjectMustBeDistinct { .. }
            | Self::ReplyAuthorityMismatch { .. }
            | Self::ThreadAlreadyExists { .. } => KernelErrorCode::IllegalOperation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cap::{
        Capability, CapabilitySpace, EndpointCap, FrameCap, ObjectId, RetypeTarget, Rights, TcbCap,
        UntypedCap,
    };
    use crate::invocation::{Invocation, invoke};
    use crate::ipc::{IpcError, IpcPayload, MAX_IPC_WORDS};
    use crate::object::{ObjectTable, ObjectTableError};
    use crate::reply::{Reply, ReplyCaller, ReplyCallerParams, ReplyError};
    use crate::scheduler::Scheduler;
    use crate::state::{InvocationContext, KernelState};
    use crate::thread::{
        action::ThreadTable,
        tcb::{CpuId, Tcb, ThreadId, ThreadState},
    };

    fn endpoint(rights: Rights) -> Capability {
        Capability::Endpoint(EndpointCap { badge: 0, rights })
    }

    fn frame(rights: Rights) -> Capability {
        Capability::Frame(FrameCap { rights })
    }

    fn untyped(size_bits: u8) -> Capability {
        Capability::Untyped(UntypedCap { size_bits })
    }

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    fn tcb_state_with_object() -> (KernelState, crate::cap::CapabilityDescriptor, ObjectId) {
        let mut cspace = CapabilitySpace::new();
        let descriptor = cspace
            .insert_initial_capability(Capability::Tcb(TcbCap {
                rights: Rights::MANAGE,
            }))
            .unwrap();
        let object = cspace.object_of(descriptor).unwrap();
        let mut objects = ObjectTable::new();
        objects.insert_tcb(object).unwrap();
        let threads = ThreadTable::new();
        let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        (
            KernelState::from_parts(cspace, objects, threads, scheduler),
            descriptor,
            object,
        )
    }

    // Error tests protect the stable boundary code categories. Most cases
    // intentionally trigger errors through public module or executor paths so
    // the mapping can change only with an explicit semantic/API decision.

    #[test]
    fn error_code_values_match_sel4_ordering() {
        // Goal: preserve stable seL4-like numeric ordering for external callers.
        // Scope: unit test for the public error-code ABI.
        // Semantics: changing these values is a compatibility decision, not cleanup.
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
        // Goal: deleted capability lookup maps to the stable failed-lookup boundary code.
        // Scope: CapError to KernelErrorCode mapping through a real lookup path.
        // Semantics: missing slot details stay diagnostic; callers observe FailedLookup.
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
        // Goal: stale descriptors map to invalid capability instead of failed lookup.
        // Scope: object generation check through a real capability lookup path.
        // Semantics: ABA protection remains distinguishable at the public error-code boundary.
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
        // Goal: rights escalation and object-policy violations share illegal-operation semantics.
        // Scope: capability derivation and initial insertion policy boundaries.
        // Semantics: caller-visible error code hides internal policy detail without losing rejection.
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
        // Goal: retype requests larger than the untyped source map to range error.
        // Scope: Untyped capacity model through a real retype request.
        // Semantics: invalid size is not reported as allocation exhaustion.
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
    fn cap_retype_capacity_failure_collapses_to_not_enough_memory_code() {
        // Goal: exhausted Untyped capacity maps to not-enough-memory.
        // Scope: repeated retype request after prior child allocation.
        // Semantics: source size is valid, but remaining capacity cannot satisfy the request.
        let mut cspace = CapabilitySpace::new();
        let cap = cspace.insert_initial_capability(untyped(12)).unwrap();
        cspace
            .retype_untyped(
                cap,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();

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
            KernelErrorCode::NotEnoughMemory
        );
    }

    #[test]
    fn invocation_errors_collapse_to_boundary_error_codes() {
        // Goal: invocation authorization failures collapse into stable boundary categories.
        // Scope: unit test through invoke(), not direct InvocationError construction.
        // Semantics: detailed errors remain diagnostic; callers observe stable codes.
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
        // Goal: reply target mismatch maps to illegal operation at invocation boundary.
        // Scope: real reply invocation authorization over a Reply cap.
        // Semantics: target authority failure is not a lookup or capability-staleness failure.
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

    #[test]
    fn ipc_payload_failure_collapses_to_truncated_message_code() {
        // Goal: oversized IPC payloads map to the truncated-message boundary code.
        // Scope: payload construction error before endpoint or thread side effects.
        // Semantics: message-register limit failure remains distinct from generic invalid argument.
        let error = IpcPayload::new(&[1, 2, 3, 4, 5]).unwrap_err();

        assert_eq!(
            error,
            IpcError::TooManyMessageWords {
                requested: MAX_IPC_WORDS + 1,
                limit: MAX_IPC_WORDS,
            }
        );
        assert_eq!(error.error_code(), KernelErrorCode::TruncatedMessage);
    }

    #[test]
    fn reply_state_errors_collapse_to_illegal_operation_code() {
        // Goal: invalid Reply state transitions map to illegal operation.
        // Scope: Reply local state-machine errors through public error-code mapping.
        // Semantics: empty reply and overwrite attempts remain stable caller-visible failures.
        let mut reply = Reply::new();
        let empty_reply_error = reply.reply().unwrap_err();

        assert_eq!(empty_reply_error, ReplyError::NoPendingCaller);
        assert_eq!(
            empty_reply_error.error_code(),
            KernelErrorCode::IllegalOperation,
        );

        reply
            .record_caller(ReplyCaller::new(ReplyCallerParams {
                caller: ObjectId::new(1),
                target: ObjectId::new(2),
                thread: ThreadId::new(3),
                cpu: CpuId::new(0),
                can_grant: false,
            }))
            .unwrap();

        assert_eq!(
            reply
                .record_caller(ReplyCaller::new(ReplyCallerParams {
                    caller: ObjectId::new(4),
                    target: ObjectId::new(2),
                    thread: ThreadId::new(5),
                    cpu: CpuId::new(1),
                    can_grant: true,
                }))
                .unwrap_err()
                .error_code(),
            KernelErrorCode::IllegalOperation,
        );
    }

    #[test]
    fn executor_object_lookup_failure_collapses_to_failed_lookup_code() {
        // Goal: executor object lookup failures map to failed lookup at the boundary.
        // Scope: host-style KernelState path crossing CSpace, ObjectTable, threads, and scheduler.
        // Semantics: no endpoint object means lookup failed before IPC side effects.
        let mut cspace = CapabilitySpace::new();
        let descriptor = cspace
            .insert_initial_capability(Capability::Endpoint(EndpointCap {
                badge: 0,
                rights: Rights::WRITE,
            }))
            .unwrap();
        let mut tcb = Tcb::new(thread(1), cpu(0));
        tcb.set_state(ThreadState::Running);
        let mut threads = ThreadTable::new();
        threads
            .insert(tcb.clone())
            .expect("test thread table must have capacity");
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&tcb).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();
        let mut state = KernelState::from_parts(cspace, ObjectTable::new(), threads, scheduler);

        assert_eq!(
            state
                .execute_invocation(
                    InvocationContext::new(thread(1), cpu(0)),
                    descriptor,
                    Invocation::EndpointSend {
                        message_words: 0,
                        op: crate::invocation::EndpointSendOp::Send,
                    },
                )
                .unwrap_err()
                .error_code(),
            KernelErrorCode::FailedLookup
        );
    }

    #[test]
    fn tcb_configure_unknown_cpu_collapses_to_invalid_argument_code() {
        // Goal: unknown CPU during TCB configure maps to invalid argument.
        // Scope: KernelState TCB configure path crossing ObjectTable, ThreadTable, and Scheduler.
        // Semantics: failed CPU validation leaves TCB object unbound and no thread inserted.
        let (mut state, descriptor, object) = tcb_state_with_object();

        assert_eq!(
            state
                .execute_invocation(
                    InvocationContext::new(thread(1), cpu(0)),
                    descriptor,
                    Invocation::TcbConfigure {
                        thread: thread(2),
                        affinity: cpu(9),
                    },
                )
                .unwrap_err()
                .error_code(),
            KernelErrorCode::InvalidArgument
        );
        assert_eq!(
            state.objects.tcb_thread(object),
            Err(ObjectTableError::TcbObjectUnbound { object })
        );
        assert_eq!(state.threads.get(thread(2)), None);
    }

    #[test]
    fn tcb_resume_unbound_object_collapses_to_illegal_operation_code() {
        // Goal: resuming an unbound TCB object maps to illegal operation.
        // Scope: KernelState TCB resume boundary before scheduler enqueue.
        // Semantics: object binding failure leaves scheduler queues empty.
        let (mut state, descriptor, object) = tcb_state_with_object();

        assert_eq!(
            state
                .execute_invocation(
                    InvocationContext::new(thread(1), cpu(0)),
                    descriptor,
                    Invocation::TcbResume,
                )
                .unwrap_err()
                .error_code(),
            KernelErrorCode::IllegalOperation
        );
        assert_eq!(
            state.objects.tcb_thread(object),
            Err(ObjectTableError::TcbObjectUnbound { object })
        );
        assert_eq!(state.scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(state.scheduler.run_queue(cpu(1)).unwrap().ready_len(), 0);
    }

    #[test]
    fn tcb_resume_missing_thread_collapses_to_failed_lookup_code() {
        // Goal: a bound but missing runtime thread maps to failed lookup.
        // Scope: KernelState TCB resume path after ObjectTable binding succeeds.
        // Semantics: missing ThreadTable entry prevents scheduler mutation.
        let (mut state, descriptor, object) = tcb_state_with_object();
        state.objects.bind_tcb(object, thread(2)).unwrap();

        assert_eq!(
            state
                .execute_invocation(
                    InvocationContext::new(thread(1), cpu(0)),
                    descriptor,
                    Invocation::TcbResume,
                )
                .unwrap_err()
                .error_code(),
            KernelErrorCode::FailedLookup
        );
        assert_eq!(state.scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(state.scheduler.run_queue(cpu(1)).unwrap().ready_len(), 0);
    }

    #[test]
    fn duplicate_thread_bootstrap_collapses_to_illegal_operation_code() {
        // Goal: duplicate thread bootstrap fails without rebinding the new TCB object.
        // Scope: KernelState path for TCB object/thread/scheduler ownership.
        // Semantics: the error code is illegal operation and all existing ownership stays intact.
        let mut state = KernelState::new(&[cpu(0), cpu(1)]).unwrap();
        state.objects.insert_tcb(ObjectId::new(1)).unwrap();
        state
            .insert_thread_object(ObjectId::new(1), Tcb::new(thread(2), cpu(0)))
            .unwrap();
        state.objects.insert_tcb(ObjectId::new(2)).unwrap();

        assert_eq!(
            state
                .insert_thread_object(ObjectId::new(2), Tcb::new(thread(2), cpu(1)))
                .unwrap_err()
                .error_code(),
            KernelErrorCode::IllegalOperation
        );
        assert_eq!(
            state.objects.tcb_thread(ObjectId::new(2)),
            Err(ObjectTableError::TcbObjectUnbound {
                object: ObjectId::new(2),
            })
        );
        assert_eq!(state.threads.affinity(thread(2)), Some(cpu(0)));
        assert_eq!(state.scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(state.scheduler.run_queue(cpu(1)).unwrap().ready_len(), 0);
    }
}
