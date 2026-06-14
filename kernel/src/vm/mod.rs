use crate::{
    error::{KernelError, KernelResult},
    handle::HandleRights,
    memory::frame::FrameRange as MemoryFrameRange,
    object::ObjectRef,
};
use ostd::{
    cpu::{CpuGeneration, CpuSet},
    mm::{
        frame::FrameRange as PageTableFrameRange,
        page_table::{PageTableRights, PageTableUpdateIntent, TlbInvalidationIntent, VirtualRange},
    },
};

pub const MAX_PENDING_TLB_INVALIDATIONS: usize = 8;

// Capacity bound for the first fixed-capacity VmRangeSet. Not a product limit;
// the final VMAR/VMA range owner will replace this with a dynamic data structure.
const DEFAULT_VM_RANGE_CAPACITY: usize = 8;

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
    pub frame_range: MemoryFrameRange,
    pub active_mappings: usize,
}

impl MemoryObject {
    pub fn new(
        size_bytes: u64,
        mapping_policy: MappingPolicy,
        frame_range: MemoryFrameRange,
    ) -> KernelResult<Self> {
        Self::validate_size(size_bytes)?;
        if frame_range.len() != size_bytes {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self {
            size_bytes,
            mapping_policy,
            frame_range,
            active_mappings: 0,
        })
    }

    pub fn validate_size(size_bytes: u64) -> KernelResult<()> {
        VirtualRange::new(0, size_bytes).map_err(|_| KernelError::InvalidArgument)?;
        Ok(())
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

    pub fn add_mapping(&mut self) -> KernelResult<()> {
        self.active_mappings = self
            .active_mappings
            .checked_add(1)
            .ok_or(KernelError::NoCapacity)?;
        Ok(())
    }

    pub fn remove_mapping(&mut self) -> KernelResult<()> {
        if self.active_mappings == 0 {
            return Err(KernelError::InvalidArgument);
        }
        self.active_mappings -= 1;
        Ok(())
    }

    pub const fn can_reclaim(self) -> bool {
        self.active_mappings == 0
    }

    fn page_table_frame_range(
        &self,
        descriptor: VmMapDescriptor,
    ) -> KernelResult<PageTableFrameRange> {
        let start = self
            .frame_range
            .start
            .checked_add(descriptor.memory_offset)
            .ok_or(KernelError::InvalidArgument)?;
        let end = start
            .checked_add(descriptor.size_bytes)
            .ok_or(KernelError::InvalidArgument)?;
        PageTableFrameRange::new(
            usize::try_from(start).map_err(|_| KernelError::InvalidArgument)?,
            usize::try_from(end).map_err(|_| KernelError::InvalidArgument)?,
        )
        .map_err(|_| KernelError::InvalidArgument)
    }
}

/// Owns virtual-address range metadata for one address space.
///
/// The first version uses a fixed-capacity slot set; the final VMAR/VMA range
/// owner will replace this with a dynamic data structure that can grow beyond
/// the initial capacity and support tree-shaped allocation.
///
/// Callers must go through `reserve_map` / `reserve_unmap` and commit through
/// the returned reservation; the mapping iteration order is unspecified and
/// must not be relied on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VmRangeSet {
    mappings: [Option<VmMapping>; DEFAULT_VM_RANGE_CAPACITY],
    count: usize,
}

impl VmRangeSet {
    const fn new() -> Self {
        Self {
            mappings: [None; DEFAULT_VM_RANGE_CAPACITY],
            count: 0,
        }
    }

