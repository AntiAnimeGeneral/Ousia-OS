use alloc::collections::VecDeque;

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
pub struct IpcPayload {
    words: [u64; MAX_IPC_WORDS],
    len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndpointWaiter {
    thread: ThreadId,
    cpu: CpuId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcMessage {
    sender: ThreadId,
    sender_cpu: CpuId,
    badge: u64,
    payload: IpcPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcAction {
    SenderBlocked {
        thread: ThreadId,
        cpu: CpuId,
    },
    ReceiverBlocked {
        thread: ThreadId,
        cpu: CpuId,
    },
    Delivered {
        receiver: ThreadId,
        receiver_cpu: CpuId,
        message: IpcMessage,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcError {
    TooManyMessageWords { requested: usize, limit: usize },
}

#[derive(Debug, Default)]
pub struct Endpoint {
    senders: VecDeque<IpcMessage>,
    receivers: VecDeque<EndpointWaiter>,
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

    pub const fn payload(self) -> IpcPayload {
        self.payload
    }
}

impl Endpoint {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn send(
        &mut self,
        sender: ThreadId,
        sender_cpu: CpuId,
        badge: u64,
        payload: IpcPayload,
    ) -> IpcAction {
        let message = IpcMessage {
            sender,
            sender_cpu,
            badge,
            payload,
        };

        if let Some(receiver) = self.receivers.pop_front() {
            return IpcAction::Delivered {
                receiver: receiver.thread,
                receiver_cpu: receiver.cpu,
                message,
            };
        }

        self.senders.push_back(message);
        IpcAction::SenderBlocked {
            thread: sender,
            cpu: sender_cpu,
        }
    }

    pub fn recv(&mut self, receiver: ThreadId, receiver_cpu: CpuId) -> IpcAction {
        if let Some(message) = self.senders.pop_front() {
            return IpcAction::Delivered {
                receiver,
                receiver_cpu,
                message,
            };
        }

        self.receivers.push_back(EndpointWaiter {
            thread: receiver,
            cpu: receiver_cpu,
        });
        IpcAction::ReceiverBlocked {
            thread: receiver,
            cpu: receiver_cpu,
        }
    }

    pub fn queued_senders(&self) -> usize {
        self.senders.len()
    }

    pub fn queued_receivers(&self) -> usize {
        self.receivers.len()
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
        let action = endpoint.send(thread(1), cpu(0), 7, IpcPayload::new(&[10]).unwrap());

        assert_eq!(
            action,
            IpcAction::SenderBlocked {
                thread: thread(1),
                cpu: cpu(0),
            }
        );
        assert_eq!(endpoint.queued_senders(), 1);
        assert_eq!(endpoint.queued_receivers(), 0);
    }

    #[test]
    fn recv_delivers_oldest_waiting_sender() {
        let mut endpoint = Endpoint::new();
        endpoint.send(thread(1), cpu(0), 7, IpcPayload::new(&[10]).unwrap());
        endpoint.send(thread(2), cpu(1), 8, IpcPayload::new(&[20]).unwrap());

        assert_eq!(
            endpoint.recv(thread(3), cpu(2)),
            IpcAction::Delivered {
                receiver: thread(3),
                receiver_cpu: cpu(2),
                message: IpcMessage {
                    sender: thread(1),
                    sender_cpu: cpu(0),
                    badge: 7,
                    payload: IpcPayload::new(&[10]).unwrap(),
                },
            }
        );
        assert_eq!(endpoint.queued_senders(), 1);
    }

    #[test]
    fn recv_blocks_when_no_sender_waits() {
        let mut endpoint = Endpoint::new();
        let action = endpoint.recv(thread(3), cpu(2));

        assert_eq!(
            action,
            IpcAction::ReceiverBlocked {
                thread: thread(3),
                cpu: cpu(2),
            }
        );
        assert_eq!(endpoint.queued_senders(), 0);
        assert_eq!(endpoint.queued_receivers(), 1);
    }

    #[test]
    fn send_delivers_to_oldest_waiting_receiver() {
        let mut endpoint = Endpoint::new();
        endpoint.recv(thread(3), cpu(2));
        endpoint.recv(thread(4), cpu(3));

        assert_eq!(
            endpoint.send(thread(1), cpu(0), 7, IpcPayload::new(&[10]).unwrap()),
            IpcAction::Delivered {
                receiver: thread(3),
                receiver_cpu: cpu(2),
                message: IpcMessage {
                    sender: thread(1),
                    sender_cpu: cpu(0),
                    badge: 7,
                    payload: IpcPayload::new(&[10]).unwrap(),
                },
            }
        );
        assert_eq!(endpoint.queued_receivers(), 1);
    }
}
