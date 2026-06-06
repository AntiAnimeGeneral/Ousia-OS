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
pub enum ObjectState {
    Live,
    Dead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectEntry {
    pub id: ObjectId,
    pub kind: ObjectKind,
    pub generation: ObjectGeneration,
    pub state: ObjectState,
    pub handle_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectSnapshot {
    pub id: ObjectId,
    pub kind: ObjectKind,
    pub generation: ObjectGeneration,
    pub state: ObjectState,
    pub handle_count: usize,
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
        let index = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        let id = ObjectId::new(index as u64);
        let generation = self.generations[index];
        let entry = ObjectEntry {
            id,
            kind,
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
        if entry.kind != kind {
            return Err(KernelError::WrongObjectType);
        }
        Ok(ObjectSnapshot::from(*entry))
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
            kind: entry.kind,
            generation: entry.generation,
            state: entry.state,
            handle_count: entry.handle_count,
        }
    }
}
