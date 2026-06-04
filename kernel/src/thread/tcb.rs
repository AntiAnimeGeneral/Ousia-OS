use crate::{
    cap::{ObjectId, ReplyCapabilitySlot},
    ipc::message::IpcPayload,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CpuId(u32);

impl CpuId {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ThreadId(u64);

impl ThreadId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadState {
    Inactive,
    Running,
    Restart,
    BlockedOnReceive {
        endpoint: ObjectId,
        can_grant: bool,
        reply: Option<ReplyCapabilitySlot>,
    },
    BlockedOnSend {
        endpoint: ObjectId,
        sender_cpu: CpuId,
        badge: u64,
        can_grant: bool,
        can_grant_reply: bool,
        is_call: bool,
        payload: IpcPayload,
    },
    BlockedOnReply,
    BlockedOnNotification {
        notification: ObjectId,
        receiver_cpu: CpuId,
    },
    IdleThreadState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tcb {
    id: ThreadId,
    affinity: CpuId,
    state: ThreadState,
    bound_notification: Option<ObjectId>,
}

impl ThreadState {
    pub const fn is_runnable(self) -> bool {
        matches!(self, Self::Running | Self::Restart)
    }

    pub const fn is_blocked(self) -> bool {
        matches!(
            self,
            Self::BlockedOnReceive { .. }
                | Self::BlockedOnSend { .. }
                | Self::BlockedOnReply
                | Self::BlockedOnNotification { .. }
        )
    }

    pub const fn is_stopped(self) -> bool {
        matches!(self, Self::Inactive) || self.is_blocked()
    }
}

impl Tcb {
    pub const fn new(id: ThreadId, affinity: CpuId) -> Self {
        Self {
            id,
            affinity,
            state: ThreadState::Inactive,
            bound_notification: None,
        }
    }

    pub const fn id(&self) -> ThreadId {
        self.id
    }

    pub const fn affinity(&self) -> CpuId {
        self.affinity
    }

    pub const fn state(&self) -> ThreadState {
        self.state
    }

    pub const fn bound_notification(&self) -> Option<ObjectId> {
        self.bound_notification
    }

    pub fn set_state(&mut self, state: ThreadState) {
        self.state = state;
    }

    pub fn set_affinity(&mut self, affinity: CpuId) {
        self.affinity = affinity;
    }

    pub fn bind_notification(&mut self, notification: ObjectId) {
        self.bound_notification = Some(notification);
    }

    pub fn unbind_notification(&mut self) -> Option<ObjectId> {
        self.bound_notification.take()
    }

    pub fn waits_on_bound_notification_receive(&self, notification: ObjectId) -> bool {
        matches!(self.state, ThreadState::BlockedOnReceive { .. })
            && matches!(self.bound_notification, Some(bound) if bound == notification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn object(raw: u64) -> ObjectId {
        ObjectId::new(raw)
    }

    #[test]
    fn new_tcb_starts_inactive_with_affinity() {
        // Goal: TCB construction establishes an inactive thread with explicit affinity.
        // Scope: local TCB default-state contract before KernelState binding or scheduling.
        // Semantics: new TCBs are stopped, unbound to notifications, and keep their requested CPU affinity.
        let tcb = Tcb::new(ThreadId::new(1), CpuId::new(2));

        assert_eq!(tcb.id(), ThreadId::new(1));
        assert_eq!(tcb.affinity(), CpuId::new(2));
        assert_eq!(tcb.state(), ThreadState::Inactive);
        assert_eq!(tcb.bound_notification(), None);
        assert!(tcb.state().is_stopped());
    }

    #[rstest]
    #[case::inactive_is_stopped_before_configuration(ThreadState::Inactive, false, false, true)]
    #[case::blocked_receive_is_blocked_and_stopped(
        ThreadState::BlockedOnReceive { endpoint: object(1), can_grant: true, reply: None },
        true,
        false,
        true
    )]
    #[case::blocked_send_is_blocked_and_stopped(
        ThreadState::BlockedOnSend {
            endpoint: object(1),
            sender_cpu: CpuId::new(0),
            badge: 1,
            can_grant: true,
            can_grant_reply: false,
            is_call: true,
            payload: IpcPayload::empty(),
        },
        true,
        false,
        true
    )]
    #[case::blocked_reply_is_blocked_and_stopped(ThreadState::BlockedOnReply, true, false, true)]
    #[case::blocked_notification_is_blocked_and_stopped(
        ThreadState::BlockedOnNotification { notification: object(2), receiver_cpu: CpuId::new(1) },
        true,
        false,
        true
    )]
    #[case::running_is_runnable_only(ThreadState::Running, false, true, false)]
    #[case::restart_is_runnable_only(ThreadState::Restart, false, true, false)]
    #[case::idle_thread_state_is_outside_normal_scheduling_classes(
        ThreadState::IdleThreadState,
        false,
        false,
        false
    )]
    fn thread_state_predicates_match_sel4_scheduling_classes(
        #[case] state: ThreadState,
        #[case] blocked: bool,
        #[case] runnable: bool,
        #[case] stopped: bool,
    ) {
        // Goal: ThreadState predicates expose seL4 blocked, runnable, and stopped classes.
        // Scope: pure ThreadState classification without scheduler or endpoint side effects.
        // Semantics: each case belongs to the expected scheduling class matrix.
        assert_eq!(state.is_blocked(), blocked);
        assert_eq!(state.is_runnable(), runnable);
        assert_eq!(state.is_stopped(), stopped);
    }

    #[test]
    fn bound_notification_receive_is_derived_from_receive_state() {
        // Goal: bound notification readiness is derived from both binding and receive state.
        // Scope: local TCB state/binding contract without Notification owner mutation.
        // Semantics: only a matching bound notification plus BlockedOnReceive counts as receive readiness.
        let mut tcb = Tcb::new(ThreadId::new(1), CpuId::new(0));

        tcb.bind_notification(object(10));
        tcb.set_state(ThreadState::BlockedOnReceive {
            endpoint: object(20),
            can_grant: false,
            reply: None,
        });

        assert!(tcb.waits_on_bound_notification_receive(object(10)));
        assert!(!tcb.waits_on_bound_notification_receive(object(11)));

        tcb.set_state(ThreadState::BlockedOnNotification {
            notification: object(10),
            receiver_cpu: CpuId::new(0),
        });

        assert!(!tcb.waits_on_bound_notification_receive(object(10)));
        assert_eq!(tcb.unbind_notification(), Some(object(10)));
        assert_eq!(tcb.bound_notification(), None);
    }
}
