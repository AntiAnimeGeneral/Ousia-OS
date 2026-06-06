use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

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
    pub peer: Option<ObjectId>,
    pub queued_messages: usize,
    pub max_messages: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventObject {
    pub state: EventState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryObject {
    pub size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressSpaceObject {
    pub mapping_count: usize,
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
        self.create_payload(match kind {
            ObjectKind::Process => ObjectPayload::Process(ProcessObject),
            ObjectKind::ChannelEndpoint => ObjectPayload::ChannelEndpoint(ChannelEndpointObject {
                peer: None,
                queued_messages: 0,
                max_messages: 0,
            }),
            ObjectKind::Event => ObjectPayload::Event(EventObject {
                state: EventState::Unsignaled,
            }),
            ObjectKind::MemoryObject => ObjectPayload::MemoryObject(MemoryObject { size_bytes: 0 }),
            ObjectKind::AddressSpace => {
                ObjectPayload::AddressSpace(AddressSpaceObject { mapping_count: 0 })
            }
            ObjectKind::Thread => ObjectPayload::Thread(ThreadObject {
                lifecycle: ThreadLifecycle::Initial,
            }),
        })
    }

    pub fn create_memory_object(&mut self, size_bytes: u64) -> KernelResult<ObjectSnapshot> {
        self.create_payload(ObjectPayload::MemoryObject(MemoryObject { size_bytes }))
    }

    pub fn create_channel_endpoint(&mut self, max_messages: usize) -> KernelResult<ObjectSnapshot> {
        self.create_payload(ObjectPayload::ChannelEndpoint(ChannelEndpointObject {
            peer: None,
            queued_messages: 0,
            max_messages,
        }))
    }

    fn create_payload(&mut self, payload: ObjectPayload) -> KernelResult<ObjectSnapshot> {
        let index = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        let id = ObjectId::new(index as u64);
        let generation = self.generations[index];
        let entry = ObjectEntry {
            id,
            payload,
            generation,
            state: ObjectState::Live,
            handle_count: 0,
        };
        self.entries[index] = Some(entry);
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
