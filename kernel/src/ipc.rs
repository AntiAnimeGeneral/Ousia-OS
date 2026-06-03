use alloc::{collections::VecDeque, vec::Vec};

use crate::tcb::{CpuId, ThreadId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcPayload {
    words: [u64; MAX_IPC_WORDS],
    len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndpointWaiter {
    thread: ThreadId,
    cpu: CpuId,
    can_grant: bool,
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
pub enum IpcSendMode {
    Send,
    Call,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcSendOptions {
    pub blocking: bool,
    pub mode: IpcSendMode,
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
        receiver_can_grant: bool,
        message: IpcMessage,
        reply_request: Option<ReplyRequest>,
    },
    SenderReleased {
        receiver: ThreadId,
        receiver_cpu: CpuId,
        receiver_can_grant: bool,
        message: IpcMessage,
        reply_request: Option<ReplyRequest>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcError {
    TooManyMessageWords { requested: usize, limit: usize },
}

#[derive(Debug)]
pub struct Endpoint {
    state: EndpointState,
    senders: VecDeque<IpcMessage>,
    receivers: VecDeque<EndpointWaiter>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointCancellation {
    pub senders: Vec<EndpointWaiter>,
    pub receivers: Vec<EndpointWaiter>,
}

pub const MAX_IPC_WORDS: usize = 4;

impl IpcPayload {
    pub const fn empty() -> Self {
        Self {
            words: [0; MAX_IPC_WORDS],
            len: 0,
        }
    }

    pub fn new(words: &[u64]) -> Result<Self, IpcError> {
        if words.len() > MAX_IPC_WORDS {
            return Err(IpcError::TooManyMessageWords {
                requested: words.len(),
                limit: MAX_IPC_WORDS,
            });
        }

        let mut payload = Self::empty();
        payload.words[..words.len()].copy_from_slice(words);
        payload.len = words.len();
        Ok(payload)
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn words(&self) -> &[u64] {
        &self.words[..self.len]
    }
}

impl EndpointWaiter {
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

impl IpcMessage {
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

impl IpcSendOptions {
    pub const fn send(blocking: bool, can_grant: bool, can_grant_reply: bool) -> Self {
        Self {
            blocking,
            mode: IpcSendMode::Send,
            can_grant,
            can_grant_reply,
        }
    }

    pub const fn call(blocking: bool, can_grant: bool, can_grant_reply: bool) -> Self {
        Self {
            blocking,
            mode: IpcSendMode::Call,
            can_grant,
            can_grant_reply,
        }
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
            senders: VecDeque::new(),
            receivers: VecDeque::new(),
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
            mode: options.mode,
            payload,
        };

        match self.state {
            EndpointState::Idle | EndpointState::Send => {
                if !options.blocking {
                    return IpcAction::SendIgnored {
                        thread: sender,
                        cpu: sender_cpu,
                    };
                }

                self.senders.push_back(message);
                self.state = EndpointState::Send;
                IpcAction::SenderBlocked {
                    thread: sender,
                    cpu: sender_cpu,
                    badge,
                    can_grant: options.can_grant,
                    can_grant_reply: options.can_grant_reply,
                    is_call: options.mode.is_call(),
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
                    receiver_can_grant: receiver.can_grant,
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

                self.receivers.push_back(EndpointWaiter {
                    thread: receiver,
                    cpu: receiver_cpu,
                    can_grant: options.can_grant,
                });
                self.state = EndpointState::Recv;
                IpcAction::ReceiverBlocked {
                    thread: receiver,
                    cpu: receiver_cpu,
                    can_grant: options.can_grant,
                }
            }
            EndpointState::Send => {
                let message = self
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
                    reply_request: reply_request_for(message),
                    message,
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

    pub fn next_receiver(&self) -> Option<EndpointWaiter> {
        self.receivers.front().copied()
    }

    pub fn next_sender(&self) -> Option<IpcMessage> {
        self.senders.front().copied()
    }

    pub fn cancel_all(&mut self) -> EndpointCancellation {
        let senders = self
            .senders
            .drain(..)
            .map(|message| EndpointWaiter {
                thread: message.sender,
                cpu: message.sender_cpu,
                can_grant: false,
            })
            .collect();
        let receivers = self.receivers.drain(..).collect();
        self.state = EndpointState::Idle;

        EndpointCancellation { senders, receivers }
    }

    pub fn cancel_thread(&mut self, thread: ThreadId) -> bool {
        let sender_count = self.senders.len();
        let receiver_count = self.receivers.len();
        self.senders.retain(|message| message.sender != thread);
        self.receivers.retain(|waiter| waiter.thread != thread);

        if self.senders.is_empty() && self.receivers.is_empty() {
            self.state = EndpointState::Idle;
        }

        sender_count != self.senders.len() || receiver_count != self.receivers.len()
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
        IpcSendOptions::call(true, true, true)
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

    #[test]
    fn payload_rejects_too_many_words() {
        assert_eq!(
            IpcPayload::new(&[1, 2, 3, 4, 5]),
            Err(IpcError::TooManyMessageWords {
                requested: 5,
                limit: MAX_IPC_WORDS,
            })
        );
    }

    #[test]
    fn send_blocks_when_no_receiver_waits() {
        let mut endpoint = Endpoint::new();
        let action = endpoint.send(
            thread(1),
            cpu(0),
            7,
            blocking_call(),
            IpcPayload::new(&[10]).unwrap(),
        );

        assert_eq!(
            action,
            IpcAction::SenderBlocked {
                thread: thread(1),
                cpu: cpu(0),
                badge: 7,
                can_grant: true,
                can_grant_reply: true,
                is_call: true,
            }
        );
        assert_eq!(endpoint.state(), EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(endpoint.queued_receivers(), 0);
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
        assert_eq!(endpoint.state(), EndpointState::Send);
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(endpoint.queued_receivers(), 0);
    }

    #[test]
    fn recv_blocks_when_no_sender_waits() {
        let mut endpoint = Endpoint::new();
        let action = endpoint.recv(thread(3), cpu(2), blocking_recv());

        assert_eq!(
            action,
            IpcAction::ReceiverBlocked {
                thread: thread(3),
                cpu: cpu(2),
                can_grant: true,
            }
        );
        assert_eq!(endpoint.state(), EndpointState::Recv);
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.queued_receivers(), 1);
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
                receiver_can_grant: true,
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
                receiver_can_grant: true,
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
                IpcSendOptions::call(true, false, true),
                IpcPayload::new(&[10]).unwrap(),
            ),
            IpcAction::DeliveredToReceiver {
                receiver: thread(3),
                receiver_cpu: cpu(2),
                receiver_can_grant: false,
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

    #[test]
    fn nonblocking_send_does_not_queue_when_endpoint_is_not_receiving() {
        let mut endpoint = Endpoint::new();

        assert_eq!(
            endpoint.send(
                thread(1),
                cpu(0),
                7,
                nonblocking_send(),
                IpcPayload::new(&[10]).unwrap(),
            ),
            IpcAction::SendIgnored {
                thread: thread(1),
                cpu: cpu(0),
            }
        );
        assert_eq!(endpoint.state(), EndpointState::Idle);
        assert_eq!(endpoint.queued_senders(), 0);
    }

    #[test]
    fn nonblocking_receive_fails_without_waiting_sender() {
        let mut endpoint = Endpoint::new();

        assert_eq!(
            endpoint.recv(thread(1), cpu(0), nonblocking_recv()),
            IpcAction::NonblockingReceiveFailed {
                thread: thread(1),
                cpu: cpu(0),
            }
        );
        assert_eq!(endpoint.state(), EndpointState::Idle);
        assert_eq!(endpoint.queued_receivers(), 0);
    }
}
