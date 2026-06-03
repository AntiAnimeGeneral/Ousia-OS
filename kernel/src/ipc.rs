use alloc::vec::Vec;

pub use crate::message::{IpcError, IpcPayload, MAX_IPC_WORDS};
use crate::tcb::{CpuId, ThreadId};

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
    senders: EndpointQueue<QueuedSender>,
    receivers: EndpointQueue<QueuedReceiver>,
}

#[derive(Debug)]
struct EndpointQueue<T> {
    entries: Vec<T>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointCancellation {
    pub senders: Vec<QueuedSender>,
    pub receivers: Vec<QueuedReceiver>,
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
    pub fn new() -> Self {
        Self {
            state: EndpointState::Idle,
            senders: EndpointQueue::new(),
            receivers: EndpointQueue::new(),
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
        let message = IpcMessage {
            sender,
            sender_cpu,
            badge,
            can_grant: options.can_grant,
            can_grant_reply: options.can_grant_reply,
            mode: options.mode(),
            payload,
        };

        match self.state {
            EndpointState::Idle | EndpointState::Send => {
                if !options.is_blocking() {
                    return IpcAction::SendIgnored {
                        thread: sender,
                        cpu: sender_cpu,
                    };
                }

                self.senders
                    .push_back(QueuedSender::new(sender, sender_cpu));
                self.state = EndpointState::Send;
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
            EndpointState::Recv => {
                let receiver = self
                    .receivers
                    .pop_front()
                    .expect("Recv endpoint state must have a waiting receiver");
                if self.receivers.is_empty() {
                    self.state = EndpointState::Idle;
                }
                IpcAction::DeliveredToReceiver {
                    receiver: receiver.thread,
                    receiver_cpu: receiver.cpu,
                    reply_request: reply_request_for(message),
                    message,
                }
            }
        }
    }

    pub fn recv(
        &mut self,
        receiver: ThreadId,
        receiver_cpu: CpuId,
        options: IpcReceiveOptions,
    ) -> IpcAction {
        match self.state {
            EndpointState::Idle | EndpointState::Recv => {
                if !options.blocking {
                    return IpcAction::NonblockingReceiveFailed {
                        thread: receiver,
                        cpu: receiver_cpu,
                    };
                }

                self.receivers
                    .push_back(QueuedReceiver::new(receiver, receiver_cpu));
                self.state = EndpointState::Recv;
                IpcAction::ReceiverBlocked {
                    thread: receiver,
                    cpu: receiver_cpu,
                    can_grant: options.can_grant,
                }
            }
            EndpointState::Send => {
                let sender = self
                    .senders
                    .pop_front()
                    .expect("Send endpoint state must have a waiting sender");
                if self.senders.is_empty() {
                    self.state = EndpointState::Idle;
                }
                IpcAction::SenderReleased {
                    receiver,
                    receiver_cpu,
                    receiver_can_grant: options.can_grant,
                    sender,
                }
            }
        }
    }

    pub const fn state(&self) -> EndpointState {
        self.state
    }

    pub fn queued_senders(&self) -> usize {
        self.senders.len()
    }

    pub fn queued_receivers(&self) -> usize {
        self.receivers.len()
    }

    pub fn next_receiver(&self) -> Option<QueuedReceiver> {
        self.receivers.front().copied()
    }

    pub fn next_sender(&self) -> Option<QueuedSender> {
        self.senders.front().copied()
    }

    pub fn cancel_all(&mut self) -> EndpointCancellation {
        let senders = self.senders.drain_all().collect();
        let receivers = self.receivers.drain_all().collect();
        self.state = EndpointState::Idle;

        EndpointCancellation { senders, receivers }
    }

    pub fn cancel_thread(&mut self, thread: ThreadId) -> bool {
        let sender_count = self.senders.len();
        let receiver_count = self.receivers.len();
        self.senders.retain(|sender| sender.thread != thread);
        self.receivers.retain(|waiter| waiter.thread != thread);

        if self.senders.is_empty() && self.receivers.is_empty() {
            self.state = EndpointState::Idle;
        }

        sender_count != self.senders.len() || receiver_count != self.receivers.len()
    }
}

impl<T> EndpointQueue<T> {
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
    use rstest::rstest;

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    fn blocking_send() -> IpcSendOptions {
        IpcSendOptions::send(true, true, false)
    }

    fn blocking_call() -> IpcSendOptions {
        IpcSendOptions::call(true, true)
    }

    fn nonblocking_send() -> IpcSendOptions {
        IpcSendOptions::send(false, false, false)
    }

    fn blocking_recv() -> IpcReceiveOptions {
        IpcReceiveOptions::new(true, true)
    }

    fn nonblocking_recv() -> IpcReceiveOptions {
        IpcReceiveOptions::new(false, false)
    }

    #[rstest]
    #[case::blocking_send_queues_sender_when_no_receiver_waits(
        |endpoint: &mut Endpoint| endpoint.send(
            thread(1),
            cpu(0),
            7,
            blocking_call(),
            IpcPayload::new(&[10]).unwrap(),
        ),
        IpcAction::SenderBlocked {
            thread: thread(1),
            cpu: cpu(0),
            badge: 7,
            can_grant: true,
            can_grant_reply: true,
            is_call: true,
            payload: IpcPayload::new(&[10]).unwrap(),
        },
        EndpointState::Send,
        1,
        0
    )]
    #[case::blocking_receive_queues_receiver_when_no_sender_waits(
        |endpoint: &mut Endpoint| endpoint.recv(thread(3), cpu(2), blocking_recv()),
        IpcAction::ReceiverBlocked { thread: thread(3), cpu: cpu(2), can_grant: true },
        EndpointState::Recv,
        0,
        1
    )]
    #[case::nonblocking_send_leaves_idle_endpoint_unqueued(
        |endpoint: &mut Endpoint| endpoint.send(
            thread(1),
            cpu(0),
            7,
            nonblocking_send(),
            IpcPayload::new(&[10]).unwrap(),
        ),
        IpcAction::SendIgnored { thread: thread(1), cpu: cpu(0) },
        EndpointState::Idle,
        0,
        0
    )]
    #[case::nonblocking_receive_leaves_idle_endpoint_unqueued(
        |endpoint: &mut Endpoint| endpoint.recv(thread(1), cpu(0), nonblocking_recv()),
        IpcAction::NonblockingReceiveFailed { thread: thread(1), cpu: cpu(0) },
        EndpointState::Idle,
        0,
        0
    )]
    fn empty_endpoint_blocking_and_nonblocking_actions_preserve_queue_contract(
        #[case] run: fn(&mut Endpoint) -> IpcAction,
        #[case] expected: IpcAction,
        #[case] expected_state: EndpointState,
        #[case] expected_senders: usize,
        #[case] expected_receivers: usize,
    ) {
        // Goal: empty Endpoint send/receive operations follow blocking and nonblocking contracts.
        // Scope: Endpoint local state without cross-object ThreadTable or Scheduler effects.
        // Semantics: blocking operations enqueue the caller; nonblocking operations leave queues idle.
        let mut endpoint = Endpoint::new();

        assert_eq!(run(&mut endpoint), expected);
        assert_eq!(endpoint.state(), expected_state);
        assert_eq!(endpoint.queued_senders(), expected_senders);
        assert_eq!(endpoint.queued_receivers(), expected_receivers);
    }

    #[test]
    fn recv_delivers_oldest_waiting_sender() {
        let mut endpoint = Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            blocking_call(),
            IpcPayload::new(&[10]).unwrap(),
        );
        endpoint.send(
            thread(2),
            cpu(1),
            8,
            blocking_send(),
            IpcPayload::new(&[20]).unwrap(),
        );

        assert_eq!(
            endpoint.recv(thread(3), cpu(2), blocking_recv()),
            IpcAction::SenderReleased {
                receiver: thread(3),
                receiver_cpu: cpu(2),
                receiver_can_grant: true,
                sender: QueuedSender::new(thread(1), cpu(0)),
            }
        );
        assert_eq!(endpoint.state(), EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(endpoint.queued_receivers(), 0);
    }

    #[test]
    fn send_delivers_to_oldest_waiting_receiver() {
        let mut endpoint = Endpoint::new();
        endpoint.recv(thread(3), cpu(2), blocking_recv());
        endpoint.recv(thread(4), cpu(3), blocking_recv());

        assert_eq!(
            endpoint.send(
                thread(1),
                cpu(0),
                7,
                blocking_call(),
                IpcPayload::new(&[10]).unwrap(),
            ),
            IpcAction::DeliveredToReceiver {
                receiver: thread(3),
                receiver_cpu: cpu(2),
                reply_request: Some(ReplyRequest {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    sender_can_reply: true,
                }),
                message: IpcMessage {
                    sender: thread(1),
                    sender_cpu: cpu(0),
                    badge: 7,
                    can_grant: true,
                    can_grant_reply: true,
                    mode: IpcSendMode::Call,
                    payload: IpcPayload::new(&[10]).unwrap(),
                },
            }
        );
        assert_eq!(endpoint.state(), EndpointState::Recv);
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.queued_receivers(), 1);
    }

    #[test]
    fn endpoint_returns_to_idle_after_last_waiter_is_matched() {
        let mut endpoint = Endpoint::new();
        endpoint.send(
            thread(1),
            cpu(0),
            7,
            blocking_send(),
            IpcPayload::new(&[10]).unwrap(),
        );

        let _ = endpoint.recv(thread(2), cpu(1), blocking_recv());

        assert_eq!(endpoint.state(), EndpointState::Idle);
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.queued_receivers(), 0);

        endpoint.recv(thread(3), cpu(2), blocking_recv());
        let _ = endpoint.send(
            thread(4),
            cpu(3),
            8,
            blocking_send(),
            IpcPayload::new(&[20]).unwrap(),
        );

        assert_eq!(endpoint.state(), EndpointState::Idle);
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.queued_receivers(), 0);
    }

    #[test]
    fn one_way_send_delivery_does_not_request_reply_setup() {
        let mut endpoint = Endpoint::new();

        endpoint.recv(thread(3), cpu(2), blocking_recv());

        assert_eq!(
            endpoint.send(
                thread(1),
                cpu(0),
                7,
                blocking_send(),
                IpcPayload::new(&[10]).unwrap(),
            ),
            IpcAction::DeliveredToReceiver {
                receiver: thread(3),
                receiver_cpu: cpu(2),
                reply_request: None,
                message: IpcMessage {
                    sender: thread(1),
                    sender_cpu: cpu(0),
                    badge: 7,
                    can_grant: true,
                    can_grant_reply: false,
                    mode: IpcSendMode::Send,
                    payload: IpcPayload::new(&[10]).unwrap(),
                },
            }
        );
    }

    #[test]
    fn call_reply_request_reports_sender_reply_authority_only() {
        let mut endpoint = Endpoint::new();

        endpoint.recv(thread(3), cpu(2), IpcReceiveOptions::new(true, false));

        assert_eq!(
            endpoint.send(
                thread(1),
                cpu(0),
                7,
                IpcSendOptions::call(false, true),
                IpcPayload::new(&[10]).unwrap(),
            ),
            IpcAction::DeliveredToReceiver {
                receiver: thread(3),
                receiver_cpu: cpu(2),
                reply_request: Some(ReplyRequest {
                    caller: thread(1),
                    caller_cpu: cpu(0),
                    sender_can_reply: true,
                }),
                message: IpcMessage {
                    sender: thread(1),
                    sender_cpu: cpu(0),
                    badge: 7,
                    can_grant: false,
                    can_grant_reply: true,
                    mode: IpcSendMode::Call,
                    payload: IpcPayload::new(&[10]).unwrap(),
                },
            }
        );
    }
}