    fn mappings(&self) -> impl Iterator<Item = VmMapping> + '_ {
        self.mappings.iter().filter_map(|mapping| *mapping)
    }

    const fn count(self) -> usize {
        self.count
    }

    /// Reserves a mapping slot after checking capacity and range overlap.
    ///
    /// Returns the slot index. The index is valid only while the returned
    /// reservation (which borrows the parent `AddressSpaceObject` exclusively)
    /// is live.
    fn reserve_map(&self, mapping: VmMapping) -> KernelResult<usize> {
        if self.count == DEFAULT_VM_RANGE_CAPACITY {
            return Err(KernelError::NoCapacity);
        }
        if self
            .mappings()
            .any(|existing| ranges_overlap(mapping.base, mapping.end(), existing))
        {
            return Err(KernelError::InvalidArgument);
        }
        self.mappings
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)
    }

    fn commit_map(&mut self, index: usize, mapping: VmMapping) {
        assert!(
            index < DEFAULT_VM_RANGE_CAPACITY && self.mappings[index].is_none(),
            "vm map reservation slot must remain free until commit"
        );
        self.mappings[index] = Some(mapping);
        self.count += 1;
    }

    /// Reserves an unmap slot for an exact base+size match.
    ///
    /// Returns the index of the matching mapping.
    fn reserve_unmap(&self, base: u64, size_bytes: u64) -> KernelResult<usize> {
        let end = base
            .checked_add(size_bytes)
            .ok_or(KernelError::InvalidArgument)?;
        self.mappings
            .iter()
            .position(|mapping| mapping.is_some_and(|m| m.base == base && m.end() == end))
            .ok_or(KernelError::InvalidArgument)
    }

    fn reserved_mapping(&self, index: usize) -> VmMapping {
        self.mappings[index].expect("vm mapping reservation selected an occupied slot")
    }

    fn commit_unmap(&mut self, index: usize) {
        self.mappings[index]
            .take()
            .expect("vm unmap slot was selected as occupied");
        self.count -= 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressSpaceObject {
    pub pending_tlb_invalidations: PendingTlbInvalidations,
    range_set: VmRangeSet,
}

impl AddressSpaceObject {
    pub const fn new() -> Self {
        Self {
            pending_tlb_invalidations: PendingTlbInvalidations::empty(),
            range_set: VmRangeSet::new(),
        }
    }

    pub const fn mapping_count(self) -> usize {
        self.range_set.count()
    }

    pub fn mappings(&self) -> impl Iterator<Item = VmMapping> + '_ {
        self.range_set.mappings()
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
        let mapping = VmMapping {
            base: descriptor.base,
            size_bytes: descriptor.size_bytes,
            memory,
            memory_offset: descriptor.memory_offset,
            rights: descriptor.rights,
        };
        let index = self.range_set.reserve_map(mapping)?;
        let page_table = PageTableUpdateIntent::map(
            range,
            memory_object.page_table_frame_range(descriptor)?,
            page_table_rights(descriptor.rights),
        )
        .map_err(|_| KernelError::InvalidArgument)?;

        Ok(VmMapReservation {
            address_space: self,
            mapping_slot: MappingSlotReservation { index },
            mapping,
            page_table,
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
        let index = self.range_set.reserve_unmap(base, size_bytes)?;
        let range =
            VirtualRange::new(base, size_bytes).map_err(|_| KernelError::InvalidArgument)?;
        let page_table = PageTableUpdateIntent::unmap(range);
        let memory = self.range_set.reserved_mapping(index).memory;
        let tlb_slot = self.pending_tlb_invalidations.reserve(range, memory)?;

        Ok(VmUnmapReservation {
            address_space: self,
            mapping_slot: MappingSlotReservation { index },
            memory,
            page_table,
            tlb_slot,
        })
    }

    pub fn take_pending_tlb_invalidation(&mut self) -> Option<PendingTlbInvalidationWork> {
        self.pending_tlb_invalidations.take_next()
    }

    pub const fn can_destroy(self) -> bool {
        self.mapping_count() == 0 && self.pending_tlb_invalidations.count() == 0
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
    page_table: PageTableUpdateIntent,
}

impl VmMapReservation<'_> {
    pub const fn page_table(&self) -> &PageTableUpdateIntent {
        &self.page_table
    }

    pub fn commit(self) {
        self.address_space
            .range_set
            .commit_map(self.mapping_slot.index, self.mapping);
    }
}

#[derive(Debug)]
pub struct VmUnmapReservation<'a> {
    address_space: &'a mut AddressSpaceObject,
    mapping_slot: MappingSlotReservation,
    memory: ObjectRef,
    page_table: PageTableUpdateIntent,
    tlb_slot: PendingTlbInvalidationReservation,
}

impl VmUnmapReservation<'_> {
    pub const fn memory(&self) -> ObjectRef {
        self.memory
    }

    pub const fn page_table(&self) -> &PageTableUpdateIntent {
        &self.page_table
    }

    pub const fn tlb_invalidation(&self) -> &TlbInvalidationIntent {
        &self.tlb_slot.intent
    }

    pub fn commit(self) {
        self.address_space
            .range_set
            .commit_unmap(self.mapping_slot.index);
        self.address_space
            .pending_tlb_invalidations
            .commit(self.tlb_slot);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingTlbInvalidations {
    // TODO(vm-tlb): count is diagnostic scaffolding for the missing invalidation
    // queue. Do not use it as correctness evidence for TLB completion. The current
    // work is fixed pending-work storage with a target and generation, but still
    // lacks per-CPU completion, shootdown execution, and final reclaim semantics;
    // MAX_PENDING_TLB_INVALIDATIONS is not a product limit. Replace with the final
    // pending-work owner when map/unmap and flush-consumer tests prove publication,
    // consumption, and completion.
    count: usize,
    next_sequence: u64,
    work: [Option<PendingTlbInvalidation>; MAX_PENDING_TLB_INVALIDATIONS],
}

impl PendingTlbInvalidations {
    pub const fn empty() -> Self {
        Self {
            count: 0,
            next_sequence: 0,
            work: [None; MAX_PENDING_TLB_INVALIDATIONS],
        }
    }

    pub const fn count(self) -> usize {
        self.count
    }

    fn reserve(
        &self,
        range: VirtualRange,
        deferred_reclaim: ObjectRef,
    ) -> KernelResult<PendingTlbInvalidationReservation> {
        let index = self
            .work
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        let generation = CpuGeneration::new(self.next_sequence);
        Ok(PendingTlbInvalidationReservation {
            index,
            sequence: self.next_sequence,
            intent: TlbInvalidationIntent::new(range, CpuSet::AllActive, generation),
            deferred_reclaim,
        })
    }

    fn commit(&mut self, reservation: PendingTlbInvalidationReservation) {
        assert!(
            reservation.index < MAX_PENDING_TLB_INVALIDATIONS
                && self.work[reservation.index].is_none(),
            "tlb invalidation reservation slot must remain free until commit"
        );
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("pending TLB invalidation sequence exhausted");
        self.work[reservation.index] = Some(PendingTlbInvalidation {
            sequence: reservation.sequence,
            intent: reservation.intent,
            deferred_reclaim: reservation.deferred_reclaim,
        });
        self.next_sequence = next_sequence;
        self.count += 1;
    }

    fn take_next(&mut self) -> Option<PendingTlbInvalidationWork> {
        let index = self
            .work
            .iter()
            .enumerate()
            .filter_map(|(index, pending)| pending.map(|pending| (index, pending.sequence)))
            .min_by_key(|(_, sequence)| *sequence)?
            .0;
        let pending = self.work[index]
            .take()
            .expect("pending TLB invalidation slot was selected as occupied");
        self.count -= 1;
        Some(PendingTlbInvalidationWork {
            intent: pending.intent,
            deferred_reclaim: pending.deferred_reclaim,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingTlbInvalidationWork {
    pub intent: TlbInvalidationIntent,
    pub deferred_reclaim: ObjectRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingTlbInvalidation {
    sequence: u64,
    intent: TlbInvalidationIntent,
    deferred_reclaim: ObjectRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingTlbInvalidationReservation {
    index: usize,
    sequence: u64,
    intent: TlbInvalidationIntent,
    deferred_reclaim: ObjectRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MappingSlotReservation {
    index: usize,
}

fn ranges_overlap(base: u64, end: u64, mapping: VmMapping) -> bool {
    base < mapping.end() && mapping.base < end
}

fn page_table_rights(rights: HandleRights) -> PageTableRights {
    let mut page_table_rights = PageTableRights::empty();
    if rights.contains(HandleRights::READ) {
        page_table_rights |= PageTableRights::READ;
    }
    if rights.contains(HandleRights::WRITE) {
        page_table_rights |= PageTableRights::WRITE;
    }
    if rights.contains(HandleRights::EXECUTE) {
        page_table_rights |= PageTableRights::EXECUTE;
    }
    page_table_rights
}
