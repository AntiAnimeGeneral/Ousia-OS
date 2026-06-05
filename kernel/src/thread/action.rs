use alloc::{boxed::Box, vec, vec::Vec};

use crate::{
    cap::ObjectId,
    ipc::{
        Endpoint, IpcMessage, IpcPayload, IpcReceiveOptions, IpcSendOptions, ReplyRequest,
        ReplySetup,
    },
    notification::{BoundTcbSignal, Notification, NotificationAction, NotificationState},
    reply::{Reply, ReplyAction, ReplyCaller, ReplyCallerParams, ReplyError, ReplyState},
    scheduler::{Scheduler, SchedulerAction, SchedulerError},
    thread::tcb::{CpuId, Tcb, TcbWaitQueueLink, ThreadId, ThreadState},
};

#[cfg(test)]
use crate::ipc::IpcAction;

fn reply_setup_for(request: ReplyRequest, receiver_can_grant: bool) -> ReplySetup {
    ReplySetup {
        caller: request.caller,
        caller_cpu: request.caller_cpu,
        reply_can_grant: receiver_can_grant,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThreadAction {
    NoThread,
    Blocked {
        thread: ThreadId,
        cpu: CpuId,
    },
    Woken {
        thread: ThreadId,
        cpu: CpuId,
        scheduler: SchedulerAction,
    },
    KeptRunning {
        thread: ThreadId,
        cpu: CpuId,
    },
    Stopped {
        thread: ThreadId,
        cpu: CpuId,
    },
    Ignored {
        thread: ThreadId,
        cpu: CpuId,
    },
    ReplyRecorded {
        setup: ReplySetup,
    },
    Resumed {
        thread: ThreadId,
        cpu: CpuId,
        scheduler: SchedulerAction,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThreadActionError {
    UnknownThread {
        thread: ThreadId,
    },
    WrongCpu {
        thread: ThreadId,
        expected_cpu: CpuId,
        actual_cpu: CpuId,
    },
    ThreadNotCurrent {
        thread: ThreadId,
        cpu: CpuId,
    },
    UnexpectedThreadState {
        thread: ThreadId,
        expected: ThreadState,
        actual: ThreadState,
    },
    NotWaitingOnBoundNotification {
        thread: ThreadId,
        notification: ObjectId,
        actual: ThreadState,
    },
    MissingReplyObject {
        setup: ReplySetup,
    },
    MissingCallerObject {
        setup: ReplySetup,
    },
    ReceiveCallTransactionUnsupported {
        setup: ReplySetup,
    },
    ReplyAlreadyPending,
    ThreadNotResumable {
        thread: ThreadId,
        state: ThreadState,
    },
    ThreadTableFull {
        capacity: usize,
    },
    Reply(ReplyError),
    Scheduler(SchedulerError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WakeExpectation {
    State(ThreadState),
    Receive { endpoint: ObjectId, can_grant: bool },
    BoundNotificationReceive { notification: ObjectId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockedSenderMessage {
    message: IpcMessage,
    reply_request: Option<ReplyRequest>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockedReceiverContext {
    thread: ThreadId,
    cpu: CpuId,
    can_grant: bool,
    reply: Option<ObjectId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockedNotificationContext {
    thread: ThreadId,
    cpu: CpuId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendIpcRequest {
    endpoint: ObjectId,
    caller: Option<ObjectId>,
    sender: ThreadId,
    sender_cpu: CpuId,
    badge: u64,
    options: IpcSendOptions,
    payload: IpcPayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveIpcRequest {
    endpoint: ObjectId,
    caller: Option<ObjectId>,
    receiver_reply: Option<ObjectId>,
    receiver: ThreadId,
    receiver_cpu: CpuId,
    options: IpcReceiveOptions,
}

#[derive(Debug, Default)]
pub struct ThreadTable {
    tcbs: Box<[Option<Tcb>]>,
}

impl ThreadTable {
    pub const DEFAULT_CAPACITY: usize = 256;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tcbs: vec![None; capacity].into_boxed_slice(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.tcbs.len()
    }

    pub fn validate_insert_capacity(&self, thread: ThreadId) -> Result<(), ThreadActionError> {
        if self.get(thread).is_some() || self.tcbs.iter().any(Option::is_none) {
            return Ok(());
        }

        Err(ThreadActionError::ThreadTableFull {
            capacity: self.capacity(),
        })
    }

    pub fn insert(&mut self, tcb: Tcb) -> Result<Option<Tcb>, ThreadActionError> {
        if let Some(existing) = self
            .tcbs
            .iter_mut()
            .filter_map(Option::as_mut)
            .find(|existing| existing.id() == tcb.id())
        {
            return Ok(Some(core::mem::replace(existing, tcb)));
        }

        let capacity = self.capacity();
        let slot = self
            .tcbs
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(ThreadActionError::ThreadTableFull { capacity })?;
        *slot = Some(tcb);
        Ok(None)
    }

    pub fn get(&self, thread: ThreadId) -> Option<&Tcb> {
        self.tcbs
            .iter()
            .filter_map(Option::as_ref)
            .find(|tcb| tcb.id() == thread)
    }

    pub fn get_mut(&mut self, thread: ThreadId) -> Option<&mut Tcb> {
        self.tcbs
            .iter_mut()
            .filter_map(Option::as_mut)
            .find(|tcb| tcb.id() == thread)
    }

    pub fn state(&self, thread: ThreadId) -> Option<ThreadState> {
        self.get(thread).map(Tcb::state)
    }

    pub fn affinity(&self, thread: ThreadId) -> Option<CpuId> {
        self.get(thread).map(Tcb::affinity)
    }

    pub fn restart(&mut self, thread: ThreadId) -> Option<CpuId> {
        let tcb = self.get_mut(thread)?;
        tcb.set_state(ThreadState::Restart);
        Some(tcb.affinity())
    }

    pub fn remove(&mut self, thread: ThreadId) -> Option<Tcb> {
        let slot = self
            .tcbs
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|tcb| tcb.id() == thread))?;
        slot.take()
    }

    pub fn unbind_notification(&mut self, thread: ThreadId) -> Option<ObjectId> {
        self.get_mut(thread)?.unbind_notification()
    }

    fn append_endpoint_sender(&mut self, endpoint: &mut Endpoint, thread: ThreadId) {
        let prev = endpoint.enqueue_sender(thread);
        if let Some(prev) = prev {
            let prev_link = self
                .get(prev)
                .expect("endpoint sender tail must reference an existing TCB")
                .wait_queue_link();
            self.get_mut(prev)
                .expect("endpoint sender tail must reference an existing TCB")
                .set_wait_queue_link(TcbWaitQueueLink::new(prev_link.prev(), Some(thread)));
        }
        self.get_mut(thread)
            .expect("blocked sender must exist before endpoint enqueue")
            .set_wait_queue_link(TcbWaitQueueLink::new(prev, None));
    }

    fn append_endpoint_receiver(&mut self, endpoint: &mut Endpoint, thread: ThreadId) {
        let prev = endpoint.enqueue_receiver(thread);
        if let Some(prev) = prev {
            let prev_link = self
                .get(prev)
                .expect("endpoint receiver tail must reference an existing TCB")
                .wait_queue_link();
            self.get_mut(prev)
                .expect("endpoint receiver tail must reference an existing TCB")
                .set_wait_queue_link(TcbWaitQueueLink::new(prev_link.prev(), Some(thread)));
        }
        self.get_mut(thread)
            .expect("blocked receiver must exist before endpoint enqueue")
            .set_wait_queue_link(TcbWaitQueueLink::new(prev, None));
    }

    pub fn pop_endpoint_sender(
        &mut self,
        endpoint: &mut Endpoint,
    ) -> Result<Option<ThreadId>, ThreadActionError> {
        let Some(sender) = endpoint.sender_head() else {
            return Ok(None);
        };
        let link = self
            .get(sender)
            .ok_or(ThreadActionError::UnknownThread { thread: sender })?
            .wait_queue_link();
        endpoint.dequeue_sender_head(link.next());
        if let Some(next) = link.next() {
            let next_link = self
                .get(next)
                .ok_or(ThreadActionError::UnknownThread { thread: next })?
                .wait_queue_link();
            self.get_mut(next)
                .expect("validated sender queue successor must exist")
                .set_wait_queue_link(TcbWaitQueueLink::new(None, next_link.next()));
        }
        self.get_mut(sender)
            .expect("validated sender queue head must exist")
            .clear_wait_queue_link();
        Ok(Some(sender))
    }

    pub fn pop_endpoint_receiver(
        &mut self,
        endpoint: &mut Endpoint,
    ) -> Result<Option<ThreadId>, ThreadActionError> {
        let Some(receiver) = endpoint.receiver_head() else {
            return Ok(None);
        };
        let link = self
            .get(receiver)
            .ok_or(ThreadActionError::UnknownThread { thread: receiver })?
            .wait_queue_link();
        endpoint.dequeue_receiver_head(link.next());
        if let Some(next) = link.next() {
            let next_link = self
                .get(next)
                .ok_or(ThreadActionError::UnknownThread { thread: next })?
                .wait_queue_link();
            self.get_mut(next)
                .expect("validated receiver queue successor must exist")
                .set_wait_queue_link(TcbWaitQueueLink::new(None, next_link.next()));
        }
        self.get_mut(receiver)
            .expect("validated receiver queue head must exist")
            .clear_wait_queue_link();
        Ok(Some(receiver))
    }

    pub fn unlink_endpoint_waiter(&mut self, endpoint: &mut Endpoint, thread: ThreadId) -> bool {
        let Some(tcb) = self.get(thread) else {
            return false;
        };
        let link = tcb.wait_queue_link();
        let removed = match tcb.state() {
            ThreadState::BlockedOnSend { .. } => {
                endpoint.unlink_sender(thread, link.prev(), link.next())
            }
            ThreadState::BlockedOnReceive { .. } => {
                endpoint.unlink_receiver(thread, link.prev(), link.next())
            }
            ThreadState::Inactive
            | ThreadState::Running
            | ThreadState::Restart
            | ThreadState::BlockedOnReply
            | ThreadState::BlockedOnNotification { .. }
            | ThreadState::IdleThreadState => false,
        };
        if !removed {
            return false;
        }
        if let Some(prev) = link.prev() {
            let prev_link = self
                .get(prev)
                .expect("endpoint queue predecessor must reference an existing TCB")
                .wait_queue_link();
            self.get_mut(prev)
                .expect("endpoint queue predecessor must reference an existing TCB")
                .set_wait_queue_link(TcbWaitQueueLink::new(prev_link.prev(), link.next()));
        }
        if let Some(next) = link.next() {
            let next_link = self
                .get(next)
                .expect("endpoint queue successor must reference an existing TCB")
                .wait_queue_link();
            self.get_mut(next)
                .expect("endpoint queue successor must reference an existing TCB")
                .set_wait_queue_link(TcbWaitQueueLink::new(link.prev(), next_link.next()));
        }
        self.get_mut(thread)
            .expect("unlinked endpoint waiter must exist")
            .clear_wait_queue_link();
        true
    }

    pub fn drain_endpoint_waiters(&mut self, endpoint: &mut Endpoint) -> Vec<(ThreadId, CpuId)> {
        let mut waiters = Vec::new();
        while let Some(sender) = endpoint.sender_head() {
            let cpu = match self.state(sender) {
                Some(ThreadState::BlockedOnSend { sender_cpu, .. }) => sender_cpu,
                _ => self.affinity(sender).unwrap_or(CpuId::new(0)),
            };
            self.pop_endpoint_sender(endpoint)
                .expect("endpoint sender drain must consume existing queue head");
            waiters.push((sender, cpu));
        }
        while let Some(receiver) = endpoint.receiver_head() {
            let cpu = match self.state(receiver) {
                Some(ThreadState::BlockedOnReceive { receiver_cpu, .. }) => receiver_cpu,
                _ => self.affinity(receiver).unwrap_or(CpuId::new(0)),
            };
            self.pop_endpoint_receiver(endpoint)
                .expect("endpoint receiver drain must consume existing queue head");
            waiters.push((receiver, cpu));
        }
        waiters
    }

    fn append_notification_waiter(&mut self, notification: &mut Notification, thread: ThreadId) {
        let prev = notification.enqueue_waiter(thread);
        if let Some(prev) = prev {
            let prev_link = self
                .get(prev)
                .expect("notification waiter tail must reference an existing TCB")
                .wait_queue_link();
            self.get_mut(prev)
                .expect("notification waiter tail must reference an existing TCB")
                .set_wait_queue_link(TcbWaitQueueLink::new(prev_link.prev(), Some(thread)));
        }
        self.get_mut(thread)
            .expect("blocked notification waiter must exist before notification enqueue")
            .set_wait_queue_link(TcbWaitQueueLink::new(prev, None));
    }

    pub fn pop_notification_waiter(
        &mut self,
        notification: &mut Notification,
    ) -> Result<Option<ThreadId>, ThreadActionError> {
        let Some(waiter) = notification.next_waiter().map(|waiter| waiter.thread()) else {
            return Ok(None);
        };
        let link = self
            .get(waiter)
            .ok_or(ThreadActionError::UnknownThread { thread: waiter })?
            .wait_queue_link();
        let next_link = if let Some(next) = link.next() {
            Some(
                self.get(next)
                    .ok_or(ThreadActionError::UnknownThread { thread: next })?
                    .wait_queue_link(),
            )
        } else {
            None
        };
        notification.dequeue_waiter_head(link.next());
        if let Some(next) = link.next() {
            self.get_mut(next)
                .expect("validated notification waiter successor must exist")
                .set_wait_queue_link(TcbWaitQueueLink::new(
                    None,
                    next_link
                        .expect("validated notification successor link must exist")
                        .next(),
                ));
        }
        self.get_mut(waiter)
            .expect("validated notification waiter head must exist")
            .clear_wait_queue_link();
        Ok(Some(waiter))
    }

    pub fn unlink_notification_waiter(
        &mut self,
        notification: &mut Notification,
        notification_object: ObjectId,
        thread: ThreadId,
    ) -> bool {
        let Some(tcb) = self.get(thread) else {
            return false;
        };
        let ThreadState::BlockedOnNotification {
            notification: blocked_notification,
            ..
        } = tcb.state()
        else {
            return false;
        };
        if blocked_notification != notification_object {
            return false;
        }
        let link = tcb.wait_queue_link();
        if !notification.unlink_waiter(thread, link.prev(), link.next()) {
            return false;
        }
        if let Some(prev) = link.prev() {
            let prev_link = self
                .get(prev)
                .expect("notification queue predecessor must reference an existing TCB")
                .wait_queue_link();
            self.get_mut(prev)
                .expect("notification queue predecessor must reference an existing TCB")
                .set_wait_queue_link(TcbWaitQueueLink::new(prev_link.prev(), link.next()));
        }
        if let Some(next) = link.next() {
            let next_link = self
                .get(next)
                .expect("notification queue successor must reference an existing TCB")
                .wait_queue_link();
            self.get_mut(next)
                .expect("notification queue successor must reference an existing TCB")
                .set_wait_queue_link(TcbWaitQueueLink::new(link.prev(), next_link.next()));
        }
        self.get_mut(thread)
            .expect("unlinked notification waiter must exist")
            .clear_wait_queue_link();
        true
    }

    pub fn drain_notification_waiters(
        &mut self,
        notification: &mut Notification,
    ) -> Vec<(ThreadId, CpuId)> {
        let mut waiters = Vec::new();
        while let Some(waiter) = notification.next_waiter().map(|waiter| waiter.thread()) {
            let cpu = match self.state(waiter) {
                Some(ThreadState::BlockedOnNotification { receiver_cpu, .. }) => receiver_cpu,
                _ => self.affinity(waiter).unwrap_or(CpuId::new(0)),
            };
            self.pop_notification_waiter(notification)
                .expect("notification waiter drain must consume existing queue head");
            waiters.push((waiter, cpu));
        }
        waiters
    }
}

impl SendIpcRequest {
    pub const fn new(
        endpoint: ObjectId,
        sender: ThreadId,
        sender_cpu: CpuId,
        badge: u64,
        options: IpcSendOptions,
        payload: IpcPayload,
    ) -> Self {
        Self {
            endpoint,
            caller: None,
            sender,
            sender_cpu,
            badge,
            options,
            payload,
        }
    }

    pub const fn with_caller(mut self, caller: ObjectId) -> Self {
        self.caller = Some(caller);
        self
    }
}

impl ReceiveIpcRequest {
    pub const fn new(
        endpoint: ObjectId,
        receiver: ThreadId,
        receiver_cpu: CpuId,
        options: IpcReceiveOptions,
    ) -> Self {
        Self {
            endpoint,
            caller: None,
            receiver_reply: None,
            receiver,
            receiver_cpu,
            options,
        }
    }

    pub const fn with_caller(mut self, caller: ObjectId) -> Self {
        self.caller = Some(caller);
        self
    }

    pub const fn with_receiver_reply(mut self, reply: ObjectId) -> Self {
        self.receiver_reply = Some(reply);
        self
    }
}

fn blocked_sender_message(
    threads: &ThreadTable,
    endpoint: ObjectId,
    sender: ThreadId,
) -> Result<BlockedSenderMessage, ThreadActionError> {
    let Some(state) = threads.state(sender) else {
        return Err(ThreadActionError::UnknownThread { thread: sender });
    };
    let ThreadState::BlockedOnSend {
        endpoint: blocked_endpoint,
        sender_cpu,
        badge,
        can_grant,
        can_grant_reply,
        is_call,
        payload,
    } = state
    else {
        return Err(ThreadActionError::UnexpectedThreadState {
            thread: sender,
            expected: ThreadState::BlockedOnSend {
                endpoint,
                sender_cpu: CpuId::new(0),
                badge: 0,
                can_grant: false,
                can_grant_reply: false,
                is_call: false,
                payload: IpcPayload::empty(),
            },
            actual: state,
        });
    };
    if blocked_endpoint != endpoint {
        return Err(ThreadActionError::UnexpectedThreadState {
            thread: sender,
            expected: ThreadState::BlockedOnSend {
                endpoint,
                sender_cpu,
                badge,
                can_grant,
                can_grant_reply,
                is_call,
                payload,
            },
            actual: state,
        });
    }

    let mode = if is_call {
        crate::ipc::IpcSendMode::Call
    } else {
        crate::ipc::IpcSendMode::Send
    };
    let message = IpcMessage::new_for_blocked_sender(
        sender,
        sender_cpu,
        badge,
        can_grant,
        can_grant_reply,
        mode,
        payload,
    );
    let reply_request = is_call.then_some(ReplyRequest {
        caller: sender,
        caller_cpu: sender_cpu,
        sender_can_reply: can_grant || can_grant_reply,
    });

    Ok(BlockedSenderMessage {
        message,
        reply_request,
    })
}

fn blocked_receiver_context(
    threads: &ThreadTable,
    endpoint: ObjectId,
    receiver: ThreadId,
) -> Result<BlockedReceiverContext, ThreadActionError> {
    let Some(state) = threads.state(receiver) else {
        return Err(ThreadActionError::UnknownThread { thread: receiver });
    };
    let ThreadState::BlockedOnReceive {
        endpoint: blocked_endpoint,
        receiver_cpu,
        can_grant,
        reply,
    } = state
    else {
        return Err(ThreadActionError::UnexpectedThreadState {
            thread: receiver,
            expected: ThreadState::BlockedOnReceive {
                endpoint,
                receiver_cpu: threads.affinity(receiver).unwrap_or(CpuId::new(0)),
                can_grant: false,
                reply: None,
            },
            actual: state,
        });
    };
    if blocked_endpoint != endpoint {
        return Err(ThreadActionError::UnexpectedThreadState {
            thread: receiver,
            expected: ThreadState::BlockedOnReceive {
                endpoint,
                receiver_cpu,
                can_grant,
                reply,
            },
            actual: state,
        });
    }

    Ok(BlockedReceiverContext {
        thread: receiver,
        cpu: receiver_cpu,
        can_grant,
        reply,
    })
}

fn blocked_notification_context(
    threads: &ThreadTable,
    notification: ObjectId,
    receiver: ThreadId,
) -> Result<BlockedNotificationContext, ThreadActionError> {
    let Some(tcb) = threads.get(receiver) else {
        return Err(ThreadActionError::UnknownThread { thread: receiver });
    };

    match tcb.state() {
        ThreadState::BlockedOnNotification {
            notification: blocked_notification,
            receiver_cpu,
        } if blocked_notification == notification => Ok(BlockedNotificationContext {
            thread: receiver,
            cpu: receiver_cpu,
        }),
        actual => Err(ThreadActionError::UnexpectedThreadState {
            thread: receiver,
            expected: ThreadState::BlockedOnNotification {
                notification,
                receiver_cpu: tcb.affinity(),
            },
            actual,
        }),
    }
}

#[cfg(test)]
fn apply_ipc_action(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    endpoint: ObjectId,
    receiver_reply: Option<ObjectId>,
    action: IpcAction,
) -> Result<ThreadAction, ThreadActionError> {
    match action {
        IpcAction::SenderBlocked {
            thread,
            cpu,
            badge,
            can_grant,
            can_grant_reply,
            is_call,
            payload,
        } => block_current(
            threads,
            scheduler,
            thread,
            cpu,
            ThreadState::BlockedOnSend {
                endpoint,
                sender_cpu: cpu,
                badge,
                can_grant,
                can_grant_reply,
                is_call,
                payload,
            },
        ),
        IpcAction::ReceiverBlocked {
            thread,
            cpu,
            can_grant,
        } => block_current(
            threads,
            scheduler,
            thread,
            cpu,
            ThreadState::BlockedOnReceive {
                endpoint,
                receiver_cpu: cpu,
                can_grant,
                reply: receiver_reply,
            },
        ),
        IpcAction::DeliveredToReceiver { receiver, .. } => {
            let receiver_context = blocked_receiver_context(threads, endpoint, receiver)?;
            wake_thread(
                threads,
                scheduler,
                receiver_context.thread,
                receiver_context.cpu,
                WakeExpectation::Receive {
                    endpoint,
                    can_grant: receiver_context.can_grant,
                },
            )
        }
        IpcAction::SenderReleased { sender, .. } => {
            let blocked = blocked_sender_message(threads, endpoint, sender.thread())?;
            let message = blocked.message;
            let reply_request = blocked.reply_request;
            if let Some(request) = reply_request {
                return Err(ThreadActionError::ReceiveCallTransactionUnsupported {
                    setup: reply_setup_for(request, false),
                });
            }

            wake_thread(
                threads,
                scheduler,
                message.sender(),
                message.sender_cpu(),
                WakeExpectation::State(ThreadState::BlockedOnSend {
                    endpoint,
                    sender_cpu: message.sender_cpu(),
                    badge: message.badge(),
                    can_grant: message.can_grant(),
                    can_grant_reply: message.can_grant_reply(),
                    is_call: false,
                    payload: message.payload(),
                }),
            )
        }
        IpcAction::SendIgnored { thread, cpu }
        | IpcAction::NonblockingReceiveFailed { thread, cpu } => {
            Ok(ThreadAction::Ignored { thread, cpu })
        }
    }
}

pub fn send_ipc(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    endpoint: &mut Endpoint,
    reply: Option<&mut Reply>,
    request: SendIpcRequest,
) -> Result<ThreadAction, ThreadActionError> {
    validate_block_current(threads, scheduler, request.sender, request.sender_cpu)?;

    let mut reply = reply;

    let receiver_context = if let Some(receiver) = endpoint.receiver_head() {
        let receiver = blocked_receiver_context(threads, request.endpoint, receiver)?;
        validate_wake(
            threads,
            scheduler,
            receiver.thread,
            receiver.cpu,
            WakeExpectation::Receive {
                endpoint: request.endpoint,
                can_grant: receiver.can_grant,
            },
        )?;

        if request.options.is_call() {
            let caller_can_reply = request.options.can_grant || request.options.can_grant_reply;
            let reply_request = ReplyRequest {
                caller: request.sender,
                caller_cpu: request.sender_cpu,
                sender_can_reply: caller_can_reply,
            };
            let setup = reply_setup_for(reply_request, receiver.can_grant);
            if caller_can_reply {
                let reply = reply
                    .as_deref()
                    .ok_or(ThreadActionError::MissingReplyObject { setup })?;
                if request.caller.is_none() {
                    return Err(ThreadActionError::MissingCallerObject { setup });
                }
                if reply.is_pending() {
                    return Err(ThreadActionError::ReplyAlreadyPending);
                }
            }
        }
        Some(receiver)
    } else {
        None
    };

    if let Some(receiver_context) = receiver_context {
        let receiver = threads
            .pop_endpoint_receiver(endpoint)?
            .expect("prechecked receiver queue must have a head");
        assert_eq!(receiver, receiver_context.thread);
        let message = IpcMessage::new_for_blocked_sender(
            request.sender,
            request.sender_cpu,
            request.badge,
            request.options.can_grant,
            request.options.can_grant_reply,
            request.options.mode(),
            request.payload,
        );
        let reply_request = request.options.is_call().then_some(ReplyRequest {
            caller: request.sender,
            caller_cpu: request.sender_cpu,
            sender_can_reply: request.options.can_grant || request.options.can_grant_reply,
        });
        if let Some(reply_request) = reply_request {
            let caller_can_reply = reply_request.sender_can_reply;
            let setup = reply_setup_for(reply_request, receiver_context.can_grant);
            let sender_action = if caller_can_reply {
                let caller_object = request
                    .caller
                    .expect("prechecked immediate call delivery must provide caller TCB object");
                let reply = reply
                    .as_deref_mut()
                    .expect("prechecked immediate call delivery must provide reply object");
                let _ = reply
                    .record_caller(ReplyCaller::new(ReplyCallerParams {
                        caller: caller_object,
                        target: request.endpoint,
                        thread: setup.caller,
                        cpu: setup.caller_cpu,
                        can_grant: setup.reply_can_grant,
                    }))
                    .expect("prechecked immediate call reply object must be empty");
                block_current_validated(
                    threads,
                    scheduler,
                    message.sender(),
                    message.sender_cpu(),
                    ThreadState::BlockedOnReply,
                )
            } else {
                stop_current_validated(threads, scheduler, message.sender(), message.sender_cpu())
            };
            let wake = wake_thread_validated(
                threads,
                scheduler,
                receiver_context.thread,
                receiver_context.cpu,
            );
            let _ = sender_action;
            Ok(wake)
        } else {
            let sender_action =
                stop_current_validated(threads, scheduler, request.sender, request.sender_cpu);
            let wake = wake_thread_validated(
                threads,
                scheduler,
                receiver_context.thread,
                receiver_context.cpu,
            );
            let _ = sender_action;
            Ok(wake)
        }
    } else if request.options.is_blocking() {
        block_current_validated(
            threads,
            scheduler,
            request.sender,
            request.sender_cpu,
            ThreadState::BlockedOnSend {
                endpoint: request.endpoint,
                sender_cpu: request.sender_cpu,
                badge: request.badge,
                can_grant: request.options.can_grant,
                can_grant_reply: request.options.can_grant_reply,
                is_call: request.options.is_call(),
                payload: request.payload,
            },
        );
        threads.append_endpoint_sender(endpoint, request.sender);
        Ok(ThreadAction::Blocked {
            thread: request.sender,
            cpu: request.sender_cpu,
        })
    } else {
        Ok(ThreadAction::Ignored {
            thread: request.sender,
            cpu: request.sender_cpu,
        })
    }
}

pub fn recv_ipc(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    endpoint: &mut Endpoint,
    reply: Option<&mut Reply>,
    request: ReceiveIpcRequest,
) -> Result<ThreadAction, ThreadActionError> {
    validate_block_current(threads, scheduler, request.receiver, request.receiver_cpu)?;

    let mut reply = reply;

    if let Some(sender) = endpoint.sender_head() {
        let blocked = blocked_sender_message(threads, request.endpoint, sender)?;
        let message = blocked.message;
        let expected = ThreadState::BlockedOnSend {
            endpoint: request.endpoint,
            sender_cpu: message.sender_cpu(),
            badge: message.badge(),
            can_grant: message.can_grant(),
            can_grant_reply: message.can_grant_reply(),
            is_call: message.is_call(),
            payload: message.payload(),
        };

        if message.is_call() {
            validate_blocked_sender_reply_transition(
                threads,
                scheduler,
                message.sender(),
                message.sender_cpu(),
                expected,
            )?;
            let caller_can_reply = message.can_grant() || message.can_grant_reply();
            let reply_request = ReplyRequest {
                caller: message.sender(),
                caller_cpu: message.sender_cpu(),
                sender_can_reply: caller_can_reply,
            };
            let setup = reply_setup_for(reply_request, request.options.can_grant);
            if caller_can_reply {
                let reply = reply
                    .as_deref()
                    .ok_or(ThreadActionError::MissingReplyObject { setup })?;
                if request.caller.is_none() {
                    return Err(ThreadActionError::MissingCallerObject { setup });
                }
                if reply.is_pending() {
                    return Err(ThreadActionError::ReplyAlreadyPending);
                }
            }
        } else {
            validate_wake(
                threads,
                scheduler,
                message.sender(),
                message.sender_cpu(),
                WakeExpectation::State(expected),
            )?;
        }
    }

    if endpoint.sender_head().is_some() {
        let sender = threads
            .pop_endpoint_sender(endpoint)?
            .expect("prechecked sender queue must have a head");
        let blocked = blocked_sender_message(threads, request.endpoint, sender)?;
        let message = blocked.message;
        if let Some(reply_request) = blocked.reply_request {
            let caller_can_reply = reply_request.sender_can_reply;
            let setup = reply_setup_for(reply_request, request.options.can_grant);
            if caller_can_reply {
                let caller_object = request
                    .caller
                    .expect("prechecked receive-side call must provide caller TCB object");
                let reply = reply
                    .as_deref_mut()
                    .expect("prechecked receive-side call must provide reply object");
                let _ = reply
                    .record_caller(ReplyCaller::new(ReplyCallerParams {
                        caller: caller_object,
                        target: request.endpoint,
                        thread: setup.caller,
                        cpu: setup.caller_cpu,
                        can_grant: setup.reply_can_grant,
                    }))
                    .expect("prechecked receive-side call reply object must be empty");
                threads
                    .get_mut(message.sender())
                    .expect("prechecked receive-side call sender must exist")
                    .set_state(ThreadState::BlockedOnReply);
                Ok(ThreadAction::ReplyRecorded { setup })
            } else {
                Ok(stop_thread_validated(
                    threads,
                    message.sender(),
                    message.sender_cpu(),
                ))
            }
        } else {
            Ok(wake_thread_validated(
                threads,
                scheduler,
                message.sender(),
                message.sender_cpu(),
            ))
        }
    } else if request.options.blocking {
        block_current_validated(
            threads,
            scheduler,
            request.receiver,
            request.receiver_cpu,
            ThreadState::BlockedOnReceive {
                endpoint: request.endpoint,
                receiver_cpu: request.receiver_cpu,
                can_grant: request.options.can_grant,
                reply: request.receiver_reply,
            },
        );
        threads.append_endpoint_receiver(endpoint, request.receiver);
        Ok(ThreadAction::Blocked {
            thread: request.receiver,
            cpu: request.receiver_cpu,
        })
    } else {
        Ok(ThreadAction::Ignored {
            thread: request.receiver,
            cpu: request.receiver_cpu,
        })
    }
}

fn apply_notification_action(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    notification: ObjectId,
    action: NotificationAction,
) -> Result<ThreadAction, ThreadActionError> {
    match action {
        NotificationAction::ReceiverBlocked { thread, cpu } => block_current(
            threads,
            scheduler,
            thread,
            cpu,
            ThreadState::BlockedOnNotification {
                notification,
                receiver_cpu: cpu,
            },
        ),
        NotificationAction::Delivered { receiver, .. } => {
            let context = blocked_notification_context(threads, notification, receiver)?;
            wake_thread(
                threads,
                scheduler,
                context.thread,
                context.cpu,
                WakeExpectation::State(ThreadState::BlockedOnNotification {
                    notification,
                    receiver_cpu: context.cpu,
                }),
            )
        }
        NotificationAction::BoundReceiveCompleted {
            receiver,
            receiver_cpu,
            ..
        } => wake_thread(
            threads,
            scheduler,
            receiver,
            receiver_cpu,
            WakeExpectation::BoundNotificationReceive { notification },
        ),
        NotificationAction::BadgeConsumed { thread, cpu, .. } => {
            Ok(ThreadAction::KeptRunning { thread, cpu })
        }
        NotificationAction::BecameActive { .. } => Ok(ThreadAction::NoThread),
        NotificationAction::PollFailed { thread, cpu } => Ok(ThreadAction::Ignored { thread, cpu }),
    }
}

pub fn wait_notification(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    notification: &mut Notification,
    notification_object: ObjectId,
    receiver: ThreadId,
    receiver_cpu: CpuId,
) -> Result<ThreadAction, ThreadActionError> {
    validate_block_current(threads, scheduler, receiver, receiver_cpu)?;

    match notification.state() {
        NotificationState::Active => {
            let action = notification.wait(receiver, receiver_cpu);
            apply_notification_action(threads, scheduler, notification_object, action)
        }
        NotificationState::Idle | NotificationState::Waiting => {
            block_current(
                threads,
                scheduler,
                receiver,
                receiver_cpu,
                ThreadState::BlockedOnNotification {
                    notification: notification_object,
                    receiver_cpu,
                },
            )?;
            threads.append_notification_waiter(notification, receiver);
            Ok(ThreadAction::Blocked {
                thread: receiver,
                cpu: receiver_cpu,
            })
        }
    }
}

pub fn poll_notification(
    threads: &ThreadTable,
    scheduler: &Scheduler,
    notification: &mut Notification,
    notification_object: ObjectId,
    receiver: ThreadId,
    receiver_cpu: CpuId,
) -> Result<ThreadAction, ThreadActionError> {
    validate_block_current(threads, scheduler, receiver, receiver_cpu)?;

    let action = notification.poll(receiver, receiver_cpu);
    let _ = notification_object;
    apply_notification_poll_action(action)
}

fn apply_notification_poll_action(
    action: NotificationAction,
) -> Result<ThreadAction, ThreadActionError> {
    match action {
        NotificationAction::BadgeConsumed { thread, cpu, .. } => {
            Ok(ThreadAction::KeptRunning { thread, cpu })
        }
        NotificationAction::PollFailed { thread, cpu } => Ok(ThreadAction::Ignored { thread, cpu }),
        NotificationAction::ReceiverBlocked { .. }
        | NotificationAction::Delivered { .. }
        | NotificationAction::BoundReceiveCompleted { .. }
        | NotificationAction::BecameActive { .. } => {
            unreachable!("notification poll must not produce blocking, delivery, or active actions")
        }
    }
}

pub fn signal_notification(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    notification: &mut Notification,
    notification_object: ObjectId,
    badge: u64,
    bound_tcb: BoundTcbSignal,
) -> Result<ThreadAction, ThreadActionError> {
    if let Some(waiter) = notification.next_waiter() {
        let context = blocked_notification_context(threads, notification_object, waiter.thread())?;
        validate_wake(
            threads,
            scheduler,
            context.thread,
            context.cpu,
            WakeExpectation::State(ThreadState::BlockedOnNotification {
                notification: notification_object,
                receiver_cpu: context.cpu,
            }),
        )?;
        let delivered = threads
            .pop_notification_waiter(notification)?
            .expect("prechecked notification delivery must consume waiter head");
        assert_eq!(delivered, context.thread);
        return Ok(wake_thread_validated(
            threads,
            scheduler,
            context.thread,
            context.cpu,
        ));
    }

    if notification.state() == NotificationState::Idle && bound_tcb.is_ready() {
        if let Some(bound) = notification.bound_tcb() {
            validate_wake(
                threads,
                scheduler,
                bound.thread(),
                bound.cpu(),
                WakeExpectation::BoundNotificationReceive {
                    notification: notification_object,
                },
            )?;
        }
    }

    let action = notification.signal(badge, bound_tcb);
    apply_notification_action(threads, scheduler, notification_object, action)
}

fn apply_reply_action(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    action: ReplyAction,
) -> Result<ThreadAction, ThreadActionError> {
    match action {
        ReplyAction::CallerRecorded {
            caller_thread,
            caller_cpu,
            ..
        } => block_current(
            threads,
            scheduler,
            caller_thread,
            caller_cpu,
            ThreadState::BlockedOnReply,
        ),
        ReplyAction::Replied {
            caller_thread,
            caller_cpu,
            ..
        } => wake_thread(
            threads,
            scheduler,
            caller_thread,
            caller_cpu,
            WakeExpectation::State(ThreadState::BlockedOnReply),
        ),
    }
}

pub fn record_reply_caller(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    reply: &mut Reply,
    caller: ReplyCaller,
) -> Result<ThreadAction, ThreadActionError> {
    validate_block_current(threads, scheduler, caller.thread(), caller.cpu())?;

    let action = reply
        .record_caller(caller)
        .map_err(ThreadActionError::Reply)?;
    apply_reply_action(threads, scheduler, action)
}

pub fn reply_to_caller(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    reply: &mut Reply,
) -> Result<ThreadAction, ThreadActionError> {
    if let ReplyState::Pending { caller } = reply.state() {
        validate_wake(
            threads,
            scheduler,
            caller.thread(),
            caller.cpu(),
            WakeExpectation::State(ThreadState::BlockedOnReply),
        )?;
    }

    let action = reply.reply().map_err(ThreadActionError::Reply)?;
    apply_reply_action(threads, scheduler, action)
}

pub fn resume_tcb(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    thread: ThreadId,
) -> Result<ThreadAction, ThreadActionError> {
    let tcb = threads
        .get(thread)
        .ok_or(ThreadActionError::UnknownThread { thread })?;
    let cpu = tcb.affinity();
    let state = tcb.state();
    if state != ThreadState::Inactive {
        return Err(ThreadActionError::ThreadNotResumable { thread, state });
    }

    scheduler.validate_enqueue_fields(thread, cpu, ThreadState::Restart)?;

    threads
        .get_mut(thread)
        .expect("validated resumed thread must exist")
        .set_state(ThreadState::Restart);
    let scheduler_action = scheduler.enqueue_validated(thread, cpu);

    Ok(ThreadAction::Resumed {
        thread,
        cpu,
        scheduler: scheduler_action,
    })
}

fn block_current(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    thread: ThreadId,
    cpu: CpuId,
    state: ThreadState,
) -> Result<ThreadAction, ThreadActionError> {
    let current = scheduler.run_queue(cpu)?.current();
    if current != Some(thread) {
        return Err(ThreadActionError::ThreadNotCurrent { thread, cpu });
    }

    validate_thread_cpu(threads, thread, cpu)?;

    scheduler.block_current(cpu)?;
    threads
        .get_mut(thread)
        .expect("validated blocked thread must exist")
        .set_state(state);

    Ok(ThreadAction::Blocked { thread, cpu })
}

fn block_current_validated(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    thread: ThreadId,
    cpu: CpuId,
    state: ThreadState,
) -> ThreadAction {
    scheduler
        .block_current(cpu)
        .expect("validated blocked thread must target a known CPU");
    threads
        .get_mut(thread)
        .expect("validated blocked thread must exist")
        .set_state(state);

    ThreadAction::Blocked { thread, cpu }
}

fn stop_current_validated(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    thread: ThreadId,
    cpu: CpuId,
) -> ThreadAction {
    scheduler
        .block_current(cpu)
        .expect("validated stopped current thread must target a known CPU");
    stop_thread_validated(threads, thread, cpu)
}

fn stop_thread_validated(threads: &mut ThreadTable, thread: ThreadId, cpu: CpuId) -> ThreadAction {
    threads
        .get_mut(thread)
        .expect("validated stopped thread must exist")
        .set_state(ThreadState::Inactive);

    ThreadAction::Stopped { thread, cpu }
}

fn validate_block_current(
    threads: &ThreadTable,
    scheduler: &Scheduler,
    thread: ThreadId,
    cpu: CpuId,
) -> Result<(), ThreadActionError> {
    let current = scheduler.run_queue(cpu)?.current();
    if current != Some(thread) {
        return Err(ThreadActionError::ThreadNotCurrent { thread, cpu });
    }

    validate_thread_cpu(threads, thread, cpu)
}

fn wake_thread(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    thread: ThreadId,
    cpu: CpuId,
    expectation: WakeExpectation,
) -> Result<ThreadAction, ThreadActionError> {
    validate_wake(threads, scheduler, thread, cpu, expectation)?;

    threads
        .get_mut(thread)
        .expect("validated woken thread must exist")
        .set_state(ThreadState::Running);
    let scheduler_action = scheduler.enqueue_validated(thread, cpu);

    Ok(ThreadAction::Woken {
        thread,
        cpu,
        scheduler: scheduler_action,
    })
}

fn wake_thread_validated(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    thread: ThreadId,
    cpu: CpuId,
) -> ThreadAction {
    threads
        .get_mut(thread)
        .expect("validated woken thread must exist")
        .set_state(ThreadState::Running);
    let scheduler_action = scheduler.enqueue_validated(thread, cpu);

    ThreadAction::Woken {
        thread,
        cpu,
        scheduler: scheduler_action,
    }
}

fn validate_wake(
    threads: &ThreadTable,
    scheduler: &Scheduler,
    thread: ThreadId,
    cpu: CpuId,
    expectation: WakeExpectation,
) -> Result<(), ThreadActionError> {
    validate_thread_cpu(threads, thread, cpu)?;
    validate_wake_expectation(threads, thread, expectation)?;
    scheduler.validate_enqueue_fields(thread, cpu, ThreadState::Running)?;

    Ok(())
}

fn validate_blocked_sender_reply_transition(
    threads: &ThreadTable,
    scheduler: &Scheduler,
    thread: ThreadId,
    cpu: CpuId,
    expected: ThreadState,
) -> Result<(), ThreadActionError> {
    validate_thread_cpu(threads, thread, cpu)?;
    validate_wake_expectation(threads, thread, WakeExpectation::State(expected))?;
    scheduler.run_queue(cpu)?;
    if let Some(placement) = scheduler.placement(thread) {
        return Err(ThreadActionError::Scheduler(
            SchedulerError::ThreadAlreadyScheduled { thread, placement },
        ));
    }

    Ok(())
}

fn validate_wake_expectation(
    threads: &ThreadTable,
    thread: ThreadId,
    expectation: WakeExpectation,
) -> Result<(), ThreadActionError> {
    let tcb = threads
        .get(thread)
        .ok_or(ThreadActionError::UnknownThread { thread })?;
    match expectation {
        WakeExpectation::State(expected) => {
            let actual = tcb.state();
            if actual != expected {
                return Err(ThreadActionError::UnexpectedThreadState {
                    thread,
                    expected,
                    actual,
                });
            }
        }
        WakeExpectation::Receive {
            endpoint,
            can_grant,
        } => match tcb.state() {
            ThreadState::BlockedOnReceive {
                endpoint: actual_endpoint,
                can_grant: actual_can_grant,
                ..
            } if actual_endpoint == endpoint && actual_can_grant == can_grant => {}
            actual => {
                return Err(ThreadActionError::UnexpectedThreadState {
                    thread,
                    expected: ThreadState::BlockedOnReceive {
                        endpoint,
                        receiver_cpu: tcb.affinity(),
                        can_grant,
                        reply: None,
                    },
                    actual,
                });
            }
        },
        WakeExpectation::BoundNotificationReceive { notification } => {
            if !tcb.waits_on_bound_notification_receive(notification) {
                return Err(ThreadActionError::NotWaitingOnBoundNotification {
                    thread,
                    notification,
                    actual: tcb.state(),
                });
            }
        }
    }

    Ok(())
}

fn validate_thread_cpu(
    threads: &ThreadTable,
    thread: ThreadId,
    cpu: CpuId,
) -> Result<(), ThreadActionError> {
    let tcb = threads
        .get(thread)
        .ok_or(ThreadActionError::UnknownThread { thread })?;
    let actual_cpu = tcb.affinity();
    if actual_cpu != cpu {
        return Err(ThreadActionError::WrongCpu {
            thread,
            expected_cpu: cpu,
            actual_cpu,
        });
    }

    Ok(())
}

impl From<SchedulerError> for ThreadActionError {
    fn from(error: SchedulerError) -> Self {
        Self::Scheduler(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ipc::{IpcPayload, IpcReceiveOptions, IpcSendOptions},
        notification::Notification,
        reply::{Reply, ReplyCaller},
    };

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    fn object(raw: u64) -> ObjectId {
        ObjectId::new(raw)
    }

    fn reply_slot(raw: u64) -> ObjectId {
        object(raw)
    }

    fn runnable_tcb(raw: u64, affinity: CpuId) -> Tcb {
        let mut tcb = Tcb::new(thread(raw), affinity);
        tcb.set_state(ThreadState::Running);
        tcb
    }

    fn table_with_threads(threads: &[(u64, CpuId)]) -> ThreadTable {
        let mut table = ThreadTable::new();
        for (thread, cpu) in threads {
            assert!(
                table
                    .insert(runnable_tcb(*thread, *cpu))
                    .expect("test thread table must have capacity")
                    .is_none()
            );
        }
        table
    }

    fn scheduler_with_current(cpu0_thread: Option<u64>, cpu1_thread: Option<u64>) -> Scheduler {
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        if let Some(thread) = cpu0_thread {
            scheduler.enqueue(&runnable_tcb(thread, cpu(0))).unwrap();
            scheduler.schedule_next(cpu(0)).unwrap();
        }
        if let Some(thread) = cpu1_thread {
            scheduler.enqueue(&runnable_tcb(thread, cpu(1))).unwrap();
            scheduler.schedule_next(cpu(1)).unwrap();
        }
        scheduler
    }

    fn send_request(
        endpoint: ObjectId,
        sender: ThreadId,
        sender_cpu: CpuId,
        badge: u64,
        options: crate::ipc::IpcSendOptions,
    ) -> SendIpcRequest {
        SendIpcRequest::new(
            endpoint,
            sender,
            sender_cpu,
            badge,
            options,
            IpcPayload::empty(),
        )
    }

    fn recv_request(
        endpoint: ObjectId,
        receiver: ThreadId,
        receiver_cpu: CpuId,
        options: IpcReceiveOptions,
    ) -> ReceiveIpcRequest {
        ReceiveIpcRequest::new(endpoint, receiver, receiver_cpu, options)
    }

    fn send_options(blocking: bool, can_grant: bool) -> IpcSendOptions {
        IpcSendOptions::send(blocking, can_grant, false)
    }

    fn call_options(can_grant: bool, can_grant_reply: bool) -> IpcSendOptions {
        IpcSendOptions::call(can_grant, can_grant_reply)
    }

    // ThreadAction tests cover local thread/queue state transitions after
    // capability authorization has already succeeded. Higher-level executor
    // tests cover CSpace/ObjectTable lookup; these tests keep the mutation and
    // failure-no-side-effect rules for TCB, IPC, Notification, and Reply owners.

    #[test]
    fn resume_tcb_sets_restart_and_enqueues_on_affinity_cpu() {
        // Goal: resume turns an inactive TCB into runnable scheduler work.
        // Scope: ThreadTable and Scheduler ownership after TCB resume authorization.
        // Semantics: the TCB enters Restart and is enqueued on its affinity CPU.
        let mut threads = ThreadTable::new();
        assert!(
            threads
                .insert(Tcb::new(thread(1), cpu(1)))
                .expect("test thread table must have capacity")
                .is_none()
        );
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            resume_tcb(&mut threads, &mut scheduler, thread(1)),
            Ok(ThreadAction::Resumed {
                thread: thread(1),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(1),
                },
            })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Restart));
        assert_eq!(
            scheduler.placement(thread(1)),
            Some(crate::scheduler::ThreadPlacement::Ready { cpu: cpu(1) })
        );
    }

    #[test]
    fn resume_tcb_rejects_non_inactive_without_mutation() {
        // Goal: resume rejects TCBs that are already outside the inactive state.
        // Scope: ThreadTable precheck before Scheduler enqueue.
        // Semantics: non-inactive failure leaves TCB state and scheduler placement unchanged.
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            resume_tcb(&mut threads, &mut scheduler, thread(1)),
            Err(ThreadActionError::ThreadNotResumable {
                thread: thread(1),
                state: ThreadState::Running,
            })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
        assert_eq!(scheduler.placement(thread(1)), None);
    }

    #[test]
    fn resume_tcb_unknown_cpu_fails_before_state_change() {
        // Goal: resume validates scheduler topology before committing TCB state.
        // Scope: ThreadTable and Scheduler failure ordering for TCB resume.
        // Semantics: unknown affinity CPU leaves the TCB inactive and unplaced.
        let mut threads = ThreadTable::new();
        assert!(
            threads
                .insert(Tcb::new(thread(1), cpu(9)))
                .expect("test thread table must have capacity")
                .is_none()
        );
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            resume_tcb(&mut threads, &mut scheduler, thread(1)),
            Err(ThreadActionError::Scheduler(SchedulerError::UnknownCpu {
                cpu: cpu(9),
            }))
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Inactive));
        assert_eq!(scheduler.placement(thread(1)), None);
    }

    #[test]
    fn resume_tcb_rejects_already_scheduled_thread_without_state_change() {
        // Goal: resume cannot duplicate an already scheduled thread.
        // Scope: Scheduler placement precheck during TCB resume.
        // Semantics: duplicate placement failure leaves the inactive TCB and ready queue intact.
        let mut threads = ThreadTable::new();
        let mut tcb = Tcb::new(thread(1), cpu(0));
        tcb.set_state(ThreadState::Inactive);
        assert!(
            threads
                .insert(tcb)
                .expect("test thread table must have capacity")
                .is_none()
        );
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler
            .enqueue(&runnable_tcb(1, cpu(0)))
            .expect("test setup must schedule placeholder runnable view");

        assert_eq!(
            resume_tcb(&mut threads, &mut scheduler, thread(1)),
            Err(ThreadActionError::Scheduler(
                SchedulerError::ThreadAlreadyScheduled {
                    thread: thread(1),
                    placement: crate::scheduler::ThreadPlacement::Ready { cpu: cpu(0) },
                }
            ))
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Inactive));
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 1);
    }

    #[test]
    fn thread_table_rejects_capacity_overflow_without_inserting() {
        // Goal: ThreadTable does not allocate during TCB insertion.
        // Scope: bounded ThreadTable storage used as the current TCB owner.
        // Semantics: capacity exhaustion is recoverable and existing entries remain intact.
        let mut threads = ThreadTable::with_capacity(1);
        assert!(
            threads
                .insert(Tcb::new(thread(1), cpu(0)))
                .expect("first insert fits bounded table")
                .is_none()
        );

        assert_eq!(
            threads.insert(Tcb::new(thread(2), cpu(1))),
            Err(ThreadActionError::ThreadTableFull { capacity: 1 })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Inactive));
        assert_eq!(threads.state(thread(2)), None);
    }

    #[test]
    fn ipc_sender_block_sets_tcb_state_and_removes_current() {
        // Goal: blocking send transfers the current sender from CPU ownership into Endpoint wait state.
        // Scope: send_ipc across ThreadTable, Scheduler, and Endpoint owners.
        // Semantics: sender becomes BlockedOnSend and is removed from the current run queue.
        let mut endpoint = crate::ipc::Endpoint::new();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                send_request(object(10), thread(1), cpu(0), 7, call_options(true, false),),
            ),
            Ok(ThreadAction::Blocked {
                thread: thread(1),
                cpu: cpu(0),
            })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().current(), None);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            })
        );
    }

    #[test]
    fn endpoint_sender_queue_uses_tcb_links_for_fifo_and_middle_unlink() {
        // Goal: Endpoint wait queues keep only anchors while TCB links own membership.
        // Scope: ThreadTable endpoint sender helpers used by IPC and finalisation paths.
        // Semantics: unlinking a middle waiter patches neighboring TCB links and preserves FIFO order.
        let mut endpoint = crate::ipc::Endpoint::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1)), (3, cpu(0))]);
        for (thread_id, sender_cpu, badge) in [
            (thread(1), cpu(0), 1),
            (thread(2), cpu(1), 2),
            (thread(3), cpu(0), 3),
        ] {
            threads
                .get_mut(thread_id)
                .unwrap()
                .set_state(ThreadState::BlockedOnSend {
                    endpoint: object(10),
                    sender_cpu,
                    badge,
                    can_grant: true,
                    can_grant_reply: false,
                    is_call: false,
                    payload: IpcPayload::empty(),
                });
            threads.append_endpoint_sender(&mut endpoint, thread_id);
        }

        assert_eq!(endpoint.sender_head(), Some(thread(1)));
        assert_eq!(endpoint.queued_senders(), 3);
        assert_eq!(
            threads.get(thread(1)).unwrap().wait_queue_link().next(),
            Some(thread(2))
        );
        assert_eq!(
            threads.get(thread(2)).unwrap().wait_queue_link().prev(),
            Some(thread(1))
        );
        assert_eq!(
            threads.get(thread(2)).unwrap().wait_queue_link().next(),
            Some(thread(3))
        );
        assert_eq!(
            threads.get(thread(3)).unwrap().wait_queue_link().prev(),
            Some(thread(2))
        );

        assert!(threads.unlink_endpoint_waiter(&mut endpoint, thread(2)));
        assert_eq!(endpoint.sender_head(), Some(thread(1)));
        assert_eq!(endpoint.queued_senders(), 2);
        assert!(threads.get(thread(2)).unwrap().wait_queue_link().is_empty());
        assert_eq!(
            threads.get(thread(1)).unwrap().wait_queue_link().next(),
            Some(thread(3))
        );
        assert_eq!(
            threads.get(thread(3)).unwrap().wait_queue_link().prev(),
            Some(thread(1))
        );

        assert_eq!(
            threads.pop_endpoint_sender(&mut endpoint),
            Ok(Some(thread(1)))
        );
        assert_eq!(
            threads.pop_endpoint_sender(&mut endpoint),
            Ok(Some(thread(3)))
        );
        assert_eq!(threads.pop_endpoint_sender(&mut endpoint), Ok(None));
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.sender_head(), None);
    }

    #[test]
    fn notification_queue_uses_tcb_links_for_fifo_and_middle_unlink() {
        // Goal: Notification wait queues keep only anchors while TCB links own membership.
        // Scope: ThreadTable notification helpers used by signal, cancel, and finalisation paths.
        // Semantics: unlinking a middle waiter patches neighboring TCB links and preserves FIFO order.
        let mut notification = Notification::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1)), (3, cpu(0))]);
        for (thread_id, receiver_cpu) in [
            (thread(1), cpu(0)),
            (thread(2), cpu(1)),
            (thread(3), cpu(0)),
        ] {
            threads
                .get_mut(thread_id)
                .unwrap()
                .set_state(ThreadState::BlockedOnNotification {
                    notification: object(20),
                    receiver_cpu,
                });
            threads.append_notification_waiter(&mut notification, thread_id);
        }

        assert_eq!(
            notification.next_waiter().map(|waiter| waiter.thread()),
            Some(thread(1))
        );
        assert_eq!(notification.queued_waiters(), 3);
        assert_eq!(
            threads.get(thread(1)).unwrap().wait_queue_link().next(),
            Some(thread(2))
        );
        assert_eq!(
            threads.get(thread(2)).unwrap().wait_queue_link().prev(),
            Some(thread(1))
        );
        assert_eq!(
            threads.get(thread(2)).unwrap().wait_queue_link().next(),
            Some(thread(3))
        );
        assert_eq!(
            threads.get(thread(3)).unwrap().wait_queue_link().prev(),
            Some(thread(2))
        );

        assert!(threads.unlink_notification_waiter(&mut notification, object(20), thread(2)));
        assert_eq!(
            notification.next_waiter().map(|waiter| waiter.thread()),
            Some(thread(1))
        );
        assert_eq!(notification.queued_waiters(), 2);
        assert!(threads.get(thread(2)).unwrap().wait_queue_link().is_empty());
        assert_eq!(
            threads.get(thread(1)).unwrap().wait_queue_link().next(),
            Some(thread(3))
        );
        assert_eq!(
            threads.get(thread(3)).unwrap().wait_queue_link().prev(),
            Some(thread(1))
        );

        assert_eq!(
            threads.pop_notification_waiter(&mut notification),
            Ok(Some(thread(1)))
        );
        assert_eq!(
            threads.pop_notification_waiter(&mut notification),
            Ok(Some(thread(3)))
        );
        assert_eq!(threads.pop_notification_waiter(&mut notification), Ok(None));
        assert_eq!(notification.queued_waiters(), 0);
        assert_eq!(notification.next_waiter(), None);
    }

    #[test]
    fn send_ipc_receiver_precheck_failure_does_not_consume_waiter() {
        // Goal: send-side delivery validates the queued receiver before consuming it.
        // Scope: send_ipc preflight across Endpoint, ThreadTable, and Scheduler.
        // Semantics: receiver state mismatch leaves endpoint queue, sender state, and current placement unchanged.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnNotification {
                notification: object(20),
                receiver_cpu: cpu(1),
            });
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                send_request(object(10), thread(1), cpu(0), 7, send_options(true, false),),
            ),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(2),
                expected: ThreadState::BlockedOnReceive {
                    endpoint: object(10),
                    receiver_cpu: cpu(1),
                    can_grant: false,
                    reply: None,
                },
                actual: ThreadState::BlockedOnNotification {
                    notification: object(20),
                    receiver_cpu: cpu(1),
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Recv);
        assert_eq!(endpoint.queued_receivers(), 1);
        assert_eq!(
            scheduler.run_queue(cpu(0)).unwrap().current(),
            Some(thread(1))
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
    }

    #[test]
    fn send_ipc_call_delivery_records_reply_and_blocks_caller() {
        // Goal: call delivery records reply state while waking the waiting receiver.
        // Scope: send_ipc call path across Endpoint, Reply, ThreadTable, and Scheduler.
        // Semantics: caller blocks on reply, receiver runs, reply becomes pending, and endpoint drains.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            });
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                send_request(object(10), thread(1), cpu(0), 7, call_options(true, false),)
                    .with_caller(object(100)),
            ),
            Ok(ThreadAction::Woken {
                thread: thread(2),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(2),
                    cpu: cpu(1),
                },
            })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::BlockedOnReply));
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert!(reply.is_pending());
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Idle);
    }

    #[test]
    fn send_ipc_call_uses_tcb_receive_grant_for_reply_setup() {
        // Goal: reply grant metadata comes from the receiver TCB receive state.
        // Scope: send_ipc call path with a receiver queued before sender delivery.
        // Semantics: reply pending state records the receiver's can_grant value, not only endpoint options.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: false,
                reply: None,
            });
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                send_request(object(10), thread(1), cpu(0), 7, call_options(true, false),)
                    .with_caller(object(100)),
            ),
            Ok(ThreadAction::Woken {
                thread: thread(2),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(2),
                    cpu: cpu(1),
                },
            })
        );
        assert_eq!(
            reply.state(),
            ReplyState::Pending {
                caller: ReplyCaller::new(ReplyCallerParams {
                    caller: object(100),
                    target: object(10),
                    thread: thread(1),
                    cpu: cpu(0),
                    can_grant: false,
                })
            }
        );
    }

    #[test]
    fn send_ipc_call_without_reply_authority_stops_caller_without_reply_object() {
        // Goal: calls without reply authority stop the caller instead of creating reply state.
        // Scope: send_ipc call path when sender cannot grant or grant-reply.
        // Semantics: receiver wakes, endpoint drains, caller becomes inactive, and no reply object is needed.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            });
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                send_request(object(10), thread(1), cpu(0), 7, call_options(false, false),),
            ),
            Ok(ThreadAction::Woken {
                thread: thread(2),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(2),
                    cpu: cpu(1),
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Idle);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Inactive));
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().current(), None);
    }

    #[test]
    fn send_ipc_call_without_reply_object_does_not_consume_receiver() {
        // Goal: reply-object absence is checked before consuming the queued receiver.
        // Scope: send_ipc call preflight when reply authority exists but no Reply owner is supplied.
        // Semantics: endpoint queue, both TCB states, and sender current placement remain unchanged.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            });
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                send_request(object(10), thread(1), cpu(0), 7, call_options(true, false),),
            ),
            Err(ThreadActionError::MissingReplyObject {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    reply_can_grant: true,
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Recv);
        assert_eq!(endpoint.queued_receivers(), 1);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
        assert_eq!(
            threads.state(thread(2)),
            Some(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            })
        );
        assert_eq!(
            scheduler.run_queue(cpu(0)).unwrap().current(),
            Some(thread(1))
        );
    }

    #[test]
    fn ipc_delivered_wakes_receiver_without_full_tcb_dependency() {
        // Goal: delivered IPC can wake a receiver using only the queued receiver facts.
        // Scope: apply_ipc_action delivery path after Endpoint has matched sender and receiver.
        // Semantics: matching BlockedOnReceive state becomes Running and is enqueued by affinity.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let action = endpoint.send(
            thread(1),
            cpu(0),
            0,
            send_options(true, false),
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            });
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            apply_ipc_action(&mut threads, &mut scheduler, object(10), None, action),
            Ok(ThreadAction::Woken {
                thread: thread(2),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(2),
                    cpu: cpu(1),
                },
            })
        );
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 1);
    }

    #[test]
    fn failed_wake_does_not_change_tcb_state() {
        // Goal: scheduler wake failure does not partially update the receiver TCB.
        // Scope: apply_ipc_action failure path after delivery metadata exists.
        // Semantics: blocked receive state and existing scheduler placement remain unchanged.
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            });
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&runnable_tcb(2, cpu(1))).unwrap();
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let action = endpoint.send(
            thread(1),
            cpu(0),
            0,
            send_options(true, false),
            IpcPayload::empty(),
        );

        assert_eq!(
            apply_ipc_action(&mut threads, &mut scheduler, object(10), None, action),
            Err(ThreadActionError::Scheduler(
                SchedulerError::ThreadAlreadyScheduled {
                    thread: thread(2),
                    placement: crate::scheduler::ThreadPlacement::Ready { cpu: cpu(1) },
                }
            ))
        );
        assert_eq!(
            threads.state(thread(2)),
            Some(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 1);
    }

    #[test]
    fn delivered_ipc_requires_matching_blocked_receive_state() {
        // Goal: delivered IPC rejects stale or wrong receiver state before wakeup.
        // Scope: apply_ipc_action validation of queued receiver against ThreadTable state.
        // Semantics: mismatched state prevents scheduler enqueue and preserves the actual TCB state.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(true, true));
        let action = endpoint.send(
            thread(1),
            cpu(0),
            0,
            send_options(true, false),
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnNotification {
                notification: object(20),
                receiver_cpu: cpu(1),
            });
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            apply_ipc_action(&mut threads, &mut scheduler, object(10), None, action),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(2),
                expected: ThreadState::BlockedOnReceive {
                    endpoint: object(10),
                    receiver_cpu: cpu(1),
                    can_grant: false,
                    reply: None,
                },
                actual: ThreadState::BlockedOnNotification {
                    notification: object(20),
                    receiver_cpu: cpu(1),
                },
            })
        );
        assert_eq!(
            threads.state(thread(2)),
            Some(ThreadState::BlockedOnNotification {
                notification: object(20),
                receiver_cpu: cpu(1),
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 0);
    }

    #[test]
    fn recv_ipc_releases_one_way_sender() {
        // Goal: receive releases a queued one-way sender without creating reply state.
        // Scope: recv_ipc across Endpoint, ThreadTable, and Scheduler owners.
        // Semantics: sender becomes running and ready, receiver stays current, and endpoint drains.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            send_options(true, true),
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: false,
                payload: IpcPayload::empty(),
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                recv_request(
                    object(10),
                    thread(2),
                    cpu(1),
                    IpcReceiveOptions::new(true, true),
                ),
            ),
            Ok(ThreadAction::Woken {
                thread: thread(1),
                cpu: cpu(0),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(0),
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Idle);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 1);
        assert_eq!(
            scheduler.run_queue(cpu(1)).unwrap().current(),
            Some(thread(2))
        );
    }

    #[test]
    fn recv_ipc_call_records_reply_and_keeps_sender_blocked_on_reply() {
        // Goal: receive-side call records reply authority while keeping the caller blocked on reply.
        // Scope: recv_ipc call path with Reply owner supplied by the receiver context.
        // Semantics: sender moves from BlockedOnSend to BlockedOnReply and receiver remains current.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            call_options(true, false),
            IpcPayload::empty(),
        );
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                recv_request(
                    object(10),
                    thread(2),
                    cpu(1),
                    IpcReceiveOptions::new(true, true),
                )
                .with_caller(object(100))
                .with_receiver_reply(reply_slot(200)),
            ),
            Ok(ThreadAction::ReplyRecorded {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    reply_can_grant: true,
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Idle);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::BlockedOnReply));
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert!(reply.is_pending());
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(
            scheduler.run_queue(cpu(1)).unwrap().current(),
            Some(thread(2))
        );
    }

    #[test]
    fn recv_ipc_call_without_reply_authority_stops_sender_without_reply_object() {
        // Goal: receive-side call without reply authority stops the sender.
        // Scope: recv_ipc call path when queued sender cannot grant reply authority.
        // Semantics: endpoint drains, sender becomes inactive, receiver remains running, and no reply is required.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            call_options(false, false),
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: false,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                recv_request(
                    object(10),
                    thread(2),
                    cpu(1),
                    IpcReceiveOptions::new(true, true),
                ),
            ),
            Ok(ThreadAction::Stopped {
                thread: thread(1),
                cpu: cpu(0),
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Idle);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Inactive));
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(
            scheduler.run_queue(cpu(1)).unwrap().current(),
            Some(thread(2))
        );
    }

    #[test]
    fn recv_ipc_call_without_reply_object_does_not_consume_sender() {
        // Goal: missing reply object is checked before consuming a queued call sender.
        // Scope: recv_ipc call preflight after Endpoint has a queued sender.
        // Semantics: endpoint sender queue, sender state, receiver state, and receiver current placement remain unchanged.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            call_options(true, false),
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                recv_request(
                    object(10),
                    thread(2),
                    cpu(1),
                    IpcReceiveOptions::new(true, true),
                ),
            ),
            Err(ThreadActionError::MissingReplyObject {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    reply_can_grant: true,
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            })
        );
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert_eq!(
            scheduler.run_queue(cpu(1)).unwrap().current(),
            Some(thread(2))
        );
    }

    #[test]
    fn recv_ipc_call_pending_reply_does_not_consume_sender() {
        // Goal: pending Reply owner rejects a second call setup before sender consumption.
        // Scope: recv_ipc call path with an already pending Reply object.
        // Semantics: sender remains queued and blocked, and existing reply state is preserved.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            call_options(true, false),
            IpcPayload::empty(),
        );
        let mut reply = Reply::new();
        reply
            .record_caller(ReplyCaller::new(ReplyCallerParams {
                caller: object(100),
                target: object(200),
                thread: thread(9),
                cpu: cpu(0),
                can_grant: true,
            }))
            .unwrap();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                recv_request(
                    object(10),
                    thread(2),
                    cpu(1),
                    IpcReceiveOptions::new(true, true),
                )
                .with_caller(object(100))
                .with_receiver_reply(reply_slot(200)),
            ),
            Err(ThreadActionError::ReplyAlreadyPending)
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            })
        );
        assert!(reply.is_pending());
    }

    #[test]
    fn recv_ipc_call_unknown_sender_cpu_does_not_consume_sender() {
        // Goal: wakeup scheduler precheck runs before consuming a queued call sender.
        // Scope: recv_ipc call failure path with sender affinity outside topology.
        // Semantics: endpoint queue and sender state remain unchanged, and reply state is not recorded.
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(3),
            7,
            call_options(true, false),
            IpcPayload::empty(),
        );
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(3)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(3),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                recv_request(
                    object(10),
                    thread(2),
                    cpu(1),
                    IpcReceiveOptions::new(true, true),
                )
                .with_receiver_reply(reply_slot(200)),
            ),
            Err(ThreadActionError::Scheduler(SchedulerError::UnknownCpu {
                cpu: cpu(3),
            }))
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                sender_cpu: cpu(3),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
                payload: IpcPayload::empty(),
            })
        );
        assert!(!reply.is_pending());
    }

    #[test]
    fn recv_ipc_blocks_receiver_when_no_sender_waits() {
        // Goal: blocking receive without sender moves the current receiver into Endpoint wait state.
        // Scope: recv_ipc across ThreadTable, Scheduler, and Endpoint owners.
        // Semantics: receiver becomes BlockedOnReceive, endpoint queues it, and CPU current is cleared.
        let mut endpoint = crate::ipc::Endpoint::new();
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                recv_request(
                    object(10),
                    thread(2),
                    cpu(1),
                    IpcReceiveOptions::new(true, true),
                ),
            ),
            Ok(ThreadAction::Blocked {
                thread: thread(2),
                cpu: cpu(1),
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Recv);
        assert_eq!(endpoint.queued_receivers(), 1);
        assert_eq!(
            threads.state(thread(2)),
            Some(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(1),
                can_grant: true,
                reply: None,
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().current(), None);
    }

    #[test]
    fn notification_delivered_wakes_waiter() {
        // Goal: notification signal wakes a queued notification waiter.
        // Scope: signal_notification across Notification, ThreadTable, and Scheduler owners.
        // Semantics: waiter state becomes Running and is enqueued on its affinity CPU.
        let mut notification = Notification::new();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        let mut scheduler = scheduler_with_current(Some(1), None);

        wait_notification(
            &mut threads,
            &mut scheduler,
            &mut notification,
            object(20),
            thread(1),
            cpu(0),
        )
        .unwrap();
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            signal_notification(
                &mut threads,
                &mut scheduler,
                &mut notification,
                object(20),
                0b100,
                BoundTcbSignal::NotReady,
            ),
            Ok(ThreadAction::Woken {
                thread: thread(1),
                cpu: cpu(0),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(0),
                },
            })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
    }

    #[test]
    fn notification_delivery_uses_tcb_blocked_cpu_owner_state() {
        // Goal: notification queues carry waiter identity while TCB state owns blocked CPU metadata.
        // Scope: signal_notification delivery across Notification and ThreadTable owner boundary.
        // Semantics: a queued waiter wakes on the CPU recorded by its BlockedOnNotification state.
        let mut notification = Notification::new();
        let mut threads = table_with_threads(&[(1, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnNotification {
                notification: object(20),
                receiver_cpu: cpu(1),
            });
        threads.append_notification_waiter(&mut notification, thread(1));
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            signal_notification(
                &mut threads,
                &mut scheduler,
                &mut notification,
                object(20),
                0b100,
                BoundTcbSignal::NotReady,
            ),
            Ok(ThreadAction::Woken {
                thread: thread(1),
                cpu: cpu(1),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(1),
                },
            })
        );
        assert_eq!(notification.queued_waiters(), 0);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
    }

    #[test]
    fn signal_notification_precheck_failure_does_not_consume_waiter() {
        // Goal: notification waiter state is validated before queue consumption.
        // Scope: signal_notification failure path for stale Notification waiters.
        // Semantics: mismatched TCB state leaves notification queue and Waiting state intact.
        let mut notification = Notification::new();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(0),
                can_grant: true,
                reply: None,
            });
        notification.enqueue_waiter(thread(1));
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            signal_notification(
                &mut threads,
                &mut scheduler,
                &mut notification,
                object(20),
                0b100,
                BoundTcbSignal::NotReady,
            ),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(1),
                expected: ThreadState::BlockedOnNotification {
                    notification: object(20),
                    receiver_cpu: cpu(0),
                },
                actual: ThreadState::BlockedOnReceive {
                    endpoint: object(10),
                    receiver_cpu: cpu(0),
                    can_grant: true,
                    reply: None,
                },
            })
        );
        assert_eq!(notification.queued_waiters(), 1);
        assert_eq!(
            notification.state(),
            crate::notification::NotificationState::Waiting
        );
    }

    #[test]
    fn signal_notification_stale_successor_does_not_consume_waiter() {
        // Goal: notification queue successor validation happens before anchor mutation.
        // Scope: signal_notification failure path when the head TCB link points to a missing successor.
        // Semantics: missing successor leaves the queued head, queue count, and head link intact.
        let mut notification = Notification::new();
        notification.enqueue_waiter(thread(1));
        notification.enqueue_waiter(thread(2));
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnNotification {
                notification: object(20),
                receiver_cpu: cpu(0),
            });
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_wait_queue_link(TcbWaitQueueLink::new(None, Some(thread(2))));
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            signal_notification(
                &mut threads,
                &mut scheduler,
                &mut notification,
                object(20),
                0b100,
                BoundTcbSignal::NotReady,
            ),
            Err(ThreadActionError::UnknownThread { thread: thread(2) })
        );
        assert_eq!(
            notification.next_waiter().map(|waiter| waiter.thread()),
            Some(thread(1))
        );
        assert_eq!(notification.queued_waiters(), 2);
        assert_eq!(
            threads.get(thread(1)).unwrap().wait_queue_link().next(),
            Some(thread(2))
        );
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnNotification {
                notification: object(20),
                receiver_cpu: cpu(0),
            })
        );
    }

    #[test]
    fn bound_notification_completion_wakes_bound_receiver() {
        // Goal: bound notification completion wakes the TCB bound to that notification.
        // Scope: signal_notification bound-TCB path across Notification and ThreadTable ownership.
        // Semantics: matching bound receive state becomes Running and is scheduled.
        let mut notification = Notification::new();
        notification.bind_tcb(crate::notification::BoundTcb::new(
            object(100),
            thread(1),
            cpu(0),
        ));
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .bind_notification(object(20));
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                receiver_cpu: cpu(0),
                can_grant: false,
                reply: None,
            });
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            signal_notification(
                &mut threads,
                &mut scheduler,
                &mut notification,
                object(20),
                0b100,
                BoundTcbSignal::ReadyToReceive,
            ),
            Ok(ThreadAction::Woken {
                thread: thread(1),
                cpu: cpu(0),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(0),
                },
            })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
    }

    #[test]
    fn active_bound_notification_accumulates_badge_without_bound_wake_precheck() {
        // Goal: active notifications accumulate badges without running bound receive prechecks.
        // Scope: signal_notification path when Notification owner is already Active.
        // Semantics: no thread is woken, badge bits accumulate, and unrelated TCB state remains unchanged.
        let mut notification = Notification::new();
        notification.bind_tcb(crate::notification::BoundTcb::new(
            object(100),
            thread(1),
            cpu(0),
        ));
        assert_eq!(
            notification.signal(0b001, BoundTcbSignal::NotReady),
            NotificationAction::BecameActive { badge: 0b001 }
        );
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .bind_notification(object(20));
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            signal_notification(
                &mut threads,
                &mut scheduler,
                &mut notification,
                object(20),
                0b100,
                BoundTcbSignal::ReadyToReceive,
            ),
            Ok(ThreadAction::NoThread)
        );
        assert_eq!(
            notification.state(),
            crate::notification::NotificationState::Active
        );
        assert_eq!(notification.badge(), 0b101);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
    }

    #[test]
    fn reply_records_caller_as_blocked_on_reply() {
        // Goal: recording a reply caller moves the current caller into reply-blocked state.
        // Scope: record_reply_caller across Reply, ThreadTable, and Scheduler owners.
        // Semantics: Reply becomes pending, caller blocks on reply, and current CPU ownership is cleared.
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            record_reply_caller(
                &mut threads,
                &mut scheduler,
                &mut reply,
                ReplyCaller::new(ReplyCallerParams {
                    caller: object(100),
                    target: object(200),
                    thread: thread(1),
                    cpu: cpu(0),
                    can_grant: true,
                }),
            ),
            Ok(ThreadAction::Blocked {
                thread: thread(1),
                cpu: cpu(0),
            })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::BlockedOnReply));
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().current(), None);
    }

    #[test]
    fn reply_to_caller_wakes_blocked_caller() {
        // Goal: replying to a pending caller wakes the thread recorded in Reply state.
        // Scope: reply_to_caller across Reply, ThreadTable, and Scheduler owners.
        // Semantics: caller becomes Running, is enqueued, and Reply returns to empty.
        let mut reply = Reply::new();
        reply
            .record_caller(ReplyCaller::new(ReplyCallerParams {
                caller: object(100),
                target: object(200),
                thread: thread(1),
                cpu: cpu(0),
                can_grant: true,
            }))
            .unwrap();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnReply);
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            reply_to_caller(&mut threads, &mut scheduler, &mut reply),
            Ok(ThreadAction::Woken {
                thread: thread(1),
                cpu: cpu(0),
                scheduler: SchedulerAction::Enqueued {
                    thread: thread(1),
                    cpu: cpu(0),
                },
            })
        );
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
        assert!(!reply.is_pending());
    }

    #[test]
    fn reply_to_caller_precheck_failure_does_not_consume_reply() {
        // Goal: reply delivery validates caller thread state before consuming Reply state.
        // Scope: reply_to_caller failure path with stale caller TCB state.
        // Semantics: pending Reply and actual TCB state remain unchanged after failure.
        let mut reply = Reply::new();
        reply
            .record_caller(ReplyCaller::new(ReplyCallerParams {
                caller: object(100),
                target: object(200),
                thread: thread(1),
                cpu: cpu(0),
                can_grant: true,
            }))
            .unwrap();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::Running);
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            reply_to_caller(&mut threads, &mut scheduler, &mut reply),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(1),
                expected: ThreadState::BlockedOnReply,
                actual: ThreadState::Running,
            })
        );
        assert!(reply.is_pending());
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
    }
}
