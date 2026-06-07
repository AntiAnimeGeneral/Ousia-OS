use alloc::vec::Vec;

use crate::{
    error::{KernelError, KernelResult},
    handle::HandleRights,
    vm::{AddressSpaceObject, MappingPolicy, MemoryObject, VmMapDescriptor},
};

pub const MAX_CHANNEL_MESSAGES: usize = 4;
pub const MAX_CHANNEL_MESSAGE_BYTES: usize = 64;
pub const MAX_CHANNEL_MESSAGE_HANDLES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ObjectId(u64);

impl ObjectId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ObjectGeneration(u64);

impl ObjectGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn raw(self) -> u64 {
        self.0
    }

    fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("object generation exhausted before object id reuse"),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectKind {
    Process,
    ChannelEndpoint,
    Event,
    MemoryObject,
    AddressSpace,
    Thread,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadLifecycle {
    Initial,
    Runnable,
    Blocked,
    Exited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventState {
    Unsignaled,
    Signaled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessObject;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelEndpointObject {
    pub peer: Option<ObjectRef>,
    pub queued_messages: usize,
    pub max_messages: usize,
    pub peer_closed: bool,
    messages: [Option<ChannelMessage>; MAX_CHANNEL_MESSAGES],
}

impl ChannelEndpointObject {
    fn new(max_messages: usize) -> KernelResult<Self> {
        if max_messages > MAX_CHANNEL_MESSAGES {
            return Err(KernelError::NoCapacity);
        }
        Ok(Self {
            peer: None,
            queued_messages: 0,
            max_messages,
            peer_closed: false,
            messages: [None; MAX_CHANNEL_MESSAGES],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectRef {
    pub id: ObjectId,
    pub generation: ObjectGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelMessage {
    pub bytes: [u8; MAX_CHANNEL_MESSAGE_BYTES],
    pub byte_len: usize,
    pub handles: [Option<crate::handle::HandleTableEntry>; MAX_CHANNEL_MESSAGE_HANDLES],
    pub handle_count: usize,
}

impl ChannelMessage {
    pub fn new(bytes: &[u8], handles: &[crate::handle::HandleTableEntry]) -> KernelResult<Self> {
        if bytes.len() > MAX_CHANNEL_MESSAGE_BYTES || handles.len() > MAX_CHANNEL_MESSAGE_HANDLES {
            return Err(KernelError::NoCapacity);
        }

        let mut message = Self {
            bytes: [0; MAX_CHANNEL_MESSAGE_BYTES],
            byte_len: bytes.len(),
            handles: [None; MAX_CHANNEL_MESSAGE_HANDLES],
            handle_count: handles.len(),
        };
        message.bytes[..bytes.len()].copy_from_slice(bytes);
        for (index, handle) in handles.iter().copied().enumerate() {
            message.handles[index] = Some(handle);
        }
        Ok(message)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.byte_len]
    }

    pub fn handle_entries(&self) -> impl Iterator<Item = crate::handle::HandleTableEntry> + '_ {
        self.handles
            .iter()
            .take(self.handle_count)
            .filter_map(|entry| *entry)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventObject {
    pub state: EventState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadObject {
    pub lifecycle: ThreadLifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectPayload {
    Process(ProcessObject),
    ChannelEndpoint(ChannelEndpointObject),
    Event(EventObject),
    MemoryObject(MemoryObject),
    AddressSpace(AddressSpaceObject),
    Thread(ThreadObject),
}

impl ObjectPayload {
    pub const fn kind(self) -> ObjectKind {
        match self {
            Self::Process(_) => ObjectKind::Process,
            Self::ChannelEndpoint(_) => ObjectKind::ChannelEndpoint,
            Self::Event(_) => ObjectKind::Event,
            Self::MemoryObject(_) => ObjectKind::MemoryObject,
            Self::AddressSpace(_) => ObjectKind::AddressSpace,
            Self::Thread(_) => ObjectKind::Thread,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectState {
    Live,
    Dead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectEntry {
    pub id: ObjectId,
    pub payload: ObjectPayload,
    pub generation: ObjectGeneration,
    pub state: ObjectState,
    pub handle_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectSnapshot {
    pub id: ObjectId,
    pub payload: ObjectPayload,
    pub generation: ObjectGeneration,
    pub state: ObjectState,
    pub handle_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObjectEntryReservation {
    index: usize,
    generation: ObjectGeneration,
}

impl ObjectSnapshot {
    pub const fn kind(self) -> ObjectKind {
        self.payload.kind()
    }
}

pub struct ObjectManager {
    entries: Vec<Option<ObjectEntry>>,
    generations: Vec<ObjectGeneration>,
    capacity: usize,
}

impl ObjectManager {
    pub fn with_capacity(capacity: usize) -> KernelResult<Self> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(capacity)
            .map_err(|_| KernelError::NoMemory)?;
        entries.resize_with(capacity, || None);

        let mut generations = Vec::new();
        generations
            .try_reserve_exact(capacity)
            .map_err(|_| KernelError::NoMemory)?;
        generations.resize(capacity, ObjectGeneration::INITIAL);

        Ok(Self {
            entries,
            generations,
            capacity,
        })
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn live_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_some()).count()
    }

    pub fn create(&mut self, kind: ObjectKind) -> KernelResult<ObjectSnapshot> {
        self.create_payload(Self::payload_for_kind(kind))
    }

    pub(crate) fn payload_for_kind(kind: ObjectKind) -> ObjectPayload {
        match kind {
            ObjectKind::Process => ObjectPayload::Process(ProcessObject),
            ObjectKind::ChannelEndpoint => ObjectPayload::ChannelEndpoint(ChannelEndpointObject {
                peer: None,
                queued_messages: 0,
                max_messages: 0,
                peer_closed: false,
                messages: [None; MAX_CHANNEL_MESSAGES],
            }),
            ObjectKind::Event => ObjectPayload::Event(EventObject {
                state: EventState::Unsignaled,
            }),
            ObjectKind::MemoryObject => ObjectPayload::MemoryObject(MemoryObject::anonymous(
                0,
                MappingPolicy::new(
                    HandleRights::READ | HandleRights::WRITE | HandleRights::EXECUTE,
                ),
            )),
            ObjectKind::AddressSpace => ObjectPayload::AddressSpace(AddressSpaceObject::new()),
            ObjectKind::Thread => ObjectPayload::Thread(ThreadObject {
                lifecycle: ThreadLifecycle::Initial,
            }),
        }
    }

    pub fn create_memory_object(&mut self, size_bytes: u64) -> KernelResult<ObjectSnapshot> {
        self.create_payload(ObjectPayload::MemoryObject(MemoryObject::anonymous(
            size_bytes,
            MappingPolicy::new(HandleRights::READ | HandleRights::WRITE | HandleRights::EXECUTE),
        )))
    }

    pub fn create_channel_endpoint(&mut self, max_messages: usize) -> KernelResult<ObjectSnapshot> {
        self.create_payload(ObjectPayload::ChannelEndpoint(ChannelEndpointObject::new(
            max_messages,
        )?))
    }

    pub fn create_channel_pair(
        &mut self,
        max_messages: usize,
    ) -> KernelResult<(ObjectSnapshot, ObjectSnapshot)> {
        let first = self.create_channel_endpoint(max_messages)?;
        let second = match self.create_channel_endpoint(max_messages) {
            Ok(second) => second,
            Err(error) => {
                let _ = self.destroy(first.id, first.generation);
                return Err(error);
            }
        };
        if let Err(error) = self.link_channel_pair(first, second) {
            let _ = self.destroy(first.id, first.generation);
            let _ = self.destroy(second.id, second.generation);
            return Err(error);
        }
        Ok((self.snapshot(first.id)?, self.snapshot(second.id)?))
    }

    fn link_channel_pair(
        &mut self,
        first: ObjectSnapshot,
        second: ObjectSnapshot,
    ) -> KernelResult<()> {
        let first_payload = self.live_payload_mut(first.id, first.generation)?;
        let ObjectPayload::ChannelEndpoint(first_endpoint) = first_payload else {
            return Err(KernelError::WrongObjectType);
        };
        first_endpoint.peer = Some(ObjectRef {
            id: second.id,
            generation: second.generation,
        });

        let second_payload = self.live_payload_mut(second.id, second.generation)?;
        let ObjectPayload::ChannelEndpoint(second_endpoint) = second_payload else {
            return Err(KernelError::WrongObjectType);
        };
        second_endpoint.peer = Some(ObjectRef {
            id: first.id,
            generation: first.generation,
        });
        Ok(())
    }

    fn create_payload(&mut self, payload: ObjectPayload) -> KernelResult<ObjectSnapshot> {
        let reservation = self.reserve_entry()?;
        self.commit_reserved(reservation, payload)
    }

    pub(crate) fn reserve_entry(&self) -> KernelResult<ObjectEntryReservation> {
        let index = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        Ok(ObjectEntryReservation {
            index,
            generation: self.generations[index],
        })
    }

    pub(crate) fn commit_reserved(
        &mut self,
        reservation: ObjectEntryReservation,
        payload: ObjectPayload,
    ) -> KernelResult<ObjectSnapshot> {
        if reservation.index >= self.capacity {
            return Err(KernelError::InvalidHandle);
        }
        if self.entries[reservation.index].is_some()
            || self.generations[reservation.index] != reservation.generation
        {
            return Err(KernelError::NoCapacity);
        }

        let id = ObjectId::new(reservation.index as u64);
        let generation = reservation.generation;
        let entry = ObjectEntry {
            id,
            payload,
            generation,
            state: ObjectState::Live,
            handle_count: 0,
        };
        self.entries[reservation.index] = Some(entry);
        Ok(ObjectSnapshot::from(entry))
    }

    pub fn snapshot(&self, id: ObjectId) -> KernelResult<ObjectSnapshot> {
        let entry = self.entry(id)?;
        Ok(ObjectSnapshot::from(*entry))
    }

    pub fn validate(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
        kind: ObjectKind,
    ) -> KernelResult<ObjectSnapshot> {
        let entry = self.entry(id)?;
        if entry.state == ObjectState::Dead {
            return Err(KernelError::DeadObject);
        }
        if entry.generation != generation {
            return Err(KernelError::StaleHandle);
        }
        if entry.payload.kind() != kind {
            return Err(KernelError::WrongObjectType);
        }
        Ok(ObjectSnapshot::from(*entry))
    }

    pub fn event(&self, id: ObjectId, generation: ObjectGeneration) -> KernelResult<EventObject> {
        match self.live_payload(id, generation)? {
            ObjectPayload::Event(event) => Ok(event),
            _ => Err(KernelError::WrongObjectType),
        }
    }

    pub fn signal_event(&mut self, id: ObjectId, generation: ObjectGeneration) -> KernelResult<()> {
        let payload = self.live_payload_mut(id, generation)?;
        let ObjectPayload::Event(event) = payload else {
            return Err(KernelError::WrongObjectType);
        };
        event.state = EventState::Signaled;
        Ok(())
    }

    pub fn clear_event(&mut self, id: ObjectId, generation: ObjectGeneration) -> KernelResult<()> {
        let payload = self.live_payload_mut(id, generation)?;
        let ObjectPayload::Event(event) = payload else {
            return Err(KernelError::WrongObjectType);
        };
        event.state = EventState::Unsignaled;
        Ok(())
    }

    pub fn memory_object(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<MemoryObject> {
        match self.live_payload(id, generation)? {
            ObjectPayload::MemoryObject(memory) => Ok(memory),
            _ => Err(KernelError::WrongObjectType),
        }
    }

    pub fn address_space(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<AddressSpaceObject> {
        match self.live_payload(id, generation)? {
            ObjectPayload::AddressSpace(address_space) => Ok(address_space),
            _ => Err(KernelError::WrongObjectType),
        }
    }

    pub fn map_memory_object(
        &mut self,
        address_space: ObjectRef,
        memory: ObjectRef,
        base: u64,
        size_bytes: u64,
        memory_offset: u64,
        rights: HandleRights,
    ) -> KernelResult<()> {
        let memory_object = self.memory_object(memory.id, memory.generation)?;
        let descriptor = VmMapDescriptor {
            base,
            size_bytes,
            memory_offset,
            rights,
        };

        let payload = self.live_payload_mut(address_space.id, address_space.generation)?;
        let ObjectPayload::AddressSpace(address_space) = payload else {
            return Err(KernelError::WrongObjectType);
        };
        let reservation = address_space.prepare_map(memory, memory_object, descriptor)?;
        reservation.commit();
        Ok(())
    }

    pub fn unmap_address_range(
        &mut self,
        address_space: ObjectRef,
        base: u64,
        size_bytes: u64,
    ) -> KernelResult<()> {
        let payload = self.live_payload_mut(address_space.id, address_space.generation)?;
        let ObjectPayload::AddressSpace(address_space) = payload else {
            return Err(KernelError::WrongObjectType);
        };
        address_space.unmap_exact(base, size_bytes)
    }

    pub fn channel_endpoint(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<ChannelEndpointObject> {
        match self.live_payload(id, generation)? {
            ObjectPayload::ChannelEndpoint(endpoint) => Ok(endpoint),
            _ => Err(KernelError::WrongObjectType),
        }
    }

    pub fn channel_peer(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<ObjectSnapshot> {
        let endpoint = self.channel_endpoint(id, generation)?;
        if endpoint.peer_closed {
            return Err(KernelError::PeerClosed);
        }
        let peer = endpoint.peer.ok_or(KernelError::PeerClosed)?;
        self.validate(peer.id, peer.generation, ObjectKind::ChannelEndpoint)
    }

    pub fn ensure_channel_can_enqueue(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<()> {
        let endpoint = self.channel_endpoint(id, generation)?;
        if endpoint.peer_closed {
            return Err(KernelError::PeerClosed);
        }
        if endpoint.queued_messages == endpoint.max_messages {
            return Err(KernelError::NoCapacity);
        }
        Ok(())
    }

    pub fn enqueue_channel_message(
        &mut self,
        id: ObjectId,
        generation: ObjectGeneration,
        message: ChannelMessage,
    ) -> KernelResult<()> {
        let payload = self.live_payload_mut(id, generation)?;
        let ObjectPayload::ChannelEndpoint(endpoint) = payload else {
            return Err(KernelError::WrongObjectType);
        };
        if endpoint.peer_closed {
            return Err(KernelError::PeerClosed);
        }
        if endpoint.queued_messages == endpoint.max_messages {
            return Err(KernelError::NoCapacity);
        }
        let index = endpoint
            .messages
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        endpoint.messages[index] = Some(message);
        endpoint.queued_messages += 1;
        Ok(())
    }

    pub fn next_channel_message_handle_count(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<usize> {
        let endpoint = self.channel_endpoint(id, generation)?;
        if let Some(message) = endpoint.messages.iter().flatten().next() {
            Ok(message.handle_count)
        } else if endpoint.peer_closed {
            Err(KernelError::PeerClosed)
        } else {
            Err(KernelError::WouldBlock)
        }
    }

    pub fn dequeue_channel_message(
        &mut self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<ChannelMessage> {
        let payload = self.live_payload_mut(id, generation)?;
        let ObjectPayload::ChannelEndpoint(endpoint) = payload else {
            return Err(KernelError::WrongObjectType);
        };
        let Some(index) = endpoint.messages.iter().position(Option::is_some) else {
            return if endpoint.peer_closed {
                Err(KernelError::PeerClosed)
            } else {
                Err(KernelError::WouldBlock)
            };
        };
        endpoint.queued_messages -= 1;
        Ok(endpoint.messages[index]
            .take()
            .expect("message slot was selected as occupied"))
    }

    pub fn add_handle(&mut self, id: ObjectId, generation: ObjectGeneration) -> KernelResult<()> {
        let entry = self.entry_mut(id)?;
        if entry.state == ObjectState::Dead {
            return Err(KernelError::DeadObject);
        }
        if entry.generation != generation {
            return Err(KernelError::StaleHandle);
        }
        entry.handle_count = entry.handle_count.saturating_add(1);
        Ok(())
    }

    pub fn remove_handle(
        &mut self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<()> {
        let entry = self.entry_mut(id)?;
        if entry.generation != generation {
            return Err(KernelError::StaleHandle);
        }
        entry.handle_count = entry.handle_count.saturating_sub(1);
        Ok(())
    }

    pub fn destroy(&mut self, id: ObjectId, generation: ObjectGeneration) -> KernelResult<()> {
        let index = self.index(id)?;
        let Some(entry) = self.entries[index] else {
            return Err(KernelError::DeadObject);
        };
        if entry.generation != generation {
            return Err(KernelError::StaleHandle);
        }

        if let ObjectPayload::ChannelEndpoint(endpoint) = entry.payload {
            if endpoint.queued_messages > 0 {
                return Err(KernelError::WouldBlock);
            }
            if let Some(peer) = endpoint.peer {
                if let Ok(peer_entry) = self.entry_mut(peer.id) {
                    if peer_entry.generation == peer.generation {
                        if let ObjectPayload::ChannelEndpoint(peer_endpoint) =
                            &mut peer_entry.payload
                        {
                            peer_endpoint.peer_closed = true;
                        }
                    }
                }
            }
        }

        self.entries[index] = None;
        self.generations[index] = generation.next();
        Ok(())
    }

    fn entry(&self, id: ObjectId) -> KernelResult<&ObjectEntry> {
        let index = self.index(id)?;
        self.entries[index].as_ref().ok_or(KernelError::DeadObject)
    }

    fn entry_mut(&mut self, id: ObjectId) -> KernelResult<&mut ObjectEntry> {
        let index = self.index(id)?;
        self.entries[index].as_mut().ok_or(KernelError::DeadObject)
    }

    fn live_payload(
        &self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<ObjectPayload> {
        let entry = self.entry(id)?;
        if entry.state == ObjectState::Dead {
            return Err(KernelError::DeadObject);
        }
        if entry.generation != generation {
            return Err(KernelError::StaleHandle);
        }
        Ok(entry.payload)
    }

    fn live_payload_mut(
        &mut self,
        id: ObjectId,
        generation: ObjectGeneration,
    ) -> KernelResult<&mut ObjectPayload> {
        let entry = self.entry_mut(id)?;
        if entry.state == ObjectState::Dead {
            return Err(KernelError::DeadObject);
        }
        if entry.generation != generation {
            return Err(KernelError::StaleHandle);
        }
        Ok(&mut entry.payload)
    }

    fn index(&self, id: ObjectId) -> KernelResult<usize> {
        let index = id.raw() as usize;
        if index >= self.capacity {
            return Err(KernelError::InvalidHandle);
        }
        Ok(index)
    }
}

impl From<ObjectEntry> for ObjectSnapshot {
    fn from(entry: ObjectEntry) -> Self {
        Self {
            id: entry.id,
            payload: entry.payload,
            generation: entry.generation,
            state: entry.state,
            handle_count: entry.handle_count,
        }
    }
}
