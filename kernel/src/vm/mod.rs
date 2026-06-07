use crate::{
    error::{KernelError, KernelResult},
    handle::HandleRights,
    object::ObjectRef,
};

pub const MAX_ADDRESS_SPACE_MAPPINGS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryBacking {
    Anonymous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappingPolicy {
    pub max_rights: HandleRights,
}

impl MappingPolicy {
    pub const fn new(max_rights: HandleRights) -> Self {
        Self { max_rights }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryObject {
    pub size_bytes: u64,
    pub backing: MemoryBacking,
    pub mapping_policy: MappingPolicy,
}

impl MemoryObject {
    pub const fn anonymous(size_bytes: u64, mapping_policy: MappingPolicy) -> Self {
        Self {
            size_bytes,
            backing: MemoryBacking::Anonymous,
            mapping_policy,
        }
    }

    pub fn validate_mapping(&self, descriptor: VmMapDescriptor) -> KernelResult<()> {
        if !self.mapping_policy.max_rights.contains(descriptor.rights) {
            return Err(KernelError::MissingRights);
        }
        let memory_end = descriptor
            .memory_offset
            .checked_add(descriptor.size_bytes)
            .ok_or(KernelError::InvalidArgument)?;
        if memory_end > self.size_bytes {
            return Err(KernelError::InvalidArgument);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressSpaceObject {
    pub mapping_count: usize,
    mappings: [Option<VmMapping>; MAX_ADDRESS_SPACE_MAPPINGS],
}

impl AddressSpaceObject {
    pub const fn new() -> Self {
        Self {
            mapping_count: 0,
            mappings: [None; MAX_ADDRESS_SPACE_MAPPINGS],
        }
    }

    pub fn mappings(&self) -> impl Iterator<Item = VmMapping> + '_ {
        self.mappings.iter().filter_map(|mapping| *mapping)
    }

    pub fn prepare_map(
        &self,
        memory: ObjectRef,
        memory_object: MemoryObject,
        descriptor: VmMapDescriptor,
    ) -> KernelResult<VmCommitPlan> {
        descriptor.validate()?;
        memory_object.validate_mapping(descriptor)?;
        let end = descriptor.end()?;
        if self.mapping_count == MAX_ADDRESS_SPACE_MAPPINGS {
            return Err(KernelError::NoCapacity);
        }
        if self
            .mappings()
            .any(|mapping| ranges_overlap(descriptor.base, end, mapping))
        {
            return Err(KernelError::InvalidArgument);
        }
        let index = self
            .mappings
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;

        Ok(VmCommitPlan {
            mapping_slot: MappingSlotReservation { index },
            mapping: VmMapping {
                base: descriptor.base,
                size_bytes: descriptor.size_bytes,
                memory,
                memory_offset: descriptor.memory_offset,
                rights: descriptor.rights,
            },
        })
    }

    pub fn commit_map(&mut self, plan: VmCommitPlan) -> KernelResult<()> {
        let index = plan.mapping_slot.index;
        if index >= MAX_ADDRESS_SPACE_MAPPINGS || self.mappings[index].is_some() {
            return Err(KernelError::NoCapacity);
        }
        self.mappings[index] = Some(plan.mapping);
        self.mapping_count += 1;
        Ok(())
    }

    pub fn unmap_exact(&mut self, base: u64, size_bytes: u64) -> KernelResult<()> {
        if size_bytes == 0 {
            return Err(KernelError::InvalidArgument);
        }
        let end = base
            .checked_add(size_bytes)
            .ok_or(KernelError::InvalidArgument)?;
        let Some(index) = self.mappings.iter().position(|mapping| {
            mapping.is_some_and(|mapping| mapping.base == base && mapping.end() == end)
        }) else {
            return Err(KernelError::InvalidArgument);
        };
        self.mappings[index] = None;
        self.mapping_count -= 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmMapping {
    pub base: u64,
    pub size_bytes: u64,
    pub memory: ObjectRef,
    pub memory_offset: u64,
    pub rights: HandleRights,
}

impl VmMapping {
    pub fn end(self) -> u64 {
        self.base
            .checked_add(self.size_bytes)
            .expect("vm mapping end was checked before commit")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmMapDescriptor {
    pub base: u64,
    pub size_bytes: u64,
    pub memory_offset: u64,
    pub rights: HandleRights,
}

impl VmMapDescriptor {
    pub fn validate(self) -> KernelResult<()> {
        let mapping_rights = HandleRights::READ | HandleRights::WRITE | HandleRights::EXECUTE;
        if self.rights.is_empty() || !mapping_rights.contains(self.rights) {
            return Err(KernelError::InvalidArgument);
        }
        if self.size_bytes == 0 {
            return Err(KernelError::InvalidArgument);
        }
        self.end()?;
        self.memory_offset
            .checked_add(self.size_bytes)
            .ok_or(KernelError::InvalidArgument)?;
        Ok(())
    }

    fn end(self) -> KernelResult<u64> {
        self.base
            .checked_add(self.size_bytes)
            .ok_or(KernelError::InvalidArgument)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct VmCommitPlan {
    mapping_slot: MappingSlotReservation,
    mapping: VmMapping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MappingSlotReservation {
    index: usize,
}

fn ranges_overlap(base: u64, end: u64, mapping: VmMapping) -> bool {
    base < mapping.end() && mapping.base < end
}
