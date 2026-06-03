use crate::{
    cap::ObjectId,
    tcb::{CpuId, ThreadId},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyCaller {
    caller: ObjectId,
    target: ObjectId,
    thread: ThreadId,
    cpu: CpuId,
    can_grant: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyCallerParams {
    pub caller: ObjectId,
    pub target: ObjectId,
    pub thread: ThreadId,
    pub cpu: CpuId,
    pub can_grant: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyState {
    Empty,
    Pending { caller: ReplyCaller },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplyAction {
    CallerRecorded {
        caller_object: ObjectId,
        target_object: ObjectId,
        caller_thread: ThreadId,
        caller_cpu: CpuId,
        can_grant: bool,
    },
    Replied {
        caller_object: ObjectId,
        target_object: ObjectId,
        caller_thread: ThreadId,
        caller_cpu: CpuId,
        can_grant: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplyError {
    AlreadyPending { existing: ThreadId },
    NoPendingCaller,
}

#[derive(Debug)]
pub struct Reply {
    state: ReplyState,
}

impl ReplyCaller {
    pub const fn new(params: ReplyCallerParams) -> Self {
        Self {
            caller: params.caller,
            target: params.target,
            thread: params.thread,
            cpu: params.cpu,
            can_grant: params.can_grant,
        }
    }

    pub const fn caller(self) -> ObjectId {
        self.caller
    }

    pub const fn target(self) -> ObjectId {
        self.target
    }

    pub const fn thread(self) -> ThreadId {
        self.thread
    }

    pub const fn cpu(self) -> CpuId {
        self.cpu
    }

    pub const fn can_grant(self) -> bool {
        self.can_grant
    }
}

impl Reply {
    pub const fn new() -> Self {
        Self {
            state: ReplyState::Empty,
        }
    }

    pub fn record_caller(&mut self, caller: ReplyCaller) -> Result<ReplyAction, ReplyError> {
        match self.state {
            ReplyState::Empty => {
                self.state = ReplyState::Pending { caller };
                Ok(ReplyAction::CallerRecorded {
                    caller_object: caller.caller,
                    target_object: caller.target,
                    caller_thread: caller.thread,
                    caller_cpu: caller.cpu,
                    can_grant: caller.can_grant,
                })
            }
            ReplyState::Pending { caller } => Err(ReplyError::AlreadyPending {
                existing: caller.thread,
            }),
        }
    }

    pub fn reply(&mut self) -> Result<ReplyAction, ReplyError> {
        match self.state {
            ReplyState::Empty => Err(ReplyError::NoPendingCaller),
            ReplyState::Pending { caller } => {
                self.state = ReplyState::Empty;
                Ok(ReplyAction::Replied {
                    caller_object: caller.caller,
                    target_object: caller.target,
                    caller_thread: caller.thread,
                    caller_cpu: caller.cpu,
                    can_grant: caller.can_grant,
                })
            }
        }
    }

    pub const fn state(&self) -> ReplyState {
        self.state
    }

    pub const fn is_pending(&self) -> bool {
        matches!(self.state, ReplyState::Pending { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    fn object(raw: u64) -> ObjectId {
        ObjectId::new(raw)
    }

    fn caller(caller: u64, target: u64, thread: u64, cpu: u32, can_grant: bool) -> ReplyCaller {
        ReplyCaller::new(ReplyCallerParams {
            caller: object(caller),
            target: object(target),
            thread: self::thread(thread),
            cpu: self::cpu(cpu),
            can_grant,
        })
    }

    #[test]
    fn new_reply_starts_empty() {
        // Goal: Reply starts without pending caller authority.
        // Scope: local Reply default-state contract.
        // Semantics: a newly created Reply is Empty until a caller is recorded.
        let reply = Reply::new();

        assert_eq!(reply.state(), ReplyState::Empty);
    }

    #[rstest]
    #[case::caller_can_grant(caller(100, 200, 1, 0, true))]
    #[case::caller_cannot_grant(caller(101, 201, 2, 1, false))]
    fn reply_records_and_consumes_single_pending_caller(#[case] caller: ReplyCaller) {
        // Goal: Reply owns exactly one pending caller and exposes record/reply transitions.
        // Scope: local Reply state machine without CSpace slot consumption or scheduler wakeup.
        // Semantics: record stores caller metadata; reply consumes the same metadata and returns Empty.
        let mut reply = Reply::new();

        assert_eq!(
            reply.record_caller(caller),
            Ok(ReplyAction::CallerRecorded {
                caller_object: caller.caller(),
                target_object: caller.target(),
                caller_thread: caller.thread(),
                caller_cpu: caller.cpu(),
                can_grant: caller.can_grant(),
            })
        );
        assert_eq!(reply.state(), ReplyState::Pending { caller });

        assert_eq!(
            reply.reply(),
            Ok(ReplyAction::Replied {
                caller_object: caller.caller(),
                target_object: caller.target(),
                caller_thread: caller.thread(),
                caller_cpu: caller.cpu(),
                can_grant: caller.can_grant(),
            })
        );
        assert_eq!(reply.state(), ReplyState::Empty);
    }

    #[test]
    fn cannot_overwrite_pending_caller() {
        // Goal: Reply rejects a second caller while one reply slot is pending.
        // Scope: local Reply state machine error path.
        // Semantics: the original caller remains pending after the overwrite attempt fails.
        let mut reply = Reply::new();
        let existing = caller(100, 200, 1, 0, true);

        reply.record_caller(existing).unwrap();

        assert_eq!(
            reply.record_caller(caller(101, 200, 2, 1, false)),
            Err(ReplyError::AlreadyPending {
                existing: thread(1),
            })
        );
        assert_eq!(reply.state(), ReplyState::Pending { caller: existing });
    }

    #[test]
    fn reply_requires_pending_caller() {
        // Goal: Reply cannot consume a caller that was never recorded.
        // Scope: local Reply state machine empty-state error path.
        // Semantics: replying from Empty fails and leaves the Reply empty.
        let mut reply = Reply::new();

        assert_eq!(reply.reply(), Err(ReplyError::NoPendingCaller));
        assert_eq!(reply.state(), ReplyState::Empty);
    }
}
