use alloc::vec::Vec;

use crate::{
    cap::ObjectId,
    tcb::{CpuId, ThreadId},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationState {
    Idle,
    Waiting,
    Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationWaiter {
    thread: ThreadId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundTcb {
    tcb: ObjectId,
    thread: ThreadId,
    cpu: CpuId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundTcbSignal {
    NotReady,
    ReadyToReceive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationAction {
    Delivered {
        receiver: ThreadId,
        badge: u64,
    },
    BoundReceiveCompleted {
        tcb: ObjectId,
        receiver: ThreadId,
        receiver_cpu: CpuId,
        badge: u64,
    },
    BecameActive {
        badge: u64,
    },
    BadgeConsumed {
        thread: ThreadId,
        cpu: CpuId,
        badge: u64,
    },
    ReceiverBlocked {
        thread: ThreadId,
        cpu: CpuId,
    },
    PollFailed {
        thread: ThreadId,
        cpu: CpuId,
    },
}

#[derive(Debug)]
pub struct Notification {
    state: NotificationState,
    badge: u64,
    waiters: NotificationQueue<NotificationWaiter>,
    bound_tcb: Option<BoundTcb>,
}

#[derive(Debug)]
struct NotificationQueue<T> {
    entries: Vec<T>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationCancellation {
    pub waiters: Vec<NotificationWaiter>,
    pub bound_tcb: Option<BoundTcb>,
}

impl NotificationWaiter {
    pub const fn thread(self) -> ThreadId {
        self.thread
    }
}

impl BoundTcb {
    pub const fn new(tcb: ObjectId, thread: ThreadId, cpu: CpuId) -> Self {
        Self { tcb, thread, cpu }
    }

    pub const fn tcb(self) -> ObjectId {
        self.tcb
    }

    pub const fn thread(self) -> ThreadId {
        self.thread
    }

    pub const fn cpu(self) -> CpuId {
        self.cpu
    }
}

impl BoundTcbSignal {
    pub const fn from_ready(ready: bool) -> Self {
        if ready {
            Self::ReadyToReceive
        } else {
            Self::NotReady
        }
    }

    pub const fn is_ready(self) -> bool {
        matches!(self, Self::ReadyToReceive)
    }
}

impl Notification {
    pub fn new() -> Self {
        Self {
            state: NotificationState::Idle,
            badge: 0,
            waiters: NotificationQueue::new(),
            bound_tcb: None,
        }
    }

    pub fn bind_tcb(&mut self, tcb: BoundTcb) {
        self.bound_tcb = Some(tcb);
    }

    pub fn unbind_tcb(&mut self) -> Option<BoundTcb> {
        self.bound_tcb.take()
    }

    pub fn signal(&mut self, badge: u64, bound_tcb: BoundTcbSignal) -> NotificationAction {
        match self.state {
            NotificationState::Idle | NotificationState::Active => {
                if let (NotificationState::Idle, Some(bound_tcb), true) =
                    (self.state, self.bound_tcb, bound_tcb.is_ready())
                {
                    return NotificationAction::BoundReceiveCompleted {
                        tcb: bound_tcb.tcb,
                        receiver: bound_tcb.thread,
                        receiver_cpu: bound_tcb.cpu,
                        badge,
                    };
                }

                self.badge |= badge;
                self.state = NotificationState::Active;
                NotificationAction::BecameActive { badge: self.badge }
            }
            NotificationState::Waiting => {
                let waiter = self
                    .waiters
                    .pop_front()
                    .expect("Waiting notification state must have a waiting receiver");
                if self.waiters.is_empty() {
                    self.state = NotificationState::Idle;
                }
                NotificationAction::Delivered {
                    receiver: waiter.thread,
                    badge,
                }
            }
        }
    }

    pub fn wait(&mut self, receiver: ThreadId, receiver_cpu: CpuId) -> NotificationAction {
        match self.state {
            NotificationState::Idle | NotificationState::Waiting => {
                self.waiters
                    .push_back(NotificationWaiter { thread: receiver });
                self.state = NotificationState::Waiting;
                NotificationAction::ReceiverBlocked {
                    thread: receiver,
                    cpu: receiver_cpu,
                }
            }
            NotificationState::Active => {
                let badge = self.badge;
                self.badge = 0;
                self.state = NotificationState::Idle;
                NotificationAction::BadgeConsumed {
                    thread: receiver,
                    cpu: receiver_cpu,
                    badge,
                }
            }
        }
    }

    pub fn poll(&mut self, receiver: ThreadId, receiver_cpu: CpuId) -> NotificationAction {
        match self.state {
            NotificationState::Idle | NotificationState::Waiting => {
                NotificationAction::PollFailed {
                    thread: receiver,
                    cpu: receiver_cpu,
                }
            }
            NotificationState::Active => {
                let badge = self.badge;
                self.badge = 0;
                self.state = NotificationState::Idle;
                NotificationAction::BadgeConsumed {
                    thread: receiver,
                    cpu: receiver_cpu,
                    badge,
                }
            }
        }
    }

    pub const fn state(&self) -> NotificationState {
        self.state
    }

    pub const fn badge(&self) -> u64 {
        self.badge
    }

    pub fn queued_waiters(&self) -> usize {
        self.waiters.len()
    }

    pub fn next_waiter(&self) -> Option<NotificationWaiter> {
        self.waiters.front().copied()
    }

    pub const fn bound_tcb(&self) -> Option<BoundTcb> {
        self.bound_tcb
    }

    pub fn cancel_all(&mut self) -> NotificationCancellation {
        let waiters = self.waiters.drain_all().collect();
        let bound_tcb = self.bound_tcb.take();
        self.badge = 0;
        self.state = NotificationState::Idle;

        NotificationCancellation { waiters, bound_tcb }
    }

    pub fn cancel_waiter(&mut self, thread: ThreadId) -> bool {
        let waiter_count = self.waiters.len();
        self.waiters.retain(|waiter| waiter.thread != thread);
        if self.waiters.is_empty() && self.state == NotificationState::Waiting {
            self.state = NotificationState::Idle;
        }

        waiter_count != self.waiters.len()
    }
}

impl<T> NotificationQueue<T> {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn push_back(&mut self, value: T) {
        self.entries.push(value);
    }

    fn pop_front(&mut self) -> Option<T> {
        if self.entries.is_empty() {
            return None;
        }
        Some(self.entries.remove(0))
    }

    fn front(&self) -> Option<&T> {
        self.entries.first()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn drain_all(&mut self) -> impl Iterator<Item = T> + '_ {
        self.entries.drain(..)
    }

    fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        self.entries.retain(keep);
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

    fn bound() -> BoundTcb {
        BoundTcb::new(object(100), thread(1), cpu(0))
    }

    fn idle_notification() -> Notification {
        Notification::new()
    }

    fn waiting_notification() -> Notification {
        let mut notification = Notification::new();
        notification.wait(thread(1), cpu(0));
        notification.wait(thread(2), cpu(1));
        notification
    }

    fn active_notification() -> Notification {
        let mut notification = Notification::new();
        notification.signal(0b0010, BoundTcbSignal::NotReady);
        notification
    }

    fn idle_bound_notification() -> Notification {
        let mut notification = Notification::new();
        notification.bind_tcb(bound());
        notification
    }

    fn active_bound_notification() -> Notification {
        let mut notification = active_notification();
        notification.bind_tcb(bound());
        notification
    }

    #[rstest]
    #[case::idle_without_bound_accumulates_badge(
        idle_notification(),
        0b0010,
        BoundTcbSignal::NotReady,
        NotificationAction::BecameActive { badge: 0b0010 },
        NotificationState::Active,
        0b0010,
        0,
        None,
    )]
    #[case::waiting_receives_oldest_waiter(
        waiting_notification(),
        0b1000,
        BoundTcbSignal::NotReady,
        NotificationAction::Delivered {
            receiver: thread(1),
            badge: 0b1000,
        },
        NotificationState::Waiting,
        0,
        1,
        None,
    )]
    #[case::active_without_bound_accumulates_badge(
        active_notification(),
        0b0100,
        BoundTcbSignal::NotReady,
        NotificationAction::BecameActive { badge: 0b0110 },
        NotificationState::Active,
        0b0110,
        0,
        None,
    )]
    #[case::idle_bound_receive_completes(
        idle_bound_notification(),
        0b1000,
        BoundTcbSignal::ReadyToReceive,
        NotificationAction::BoundReceiveCompleted {
            tcb: object(100),
            receiver: thread(1),
            receiver_cpu: cpu(0),
            badge: 0b1000,
        },
        NotificationState::Idle,
        0,
        0,
        Some(bound()),
    )]
    #[case::idle_bound_without_ready_accumulates_badge(
        idle_bound_notification(),
        0b0100,
        BoundTcbSignal::NotReady,
        NotificationAction::BecameActive { badge: 0b0100 },
        NotificationState::Active,
        0b0100,
        0,
        Some(bound()),
    )]
    #[case::active_bound_accumulates_badge(
        active_bound_notification(),
        0b0100,
        BoundTcbSignal::ReadyToReceive,
        NotificationAction::BecameActive { badge: 0b0110 },
        NotificationState::Active,
        0b0110,
        0,
        Some(bound()),
    )]
    fn signal_updates_notification_state_contract(
        #[case] mut notification: Notification,
        #[case] badge: u64,
        #[case] bound_tcb_signal: BoundTcbSignal,
        #[case] expected_action: NotificationAction,
        #[case] expected_state: NotificationState,
        #[case] expected_badge: u64,
        #[case] expected_waiters: usize,
        #[case] expected_bound_tcb: Option<BoundTcb>,
    ) {
        // Goal: signal rewrites notification state according to idle, waiting, and bound-TCB conditions.
        // Scope: local Notification state machine contract for signal delivery.
        // Semantics: each case preserves the owner state it does not consume and updates only the intended badge/waiter side effects.

        assert_eq!(
            notification.signal(badge, bound_tcb_signal),
            expected_action
        );
        assert_eq!(notification.state(), expected_state);
        assert_eq!(notification.badge(), expected_badge);
        assert_eq!(notification.queued_waiters(), expected_waiters);
        assert_eq!(notification.bound_tcb(), expected_bound_tcb);
    }

    #[test]
    fn wait_consumes_active_badge() {
        let mut notification = Notification::new();

        // Goal: wait drains an already active notification badge instead of blocking.
        // Scope: local Notification wait path when a badge is pending.
        // Semantics: active input becomes a badge-consumed action and resets the owner to Idle.

        notification.signal(0b1010, BoundTcbSignal::NotReady);

        assert_eq!(
            notification.wait(thread(1), cpu(0)),
            NotificationAction::BadgeConsumed {
                thread: thread(1),
                cpu: cpu(0),
                badge: 0b1010,
            }
        );
        assert_eq!(notification.state(), NotificationState::Idle);
        assert_eq!(notification.badge(), 0);
    }

    #[test]
    fn wait_blocks_when_idle() {
        let mut notification = Notification::new();

        // Goal: wait enqueues the receiver when no badge is available.
        // Scope: local Notification wait path for the idle state.
        // Semantics: the receiver becomes the first queued waiter and the owner enters Waiting.

        assert_eq!(
            notification.wait(thread(1), cpu(0)),
            NotificationAction::ReceiverBlocked {
                thread: thread(1),
                cpu: cpu(0),
            }
        );
        assert_eq!(notification.state(), NotificationState::Waiting);
        assert_eq!(notification.queued_waiters(), 1);
    }

    #[test]
    fn poll_does_not_block_without_active_badge() {
        let mut notification = Notification::new();

        // Goal: poll observes the empty owner state without changing queue ownership.
        // Scope: local Notification poll path without a pending badge.
        // Semantics: poll failure leaves the notification idle and empty.

        assert_eq!(
            notification.poll(thread(1), cpu(0)),
            NotificationAction::PollFailed {
                thread: thread(1),
                cpu: cpu(0),
            }
        );
        assert_eq!(notification.state(), NotificationState::Idle);
        assert_eq!(notification.queued_waiters(), 0);
    }
}
