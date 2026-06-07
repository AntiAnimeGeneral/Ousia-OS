use crate::{
    error::{KernelError, KernelResult},
    handle::HandleRights,
    object::ObjectRef,
};

pub const MAX_ADDRESS_SPACE_MAPPINGS: usize = 8;
pub const MAX_PENDING_TLB_SHOOTDOWNS: usize = 8;

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
    pub pending_tlb_shootdowns: PendingTlbShootdowns,
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
            pending_tlb_shootdowns: PendingTlbShootdowns::empty(),
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
        let tlb_shootdown = TlbShootdownPlan {
            range: VmRange {
                base: descriptor.base,
                size_bytes: descriptor.size_bytes,
            },
        };
        let tlb_slot = self.pending_tlb_shootdowns.reserve(tlb_shootdown)?;

        Ok(VmMapReservation {
            address_space: self,
            mapping_slot: MappingSlotReservation { index },
            page_table: PageTableCommitPlan {
                range: VmRange {
                    base: descriptor.base,
                    size_bytes: descriptor.size_bytes,
                },
            },
            tlb_slot,
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
        let tlb_shootdown = TlbShootdownPlan {
            range: VmRange { base, size_bytes },
        };
        let tlb_slot = self.pending_tlb_shootdowns.reserve(tlb_shootdown)?;

        Ok(VmUnmapReservation {
            address_space: self,
            mapping_slot: MappingSlotReservation { index },
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

#[derive(Debug)]
pub struct VmMapReservation<'a> {
    address_space: &'a mut AddressSpaceObject,
    mapping_slot: MappingSlotReservation,
    page_table: PageTableCommitPlan,
    tlb_slot: PendingTlbShootdownReservation,
    mapping: VmMapping,
}

impl VmMapReservation<'_> {
    pub const fn page_table(&self) -> &PageTableCommitPlan {
        &self.page_table
    }

    pub const fn tlb_shootdown(&self) -> &TlbShootdownPlan {
        &self.tlb_slot.plan
    }

    pub fn commit(self) {
        let index = self.mapping_slot.index;
        assert!(
            index < MAX_ADDRESS_SPACE_MAPPINGS && self.address_space.mappings[index].is_none(),
            "vm map reservation slot must remain free until commit"
        );
        self.address_space.mappings[index] = Some(self.mapping);
        self.address_space.mapping_count += 1;
        self.address_space
            .pending_tlb_shootdowns
            .commit(self.tlb_slot);
    }
}

#[derive(Debug)]
pub struct VmUnmapReservation<'a> {
    address_space: &'a mut AddressSpaceObject,
    mapping_slot: MappingSlotReservation,
    tlb_slot: PendingTlbShootdownReservation,
}

impl VmUnmapReservation<'_> {
    pub const fn tlb_shootdown(&self) -> &TlbShootdownPlan {
        &self.tlb_slot.plan
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
            .pending_tlb_shootdowns
            .commit(self.tlb_slot);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmRange {
    pub base: u64,
    pub size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageTableCommitPlan {
    // TODO(vm-page-table): this records only the range that must eventually be
    // committed to page tables. It is not proof that hardware mappings, frame
    // materialization, or page-table metadata reservations exist. Exit when the
    // reservation token carries the real page-table owner evidence and tests prove
    // failed page-table preparation leaves AddressSpace state unchanged.
    pub range: VmRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TlbShootdownPlan {
    // TODO(vm-tlb-shootdown): this range is a multi-core boundary marker, not a
    // real shootdown request. The final state needs target CPU/generation tracking
    // and a consumer that proves flush completion. Exit when map/unmap tests cover
    // pending work publication and flush consumption.
    pub range: VmRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingTlbShootdowns {
    // TODO(vm-tlb-shootdown): count is diagnostic scaffolding for the missing
    // shootdown queue. Do not use it as correctness evidence for TLB completion.
    // work is fixed pending-work storage, but still lacks target CPU/generation,
    // consumer, completion, and reclaim semantics; MAX_PENDING_TLB_SHOOTDOWNS is
    // not a product limit. Replace with the final pending-work owner when map/unmap
    // and flush-consumer tests prove publication, consumption, and completion.
    count: usize,
    work: [Option<TlbShootdownPlan>; MAX_PENDING_TLB_SHOOTDOWNS],
}

impl PendingTlbShootdowns {
    pub const fn empty() -> Self {
        Self {
            count: 0,
            work: [None; MAX_PENDING_TLB_SHOOTDOWNS],
        }
    }

    pub const fn count(self) -> usize {
        self.count
    }

    fn reserve(&self, plan: TlbShootdownPlan) -> KernelResult<PendingTlbShootdownReservation> {
        let index = self
            .work
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        Ok(PendingTlbShootdownReservation { index, plan })
    }

    fn commit(&mut self, reservation: PendingTlbShootdownReservation) {
        assert!(
            reservation.index < MAX_PENDING_TLB_SHOOTDOWNS
                && self.work[reservation.index].is_none(),
            "tlb shootdown reservation slot must remain free until commit"
        );
        self.work[reservation.index] = Some(reservation.plan);
        self.count += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingTlbShootdownReservation {
    index: usize,
    plan: TlbShootdownPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MappingSlotReservation {
    index: usize,
}

fn ranges_overlap(base: u64, end: u64, mapping: VmMapping) -> bool {
    base < mapping.end() && mapping.base < end
}
