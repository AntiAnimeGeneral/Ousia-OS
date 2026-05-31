use alloc::collections::VecDeque;

use crate::tcb::{CpuId, ThreadId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationState {
    Idle,
    Waiting,
    Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationWaiter {
    thread: ThreadId,
    cpu: CpuId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationAction {
    Delivered {
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
    waiters: VecDeque<NotificationWaiter>,
}

impl NotificationWaiter {
    pub const fn thread(self) -> ThreadId {
        self.thread
    }

    pub const fn cpu(self) -> CpuId {
        self.cpu
    }
}

impl Notification {
    pub fn new() -> Self {
        Self {
            state: NotificationState::Idle,
            badge: 0,
            waiters: VecDeque::new(),
        }
    }

    pub fn signal(&mut self, badge: u64) -> NotificationAction {
        match self.state {
            NotificationState::Idle | NotificationState::Active => {
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
                    receiver_cpu: waiter.cpu,
                    badge,
                }
            }
        }
    }

    pub fn wait(&mut self, receiver: ThreadId, receiver_cpu: CpuId) -> NotificationAction {
        match self.state {
            NotificationState::Idle | NotificationState::Waiting => {
                self.waiters.push_back(NotificationWaiter {
                    thread: receiver,
                    cpu: receiver_cpu,
                });
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

    #[test]
    fn signal_without_waiter_accumulates_badges() {
        let mut notification = Notification::new();

        assert_eq!(
            notification.signal(0b0010),
            NotificationAction::BecameActive { badge: 0b0010 }
        );
        assert_eq!(
            notification.signal(0b0100),
            NotificationAction::BecameActive { badge: 0b0110 }
        );
        assert_eq!(notification.state(), NotificationState::Active);
        assert_eq!(notification.badge(), 0b0110);
    }

    #[test]
    fn wait_consumes_active_badge() {
        let mut notification = Notification::new();

        notification.signal(0b1010);

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
    fn signal_delivers_to_oldest_waiter() {
        let mut notification = Notification::new();

        notification.wait(thread(1), cpu(0));
        notification.wait(thread(2), cpu(1));

        assert_eq!(
            notification.signal(0b1000),
            NotificationAction::Delivered {
                receiver: thread(1),
                receiver_cpu: cpu(0),
                badge: 0b1000,
            }
        );
        assert_eq!(notification.state(), NotificationState::Waiting);
        assert_eq!(notification.queued_waiters(), 1);
        assert_eq!(notification.badge(), 0);
    }

    #[test]
    fn poll_does_not_block_without_active_badge() {
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
