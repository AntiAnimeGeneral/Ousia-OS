use alloc::vec::Vec;

use bitflags::bitflags;

use crate::{
    error::{KernelError, KernelResult},
    object::{ObjectGeneration, ObjectId, ObjectKind, ObjectManager, ObjectSnapshot},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct HandleValue(u64);

impl HandleValue {
    const INDEX_BITS: u64 = 32;
    const INDEX_MASK: u64 = (1 << Self::INDEX_BITS) - 1;

    pub const fn new(index: u64, generation: HandleGeneration) -> Self {
        Self((generation.raw() << Self::INDEX_BITS) | (index & Self::INDEX_MASK))
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn index(self) -> u64 {
        self.0 & Self::INDEX_MASK
    }

    pub const fn generation(self) -> HandleGeneration {
        HandleGeneration(self.0 >> Self::INDEX_BITS)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct HandleGeneration(u64);

impl HandleGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn raw(self) -> u64 {
        self.0
    }

    fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("handle generation exhausted before slot reuse"),
        )
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct HandleRights: u32 {
        const NONE = 0;
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
        const TRANSFER = 1 << 3;
        const DUPLICATE = 1 << 4;
        const MANAGE = 1 << 5;
        const ALL = Self::READ.bits()
            | Self::WRITE.bits()
            | Self::EXECUTE.bits()
            | Self::TRANSFER.bits()
            | Self::DUPLICATE.bits()
            | Self::MANAGE.bits();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandleTableEntry {
    pub object: ObjectId,
    pub object_generation: ObjectGeneration,
    pub entry_generation: HandleGeneration,
    pub parent: Option<HandleLineage>,
    pub kind: ObjectKind,
    pub rights: HandleRights,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandleLineage {
    pub handle: HandleValue,
    pub object: ObjectId,
    pub object_generation: ObjectGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandleView {
    pub handle: HandleValue,
    pub entry: HandleTableEntry,
    pub object: ObjectSnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HandleSlot {
    entry: Option<HandleTableEntry>,
    generation: HandleGeneration,
}

pub struct HandleTable {
    slots: Vec<HandleSlot>,
    capacity: usize,
}

impl HandleTable {
    pub fn with_capacity(capacity: usize) -> KernelResult<Self> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| KernelError::NoMemory)?;
        slots.resize(
            capacity,
            HandleSlot {
                entry: None,
                generation: HandleGeneration::INITIAL,
            },
        );
        Ok(Self { slots, capacity })
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn live_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.entry.is_some())
            .count()
    }

    pub fn install(
        &mut self,
        objects: &mut ObjectManager,
        object: ObjectSnapshot,
        rights: HandleRights,
    ) -> KernelResult<HandleValue> {
        let index = self.free_index()?;
        objects.add_handle(object.id, object.generation)?;
        let generation = self.slots[index].generation;
        self.slots[index].entry = Some(HandleTableEntry {
            object: object.id,
            object_generation: object.generation,
            entry_generation: generation,
            parent: None,
            kind: object.kind(),
            rights,
        });
        Ok(HandleValue::new(index as u64, generation))
    }

    pub fn lookup(
        &self,
        objects: &ObjectManager,
        handle: HandleValue,
        expected_kind: ObjectKind,
        required_rights: HandleRights,
    ) -> KernelResult<HandleView> {
        let entry = self.valid_entry(handle)?;
        if !entry.rights.contains(required_rights) {
            return Err(KernelError::MissingRights);
        }
        let object = objects.validate(entry.object, entry.object_generation, expected_kind)?;

        Ok(HandleView {
            handle,
            entry,
            object,
        })
    }

    pub fn peek_kind(&self, handle: HandleValue) -> KernelResult<ObjectKind> {
        Ok(self.valid_entry(handle)?.kind)
    }

    pub fn duplicate(
        &mut self,
        objects: &mut ObjectManager,
        source: HandleValue,
        requested_rights: HandleRights,
    ) -> KernelResult<HandleValue> {
        let source_entry = self.valid_entry(source)?;
        if !source_entry.rights.contains(HandleRights::DUPLICATE) {
            return Err(KernelError::MissingRights);
        }
        if !source_entry.rights.contains(requested_rights) {
            return Err(KernelError::MissingRights);
        }
        let object = objects.validate(
            source_entry.object,
            source_entry.object_generation,
            source_entry.kind,
        )?;
        let derived = self.install(objects, object, requested_rights)?;
        let derived_index = self.index(derived)?;
        let Some(entry) = self.slots[derived_index].entry.as_mut() else {
            unreachable!("install returned a live handle slot");
        };
        entry.parent = Some(HandleLineage {
            handle: source,
            object: source_entry.object,
            object_generation: source_entry.object_generation,
        });
        Ok(derived)
    }

    pub fn revoke_descendants(
        &mut self,
        objects: &mut ObjectManager,
        root: HandleValue,
    ) -> KernelResult<usize> {
        let root_entry = self.valid_entry(root)?;
        let mut descendants = Vec::new();
        descendants
            .try_reserve_exact(self.live_count())
            .map_err(|_| KernelError::NoMemory)?;
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(entry) = slot.entry else {
                continue;
            };
            if entry.entry_generation == root.generation() && index as u64 == root.index() {
                continue;
            }
            if self.descends_from(entry, root, root_entry) {
                descendants.push(HandleValue::new(index as u64, entry.entry_generation));
            }
        }

        let removed = descendants.len();
        for handle in descendants {
            self.close(objects, handle)?;
        }
        Ok(removed)
    }

    pub fn close(&mut self, objects: &mut ObjectManager, handle: HandleValue) -> KernelResult<()> {
        let entry = self.valid_entry(handle)?;
        objects.remove_handle(entry.object, entry.object_generation)?;
        self.remove_entry(handle)?;
        Ok(())
    }

    pub fn remove_for_transfer(
        &mut self,
        objects: &ObjectManager,
        handle: HandleValue,
    ) -> KernelResult<HandleTableEntry> {
        let entry = self.valid_entry(handle)?;
        if !entry.rights.contains(HandleRights::TRANSFER) {
            return Err(KernelError::MissingRights);
        }
        objects.validate(entry.object, entry.object_generation, entry.kind)?;
        self.remove_entry(handle)
    }

    pub fn install_entry(&mut self, entry: HandleTableEntry) -> KernelResult<HandleValue> {
        let index = self.free_index()?;
        let generation = self.slots[index].generation;
        self.slots[index].entry = Some(HandleTableEntry {
            entry_generation: generation,
            ..entry
        });
        Ok(HandleValue::new(index as u64, generation))
    }

    fn remove_entry(&mut self, handle: HandleValue) -> KernelResult<HandleTableEntry> {
        let index = self.index(handle)?;
        let entry = self.valid_entry(handle)?;
        let Some(_) = self.slots[index].entry.take() else {
            unreachable!("valid_entry established a live handle slot");
        };
        self.slots[index].generation = self.slots[index].generation.next();
        Ok(entry)
    }

    fn free_index(&self) -> KernelResult<usize> {
        self.slots
            .iter()
            .position(|slot| slot.entry.is_none())
            .ok_or(KernelError::NoCapacity)
    }

    fn entry(&self, handle: HandleValue) -> KernelResult<HandleTableEntry> {
        self.slot(handle)?.entry.ok_or(KernelError::InvalidHandle)
    }

    fn valid_entry(&self, handle: HandleValue) -> KernelResult<HandleTableEntry> {
        let entry = self.entry(handle)?;
        if entry.entry_generation != handle.generation() {
            return Err(KernelError::StaleHandle);
        }
        Ok(entry)
    }

    fn descends_from(
        &self,
        mut entry: HandleTableEntry,
        root: HandleValue,
        root_entry: HandleTableEntry,
    ) -> bool {
        while let Some(parent) = entry.parent {
            if parent.handle == root
                && parent.object == root_entry.object
                && parent.object_generation == root_entry.object_generation
            {
                return true;
            }
            let Ok(parent_entry) = self.valid_entry(parent.handle) else {
                return false;
            };
            entry = parent_entry;
        }
        false
    }

    fn slot(&self, handle: HandleValue) -> KernelResult<&HandleSlot> {
        let index = self.index(handle)?;
        Ok(&self.slots[index])
    }

    fn index(&self, handle: HandleValue) -> KernelResult<usize> {
        let index = handle.index() as usize;
        if index >= self.capacity {
            return Err(KernelError::InvalidHandle);
        }
        Ok(index)
    }
}
