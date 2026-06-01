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
    pub const fn new(
        caller: ObjectId,
        target: ObjectId,
        thread: ThreadId,
        cpu: CpuId,
        can_grant: bool,
    ) -> Self {
        Self {
            caller,
            target,
            thread,
            cpu,
            can_grant,
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

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    fn object(raw: u64) -> ObjectId {
        ObjectId::new(raw)
    }

    #[test]
    fn new_reply_starts_empty() {
        let reply = Reply::new();

        assert_eq!(reply.state(), ReplyState::Empty);
    }

    #[test]
    fn records_single_pending_caller() {
        let mut reply = Reply::new();
        let caller = ReplyCaller::new(object(100), object(200), thread(1), cpu(0), true);

        assert_eq!(
            reply.record_caller(caller),
            Ok(ReplyAction::CallerRecorded {
                caller_object: object(100),
                target_object: object(200),
                caller_thread: thread(1),
                caller_cpu: cpu(0),
                can_grant: true,
            })
        );
        assert_eq!(reply.state(), ReplyState::Pending { caller });
    }

    #[test]
    fn cannot_overwrite_pending_caller() {
        let mut reply = Reply::new();

        reply
            .record_caller(ReplyCaller::new(
                object(100),
                object(200),
                thread(1),
                cpu(0),
                true,
            ))
            .unwrap();

        assert_eq!(
            reply.record_caller(ReplyCaller::new(
                object(101),
                object(200),
                thread(2),
                cpu(1),
                false,
            )),
            Err(ReplyError::AlreadyPending {
                existing: thread(1),
            })
        );
    }

    #[test]
    fn reply_consumes_pending_caller() {
        let mut reply = Reply::new();

        reply
            .record_caller(ReplyCaller::new(
                object(100),
                object(200),
                thread(1),
                cpu(0),
                true,
            ))
            .unwrap();

        assert_eq!(
            reply.reply(),
            Ok(ReplyAction::Replied {
                caller_object: object(100),
                target_object: object(200),
                caller_thread: thread(1),
                caller_cpu: cpu(0),
                can_grant: true,
            })
        );
        assert_eq!(reply.state(), ReplyState::Empty);
    }

    #[test]
    fn reply_requires_pending_caller() {
        let mut reply = Reply::new();

        assert_eq!(reply.reply(), Err(ReplyError::NoPendingCaller));
    }
}
