use super::message::IpcPayload;
use crate::thread::tcb::{CpuId, ThreadId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedReceiver {
    thread: ThreadId,
    cpu: CpuId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointState {
    Idle,
    Send,
    Recv,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcMessage {
    sender: ThreadId,
    sender_cpu: CpuId,
    badge: u64,
    can_grant: bool,
    can_grant_reply: bool,
    mode: IpcSendMode,
    payload: IpcPayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedSender {
    thread: ThreadId,
    cpu: CpuId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcSendMode {
    Send,
    Call,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcSendOperation {
    Send { blocking: bool },
    Call,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcSendOptions {
    pub operation: IpcSendOperation,
    pub can_grant: bool,
    pub can_grant_reply: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcReceiveOptions {
    pub blocking: bool,
    pub can_grant: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcAction {
    SenderBlocked {
        thread: ThreadId,
        cpu: CpuId,
        badge: u64,
        can_grant: bool,
        can_grant_reply: bool,
        is_call: bool,
        payload: IpcPayload,
    },
    ReceiverBlocked {
        thread: ThreadId,
        cpu: CpuId,
        can_grant: bool,
    },
    SendIgnored {
        thread: ThreadId,
        cpu: CpuId,
    },
    NonblockingReceiveFailed {
        thread: ThreadId,
        cpu: CpuId,
    },
    DeliveredToReceiver {
        receiver: ThreadId,
        receiver_cpu: CpuId,
        message: IpcMessage,
        reply_request: Option<ReplyRequest>,
    },
    SenderReleased {
        receiver: ThreadId,
        receiver_cpu: CpuId,
        receiver_can_grant: bool,
        sender: QueuedSender,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyRequest {
    pub caller: ThreadId,
    pub caller_cpu: CpuId,
    pub sender_can_reply: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplySetup {
    pub caller: ThreadId,
    pub caller_cpu: CpuId,
    pub reply_can_grant: bool,
}

#[derive(Debug)]
pub struct Endpoint {
    state: EndpointState,
    senders: EndpointWaitQueue,
    receivers: EndpointWaitQueue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EndpointWaitQueue {
    head: Option<ThreadId>,
    tail: Option<ThreadId>,
    len: usize,
}

impl QueuedReceiver {
    pub const fn new(thread: ThreadId, cpu: CpuId) -> Self {
        Self { thread, cpu }
    }

    pub const fn thread(self) -> ThreadId {
        self.thread
    }

    pub const fn cpu(self) -> CpuId {
        self.cpu
    }
}

impl QueuedSender {
    pub const fn new(thread: ThreadId, cpu: CpuId) -> Self {
        Self { thread, cpu }
    }

    pub const fn thread(self) -> ThreadId {
        self.thread
    }

    pub const fn cpu(self) -> CpuId {
        self.cpu
    }
}

impl IpcMessage {
    pub(crate) const fn new_for_blocked_sender(
        sender: ThreadId,
        sender_cpu: CpuId,
        badge: u64,
        can_grant: bool,
        can_grant_reply: bool,
        mode: IpcSendMode,
        payload: IpcPayload,
    ) -> Self {
        Self {
            sender,
            sender_cpu,
            badge,
            can_grant,
            can_grant_reply,
            mode,
            payload,
        }
    }

    pub const fn sender(self) -> ThreadId {
        self.sender
    }

    pub const fn sender_cpu(self) -> CpuId {
        self.sender_cpu
    }

    pub const fn badge(self) -> u64 {
        self.badge
    }

    pub const fn can_grant(self) -> bool {
        self.can_grant
    }

    pub const fn can_grant_reply(self) -> bool {
        self.can_grant_reply
    }

    pub const fn is_call(self) -> bool {
        self.mode.is_call()
    }

    pub const fn payload(self) -> IpcPayload {
        self.payload
    }
}

impl IpcSendMode {
    pub const fn is_call(self) -> bool {
        matches!(self, Self::Call)
    }
}

impl IpcSendOperation {
    pub const fn is_blocking(self) -> bool {
        match self {
            Self::Send { blocking } => blocking,
            Self::Call => true,
        }
    }

    pub const fn mode(self) -> IpcSendMode {
        match self {
            Self::Send { .. } => IpcSendMode::Send,
            Self::Call => IpcSendMode::Call,
        }
    }

    pub const fn is_call(self) -> bool {
        matches!(self, Self::Call)
    }
}

impl IpcSendOptions {
    pub const fn send(blocking: bool, can_grant: bool, can_grant_reply: bool) -> Self {
        Self {
            operation: IpcSendOperation::Send { blocking },
            can_grant,
            can_grant_reply,
        }
    }

    pub const fn call(can_grant: bool, can_grant_reply: bool) -> Self {
        Self {
            operation: IpcSendOperation::Call,
            can_grant,
            can_grant_reply,
        }
    }

    pub const fn is_blocking(self) -> bool {
        self.operation.is_blocking()
    }

    pub const fn mode(self) -> IpcSendMode {
        self.operation.mode()
    }

    pub const fn is_call(self) -> bool {
        self.operation.is_call()
    }
}

impl IpcReceiveOptions {
    pub const fn new(blocking: bool, can_grant: bool) -> Self {
        Self {
            blocking,
            can_grant,
        }
    }
}

impl Endpoint {
    pub const fn new() -> Self {
        Self {
            state: EndpointState::Idle,
            senders: EndpointWaitQueue::new(),
            receivers: EndpointWaitQueue::new(),
        }
    }

    pub fn send(
        &mut self,
        sender: ThreadId,
        sender_cpu: CpuId,
        badge: u64,
        options: IpcSendOptions,
        payload: IpcPayload,
    ) -> IpcAction {
        if self.state == EndpointState::Recv {
            let receiver = self
                .dequeue_receiver_head(None)
                .expect("Recv endpoint state must have a waiting receiver");
            let message = IpcMessage::new_for_blocked_sender(
                sender,
                sender_cpu,
                badge,
                options.can_grant,
                options.can_grant_reply,
                options.mode(),
                payload,
            );
            return IpcAction::DeliveredToReceiver {
                receiver,
                receiver_cpu: CpuId::new(0),
                message,
                reply_request: reply_request_for(message),
            };
        }

        if !options.is_blocking() {
            return IpcAction::SendIgnored {
                thread: sender,
                cpu: sender_cpu,
            };
        }

        self.enqueue_sender(sender);
        IpcAction::SenderBlocked {
            thread: sender,
            cpu: sender_cpu,
            badge,
            can_grant: options.can_grant,
            can_grant_reply: options.can_grant_reply,
            is_call: options.is_call(),
            payload,
        }
    }

    pub fn recv(
        &mut self,
        receiver: ThreadId,
        receiver_cpu: CpuId,
        options: IpcReceiveOptions,
    ) -> IpcAction {
        if self.state == EndpointState::Send {
            let sender = self
                .dequeue_sender_head(None)
                .expect("Send endpoint state must have a waiting sender");
            return IpcAction::SenderReleased {
                receiver,
                receiver_cpu,
                receiver_can_grant: options.can_grant,
                sender: QueuedSender::new(sender, CpuId::new(0)),
            };
        }

        if !options.blocking {
            return IpcAction::NonblockingReceiveFailed {
                thread: receiver,
                cpu: receiver_cpu,
            };
        }

        self.enqueue_receiver(receiver);
        IpcAction::ReceiverBlocked {
            thread: receiver,
            cpu: receiver_cpu,
            can_grant: options.can_grant,
        }
    }

    pub const fn state(&self) -> EndpointState {
        self.state
    }

    pub const fn queued_senders(&self) -> usize {
        self.senders.len()
    }

    pub const fn queued_receivers(&self) -> usize {
        self.receivers.len()
    }

    pub fn next_receiver(&self) -> Option<QueuedReceiver> {
        self.receiver_head()
            .map(|thread| QueuedReceiver::new(thread, CpuId::new(0)))
    }

    pub fn next_sender(&self) -> Option<QueuedSender> {
        self.sender_head()
            .map(|thread| QueuedSender::new(thread, CpuId::new(0)))
    }

    pub const fn sender_head(&self) -> Option<ThreadId> {
        self.senders.head()
    }

    pub const fn receiver_head(&self) -> Option<ThreadId> {
        self.receivers.head()
    }

    pub(crate) fn enqueue_sender(&mut self, thread: ThreadId) -> Option<ThreadId> {
        let prev = self.senders.push_back(thread);
        self.state = EndpointState::Send;
        prev
    }

    pub(crate) fn enqueue_receiver(&mut self, thread: ThreadId) -> Option<ThreadId> {
        let prev = self.receivers.push_back(thread);
        self.state = EndpointState::Recv;
        prev
    }

    pub(crate) fn dequeue_sender_head(&mut self, next: Option<ThreadId>) -> Option<ThreadId> {
        let thread = self.senders.pop_front(next)?;
        self.refresh_state_after_queue_mutation();
        Some(thread)
    }

    pub(crate) fn dequeue_receiver_head(&mut self, next: Option<ThreadId>) -> Option<ThreadId> {
        let thread = self.receivers.pop_front(next)?;
        self.refresh_state_after_queue_mutation();
        Some(thread)
    }

    pub(crate) fn unlink_sender(
        &mut self,
        thread: ThreadId,
        prev: Option<ThreadId>,
        next: Option<ThreadId>,
    ) -> bool {
        let removed = self.senders.unlink(thread, prev, next);
        if removed {
            self.refresh_state_after_queue_mutation();
        }
        removed
    }

    pub(crate) fn unlink_receiver(
        &mut self,
        thread: ThreadId,
        prev: Option<ThreadId>,
        next: Option<ThreadId>,
    ) -> bool {
        let removed = self.receivers.unlink(thread, prev, next);
        if removed {
            self.refresh_state_after_queue_mutation();
        }
        removed
    }

    pub fn cancel_all(&mut self) {
        self.senders.clear();
        self.receivers.clear();
        self.state = EndpointState::Idle;
    }

    pub fn cancel_thread(&mut self, thread: ThreadId) -> bool {
        let sender_count = self.senders.len();
        let receiver_count = self.receivers.len();
        self.unlink_sender(thread, None, None);
        self.unlink_receiver(thread, None, None);
        sender_count != self.senders.len() || receiver_count != self.receivers.len()
    }

    fn refresh_state_after_queue_mutation(&mut self) {
        self.state = if !self.senders.is_empty() {
            EndpointState::Send
        } else if !self.receivers.is_empty() {
            EndpointState::Recv
        } else {
            EndpointState::Idle
        };
    }
}

impl EndpointWaitQueue {
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

fn reply_request_for(message: IpcMessage) -> Option<ReplyRequest> {
    message.mode.is_call().then_some(ReplyRequest {
        caller: message.sender,
        caller_cpu: message.sender_cpu,
        sender_can_reply: message.can_grant || message.can_grant_reply,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    #[test]
    fn empty_endpoint_nonblocking_actions_preserve_idle_state() {
        // Goal: nonblocking Endpoint actions do not create wait queue links.
        // Scope: local Endpoint state before ThreadTable transaction ownership.
        // Semantics: without a peer, nonblocking send/receive leave the endpoint idle.
        let mut endpoint = Endpoint::new();

        assert_eq!(
            endpoint.send(
                thread(1),
                cpu(0),
                7,
                IpcSendOptions::send(false, false, false),
                IpcPayload::empty(),
            ),
            IpcAction::SendIgnored {
                thread: thread(1),
                cpu: cpu(0),
            }
        );
        assert_eq!(
            endpoint.recv(thread(2), cpu(1), IpcReceiveOptions::new(false, false)),
            IpcAction::NonblockingReceiveFailed {
                thread: thread(2),
                cpu: cpu(1),
            }
        );
        assert_eq!(endpoint.state(), EndpointState::Idle);
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.queued_receivers(), 0);
    }

    #[test]
    fn endpoint_wait_queue_anchor_tracks_head_tail_and_count() {
        // Goal: Endpoint owns only wait queue anchors, while TCB owns links.
        // Scope: local head/tail/count mutation helpers used by ThreadTable transactions.
        // Semantics: enqueue, dequeue, and unlink keep endpoint queue anchors coherent.
        let mut endpoint = Endpoint::new();

        assert_eq!(endpoint.enqueue_sender(thread(1)), None);
        assert_eq!(endpoint.enqueue_sender(thread(2)), Some(thread(1)));
        assert_eq!(endpoint.sender_head(), Some(thread(1)));
        assert_eq!(endpoint.queued_senders(), 2);
        assert_eq!(endpoint.state(), EndpointState::Send);

        assert_eq!(
            endpoint.dequeue_sender_head(Some(thread(2))),
            Some(thread(1))
        );
        assert_eq!(endpoint.sender_head(), Some(thread(2)));
        assert_eq!(endpoint.queued_senders(), 1);

        assert!(endpoint.unlink_sender(thread(2), None, None));
        assert_eq!(endpoint.sender_head(), None);
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.state(), EndpointState::Idle);
    }

    #[test]
    fn reply_request_reports_sender_reply_authority_only() {
        // Goal: call metadata reports whether the sender can receive a reply.
        // Scope: local reply request construction independent of receiver grant.
        // Semantics: grant-reply authority controls reply_request creation.
        let message = IpcMessage::new_for_blocked_sender(
            thread(1),
            cpu(0),
            7,
            false,
            true,
            IpcSendMode::Call,
            IpcPayload::empty(),
        );

        assert_eq!(
            reply_request_for(message),
            Some(ReplyRequest {
                caller: thread(1),
                caller_cpu: cpu(0),
                sender_can_reply: true,
            })
        );
    }
}
