use crate::{
    cap::ObjectId,
    thread::tcb::{CpuId, ThreadId},
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
    waiters: NotificationWaitQueue,
    bound_tcb: Option<BoundTcb>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NotificationWaitQueue {
    head: Option<ThreadId>,
    tail: Option<ThreadId>,
    len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationCancellation {
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
    pub const fn new() -> Self {
        Self {
            state: NotificationState::Idle,
            badge: 0,
            waiters: NotificationWaitQueue::new(),
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
                unreachable!(
                    "waiting notification delivery must be coordinated through ThreadTable waiter links"
                )
            }
        }
    }

    pub fn wait(&mut self, receiver: ThreadId, receiver_cpu: CpuId) -> NotificationAction {
        match self.state {
            NotificationState::Idle | NotificationState::Waiting => {
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

    pub const fn queued_waiters(&self) -> usize {
        self.waiters.len()
    }

    pub const fn next_waiter(&self) -> Option<NotificationWaiter> {
        match self.waiters.head() {
            Some(thread) => Some(NotificationWaiter { thread }),
            None => None,
        }
    }

    pub const fn bound_tcb(&self) -> Option<BoundTcb> {
        self.bound_tcb
    }

    pub(crate) fn enqueue_waiter(&mut self, thread: ThreadId) -> Option<ThreadId> {
        let prev = self.waiters.push_back(thread);
        self.state = NotificationState::Waiting;
        prev
    }

    pub(crate) fn dequeue_waiter_head(&mut self, next: Option<ThreadId>) -> Option<ThreadId> {
        let thread = self.waiters.pop_front(next)?;
        self.refresh_state_after_waiter_mutation();
        Some(thread)
    }

    pub(crate) fn unlink_waiter(
        &mut self,
        thread: ThreadId,
        prev: Option<ThreadId>,
        next: Option<ThreadId>,
    ) -> bool {
        let removed = self.waiters.unlink(thread, prev, next);
        if removed {
            self.refresh_state_after_waiter_mutation();
        }
        removed
    }

    pub fn cancel_all(&mut self) -> NotificationCancellation {
        let bound_tcb = self.bound_tcb.take();
        self.badge = 0;
        self.waiters.clear();
        self.state = NotificationState::Idle;

        NotificationCancellation { bound_tcb }
    }

    pub fn cancel_waiter(&mut self, thread: ThreadId) -> bool {
        self.unlink_waiter(thread, None, None)
    }

    fn refresh_state_after_waiter_mutation(&mut self) {
        if self.waiters.is_empty() && self.state == NotificationState::Waiting {
            self.state = NotificationState::Idle;
        }
    }
}

impl NotificationWaitQueue {
    const fn new() -> Self {
        Self {
            head: None,
            tail: None,
            len: 0,
        }
    }

    const fn head(&self) -> Option<ThreadId> {
        self.head
    }

    const fn len(&self) -> usize {
        self.len
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push_back(&mut self, thread: ThreadId) -> Option<ThreadId> {
        let prev = self.tail;
        if self.head.is_none() {
            self.head = Some(thread);
        }
        self.tail = Some(thread);
        self.len += 1;
        prev
    }

    fn pop_front(&mut self, next: Option<ThreadId>) -> Option<ThreadId> {
        let thread = self.head?;
        self.head = next;
        self.len -= 1;
        if self.len == 0 {
            self.tail = None;
        } else if next.is_none() {
            self.tail = None;
            self.len = 0;
        }
        Some(thread)
    }

    fn unlink(&mut self, thread: ThreadId, prev: Option<ThreadId>, next: Option<ThreadId>) -> bool {
        if self.head != Some(thread)
            && self.tail != Some(thread)
            && prev.is_none()
            && next.is_none()
        {
            return false;
        }
        if self.head == Some(thread) {
            self.head = next;
        }
        if self.tail == Some(thread) {
            self.tail = prev;
        }
        if self.len > 0 {
            self.len -= 1;
        }
        if self.len == 0 {
            self.head = None;
            self.tail = None;
        }
        true
    }

    fn clear(&mut self) {
        self.head = None;
        self.tail = None;
        self.len = 0;
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
        // Goal: signal rewrites notification state according to idle, active, and bound-TCB conditions.
        // Scope: local Notification state machine contract for non-waiting signal delivery.
        // Semantics: each case preserves the owner state it does not consume and updates only the intended badge side effects.
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
        // Goal: wait drains an already active notification badge instead of blocking.
        // Scope: local Notification wait path when a badge is pending.
        // Semantics: active input becomes a badge-consumed action and resets the owner to Idle.
        let mut notification = Notification::new();
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
    fn wait_queue_anchor_tracks_head_tail_and_count() {
        // Goal: Notification owns only waiter anchors while TCB owns waiter links.
        // Scope: local head/tail/count mutation helpers used by ThreadTable transactions.
        // Semantics: enqueue, dequeue, and unlink keep notification queue anchors coherent.
        let mut notification = Notification::new();

        assert_eq!(notification.enqueue_waiter(thread(1)), None);
        assert_eq!(notification.enqueue_waiter(thread(2)), Some(thread(1)));
        assert_eq!(
            notification.next_waiter(),
            Some(NotificationWaiter { thread: thread(1) })
        );
        assert_eq!(notification.queued_waiters(), 2);
        assert_eq!(notification.state(), NotificationState::Waiting);

        assert_eq!(
            notification.dequeue_waiter_head(Some(thread(2))),
            Some(thread(1))
        );
        assert_eq!(
            notification.next_waiter(),
            Some(NotificationWaiter { thread: thread(2) })
        );
        assert_eq!(notification.queued_waiters(), 1);

        assert!(notification.unlink_waiter(thread(2), None, None));
        assert_eq!(notification.next_waiter(), None);
        assert_eq!(notification.queued_waiters(), 0);
        assert_eq!(notification.state(), NotificationState::Idle);
    }

    #[test]
    fn poll_does_not_block_without_active_badge() {
        // Goal: poll observes the empty owner state without changing queue ownership.
        // Scope: local Notification poll path without a pending badge.
        // Semantics: poll failure leaves the notification idle and empty.
        let mut notification = Notification::new();

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
