use alloc::collections::BTreeMap;

use crate::{
    cap::ObjectId,
    ipc::{Endpoint, IpcAction, IpcPayload, IpcReceiveOptions, IpcSendOptions, ReplySetup},
    notification::{Notification, NotificationAction, NotificationState},
    reply::{Reply, ReplyAction, ReplyCaller, ReplyError},
    scheduler::{Scheduler, SchedulerAction, SchedulerError},
    tcb::{CpuId, Tcb, ThreadId, ThreadState},
};

fn reply_setup_with_receiver_grant(setup: ReplySetup, can_grant: bool) -> ReplySetup {
    ReplySetup { can_grant, ..setup }
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
    Reply(ReplyError),
    Scheduler(SchedulerError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WakeExpectation {
    State(ThreadState),
    Receive { endpoint: ObjectId, can_grant: bool },
    BoundNotificationReceive { notification: ObjectId },
}

#[derive(Debug, Default)]
pub struct ThreadTable {
    tcbs: BTreeMap<ThreadId, Tcb>,
}

impl ThreadTable {
    pub fn new() -> Self {
        Self {
            tcbs: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, tcb: Tcb) -> Option<Tcb> {
        self.tcbs.insert(tcb.id(), tcb)
    }

    pub fn get(&self, thread: ThreadId) -> Option<&Tcb> {
        self.tcbs.get(&thread)
    }

    pub fn get_mut(&mut self, thread: ThreadId) -> Option<&mut Tcb> {
        self.tcbs.get_mut(&thread)
    }

    pub fn state(&self, thread: ThreadId) -> Option<ThreadState> {
        self.get(thread).map(Tcb::state)
    }

    pub fn affinity(&self, thread: ThreadId) -> Option<CpuId> {
        self.get(thread).map(Tcb::affinity)
    }
}

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
        } => block_current(
            threads,
            scheduler,
            thread,
            cpu,
            ThreadState::BlockedOnSend {
                endpoint,
                badge,
                can_grant,
                can_grant_reply,
                is_call,
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
                can_grant,
                reply: receiver_reply,
            },
        ),
        IpcAction::DeliveredToReceiver {
            receiver,
            receiver_cpu,
            receiver_can_grant,
            ..
        } => wake_thread(
            threads,
            scheduler,
            receiver,
            receiver_cpu,
            WakeExpectation::Receive {
                endpoint,
                can_grant: receiver_can_grant,
            },
        ),
        IpcAction::SenderReleased {
            message,
            reply_setup,
            ..
        } => {
            if let Some(setup) = reply_setup {
                return Err(ThreadActionError::ReceiveCallTransactionUnsupported { setup });
            }

            wake_thread(
                threads,
                scheduler,
                message.sender(),
                message.sender_cpu(),
                WakeExpectation::State(ThreadState::BlockedOnSend {
                    endpoint,
                    badge: message.badge(),
                    can_grant: message.can_grant(),
                    can_grant_reply: message.can_grant_reply(),
                    is_call: false,
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
    endpoint_object: ObjectId,
    caller_object: Option<ObjectId>,
    sender: ThreadId,
    sender_cpu: CpuId,
    badge: u64,
    options: IpcSendOptions,
    payload: IpcPayload,
) -> Result<ThreadAction, ThreadActionError> {
    validate_block_current(threads, scheduler, sender, sender_cpu)?;

    let mut reply = reply;

    if let Some(receiver) = endpoint.next_receiver() {
        validate_wake(
            threads,
            scheduler,
            receiver.thread(),
            receiver.cpu(),
            WakeExpectation::Receive {
                endpoint: endpoint_object,
                can_grant: receiver.can_grant(),
            },
        )?;

        if options.is_call {
            let setup = reply_setup_with_receiver_grant(
                ReplySetup {
                    caller: sender,
                    caller_cpu: sender_cpu,
                    can_grant: options.can_grant || options.can_grant_reply,
                },
                receiver.can_grant(),
            );
            let reply = reply
                .as_deref()
                .ok_or(ThreadActionError::MissingReplyObject { setup })?;
            if caller_object.is_none() {
                return Err(ThreadActionError::MissingCallerObject { setup });
            }
            if reply.is_pending() {
                return Err(ThreadActionError::ReplyAlreadyPending);
            }
        }
    }

    let action = endpoint.send(sender, sender_cpu, badge, options, payload);
    match action {
        IpcAction::DeliveredToReceiver {
            receiver,
            receiver_cpu,
            receiver_can_grant,
            message,
            reply_setup: Some(setup),
        } => {
            let setup = reply_setup_with_receiver_grant(setup, receiver_can_grant);
            let caller_object = caller_object
                .expect("prechecked immediate call delivery must provide caller TCB object");
            let reply = reply
                .as_deref_mut()
                .expect("prechecked immediate call delivery must provide reply object");
            let _ = reply
                .record_caller(ReplyCaller::new(
                    caller_object,
                    endpoint_object,
                    setup.caller,
                    setup.caller_cpu,
                    setup.can_grant,
                ))
                .expect("prechecked immediate call reply object must be empty");
            let block = block_current_validated(
                threads,
                scheduler,
                message.sender(),
                message.sender_cpu(),
                ThreadState::BlockedOnReply,
            );
            let wake = wake_thread_validated(threads, scheduler, receiver, receiver_cpu);
            let _ = block;
            Ok(wake)
        }
        action => apply_ipc_action(threads, scheduler, endpoint_object, None, action),
    }
}

pub fn recv_ipc(
    threads: &mut ThreadTable,
    scheduler: &mut Scheduler,
    endpoint: &mut Endpoint,
    reply: Option<&mut Reply>,
    endpoint_object: ObjectId,
    caller_object: Option<ObjectId>,
    receiver_reply: Option<ObjectId>,
    receiver: ThreadId,
    receiver_cpu: CpuId,
    options: IpcReceiveOptions,
) -> Result<ThreadAction, ThreadActionError> {
    validate_block_current(threads, scheduler, receiver, receiver_cpu)?;

    let mut reply = reply;

    if let Some(message) = endpoint.next_sender() {
        let expected = ThreadState::BlockedOnSend {
            endpoint: endpoint_object,
            badge: message.badge(),
            can_grant: message.can_grant(),
            can_grant_reply: message.can_grant_reply(),
            is_call: message.is_call(),
        };

        if message.is_call() {
            validate_blocked_sender_reply_transition(
                threads,
                scheduler,
                message.sender(),
                message.sender_cpu(),
                expected,
            )?;
            let setup = reply_setup_with_receiver_grant(
                ReplySetup {
                    caller: message.sender(),
                    caller_cpu: message.sender_cpu(),
                    can_grant: message.can_grant() || message.can_grant_reply(),
                },
                options.can_grant,
            );
            let reply = reply
                .as_deref()
                .ok_or(ThreadActionError::MissingReplyObject { setup })?;
            if caller_object.is_none() {
                return Err(ThreadActionError::MissingCallerObject { setup });
            }
            if reply.is_pending() {
                return Err(ThreadActionError::ReplyAlreadyPending);
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

    let action = endpoint.recv(receiver, receiver_cpu, options);
    match action {
        IpcAction::SenderReleased {
            message,
            reply_setup: Some(setup),
            ..
        } => {
            let setup = reply_setup_with_receiver_grant(setup, options.can_grant);
            let caller_object =
                caller_object.expect("prechecked receive-side call must provide caller TCB object");
            let reply = reply
                .as_deref_mut()
                .expect("prechecked receive-side call must provide reply object");
            let _ = reply
                .record_caller(ReplyCaller::new(
                    caller_object,
                    endpoint_object,
                    setup.caller,
                    setup.caller_cpu,
                    setup.can_grant,
                ))
                .expect("prechecked receive-side call reply object must be empty");
            threads
                .get_mut(message.sender())
                .expect("prechecked receive-side call sender must exist")
                .set_state(ThreadState::BlockedOnReply);
            Ok(ThreadAction::ReplyRecorded { setup })
        }
        IpcAction::SenderReleased {
            message,
            reply_setup: None,
            ..
        } => Ok(wake_thread_validated(
            threads,
            scheduler,
            message.sender(),
            message.sender_cpu(),
        )),
        action => apply_ipc_action(threads, scheduler, endpoint_object, receiver_reply, action),
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
            ThreadState::BlockedOnNotification { notification },
        ),
        NotificationAction::Delivered {
            receiver,
            receiver_cpu,
            ..
        } => wake_thread(
            threads,
            scheduler,
            receiver,
            receiver_cpu,
            WakeExpectation::State(ThreadState::BlockedOnNotification { notification }),
        ),
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

    let action = notification.wait(receiver, receiver_cpu);
    apply_notification_action(threads, scheduler, notification_object, action)
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
    bound_tcb_accepts_receive: bool,
) -> Result<ThreadAction, ThreadActionError> {
    if let Some(waiter) = notification.next_waiter() {
        validate_wake(
            threads,
            scheduler,
            waiter.thread(),
            waiter.cpu(),
            WakeExpectation::State(ThreadState::BlockedOnNotification {
                notification: notification_object,
            }),
        )?;
    } else if notification.state() == NotificationState::Idle && bound_tcb_accepts_receive {
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

    let action = notification.signal(badge, bound_tcb_accepts_receive);
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
    if let crate::reply::ReplyState::Pending { caller } = reply.state() {
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
        ipc::{IpcPayload, IpcReceiveOptions},
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

    fn runnable_tcb(raw: u64, affinity: CpuId) -> Tcb {
        let mut tcb = Tcb::new(thread(raw), affinity);
        tcb.set_state(ThreadState::Running);
        tcb
    }

    fn table_with_threads(threads: &[(u64, CpuId)]) -> ThreadTable {
        let mut table = ThreadTable::new();
        for (thread, cpu) in threads {
            assert!(table.insert(runnable_tcb(*thread, *cpu)).is_none());
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

    #[test]
    fn resume_tcb_sets_restart_and_enqueues_on_affinity_cpu() {
        let mut threads = ThreadTable::new();
        assert!(threads.insert(Tcb::new(thread(1), cpu(1))).is_none());
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
        let mut threads = ThreadTable::new();
        assert!(threads.insert(Tcb::new(thread(1), cpu(9))).is_none());
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
        let mut threads = ThreadTable::new();
        let mut tcb = Tcb::new(thread(1), cpu(0));
        tcb.set_state(ThreadState::Inactive);
        assert!(threads.insert(tcb).is_none());
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
    fn ipc_sender_block_sets_tcb_state_and_removes_current() {
        let mut endpoint = crate::ipc::Endpoint::new();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                thread(1),
                cpu(0),
                7,
                crate::ipc::IpcSendOptions {
                    blocking: true,
                    is_call: true,
                    can_grant: true,
                    can_grant_reply: false,
                },
                IpcPayload::empty(),
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
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            })
        );
    }

    #[test]
    fn send_ipc_precheck_failure_does_not_mutate_endpoint() {
        let mut endpoint = crate::ipc::Endpoint::new();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                thread(1),
                cpu(0),
                7,
                crate::ipc::IpcSendOptions {
                    blocking: true,
                    is_call: false,
                    can_grant: true,
                    can_grant_reply: false,
                },
                IpcPayload::empty(),
            ),
            Err(ThreadActionError::ThreadNotCurrent {
                thread: thread(1),
                cpu: cpu(0),
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Idle);
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(threads.state(thread(1)), Some(ThreadState::Running));
    }

    #[test]
    fn send_ipc_receiver_precheck_failure_does_not_consume_waiter() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(
            thread(2),
            cpu(1),
            IpcReceiveOptions {
                blocking: true,
                can_grant: true,
            },
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnNotification {
                notification: object(20),
            });
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            send_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                thread(1),
                cpu(0),
                7,
                crate::ipc::IpcSendOptions {
                    blocking: true,
                    is_call: false,
                    can_grant: false,
                    can_grant_reply: false,
                },
                IpcPayload::empty(),
            ),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(2),
                expected: ThreadState::BlockedOnReceive {
                    endpoint: object(10),
                    can_grant: true,
                    reply: None,
                },
                actual: ThreadState::BlockedOnNotification {
                    notification: object(20),
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
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(
            thread(2),
            cpu(1),
            IpcReceiveOptions {
                blocking: true,
                can_grant: true,
            },
        );
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
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
                object(10),
                Some(object(100)),
                thread(1),
                cpu(0),
                7,
                crate::ipc::IpcSendOptions {
                    blocking: true,
                    is_call: true,
                    can_grant: true,
                    can_grant_reply: false,
                },
                IpcPayload::empty(),
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
    fn send_ipc_call_without_reply_object_does_not_consume_receiver() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(
            thread(2),
            cpu(1),
            IpcReceiveOptions {
                blocking: true,
                can_grant: true,
            },
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
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
                object(10),
                None,
                thread(1),
                cpu(0),
                7,
                crate::ipc::IpcSendOptions {
                    blocking: true,
                    is_call: true,
                    can_grant: true,
                    can_grant_reply: false,
                },
                IpcPayload::empty(),
            ),
            Err(ThreadActionError::MissingReplyObject {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    can_grant: true,
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
    fn send_ipc_call_without_caller_object_does_not_consume_receiver() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(
            thread(2),
            cpu(1),
            IpcReceiveOptions {
                blocking: true,
                can_grant: true,
            },
        );
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
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
                object(10),
                None,
                thread(1),
                cpu(0),
                7,
                crate::ipc::IpcSendOptions {
                    blocking: true,
                    is_call: true,
                    can_grant: true,
                    can_grant_reply: false,
                },
                IpcPayload::empty(),
            ),
            Err(ThreadActionError::MissingCallerObject {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    can_grant: true,
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
                can_grant: true,
                reply: None,
            })
        );
        assert!(!reply.is_pending());
    }

    #[test]
    fn ipc_delivered_wakes_receiver_without_full_tcb_dependency() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(
            thread(2),
            cpu(1),
            IpcReceiveOptions {
                blocking: true,
                can_grant: true,
            },
        );
        let action = endpoint.send(
            thread(1),
            cpu(0),
            0,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: false,
                can_grant: false,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
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
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                can_grant: true,
                reply: None,
            });
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        scheduler.enqueue(&runnable_tcb(2, cpu(1))).unwrap();
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(
            thread(2),
            cpu(1),
            IpcReceiveOptions {
                blocking: true,
                can_grant: true,
            },
        );
        let action = endpoint.send(
            thread(1),
            cpu(0),
            0,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: false,
                can_grant: false,
                can_grant_reply: false,
            },
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
                can_grant: true,
                reply: None,
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 1);
    }

    #[test]
    fn delivered_ipc_requires_matching_blocked_receive_state() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.recv(
            thread(2),
            cpu(1),
            IpcReceiveOptions {
                blocking: true,
                can_grant: true,
            },
        );
        let action = endpoint.send(
            thread(1),
            cpu(0),
            0,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: false,
                can_grant: false,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        threads
            .get_mut(thread(2))
            .unwrap()
            .set_state(ThreadState::BlockedOnNotification {
                notification: object(20),
            });
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            apply_ipc_action(&mut threads, &mut scheduler, object(10), None, action),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(2),
                expected: ThreadState::BlockedOnReceive {
                    endpoint: object(10),
                    can_grant: true,
                    reply: None,
                },
                actual: ThreadState::BlockedOnNotification {
                    notification: object(20),
                },
            })
        );
        assert_eq!(
            threads.state(thread(2)),
            Some(ThreadState::BlockedOnNotification {
                notification: object(20),
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 0);
    }

    #[test]
    fn recv_ipc_releases_one_way_sender() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: false,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: false,
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                None,
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
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
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: true,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                object(10),
                Some(object(100)),
                Some(object(200)),
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
            ),
            Ok(ThreadAction::ReplyRecorded {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    can_grant: true,
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
    fn recv_ipc_call_without_reply_object_does_not_consume_sender() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: true,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                None,
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
            ),
            Err(ThreadActionError::MissingReplyObject {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    can_grant: true,
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            })
        );
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert_eq!(
            scheduler.run_queue(cpu(1)).unwrap().current(),
            Some(thread(2))
        );
    }

    #[test]
    fn recv_ipc_call_without_caller_object_does_not_consume_sender() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: true,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                object(10),
                None,
                Some(object(200)),
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
            ),
            Err(ThreadActionError::MissingCallerObject {
                setup: ReplySetup {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    can_grant: true,
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            })
        );
        assert_eq!(threads.state(thread(2)), Some(ThreadState::Running));
        assert!(!reply.is_pending());
    }

    #[test]
    fn recv_ipc_call_pending_reply_does_not_consume_sender() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: true,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut reply = Reply::new();
        reply
            .record_caller(ReplyCaller::new(
                object(100),
                object(200),
                thread(9),
                cpu(0),
                true,
            ))
            .unwrap();
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                object(10),
                Some(object(100)),
                Some(object(200)),
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
            ),
            Err(ThreadActionError::ReplyAlreadyPending)
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            })
        );
        assert!(reply.is_pending());
    }

    #[test]
    fn recv_ipc_call_unknown_sender_cpu_does_not_consume_sender() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(3),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: true,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(3)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                Some(&mut reply),
                object(10),
                None,
                Some(object(200)),
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
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
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: true,
            })
        );
        assert!(!reply.is_pending());
    }

    #[test]
    fn recv_ipc_one_way_sender_already_scheduled_does_not_consume_sender() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: false,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: false,
            });
        let mut scheduler = scheduler_with_current(Some(1), Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                None,
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
            ),
            Err(ThreadActionError::Scheduler(
                SchedulerError::ThreadAlreadyScheduled {
                    thread: thread(1),
                    placement: crate::scheduler::ThreadPlacement::Current { cpu: cpu(0) },
                }
            ))
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(
            threads.state(thread(1)),
            Some(ThreadState::BlockedOnSend {
                endpoint: object(10),
                badge: 7,
                can_grant: true,
                can_grant_reply: false,
                is_call: false,
            })
        );
    }

    #[test]
    fn recv_ipc_one_way_sender_state_mismatch_does_not_consume_sender() {
        let mut endpoint = crate::ipc::Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            crate::ipc::IpcSendOptions {
                blocking: true,
                is_call: false,
                can_grant: true,
                can_grant_reply: false,
            },
            IpcPayload::empty(),
        );
        let mut threads = table_with_threads(&[(1, cpu(0)), (2, cpu(1))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                can_grant: true,
                reply: None,
            });
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                None,
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
            ),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(1),
                expected: ThreadState::BlockedOnSend {
                    endpoint: object(10),
                    badge: 7,
                    can_grant: true,
                    can_grant_reply: false,
                    is_call: false,
                },
                actual: ThreadState::BlockedOnReceive {
                    endpoint: object(10),
                    can_grant: true,
                    reply: None,
                },
            })
        );
        assert_eq!(endpoint.state(), crate::ipc::EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
    }

    #[test]
    fn recv_ipc_blocks_receiver_when_no_sender_waits() {
        let mut endpoint = crate::ipc::Endpoint::new();
        let mut threads = table_with_threads(&[(2, cpu(1))]);
        let mut scheduler = scheduler_with_current(None, Some(2));

        assert_eq!(
            recv_ipc(
                &mut threads,
                &mut scheduler,
                &mut endpoint,
                None,
                object(10),
                None,
                None,
                thread(2),
                cpu(1),
                IpcReceiveOptions {
                    blocking: true,
                    can_grant: true,
                },
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
                can_grant: true,
                reply: None,
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().current(), None);
    }

    #[test]
    fn notification_delivered_wakes_waiter() {
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
        assert_eq!(notification.queued_waiters(), 1);
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().current(), None);

        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

        assert_eq!(
            signal_notification(
                &mut threads,
                &mut scheduler,
                &mut notification,
                object(20),
                0b100,
                false,
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
    fn signal_notification_precheck_failure_does_not_consume_waiter() {
        let mut notification = Notification::new();
        notification.wait(thread(1), cpu(0));
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        threads
            .get_mut(thread(1))
            .unwrap()
            .set_state(ThreadState::BlockedOnReceive {
                endpoint: object(10),
                can_grant: true,
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
                false,
            ),
            Err(ThreadActionError::UnexpectedThreadState {
                thread: thread(1),
                expected: ThreadState::BlockedOnNotification {
                    notification: object(20),
                },
                actual: ThreadState::BlockedOnReceive {
                    endpoint: object(10),
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
    fn bound_notification_completion_wakes_bound_receiver() {
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
                true,
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
        let mut notification = Notification::new();
        notification.bind_tcb(crate::notification::BoundTcb::new(
            object(100),
            thread(1),
            cpu(0),
        ));
        assert_eq!(
            notification.signal(0b001, false),
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
                true,
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
        let mut reply = Reply::new();
        let mut threads = table_with_threads(&[(1, cpu(0))]);
        let mut scheduler = scheduler_with_current(Some(1), None);

        assert_eq!(
            record_reply_caller(
                &mut threads,
                &mut scheduler,
                &mut reply,
                ReplyCaller::new(object(100), object(200), thread(1), cpu(0), true),
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
