use crate::{
    error::{KernelError, KernelResult},
    handle::HandleRights,
    object::ObjectRef,
};
use ostd::mm::page_table::{PageTableUpdateIntent, TlbInvalidationIntent, VirtualRange};

pub const MAX_ADDRESS_SPACE_MAPPINGS: usize = 8;
pub const MAX_PENDING_TLB_INVALIDATIONS: usize = 8;

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
    pub mapping_policy: MappingPolicy,
}

impl MemoryObject {
    pub const fn new(size_bytes: u64, mapping_policy: MappingPolicy) -> Self {
        Self {
            size_bytes,
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
    pub pending_tlb_invalidations: PendingTlbInvalidations,
    // TODO(vm-range-owner): replace this fixed metadata slot set with the final
    // AddressSpace range owner. The final shape must reserve mapping metadata and
    // page-table resources before publication; callers must not rely on slot order
    // or on MAX_ADDRESS_SPACE_MAPPINGS as a product limit. Exit when VMAR/VMA owner
    // tests cover overlap, dropped reservation, unmap, and capacity failure.
    mappings: [Option<VmMapping>; MAX_ADDRESS_SPACE_MAPPINGS],
}

impl AddressSpaceObject {
    pub const fn new() -> Self {
        Self {
            mapping_count: 0,
            pending_tlb_invalidations: PendingTlbInvalidations::empty(),
            mappings: [None; MAX_ADDRESS_SPACE_MAPPINGS],
        }
    }

    pub fn mappings(&self) -> impl Iterator<Item = VmMapping> + '_ {
        self.mappings.iter().filter_map(|mapping| *mapping)
    }

    pub fn prepare_map(
        &mut self,
        memory: ObjectRef,
        memory_object: MemoryObject,
        descriptor: VmMapDescriptor,
    ) -> KernelResult<VmMapReservation<'_>> {
        descriptor.validate()?;
        memory_object.validate_mapping(descriptor)?;
        let range = descriptor.virtual_range()?;
        if self.mapping_count == MAX_ADDRESS_SPACE_MAPPINGS {
            return Err(KernelError::NoCapacity);
        }
        if self
            .mappings()
            .any(|mapping| ranges_overlap(range.base, range.end(), mapping))
        {
            return Err(KernelError::InvalidArgument);
        }
        let index = self
            .mappings
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;

        Ok(VmMapReservation {
            address_space: self,
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

    pub fn prepare_unmap(
        &mut self,
        base: u64,
        size_bytes: u64,
    ) -> KernelResult<VmUnmapReservation<'_>> {
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
        let range =
            VirtualRange::new(base, size_bytes).map_err(|_| KernelError::InvalidArgument)?;
        let page_table = PageTableUpdateIntent::unmap(range);
        let tlb_slot = self
            .pending_tlb_invalidations
            .reserve(TlbInvalidationIntent::new(range))?;

        Ok(VmUnmapReservation {
            address_space: self,
            mapping_slot: MappingSlotReservation { index },
            page_table,
            tlb_slot,
        })
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
        self.virtual_range()?;
        if !self
            .memory_offset
            .is_multiple_of(ostd::mm::frame::PAGE_SIZE as u64)
        {
            return Err(KernelError::InvalidArgument);
        }
        self.memory_offset
            .checked_add(self.size_bytes)
            .ok_or(KernelError::InvalidArgument)?;
        Ok(())
    }

    fn virtual_range(self) -> KernelResult<VirtualRange> {
        VirtualRange::new(self.base, self.size_bytes).map_err(|_| KernelError::InvalidArgument)
    }
}

#[derive(Debug)]
pub struct VmMapReservation<'a> {
    address_space: &'a mut AddressSpaceObject,
    mapping_slot: MappingSlotReservation,
    mapping: VmMapping,
}

impl VmMapReservation<'_> {
    pub fn commit(self) {
        let index = self.mapping_slot.index;
        assert!(
            index < MAX_ADDRESS_SPACE_MAPPINGS && self.address_space.mappings[index].is_none(),
            "vm map reservation slot must remain free until commit"
        );
        self.address_space.mappings[index] = Some(self.mapping);
        self.address_space.mapping_count += 1;
    }
}

#[derive(Debug)]
pub struct VmUnmapReservation<'a> {
    address_space: &'a mut AddressSpaceObject,
    mapping_slot: MappingSlotReservation,
    page_table: PageTableUpdateIntent,
    tlb_slot: PendingTlbInvalidationReservation,
}

impl VmUnmapReservation<'_> {
    pub const fn page_table(&self) -> &PageTableUpdateIntent {
        &self.page_table
    }

    pub const fn tlb_invalidation(&self) -> &TlbInvalidationIntent {
        &self.tlb_slot.intent
    }

    pub fn commit(self) {
        let index = self.mapping_slot.index;
        assert!(
            index < MAX_ADDRESS_SPACE_MAPPINGS && self.address_space.mappings[index].is_some(),
            "vm unmap reservation slot must remain mapped until commit"
        );
        self.address_space.mappings[index] = None;
        self.address_space.mapping_count -= 1;
        self.address_space
            .pending_tlb_invalidations
            .commit(self.tlb_slot);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingTlbInvalidations {
    // TODO(vm-tlb): count is diagnostic scaffolding for the missing invalidation
    // queue. Do not use it as correctness evidence for TLB completion.
    // work is fixed pending-work storage, but still lacks target CPU/generation,
    // consumer, completion, and reclaim semantics; MAX_PENDING_TLB_INVALIDATIONS is
    // not a product limit. Replace with the final pending-work owner when map/unmap
    // and flush-consumer tests prove publication, consumption, and completion.
    count: usize,
    work: [Option<TlbInvalidationIntent>; MAX_PENDING_TLB_INVALIDATIONS],
}

impl PendingTlbInvalidations {
    pub const fn empty() -> Self {
        Self {
            count: 0,
            work: [None; MAX_PENDING_TLB_INVALIDATIONS],
        }
    }

    pub const fn count(self) -> usize {
        self.count
    }

    fn reserve(
        &self,
        intent: TlbInvalidationIntent,
    ) -> KernelResult<PendingTlbInvalidationReservation> {
        let index = self
            .work
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        Ok(PendingTlbInvalidationReservation { index, intent })
    }

    fn commit(&mut self, reservation: PendingTlbInvalidationReservation) {
        assert!(
            reservation.index < MAX_PENDING_TLB_INVALIDATIONS
                && self.work[reservation.index].is_none(),
            "tlb invalidation reservation slot must remain free until commit"
        );
        self.work[reservation.index] = Some(reservation.intent);
        self.count += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingTlbInvalidationReservation {
    index: usize,
    intent: TlbInvalidationIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MappingSlotReservation {
    index: usize,
}

fn ranges_overlap(base: u64, end: u64, mapping: VmMapping) -> bool {
    base < mapping.end() && mapping.base < end
}
