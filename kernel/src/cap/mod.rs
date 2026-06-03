//! Phase 0.5 capability core model.
//!
//! This module is the first executable contract for Ousia's kernel-visible
//! capability semantics. It is intentionally narrower than the full kernel: it
//! models CSpace-like slot ownership, derivation, revocation, object lifetime,
//! and stale descriptor rejection without pulling in boot, scheduling, IPC, page
//! tables, or device code.
//!
//! Review focus:
//!
//! - Rights may only shrink during derivation.
//! - `delete` invalidates only the named slot.
//! - `revoke_descendants` invalidates MDB descendants while keeping the named slot.
//! - `destroy_object` invalidates every capability that targets the object.
//! - `slot_generation` prevents ABA when a slot is safely reused.
//! - `object_generation_snapshot` rejects descriptors after object generation
//!   changes.
//! - MDB predecessor/successor metadata is the authority for revoke traversal.
//!
//! This is still a testable model, not the final CSpace implementation. The
//! next integration step is to connect these semantics to Portal / Operation
//! handle transfer and to replace the test-friendly storage with the eventual
//! kernel allocator and CSpace representation.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use bitflags::bitflags;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ObjectId(u64);

impl ObjectId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SlotId(u64);

impl SlotId {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    /// Internal logical capability rights used by the host kernel model.
    ///
    /// seL4 encodes syscall-facing rights in machine-word-shaped message and
    /// cap-data fields. This type intentionally uses a fixed `u32` because it
    /// is not an ABI object: it is a compact, architecture-independent Rust
    /// value used inside the executable model and tests. Any future syscall or
    /// CSpace ABI boundary must convert explicitly to and from the seL4 word
    /// layout instead of relying on this bit layout.
    ///
    /// The bit positions here are local semantic flags, not a promise to match
    /// `seL4_CapRights_t` or any generated seL4 C representation.
    pub struct Rights: u32 {
        const NONE = 0;
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
        const GRANT = 1 << 3;
        const MANAGE = 1 << 4;
        const GRANT_REPLY = 1 << 5;
        const ALL = Self::READ.bits()
            | Self::WRITE.bits()
            | Self::EXECUTE.bits()
            | Self::GRANT.bits()
            | Self::MANAGE.bits()
            | Self::GRANT_REPLY.bits();
    }
}

const MIN_FRAME_SIZE_BITS: u8 = 12;
const MODEL_ENDPOINT_SIZE_BITS: u8 = 4;
const MODEL_CNODE_SIZE_BITS: u8 = 6;
const MODEL_TCB_SIZE_BITS: u8 = 10;
const MODEL_NOTIFICATION_SIZE_BITS: u8 = 5;
const ENDPOINT_ALLOWED_RIGHTS: Rights = Rights::READ
    .union(Rights::WRITE)
    .union(Rights::GRANT)
    .union(Rights::GRANT_REPLY);
const FRAME_ALLOWED_RIGHTS: Rights = Rights::READ.union(Rights::WRITE).union(Rights::EXECUTE);
const UNTYPED_ALLOWED_RIGHTS: Rights = Rights::NONE;
const TCB_ALLOWED_RIGHTS: Rights = Rights::MANAGE;
const NOTIFICATION_ALLOWED_RIGHTS: Rights = Rights::READ.union(Rights::WRITE);
const REPLY_ALLOWED_RIGHTS: Rights = Rights::NONE;
const MIN_RETYPE_BYTES: u128 = 1;

impl Rights {
    pub const fn is_subset_of(self, allowed: Self) -> bool {
        allowed.contains(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Capability {
    Endpoint(EndpointCap),
    Frame(FrameCap),
    CNode(CNodeCap),
    Untyped(UntypedCap),
    Tcb(TcbCap),
    Notification(NotificationCap),
    Reply(ReplyCap),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointCap {
    pub badge: u64,
    pub rights: Rights,
}

impl EndpointCap {
    pub const fn can_send(&self) -> bool {
        self.rights.contains(Rights::WRITE)
    }

    pub const fn can_receive(&self) -> bool {
        self.rights.contains(Rights::READ)
    }

    pub const fn can_grant(&self) -> bool {
        self.rights.contains(Rights::GRANT)
    }

    pub const fn can_grant_reply(&self) -> bool {
        self.rights.contains(Rights::GRANT_REPLY)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameCap {
    pub rights: Rights,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CNodeCap {
    pub radix: u8,
    pub guard: u64,
    pub guard_size: u8,
}

impl CNodeCap {
    pub const fn new(radix: u8) -> Self {
        Self {
            radix,
            guard: 0,
            guard_size: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UntypedCap {
    pub size_bits: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TcbCap {
    pub rights: Rights,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationCap {
    pub badge: u64,
    pub rights: Rights,
}

impl NotificationCap {
    pub const fn can_send(&self) -> bool {
        self.rights.contains(Rights::WRITE)
    }

    pub const fn can_receive(&self) -> bool {
        self.rights.contains(Rights::READ)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyCap {
    pub caller: ObjectId,
    pub target: ObjectId,
    pub can_grant: bool,
}

impl ReplyCap {
    pub fn can_reply(&self, target: ObjectId) -> bool {
        self.target == target
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectKind {
    Endpoint,
    Frame,
    CNode,
    Untyped,
    Tcb,
    Notification,
    Reply,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityDescriptor {
    pub slot: SlotId,
    pub slot_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetypeDestination {
    pub start: SlotId,
    pub count: usize,
}

impl RetypeDestination {
    pub const fn single(start: SlotId) -> Self {
        Self { start, count: 1 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetypeResult {
    pub descriptors: Vec<CapabilityDescriptor>,
    pub objects: Vec<ObjectId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetypeTarget {
    Endpoint,
    Frame { rights: Rights },
    CNode { radix: u8 },
    Untyped { size_bits: u8 },
    Tcb { rights: Rights },
    Notification,
}

impl RetypeTarget {
    pub(crate) const fn minimum_size_bits(&self) -> u8 {
        match self {
            Self::Endpoint => MODEL_ENDPOINT_SIZE_BITS,
            Self::Frame { .. } => MIN_FRAME_SIZE_BITS,
            Self::CNode { radix } => MODEL_CNODE_SIZE_BITS.saturating_add(*radix),
            Self::Untyped { size_bits } => *size_bits,
            Self::Tcb { .. } => MODEL_TCB_SIZE_BITS,
            Self::Notification => MODEL_NOTIFICATION_SIZE_BITS,
        }
    }

    pub(crate) fn validate_rights(&self) -> Result<(), CapError> {
        let object = self.object_kind();
        let requested_rights = self.requested_rights();
        validate_rights_for(object, requested_rights)
    }

    fn into_capability(self) -> Capability {
        match self {
            Self::Endpoint => Capability::Endpoint(EndpointCap {
                badge: 0,
                rights: ENDPOINT_ALLOWED_RIGHTS,
            }),
            Self::Frame { rights } => Capability::Frame(FrameCap { rights }),
            Self::CNode { radix } => Capability::CNode(CNodeCap::new(radix)),
            Self::Untyped { size_bits } => Capability::Untyped(UntypedCap { size_bits }),
            Self::Tcb { rights } => Capability::Tcb(TcbCap { rights }),
            Self::Notification => Capability::Notification(NotificationCap {
                badge: 0,
                rights: NOTIFICATION_ALLOWED_RIGHTS,
            }),
        }
    }

    fn object_kind(&self) -> ObjectKind {
        match self {
            Self::Endpoint => ObjectKind::Endpoint,
            Self::Frame { .. } => ObjectKind::Frame,
            Self::CNode { .. } => ObjectKind::CNode,
            Self::Untyped { .. } => ObjectKind::Untyped,
            Self::Tcb { .. } => ObjectKind::Tcb,
            Self::Notification => ObjectKind::Notification,
        }
    }

    fn requested_rights(&self) -> Rights {
        match self {
            Self::Endpoint => ENDPOINT_ALLOWED_RIGHTS,
            Self::Frame { rights } => *rights,
            Self::CNode { .. } => Rights::NONE,
            Self::Untyped { .. } => UNTYPED_ALLOWED_RIGHTS,
            Self::Tcb { rights } => *rights,
            Self::Notification => NOTIFICATION_ALLOWED_RIGHTS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MintParams {
    None,
    CapData { preserve: bool, data: u64 },
}

impl MintParams {
    pub const fn badge(data: u64) -> Self {
        Self::CapData {
            preserve: false,
            data,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityView {
    pub object: ObjectId,
    pub object_kind: ObjectKind,
    pub capability: Capability,
    pub rights: Rights,
    pub descriptor: CapabilityDescriptor,
    pub parent: Option<SlotId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityRevocation {
    pub revoked_objects: Vec<ObjectId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDeletion {
    pub final_object: Option<ObjectId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapError {
    SlotNotFound(SlotId),
    ObjectNotFound(ObjectId),
    ObjectDestroyed(ObjectId),
    StaleDescriptor {
        slot: SlotId,
        expected_generation: u64,
        actual_generation: u64,
    },
    RightsEscalation {
        parent: SlotId,
        parent_rights: Rights,
        requested_rights: Rights,
    },
    CapabilityNotDerivable {
        parent: SlotId,
        capability: Capability,
    },
    CapabilityNotMintable {
        parent: SlotId,
        capability: Capability,
        params: MintParams,
    },
    InvalidInitialCapability {
        capability: Capability,
    },
    WrongCapability {
        expected: ObjectKind,
        actual: ObjectKind,
    },
    InvalidRetypeSize {
        parent: SlotId,
        requested: u8,
        source: u8,
    },
    UntypedCapacityExhausted {
        parent: SlotId,
        requested: u8,
        source: u8,
    },
    InvalidRights {
        object: ObjectKind,
        requested_rights: Rights,
        allowed_rights: Rights,
    },
    SlotOccupied(SlotId),
    EmptyRetypeWindow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KernelObject {
    kind: ObjectKind,
    generation: u64,
    destroyed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CapabilitySlot {
    object: ObjectId,
    capability: Capability,
    rights: Rights,
    slot_generation: u64,
    object_generation_snapshot: u64,
    parent: Option<SlotId>,
    children: BTreeSet<SlotId>,
    mdb: MdbNode,
    alive: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MdbNode {
    prev: Option<SlotId>,
    next: Option<SlotId>,
    revocable: bool,
    first_badged: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UntypedAllocation {
    size_bits: u8,
    watermark: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UntypedAllocationPlan {
    parent_object: ObjectId,
    next_watermark: u128,
}

#[derive(Debug, Default)]
pub struct CapabilitySpace {
    next_object: u64,
    next_slot: u64,
    free_slots: Vec<SlotId>,
    objects: BTreeMap<ObjectId, KernelObject>,
    untyped_allocations: BTreeMap<ObjectId, UntypedAllocation>,
    slots: BTreeMap<SlotId, CapabilitySlot>,
}

impl CapabilitySpace {
    pub fn new() -> Self {
        Self {
            next_object: 1,
            next_slot: 1,
            free_slots: Vec::new(),
            objects: BTreeMap::new(),
            untyped_allocations: BTreeMap::new(),
            slots: BTreeMap::new(),
        }
    }

    pub fn insert_initial_capability(
        &mut self,
        capability: Capability,
    ) -> Result<CapabilityDescriptor, CapError> {
        if matches!(capability, Capability::Reply(_)) {
            return Err(CapError::InvalidInitialCapability { capability });
        }

        validate_capability_rights(&capability)?;
        Ok(self.insert_validated_initial_capability(capability))
    }

    #[cfg(test)]
    pub(crate) fn insert_reply_capability_for_test(
        &mut self,
        capability: ReplyCap,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.insert_initial_reply_capability(capability)
    }

    #[cfg(test)]
    fn insert_initial_reply_capability(
        &mut self,
        capability: ReplyCap,
    ) -> Result<CapabilityDescriptor, CapError> {
        let capability = Capability::Reply(capability);
        validate_capability_rights(&capability)?;
        Ok(self.insert_validated_initial_capability(capability))
    }

    fn insert_validated_initial_capability(
        &mut self,
        capability: Capability,
    ) -> CapabilityDescriptor {
        let kind = capability_kind(&capability);
        let (object, generation) = self.alloc_object(kind);
        self.insert_root_slot(object, capability, generation)
    }

    pub fn derive(
        &mut self,
        parent: CapabilityDescriptor,
        requested_rights: Rights,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.copy(parent, requested_rights)
    }

    pub fn copy(
        &mut self,
        source: CapabilityDescriptor,
        requested_rights: Rights,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.insert_derived_capability(source, requested_rights, MintParams::None)
    }

    pub fn mint(
        &mut self,
        source: CapabilityDescriptor,
        requested_rights: Rights,
        params: MintParams,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.insert_derived_capability(source, requested_rights, params)
    }

    pub fn retype_untyped(
        &mut self,
        source: CapabilityDescriptor,
        target: RetypeTarget,
    ) -> Result<CapabilityDescriptor, CapError> {
        let destination = RetypeDestination::single(self.alloc_slot_id());
        let result = self.retype_untyped_into(source, target, destination)?;
        Ok(result
            .descriptors
            .into_iter()
            .next()
            .expect("single-slot retype must return one descriptor"))
    }

    pub fn retype_untyped_into(
        &mut self,
        source: CapabilityDescriptor,
        target: RetypeTarget,
        destination: RetypeDestination,
    ) -> Result<RetypeResult, CapError> {
        let allocation = self.validate_retype_untyped_into(source, &target, destination)?;
        let mut descriptors = Vec::new();
        let mut objects = Vec::new();
        for offset in 0..destination.count {
            let capability = target.clone().into_capability();
            let slot = SlotId(destination.start.raw() + offset as u64);
            let descriptor =
                self.insert_retyped_capability(source.slot, slot, capability, allocation)?;
            objects.push(self.lookup(descriptor)?.object);
            descriptors.push(descriptor);
        }
        Ok(RetypeResult {
            descriptors,
            objects,
        })
    }

    pub fn preview_retype_untyped(
        &self,
        source: CapabilityDescriptor,
        target: &RetypeTarget,
    ) -> Result<ObjectId, CapError> {
        self.validate_retype_untyped(source, target)?;
        Ok(ObjectId(self.next_object))
    }

    pub fn preview_retype_untyped_into(
        &self,
        source: CapabilityDescriptor,
        target: &RetypeTarget,
        destination: RetypeDestination,
    ) -> Result<Vec<ObjectId>, CapError> {
        self.validate_retype_untyped_into(source, target, destination)?;
        Ok((0..destination.count)
            .map(|offset| ObjectId(self.next_object + offset as u64))
            .collect())
    }

    pub fn validate_reply_capability(
        &self,
        reply_object: ObjectId,
        capability: &ReplyCap,
    ) -> Result<(), CapError> {
        self.validate_reply_object(reply_object)?;
        validate_capability_rights(&Capability::Reply(capability.clone()))
    }

    pub fn validate_reply_object(&self, reply_object: ObjectId) -> Result<(), CapError> {
        let object = self.object(reply_object)?;
        if object.destroyed {
            return Err(CapError::ObjectDestroyed(reply_object));
        }
        if object.kind != ObjectKind::Reply {
            return Err(CapError::WrongCapability {
                expected: ObjectKind::Reply,
                actual: object.kind.clone(),
            });
        }

        Ok(())
    }

    pub fn insert_reply_capability(
        &mut self,
        reply_object: ObjectId,
        capability: ReplyCap,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.validate_reply_capability(reply_object, &capability)?;
        let generation = self
            .object(reply_object)
            .expect("validated reply object must remain in CSpace")
            .generation;
        Ok(self.insert_root_slot(reply_object, Capability::Reply(capability), generation))
    }

    pub fn move_capability(
        &mut self,
        source: CapabilityDescriptor,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.validate_descriptor(source)?;

        let destination = self.alloc_slot_id();
        let destination_generation = self.slot_generation_for_insert(destination);
        self.detach_reused_slot(destination);

        let moved_from = self
            .slots
            .get_mut(&source.slot)
            .expect("validated source slot must remain in CSpace during move");
        let mut moved = moved_from.clone();
        moved_from.alive = false;
        moved_from.slot_generation += 1;
        moved_from.parent = None;
        moved_from.children.clear();
        moved_from.mdb = MdbNode::default();
        moved.slot_generation = destination_generation;

        if let Some(parent) = moved.parent {
            let parent_slot = self
                .slots
                .get_mut(&parent)
                .expect("moved capability parent slot must remain in CSpace");
            parent_slot.children.remove(&source.slot);
            parent_slot.children.insert(destination);
        }

        let children = moved.children.clone();
        for child in children {
            self.slots
                .get_mut(&child)
                .expect("moved capability child slot must remain in CSpace")
                .parent = Some(destination);
        }

        if let Some(prev) = moved.mdb.prev {
            self.slots
                .get_mut(&prev)
                .expect("moved capability previous MDB slot must remain in CSpace")
                .mdb
                .next = Some(destination);
        }
        if let Some(next) = moved.mdb.next {
            self.slots
                .get_mut(&next)
                .expect("moved capability next MDB slot must remain in CSpace")
                .mdb
                .prev = Some(destination);
        }

        self.slots.insert(destination, moved);
        self.free_slots.push(source.slot);

        Ok(CapabilityDescriptor {
            slot: destination,
            slot_generation: destination_generation,
        })
    }

    pub fn lookup(&self, descriptor: CapabilityDescriptor) -> Result<CapabilityView, CapError> {
        let (slot, object) = self.validated_slot(descriptor)?;

        Ok(CapabilityView {
            object: slot.object,
            object_kind: object.kind.clone(),
            capability: slot.capability.clone(),
            rights: slot.rights,
            descriptor,
            parent: slot.parent,
        })
    }

    pub fn delete(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<CapabilityDeletion, CapError> {
        self.validate_descriptor(descriptor)?;
        let final_object = self
            .is_final_capability(descriptor.slot)
            .then(|| self.live_slot(descriptor.slot).map(|slot| slot.object))
            .transpose()?;
        self.delete_slot(descriptor.slot);
        Ok(CapabilityDeletion { final_object })
    }

    pub fn consume_reply_cap(&mut self, descriptor: CapabilityDescriptor) -> Result<(), CapError> {
        self.validate_descriptor(descriptor)?;

        let slot = self.live_slot(descriptor.slot)?;
        if !matches!(slot.capability, Capability::Reply(_)) {
            return Err(CapError::WrongCapability {
                expected: ObjectKind::Reply,
                actual: capability_kind(&slot.capability),
            });
        }

        self.delete_slot(descriptor.slot);
        Ok(())
    }

    pub fn revoke_descendants(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<CapabilityRevocation, CapError> {
        let (target_slot, _) = self.validated_slot(descriptor)?;
        let target_object = target_slot.object;
        let mut revoked_object_ids = BTreeSet::new();
        let mut final_object_ids = BTreeSet::new();

        loop {
            let Some(next_slot_id) = self
                .slots
                .get(&descriptor.slot)
                .and_then(|slot| slot.mdb.next)
            else {
                break;
            };
            let Some(next_slot) = self.slots.get(&next_slot_id).filter(|slot| slot.alive) else {
                break;
            };
            if !self.is_mdb_parent_of(descriptor.slot, next_slot_id) {
                break;
            }

            if self.is_final_capability(next_slot_id) {
                final_object_ids.insert(next_slot.object);
            }
            if next_slot.object != target_object {
                revoked_object_ids.insert(next_slot.object);
            }

            self.delete_slot(next_slot_id);
        }
        for object in &revoked_object_ids {
            self.untyped_allocations.remove(object);
        }
        self.reset_untyped_allocation(descriptor.slot);

        Ok(CapabilityRevocation {
            revoked_objects: final_object_ids
                .into_iter()
                .chain(revoked_object_ids)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        })
    }

    pub fn object_has_live_cap(&self, object: ObjectId) -> bool {
        self.slots
            .values()
            .any(|slot| slot.alive && slot.object == object)
    }

    fn is_final_capability(&self, slot: SlotId) -> bool {
        let Some(slot_ref) = self.slots.get(&slot).filter(|slot| slot.alive) else {
            return false;
        };
        if let Some(prev) = slot_ref.mdb.prev
            && let Some(prev_slot) = self.slots.get(&prev).filter(|slot| slot.alive)
            && same_object_as(&prev_slot.capability, &slot_ref.capability)
            && prev_slot.object == slot_ref.object
        {
            return false;
        }
        if let Some(next) = slot_ref.mdb.next
            && let Some(next_slot) = self.slots.get(&next).filter(|slot| slot.alive)
            && same_object_as(&next_slot.capability, &slot_ref.capability)
            && next_slot.object == slot_ref.object
        {
            return false;
        }

        true
    }

    fn is_mdb_parent_of(&self, parent: SlotId, child: SlotId) -> bool {
        let Some(parent_slot) = self.slots.get(&parent).filter(|slot| slot.alive) else {
            return false;
        };
        let Some(child_slot) = self.slots.get(&child).filter(|slot| slot.alive) else {
            return false;
        };

        if !parent_slot.mdb.revocable {
            return false;
        }
        if !same_region_as(&parent_slot.capability, &child_slot.capability) {
            return false;
        }

        match (&parent_slot.capability, &child_slot.capability) {
            (Capability::Endpoint(parent_cap), Capability::Endpoint(child_cap)) => {
                if parent_cap.badge == 0 {
                    true
                } else {
                    child_cap.badge == parent_cap.badge && !child_slot.mdb.first_badged
                }
            }
            (Capability::Notification(parent_cap), Capability::Notification(child_cap)) => {
                if parent_cap.badge == 0 {
                    true
                } else {
                    child_cap.badge == parent_cap.badge && !child_slot.mdb.first_badged
                }
            }
            _ => true,
        }
    }

    #[cfg(test)]
    pub(crate) fn bump_generation(&mut self, object: ObjectId) -> Result<u64, CapError> {
        let kernel_object = self.object_mut(object)?;
        kernel_object.generation += 1;
        Ok(kernel_object.generation)
    }

    #[cfg(test)]
    pub(crate) fn destroy_object(&mut self, object: ObjectId) -> Result<(), CapError> {
        let kernel_object = self.object_mut(object)?;
        kernel_object.destroyed = true;
        kernel_object.generation += 1;

        let slots_to_remove: Vec<_> = self
            .slots
            .iter()
            .filter_map(|(slot_id, slot)| (slot.object == object).then_some(*slot_id))
            .collect();
        for slot in slots_to_remove {
            self.delete_slot(slot);
        }

        Ok(())
    }

    #[cfg(test)]
    pub fn object_of(&self, descriptor: CapabilityDescriptor) -> Result<ObjectId, CapError> {
        let (slot, _) = self.validated_slot(descriptor)?;
        Ok(slot.object)
    }

    #[cfg(test)]
    pub fn slot_exists(&self, slot: SlotId) -> bool {
        self.slots.get(&slot).is_some_and(|slot| slot.alive)
    }

    fn insert_derived_capability(
        &mut self,
        parent: CapabilityDescriptor,
        requested_rights: Rights,
        params: MintParams,
    ) -> Result<CapabilityDescriptor, CapError> {
        let (object, object_generation_snapshot, parent_capability) = {
            let (parent_slot, _) = self.validated_slot(parent)?;
            if !requested_rights.is_subset_of(parent_slot.rights) {
                return Err(CapError::RightsEscalation {
                    parent: parent.slot,
                    parent_rights: parent_slot.rights,
                    requested_rights,
                });
            }
            if matches!(parent_slot.capability, Capability::Untyped(_))
                && parent_slot
                    .mdb
                    .next
                    .is_some_and(|next| self.is_mdb_parent_of(parent.slot, next))
            {
                return Err(CapError::CapabilityNotDerivable {
                    parent: parent.slot,
                    capability: parent_slot.capability.clone(),
                });
            }
            (
                parent_slot.object,
                parent_slot.object_generation_snapshot,
                parent_slot.capability.clone(),
            )
        };
        let parent_slot_id = parent.slot;
        let capability =
            mint_capability(parent_slot_id, &parent_capability, requested_rights, params)?;
        let slot = self.alloc_slot_id();
        let slot_generation = self.slot_generation_for_insert(slot);
        self.detach_reused_slot(slot);
        let mdb = self.cte_insert_mdb(parent_slot_id, slot, &capability, &parent_capability);
        self.slots.insert(
            slot,
            CapabilitySlot {
                object,
                capability: capability.clone(),
                rights: capability_rights(&capability),
                slot_generation,
                object_generation_snapshot,
                parent: Some(parent_slot_id),
                children: BTreeSet::new(),
                mdb,
                alive: true,
            },
        );
        self.slots
            .get_mut(&parent_slot_id)
            .expect("validated parent slot must remain in CSpace during derivation")
            .children
            .insert(slot);

        Ok(CapabilityDescriptor {
            slot,
            slot_generation,
        })
    }

    fn insert_retyped_capability(
        &mut self,
        parent: SlotId,
        slot: SlotId,
        capability: Capability,
        allocation: UntypedAllocationPlan,
    ) -> Result<CapabilityDescriptor, CapError> {
        let kind = capability_kind(&capability);
        let child_untyped_size = match &capability {
            Capability::Untyped(capability) => Some(capability.size_bits),
            _ => None,
        };
        let (object, object_generation) = self.alloc_object(kind);
        self.untyped_allocations
            .get_mut(&allocation.parent_object)
            .expect("validated parent untyped allocation must remain in CSpace")
            .watermark = allocation.next_watermark;
        if let Some(size_bits) = child_untyped_size {
            self.untyped_allocations.insert(
                object,
                UntypedAllocation {
                    size_bits,
                    watermark: 0,
                },
            );
        }
        let slot_generation = self.slot_generation_for_insert(slot);
        self.detach_reused_slot(slot);
        let parent_capability = self
            .slots
            .get(&parent)
            .expect("validated parent slot must remain in CSpace during retype")
            .capability
            .clone();
        let mdb = self.cte_insert_mdb(parent, slot, &capability, &parent_capability);
        self.slots.insert(
            slot,
            CapabilitySlot {
                object,
                rights: capability_rights(&capability),
                capability,
                slot_generation,
                object_generation_snapshot: object_generation,
                parent: Some(parent),
                children: BTreeSet::new(),
                mdb,
                alive: true,
            },
        );
        self.slots
            .get_mut(&parent)
            .expect("validated parent slot must remain in CSpace during retype")
            .children
            .insert(slot);

        Ok(CapabilityDescriptor {
            slot,
            slot_generation,
        })
    }

    fn validate_retype_untyped(
        &self,
        source: CapabilityDescriptor,
        target: &RetypeTarget,
    ) -> Result<UntypedAllocationPlan, CapError> {
        self.validate_retype_untyped_capacity(source, target, 1)
    }

    fn validate_retype_untyped_into(
        &self,
        source: CapabilityDescriptor,
        target: &RetypeTarget,
        destination: RetypeDestination,
    ) -> Result<UntypedAllocationPlan, CapError> {
        if destination.count == 0 {
            return Err(CapError::EmptyRetypeWindow);
        }
        for offset in 0..destination.count {
            let slot = SlotId(destination.start.raw() + offset as u64);
            if self.slots.get(&slot).is_some_and(|slot| slot.alive) {
                return Err(CapError::SlotOccupied(slot));
            }
        }
        self.validate_retype_untyped_capacity(source, target, destination.count)
    }

    fn validate_retype_untyped_capacity(
        &self,
        source: CapabilityDescriptor,
        target: &RetypeTarget,
        count: usize,
    ) -> Result<UntypedAllocationPlan, CapError> {
        let (source_size, source_object) = {
            let (parent_slot, _) = self.validated_slot(source)?;
            let Capability::Untyped(parent_cap) = &parent_slot.capability else {
                return Err(CapError::WrongCapability {
                    expected: ObjectKind::Untyped,
                    actual: capability_kind(&parent_slot.capability),
                });
            };
            (parent_cap.size_bits, parent_slot.object)
        };

        let requested_size = target.minimum_size_bits();
        if requested_size > source_size {
            return Err(CapError::InvalidRetypeSize {
                parent: source.slot,
                requested: requested_size,
                source: source_size,
            });
        }

        target.validate_rights()?;

        let allocation = self
            .untyped_allocations
            .get(&source_object)
            .expect("validated Untyped cap must have allocation metadata");
        let next_watermark = allocation.next_watermark(source.slot, requested_size, count)?;
        Ok(UntypedAllocationPlan {
            parent_object: source_object,
            next_watermark,
        })
    }

    fn alloc_object(&mut self, kind: ObjectKind) -> (ObjectId, u64) {
        let object = ObjectId(self.next_object);
        self.next_object += 1;
        let generation = 1;
        self.objects.insert(
            object,
            KernelObject {
                kind,
                generation,
                destroyed: false,
            },
        );
        (object, generation)
    }

    fn insert_root_slot(
        &mut self,
        object: ObjectId,
        capability: Capability,
        generation: u64,
    ) -> CapabilityDescriptor {
        let untyped_size = match &capability {
            Capability::Untyped(capability) => Some(capability.size_bits),
            _ => None,
        };
        let slot = self.alloc_slot_id();
        let slot_generation = self.slot_generation_for_insert(slot);
        self.detach_reused_slot(slot);
        self.slots.insert(
            slot,
            CapabilitySlot {
                object,
                rights: capability_rights(&capability),
                capability,
                slot_generation,
                object_generation_snapshot: generation,
                parent: None,
                children: BTreeSet::new(),
                mdb: MdbNode {
                    revocable: true,
                    first_badged: true,
                    ..MdbNode::default()
                },
                alive: true,
            },
        );
        if let Some(size_bits) = untyped_size {
            self.untyped_allocations.insert(
                object,
                UntypedAllocation {
                    size_bits,
                    watermark: 0,
                },
            );
        }

        CapabilityDescriptor {
            slot,
            slot_generation,
        }
    }

    fn alloc_slot_id(&mut self) -> SlotId {
        if let Some(slot) = self.free_slots.pop() {
            return slot;
        }

        let slot = SlotId(self.next_slot);
        self.next_slot += 1;
        slot
    }

    fn slot_generation_for_insert(&self, slot: SlotId) -> u64 {
        self.slots
            .get(&slot)
            .map_or(1, |slot| slot.slot_generation + 1)
    }

    fn validated_slot(
        &self,
        descriptor: CapabilityDescriptor,
    ) -> Result<(&CapabilitySlot, &KernelObject), CapError> {
        let slot = self.live_slot(descriptor.slot)?;
        let object = self.object(slot.object)?;

        if object.destroyed {
            return Err(CapError::ObjectDestroyed(slot.object));
        }

        if descriptor.slot_generation != slot.slot_generation {
            return Err(CapError::StaleDescriptor {
                slot: descriptor.slot,
                expected_generation: slot.slot_generation,
                actual_generation: descriptor.slot_generation,
            });
        }

        if slot.object_generation_snapshot != object.generation {
            return Err(CapError::StaleDescriptor {
                slot: descriptor.slot,
                expected_generation: object.generation,
                actual_generation: slot.object_generation_snapshot,
            });
        }

        Ok((slot, object))
    }

    fn validate_descriptor(&self, descriptor: CapabilityDescriptor) -> Result<(), CapError> {
        self.validated_slot(descriptor).map(|_| ())
    }

    fn live_slot(&self, slot: SlotId) -> Result<&CapabilitySlot, CapError> {
        let slot_ref = self.slots.get(&slot).ok_or(CapError::SlotNotFound(slot))?;
        if !slot_ref.alive {
            return Err(CapError::SlotNotFound(slot));
        }

        Ok(slot_ref)
    }

    fn object(&self, object: ObjectId) -> Result<&KernelObject, CapError> {
        self.objects
            .get(&object)
            .ok_or(CapError::ObjectNotFound(object))
    }

    #[cfg(test)]
    fn object_mut(&mut self, object: ObjectId) -> Result<&mut KernelObject, CapError> {
        self.objects
            .get_mut(&object)
            .ok_or(CapError::ObjectNotFound(object))
    }

    fn delete_slot(&mut self, slot: SlotId) {
        let Some(removed) = self.slots.get_mut(&slot) else {
            return;
        };

        if !removed.alive {
            return;
        }

        removed.alive = false;
        let reusable = removed.children.is_empty();
        let _ = removed;
        self.empty_mdb_slot(slot);
        if reusable {
            self.free_slots.push(slot);
        }
    }

    fn detach_reused_slot(&mut self, slot: SlotId) {
        let Some(old_parent) = self.slots.get(&slot).and_then(|slot| slot.parent) else {
            return;
        };

        if let Some(parent) = self.slots.get_mut(&old_parent) {
            parent.children.remove(&slot);
        }
        self.empty_mdb_slot(slot);
    }

    fn cte_insert_mdb(
        &mut self,
        parent: SlotId,
        slot: SlotId,
        new_cap: &Capability,
        parent_cap: &Capability,
    ) -> MdbNode {
        let next = self.slots.get(&parent).and_then(|parent| parent.mdb.next);
        let revocable = is_cap_revocable(new_cap, parent_cap);
        if let Some(next) = next {
            self.slots
                .get_mut(&next)
                .expect("parent MDB next slot must remain in CSpace")
                .mdb
                .prev = Some(slot);
        }
        self.slots
            .get_mut(&parent)
            .expect("validated parent slot must remain in CSpace during cteInsert")
            .mdb
            .next = Some(slot);

        MdbNode {
            prev: Some(parent),
            next,
            revocable,
            first_badged: revocable,
        }
    }

    fn empty_mdb_slot(&mut self, slot: SlotId) {
        let Some(mdb) = self.slots.get(&slot).map(|slot| slot.mdb) else {
            return;
        };
        if let Some(prev) = mdb.prev
            && let Some(prev_slot) = self.slots.get_mut(&prev)
        {
            prev_slot.mdb.next = mdb.next;
        }
        if let Some(next) = mdb.next
            && let Some(next_slot) = self.slots.get_mut(&next)
        {
            next_slot.mdb.prev = mdb.prev;
            next_slot.mdb.first_badged |= mdb.first_badged;
        }
        if let Some(slot_ref) = self.slots.get_mut(&slot) {
            slot_ref.mdb = MdbNode::default();
        }
    }

    fn reset_untyped_allocation(&mut self, slot: SlotId) {
        let Some(slot_ref) = self.slots.get(&slot) else {
            return;
        };
        if !matches!(slot_ref.capability, Capability::Untyped(_)) {
            return;
        };

        if let Some(allocation) = self.untyped_allocations.get_mut(&slot_ref.object) {
            allocation.watermark = 0;
        }
    }
}

impl UntypedAllocation {
    fn next_watermark(
        self,
        parent: SlotId,
        requested_size_bits: u8,
        count: usize,
    ) -> Result<u128, CapError> {
        let source_bytes =
            bytes_for_size_bits(self.size_bits).ok_or(CapError::UntypedCapacityExhausted {
                parent,
                requested: requested_size_bits,
                source: self.size_bits,
            })?;
        let requested_bytes =
            bytes_for_size_bits(requested_size_bits).ok_or(CapError::UntypedCapacityExhausted {
                parent,
                requested: requested_size_bits,
                source: self.size_bits,
            })?;
        let requested_total = requested_bytes.checked_mul(count as u128).ok_or(
            CapError::UntypedCapacityExhausted {
                parent,
                requested: requested_size_bits,
                source: self.size_bits,
            },
        )?;
        let aligned_watermark = align_up(self.watermark, requested_bytes).ok_or(
            CapError::UntypedCapacityExhausted {
                parent,
                requested: requested_size_bits,
                source: self.size_bits,
            },
        )?;
        let next_watermark = aligned_watermark.checked_add(requested_total).ok_or(
            CapError::UntypedCapacityExhausted {
                parent,
                requested: requested_size_bits,
                source: self.size_bits,
            },
        )?;

        if next_watermark <= source_bytes {
            return Ok(next_watermark);
        }

        Err(CapError::UntypedCapacityExhausted {
            parent,
            requested: requested_size_bits,
            source: self.size_bits,
        })
    }
}

fn bytes_for_size_bits(size_bits: u8) -> Option<u128> {
    if size_bits == 0 {
        return Some(MIN_RETYPE_BYTES);
    }

    1u128.checked_shl(size_bits.into())
}

fn align_up(value: u128, alignment: u128) -> Option<u128> {
    let mask = alignment.checked_sub(1)?;
    value.checked_add(mask).map(|value| value & !mask)
}

fn capability_kind(capability: &Capability) -> ObjectKind {
    match capability {
        Capability::Endpoint(_) => ObjectKind::Endpoint,
        Capability::Frame(_) => ObjectKind::Frame,
        Capability::CNode(_) => ObjectKind::CNode,
        Capability::Untyped(_) => ObjectKind::Untyped,
        Capability::Tcb(_) => ObjectKind::Tcb,
        Capability::Notification(_) => ObjectKind::Notification,
        Capability::Reply(_) => ObjectKind::Reply,
    }
}

fn capability_rights(capability: &Capability) -> Rights {
    match capability {
        Capability::Endpoint(cap) => cap.rights,
        Capability::Frame(cap) => cap.rights,
        Capability::CNode(_) => Rights::NONE,
        Capability::Untyped(_) => Rights::NONE,
        Capability::Tcb(cap) => cap.rights,
        Capability::Notification(cap) => cap.rights,
        Capability::Reply(_) => Rights::NONE,
    }
}

fn allowed_rights_for(kind: ObjectKind) -> Rights {
    match kind {
        ObjectKind::Endpoint => ENDPOINT_ALLOWED_RIGHTS,
        ObjectKind::Frame => FRAME_ALLOWED_RIGHTS,
        ObjectKind::CNode => Rights::NONE,
        ObjectKind::Untyped => UNTYPED_ALLOWED_RIGHTS,
        ObjectKind::Tcb => TCB_ALLOWED_RIGHTS,
        ObjectKind::Notification => NOTIFICATION_ALLOWED_RIGHTS,
        ObjectKind::Reply => REPLY_ALLOWED_RIGHTS,
    }
}

fn validate_capability_rights(capability: &Capability) -> Result<(), CapError> {
    let object = capability_kind(capability);
    let requested_rights = capability_rights(capability);
    validate_rights_for(object, requested_rights)
}

fn validate_rights_for(object: ObjectKind, requested_rights: Rights) -> Result<(), CapError> {
    let allowed_rights = allowed_rights_for(object.clone());

    if requested_rights.is_subset_of(allowed_rights) {
        return Ok(());
    }

    Err(CapError::InvalidRights {
        object,
        requested_rights,
        allowed_rights,
    })
}

fn is_cap_revocable(new_cap: &Capability, parent_cap: &Capability) -> bool {
    match (new_cap, parent_cap) {
        (Capability::Endpoint(new_cap), Capability::Endpoint(parent_cap)) => {
            new_cap.badge != parent_cap.badge
        }
        (Capability::Notification(new_cap), Capability::Notification(parent_cap)) => {
            new_cap.badge != parent_cap.badge
        }
        (_, Capability::Untyped(_)) | (Capability::Untyped(_), _) => true,
        _ => false,
    }
}

fn same_region_as(parent: &Capability, child: &Capability) -> bool {
    match (parent, child) {
        (Capability::Untyped(parent), Capability::Untyped(child)) => {
            child.size_bits <= parent.size_bits
        }
        (Capability::Untyped(_), child) => capability_has_physical_region(child),
        (Capability::Endpoint(_), Capability::Endpoint(_))
        | (Capability::Notification(_), Capability::Notification(_))
        | (Capability::Tcb(_), Capability::Tcb(_))
        | (Capability::Reply(_), Capability::Reply(_)) => true,
        (Capability::Frame(_), Capability::Frame(_)) => true,
        (Capability::CNode(parent), Capability::CNode(child)) => parent.radix == child.radix,
        _ => false,
    }
}

fn same_object_as(left: &Capability, right: &Capability) -> bool {
    if matches!(left, Capability::Untyped(_)) {
        return false;
    }

    same_region_as(left, right)
}

fn capability_has_physical_region(capability: &Capability) -> bool {
    matches!(
        capability,
        Capability::Endpoint(_)
            | Capability::Frame(_)
            | Capability::CNode(_)
            | Capability::Untyped(_)
            | Capability::Tcb(_)
            | Capability::Notification(_)
            | Capability::Reply(_)
    )
}

fn mint_capability(
    parent_slot: SlotId,
    parent: &Capability,
    requested_rights: Rights,
    params: MintParams,
) -> Result<Capability, CapError> {
    match parent {
        Capability::Endpoint(cap) => {
            let badge = match params {
                MintParams::None => cap.badge,
                MintParams::CapData { preserve, data } => {
                    if preserve || cap.badge != 0 {
                        return Err(CapError::CapabilityNotMintable {
                            parent: parent_slot,
                            capability: Capability::Endpoint(cap.clone()),
                            params,
                        });
                    }
                    data
                }
            };
            let capability = Capability::Endpoint(EndpointCap {
                badge,
                rights: requested_rights,
            });
            Ok(capability)
        }
        Capability::Frame(cap) => match params {
            MintParams::None => Ok(Capability::Frame(FrameCap {
                rights: requested_rights,
            })),
            MintParams::CapData { .. } => Err(CapError::CapabilityNotMintable {
                parent: parent_slot,
                capability: Capability::Frame(cap.clone()),
                params,
            }),
        },
        Capability::CNode(cap) => match params {
            MintParams::None => Ok(Capability::CNode(cap.clone())),
            MintParams::CapData { preserve: _, data } => {
                Ok(Capability::CNode(update_cnode_cap_data(cap.clone(), data)))
            }
        },
        Capability::Untyped(cap) => match params {
            MintParams::None => Ok(Capability::Untyped(UntypedCap {
                size_bits: cap.size_bits,
            })),
            MintParams::CapData { .. } => Err(CapError::CapabilityNotMintable {
                parent: parent_slot,
                capability: Capability::Untyped(cap.clone()),
                params,
            }),
        },
        Capability::Tcb(cap) => match params {
            MintParams::None => Ok(Capability::Tcb(TcbCap {
                rights: requested_rights,
            })),
            MintParams::CapData { .. } => Err(CapError::CapabilityNotMintable {
                parent: parent_slot,
                capability: Capability::Tcb(cap.clone()),
                params,
            }),
        },
        Capability::Notification(cap) => {
            let badge = match params {
                MintParams::None => cap.badge,
                MintParams::CapData { preserve, data } => {
                    if preserve || cap.badge != 0 {
                        return Err(CapError::CapabilityNotMintable {
                            parent: parent_slot,
                            capability: Capability::Notification(cap.clone()),
                            params,
                        });
                    }
                    data
                }
            };
            let capability = Capability::Notification(NotificationCap {
                badge,
                rights: requested_rights,
            });
            Ok(capability)
        }
        Capability::Reply(cap) => Err(CapError::CapabilityNotDerivable {
            parent: parent_slot,
            capability: Capability::Reply(cap.clone()),
        }),
    }
}

fn update_cnode_cap_data(mut cap: CNodeCap, data: u64) -> CNodeCap {
    let guard_size = (data & 0xff) as u8;
    let guard = data >> 8;
    cap.guard_size = guard_size;
    cap.guard = guard & guard_mask(guard_size);
    cap
}

fn guard_mask(guard_size: u8) -> u64 {
    if guard_size >= u64::BITS as u8 {
        return u64::MAX;
    }

    (1u64 << guard_size) - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(rights: Rights) -> Capability {
        Capability::Endpoint(EndpointCap { badge: 0, rights })
    }

    fn badged_endpoint(rights: Rights, badge: u64) -> Capability {
        Capability::Endpoint(EndpointCap { badge, rights })
    }

    fn frame(rights: Rights) -> Capability {
        Capability::Frame(FrameCap { rights })
    }

    fn untyped(size_bits: u8) -> Capability {
        Capability::Untyped(UntypedCap { size_bits })
    }

    fn tcb(rights: Rights) -> Capability {
        Capability::Tcb(TcbCap { rights })
    }

    fn notification(rights: Rights) -> Capability {
        Capability::Notification(NotificationCap { badge: 1, rights })
    }

    fn badged_notification(rights: Rights, badge: u64) -> Capability {
        Capability::Notification(NotificationCap { badge, rights })
    }

    fn reply(caller: ObjectId, target: ObjectId, can_grant: bool) -> Capability {
        Capability::Reply(ReplyCap {
            caller,
            target,
            can_grant,
        })
    }

    // CapabilitySpace tests protect authority, slot lineage, badge minting,
    // stale descriptor handling, and retype lineage. Runtime ObjectTable entries
    // and executor transaction ordering are tested at the host integration layer.

    #[test]
    fn root_capability_can_be_created_and_looked_up() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();

        let view = cspace.lookup(root).unwrap();

        assert_eq!(view.object_kind, ObjectKind::Endpoint);
        assert_eq!(view.capability, endpoint(ENDPOINT_ALLOWED_RIGHTS));
        assert_eq!(view.rights, ENDPOINT_ALLOWED_RIGHTS);
        assert_eq!(view.descriptor, root);
        assert_eq!(view.parent, None);
    }

    #[test]
    fn initial_capability_rejects_rights_outside_object_policy() {
        let mut cspace = CapabilitySpace::new();

        assert_eq!(
            cspace.insert_initial_capability(frame(Rights::READ | Rights::GRANT_REPLY)),
            Err(CapError::InvalidRights {
                object: ObjectKind::Frame,
                requested_rights: Rights::READ | Rights::GRANT_REPLY,
                allowed_rights: Rights::READ | Rights::WRITE | Rights::EXECUTE,
            })
        );
        assert_eq!(
            cspace.insert_initial_capability(tcb(Rights::READ)),
            Err(CapError::InvalidRights {
                object: ObjectKind::Tcb,
                requested_rights: Rights::READ,
                allowed_rights: Rights::MANAGE,
            })
        );
    }

    #[test]
    fn copy_preserves_endpoint_badge_and_reduces_rights() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(badged_endpoint(Rights::READ | Rights::WRITE, 0x44))
            .unwrap();

        let copy = cspace.copy(root, Rights::READ).unwrap();
        let view = cspace.lookup(copy).unwrap();

        assert_eq!(view.capability, badged_endpoint(Rights::READ, 0x44));
        assert_eq!(view.parent, Some(root.slot));
    }

    #[test]
    fn mint_can_set_endpoint_badge_without_escalating_rights() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE))
            .unwrap();

        let minted = cspace
            .mint(root, Rights::READ, MintParams::badge(0x55))
            .unwrap();
        let view = cspace.lookup(minted).unwrap();

        assert_eq!(view.capability, badged_endpoint(Rights::READ, 0x55));
        assert_eq!(view.parent, Some(root.slot));
    }

    #[test]
    fn mint_can_set_notification_badge() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(badged_notification(Rights::READ | Rights::WRITE, 0))
            .unwrap();

        let minted = cspace
            .mint(root, Rights::WRITE, MintParams::badge(0x77))
            .unwrap();
        let view = cspace.lookup(minted).unwrap();

        assert_eq!(view.capability, badged_notification(Rights::WRITE, 0x77));
        assert_eq!(view.parent, Some(root.slot));
    }

    #[test]
    fn badge_mint_is_rejected_for_non_badge_capabilities() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE))
            .unwrap();

        assert_eq!(
            cspace.mint(root, Rights::READ, MintParams::badge(0x66)),
            Err(CapError::CapabilityNotMintable {
                parent: root.slot,
                capability: frame(Rights::READ | Rights::WRITE),
                params: MintParams::badge(0x66),
            })
        );
    }

    #[test]
    fn badge_mint_is_rejected_when_badge_already_present() {
        let mut cspace = CapabilitySpace::new();
        let endpoint_root = cspace
            .insert_initial_capability(badged_endpoint(Rights::READ | Rights::WRITE, 1))
            .unwrap();
        let notification_root = cspace
            .insert_initial_capability(badged_notification(Rights::READ | Rights::WRITE, 1))
            .unwrap();

        assert_eq!(
            cspace.mint(endpoint_root, Rights::READ, MintParams::badge(2)),
            Err(CapError::CapabilityNotMintable {
                parent: endpoint_root.slot,
                capability: badged_endpoint(Rights::READ | Rights::WRITE, 1),
                params: MintParams::badge(2),
            })
        );
        assert_eq!(
            cspace.mint(notification_root, Rights::READ, MintParams::badge(2)),
            Err(CapError::CapabilityNotMintable {
                parent: notification_root.slot,
                capability: badged_notification(Rights::READ | Rights::WRITE, 1),
                params: MintParams::badge(2),
            })
        );
    }

    #[test]
    fn failed_mint_does_not_consume_reusable_slot_or_change_lineage() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE))
            .unwrap();
        let child = cspace.copy(root, Rights::READ).unwrap();

        cspace.delete(child).unwrap();

        assert_eq!(
            cspace.mint(root, Rights::READ, MintParams::badge(0x66)),
            Err(CapError::CapabilityNotMintable {
                parent: root.slot,
                capability: frame(Rights::READ | Rights::WRITE),
                params: MintParams::badge(0x66),
            })
        );

        let reused = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();

        assert_eq!(reused.slot, child.slot);
        assert_eq!(cspace.lookup(root).unwrap().parent, None);
    }

    #[test]
    fn move_transfers_slot_without_creating_derivation() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace.copy(root, Rights::READ | Rights::WRITE).unwrap();
        let grandchild = cspace.copy(child, Rights::READ).unwrap();

        let moved = cspace.move_capability(child).unwrap();

        assert_ne!(child.slot, moved.slot);
        assert_eq!(
            cspace.lookup(child),
            Err(CapError::SlotNotFound(child.slot))
        );
        assert_eq!(cspace.lookup(moved).unwrap().parent, Some(root.slot));
        assert_eq!(cspace.lookup(grandchild).unwrap().parent, Some(moved.slot));

        cspace.revoke_descendants(root).unwrap();

        assert_eq!(
            cspace.lookup(moved),
            Err(CapError::SlotNotFound(moved.slot))
        );
        assert_eq!(
            cspace.lookup(grandchild),
            Err(CapError::SlotNotFound(grandchild.slot))
        );
    }

    #[test]
    fn revoke_copied_typed_cap_does_not_remove_copy_descendants() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let copy = cspace.copy(root, Rights::READ | Rights::WRITE).unwrap();
        let copy_child = cspace.copy(copy, Rights::READ).unwrap();

        cspace.revoke_descendants(copy).unwrap();

        assert!(cspace.lookup(copy_child).is_ok());
    }

    #[test]
    fn untyped_retype_creates_child_object() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();

        let frame = cspace
            .retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ | Rights::WRITE,
                },
            )
            .unwrap();
        let view = cspace.lookup(frame).unwrap();

        assert_eq!(view.object_kind, ObjectKind::Frame);
        assert_eq!(view.capability, frame_cap(Rights::READ | Rights::WRITE));
        assert_eq!(view.parent, Some(root.slot));
        assert_ne!(
            cspace.object_of(root).unwrap(),
            cspace.object_of(frame).unwrap()
        );
    }

    #[test]
    fn untyped_retype_can_create_cnode_tcb_and_notification() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();

        let cnode = cspace
            .retype_untyped(root, RetypeTarget::CNode { radix: 4 })
            .unwrap();
        let tcb_slot = cspace
            .retype_untyped(
                root,
                RetypeTarget::Tcb {
                    rights: Rights::MANAGE,
                },
            )
            .unwrap();
        let notification = cspace
            .retype_untyped(root, RetypeTarget::Notification)
            .unwrap();

        assert_eq!(cspace.lookup(cnode).unwrap().capability, cnode_cap());
        assert_eq!(
            cspace.lookup(tcb_slot).unwrap().capability,
            tcb(Rights::MANAGE)
        );
        assert_eq!(
            cspace.lookup(notification).unwrap().capability,
            Capability::Notification(NotificationCap {
                badge: 0,
                rights: Rights::READ | Rights::WRITE,
            })
        );
    }

    #[test]
    fn untyped_retype_rejects_target_rights_outside_object_policy() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();

        assert_eq!(
            cspace.retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ | Rights::GRANT,
                },
            ),
            Err(CapError::InvalidRights {
                object: ObjectKind::Frame,
                requested_rights: Rights::READ | Rights::GRANT,
                allowed_rights: Rights::READ | Rights::WRITE | Rights::EXECUTE,
            })
        );
    }

    #[test]
    fn untyped_retype_can_create_smaller_untyped() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(16)).unwrap();

        let child = cspace
            .retype_untyped(root, RetypeTarget::Untyped { size_bits: 12 })
            .unwrap();

        assert_eq!(cspace.lookup(child).unwrap().capability, untyped(12));
        assert_eq!(cspace.lookup(child).unwrap().parent, Some(root.slot));
    }

    #[test]
    fn revoke_derived_untyped_descendants_resets_child_capacity() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(16)).unwrap();
        let child = cspace
            .retype_untyped(root, RetypeTarget::Untyped { size_bits: 12 })
            .unwrap();
        let frame = cspace
            .retype_untyped(
                child,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();

        cspace.revoke_descendants(child).unwrap();

        assert_eq!(
            cspace.lookup(frame),
            Err(CapError::SlotNotFound(frame.slot))
        );
        assert!(
            cspace
                .retype_untyped(
                    child,
                    RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                )
                .is_ok()
        );
    }

    #[test]
    fn untyped_retype_rejects_oversized_child() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(11)).unwrap();

        assert_eq!(
            cspace.retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ
                }
            ),
            Err(CapError::InvalidRetypeSize {
                parent: root.slot,
                requested: 12,
                source: 11,
            })
        );
    }

    #[test]
    fn only_untyped_cap_can_retype_objects() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();

        assert_eq!(
            cspace.retype_untyped(root, RetypeTarget::Notification),
            Err(CapError::WrongCapability {
                expected: ObjectKind::Untyped,
                actual: ObjectKind::Endpoint,
            })
        );
    }

    #[test]
    fn revoke_untyped_descendants_removes_retyped_objects() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(13)).unwrap();
        let endpoint = cspace.retype_untyped(root, RetypeTarget::Endpoint).unwrap();
        let frame = cspace
            .retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();

        cspace.revoke_descendants(root).unwrap();

        assert!(cspace.lookup(root).is_ok());
        assert_eq!(
            cspace.lookup(endpoint),
            Err(CapError::SlotNotFound(endpoint.slot))
        );
        assert_eq!(
            cspace.lookup(frame),
            Err(CapError::SlotNotFound(frame.slot))
        );
    }

    #[test]
    fn revoke_untyped_descendants_resets_parent_capacity() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();
        let frame = cspace
            .retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();

        assert_eq!(
            cspace.retype_untyped(root, RetypeTarget::Endpoint),
            Err(CapError::UntypedCapacityExhausted {
                parent: root.slot,
                requested: 4,
                source: 12,
            })
        );

        cspace.revoke_descendants(root).unwrap();
        assert_eq!(
            cspace.lookup(frame),
            Err(CapError::SlotNotFound(frame.slot))
        );

        let recycled = cspace
            .retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();

        assert!(matches!(
            cspace.lookup(frame),
            Err(CapError::SlotNotFound(_)) | Err(CapError::StaleDescriptor { .. })
        ));
        assert_eq!(
            cspace.lookup(recycled).unwrap().object_kind,
            ObjectKind::Frame
        );
    }

    #[test]
    fn revoke_copied_untyped_cap_does_not_remove_parent_allocation_state() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();
        let copied = cspace.copy(root, Rights::NONE).unwrap();

        cspace.revoke_descendants(root).unwrap();

        assert_eq!(
            cspace.lookup(copied),
            Err(CapError::SlotNotFound(copied.slot))
        );
        assert!(
            cspace
                .retype_untyped(
                    root,
                    RetypeTarget::Frame {
                        rights: Rights::READ,
                    },
                )
                .is_ok()
        );
    }

    #[test]
    fn copying_untyped_with_children_is_rejected_without_resetting_capacity() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();
        cspace
            .retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();

        assert_eq!(
            cspace.copy(root, Rights::NONE),
            Err(CapError::CapabilityNotDerivable {
                parent: root.slot,
                capability: untyped(12),
            })
        );
        assert_eq!(
            cspace.retype_untyped(root, RetypeTarget::Endpoint),
            Err(CapError::UntypedCapacityExhausted {
                parent: root.slot,
                requested: 4,
                source: 12,
            })
        );
    }

    #[test]
    fn revoke_nested_untyped_removes_child_allocation_state() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();
        let child = cspace
            .retype_untyped(root, RetypeTarget::Untyped { size_bits: 10 })
            .unwrap();
        cspace
            .retype_untyped(child, RetypeTarget::Notification)
            .unwrap();

        cspace.revoke_descendants(root).unwrap();

        assert_eq!(
            cspace.retype_untyped(child, RetypeTarget::Notification),
            Err(CapError::SlotNotFound(child.slot))
        );
        assert!(
            cspace
                .retype_untyped(root, RetypeTarget::Untyped { size_bits: 12 })
                .is_ok()
        );
    }

    #[test]
    fn untyped_retype_uses_model_object_size_and_alignment() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(12)).unwrap();

        cspace.retype_untyped(root, RetypeTarget::Endpoint).unwrap();
        assert_eq!(
            cspace.retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            ),
            Err(CapError::UntypedCapacityExhausted {
                parent: root.slot,
                requested: 12,
                source: 12,
            })
        );
    }

    #[test]
    fn untyped_retype_into_destination_window_creates_multiple_caps() {
        // Goal: model seL4 invokeUntyped_Retype destination length semantics.
        // Scope: cap-layer transaction with explicit destination slots.
        // Semantics: the whole destination window is checked before capacity is consumed.
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(13)).unwrap();
        let result = cspace
            .retype_untyped_into(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
                RetypeDestination {
                    start: SlotId(20),
                    count: 2,
                },
            )
            .unwrap();

        assert_eq!(result.descriptors.len(), 2);
        assert_eq!(result.descriptors[0].slot, SlotId(20));
        assert_eq!(result.descriptors[1].slot, SlotId(21));
        assert_eq!(
            cspace.lookup(result.descriptors[0]).unwrap().capability,
            frame(Rights::READ)
        );
        assert_eq!(
            cspace.lookup(result.descriptors[1]).unwrap().capability,
            frame(Rights::READ)
        );
        assert_eq!(
            cspace.retype_untyped(root, RetypeTarget::Endpoint),
            Err(CapError::UntypedCapacityExhausted {
                parent: root.slot,
                requested: 4,
                source: 13,
            })
        );
    }

    #[test]
    fn model_sized_kernel_objects_consume_untyped_capacity() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(10)).unwrap();

        cspace
            .retype_untyped(
                root,
                RetypeTarget::Tcb {
                    rights: Rights::MANAGE,
                },
            )
            .unwrap();

        assert_eq!(
            cspace.retype_untyped(root, RetypeTarget::Notification),
            Err(CapError::UntypedCapacityExhausted {
                parent: root.slot,
                requested: 5,
                source: 10,
            })
        );

        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(6)).unwrap();
        cspace
            .retype_untyped(root, RetypeTarget::CNode { radix: 0 })
            .unwrap();

        assert_eq!(
            cspace.retype_untyped(root, RetypeTarget::Notification),
            Err(CapError::UntypedCapacityExhausted {
                parent: root.slot,
                requested: 5,
                source: 6,
            })
        );
    }

    fn frame_cap(rights: Rights) -> Capability {
        Capability::Frame(FrameCap { rights })
    }

    fn cnode_cap() -> Capability {
        Capability::CNode(CNodeCap::new(4))
    }

    #[test]
    fn derivation_rejects_rights_outside_object_policy_at_boundary() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE | Rights::EXECUTE))
            .unwrap();

        assert_eq!(
            cspace.copy(root, Rights::READ | Rights::GRANT),
            Err(CapError::RightsEscalation {
                parent: root.slot,
                parent_rights: Rights::READ | Rights::WRITE | Rights::EXECUTE,
                requested_rights: Rights::READ | Rights::GRANT,
            })
        );

        let cnode = cspace.insert_initial_capability(cnode_cap()).unwrap();
        assert_eq!(
            cspace.copy(cnode, Rights::READ),
            Err(CapError::RightsEscalation {
                parent: cnode.slot,
                parent_rights: Rights::NONE,
                requested_rights: Rights::READ,
            })
        );
    }

    #[test]
    fn deleting_a_slot_only_invalidates_that_slot() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace.derive(root, Rights::READ | Rights::WRITE).unwrap();
        let grandchild = cspace.derive(child, Rights::READ).unwrap();

        cspace.delete(child).unwrap();

        assert!(cspace.lookup(root).is_ok());
        assert_eq!(
            cspace.lookup(child),
            Err(CapError::SlotNotFound(child.slot))
        );
        assert!(cspace.lookup(grandchild).is_ok());
        assert!(!cspace.slot_exists(child.slot));
    }

    #[test]
    fn revoke_descendants_keeps_revoked_slot() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace.derive(root, Rights::READ | Rights::WRITE).unwrap();
        let grandchild = cspace.derive(child, Rights::READ).unwrap();

        cspace.revoke_descendants(root).unwrap();

        assert!(cspace.lookup(root).is_ok());
        assert_eq!(
            cspace.lookup(child),
            Err(CapError::SlotNotFound(child.slot))
        );
        assert_eq!(
            cspace.lookup(grandchild),
            Err(CapError::SlotNotFound(grandchild.slot))
        );
    }

    #[test]
    fn revoke_descendants_can_traverse_deleted_intermediate_slots() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace.derive(root, Rights::READ | Rights::WRITE).unwrap();
        let grandchild = cspace.derive(child, Rights::READ).unwrap();

        cspace.delete(child).unwrap();
        cspace.revoke_descendants(root).unwrap();

        assert!(cspace.lookup(root).is_ok());
        assert_eq!(
            cspace.lookup(child),
            Err(CapError::SlotNotFound(child.slot))
        );
        assert_eq!(
            cspace.lookup(grandchild),
            Err(CapError::SlotNotFound(grandchild.slot))
        );
    }

    #[test]
    fn deleting_leaf_then_revoke_does_not_reuse_slot_twice() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace.copy(root, Rights::READ).unwrap();

        cspace.delete(child).unwrap();
        cspace.revoke_descendants(root).unwrap();

        let first = cspace
            .insert_initial_capability(frame(Rights::READ))
            .unwrap();
        let second = cspace
            .insert_initial_capability(tcb(Rights::MANAGE))
            .unwrap();

        assert_ne!(first.slot, second.slot);
        assert_eq!(
            cspace.lookup(first).unwrap().capability,
            frame(Rights::READ)
        );
        assert_eq!(
            cspace.lookup(second).unwrap().capability,
            tcb(Rights::MANAGE)
        );
    }

    #[test]
    fn destroying_object_invalidates_all_related_capabilities() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE | Rights::EXECUTE))
            .unwrap();
        let child = cspace.derive(root, Rights::READ).unwrap();
        let object = cspace.object_of(root).unwrap();

        cspace.destroy_object(object).unwrap();

        assert_eq!(cspace.lookup(root), Err(CapError::SlotNotFound(root.slot)));
        assert_eq!(
            cspace.lookup(child),
            Err(CapError::SlotNotFound(child.slot))
        );
    }

    #[test]
    fn generation_bump_makes_existing_descriptor_stale() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE | Rights::EXECUTE))
            .unwrap();
        let object = cspace.object_of(root).unwrap();

        cspace.bump_generation(object).unwrap();

        assert_eq!(
            cspace.lookup(root),
            Err(CapError::StaleDescriptor {
                slot: root.slot,
                expected_generation: 2,
                actual_generation: 1,
            })
        );
    }

    #[test]
    fn stale_descriptor_cannot_derive_new_capability() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(frame(Rights::READ | Rights::WRITE | Rights::EXECUTE))
            .unwrap();
        let object = cspace.object_of(root).unwrap();

        cspace.bump_generation(object).unwrap();

        assert!(matches!(
            cspace.derive(root, Rights::READ),
            Err(CapError::StaleDescriptor { .. })
        ));
    }

    #[test]
    fn reused_slot_rejects_stale_descriptor() {
        let mut cspace = CapabilitySpace::new();
        let first = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();

        cspace.delete(first).unwrap();
        let second = cspace
            .insert_initial_capability(tcb(Rights::MANAGE))
            .unwrap();

        assert_eq!(first.slot, second.slot);
        assert_ne!(first.slot_generation, second.slot_generation);
        assert_eq!(
            cspace.lookup(first),
            Err(CapError::StaleDescriptor {
                slot: first.slot,
                expected_generation: second.slot_generation,
                actual_generation: first.slot_generation,
            })
        );
        assert_eq!(cspace.lookup(second).unwrap().object_kind, ObjectKind::Tcb);
    }

    #[test]
    fn deleted_slot_with_descendants_is_not_reused_before_revoke() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace.derive(root, Rights::READ | Rights::WRITE).unwrap();
        let grandchild = cspace.derive(child, Rights::READ).unwrap();

        cspace.delete(child).unwrap();
        let next = cspace
            .insert_initial_capability(tcb(Rights::MANAGE))
            .unwrap();

        assert_ne!(child.slot, next.slot);
        assert!(cspace.lookup(grandchild).is_ok());

        cspace.revoke_descendants(root).unwrap();

        assert_eq!(
            cspace.lookup(grandchild),
            Err(CapError::SlotNotFound(grandchild.slot))
        );
    }

    #[test]
    fn notification_capability_derivation_preserves_badge_and_reduces_rights() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(notification(Rights::READ | Rights::WRITE))
            .unwrap();

        let receive_only = cspace.derive(root, Rights::READ).unwrap();
        let view = cspace.lookup(receive_only).unwrap();

        assert_eq!(view.object_kind, ObjectKind::Notification);
        assert_eq!(view.capability, notification(Rights::READ));
        assert_eq!(view.rights, Rights::READ);
    }

    #[test]
    fn reply_capability_is_not_derivable() {
        let mut cspace = CapabilitySpace::new();
        let caller = ObjectId::new(100);
        let target = ObjectId::new(200);
        let root = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller,
                target,
                can_grant: true,
            })
            .unwrap();

        assert_eq!(
            cspace.derive(root, Rights::NONE),
            Err(CapError::CapabilityNotDerivable {
                parent: root.slot,
                capability: reply(caller, target, true),
            })
        );
    }

    #[test]
    fn public_initial_capability_insertion_rejects_reply_cap() {
        let mut cspace = CapabilitySpace::new();
        let caller = ObjectId::new(100);
        let target = ObjectId::new(200);

        assert_eq!(
            cspace.insert_initial_capability(reply(caller, target, true)),
            Err(CapError::InvalidInitialCapability {
                capability: reply(caller, target, true),
            })
        );
    }

    #[test]
    fn consuming_reply_cap_invalidates_that_slot() {
        let mut cspace = CapabilitySpace::new();
        let caller = ObjectId::new(100);
        let target = ObjectId::new(200);
        let reply = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller,
                target,
                can_grant: true,
            })
            .unwrap();

        cspace.consume_reply_cap(reply).unwrap();

        assert_eq!(
            cspace.lookup(reply),
            Err(CapError::SlotNotFound(reply.slot))
        );
    }

    #[test]
    fn reply_capability_can_target_existing_reply_object() {
        let mut cspace = CapabilitySpace::new();
        let initial = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller: ObjectId::new(100),
                target: ObjectId::new(200),
                can_grant: true,
            })
            .unwrap();
        let reply_object = cspace.object_of(initial).unwrap();
        cspace.consume_reply_cap(initial).unwrap();

        let installed = cspace
            .insert_reply_capability(
                reply_object,
                ReplyCap {
                    caller: ObjectId::new(101),
                    target: ObjectId::new(201),
                    can_grant: false,
                },
            )
            .unwrap();

        let view = cspace.lookup(installed).unwrap();
        assert_eq!(view.object, reply_object);
        assert_eq!(view.object_kind, ObjectKind::Reply);
        assert_eq!(
            view.capability,
            reply(ObjectId::new(101), ObjectId::new(201), false)
        );
    }

    #[test]
    fn reply_capability_install_rejects_non_reply_object_without_slot() {
        let mut cspace = CapabilitySpace::new();
        let endpoint = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        let endpoint_object = cspace.object_of(endpoint).unwrap();

        assert_eq!(
            cspace.insert_reply_capability(
                endpoint_object,
                ReplyCap {
                    caller: ObjectId::new(100),
                    target: ObjectId::new(200),
                    can_grant: true,
                },
            ),
            Err(CapError::WrongCapability {
                expected: ObjectKind::Reply,
                actual: ObjectKind::Endpoint,
            })
        );
        assert_eq!(cspace.lookup(endpoint).unwrap().object, endpoint_object);
    }

    #[test]
    fn consumed_reply_slot_reuse_rejects_old_descriptor() {
        let mut cspace = CapabilitySpace::new();
        let caller = ObjectId::new(100);
        let target = ObjectId::new(200);
        let reply = cspace
            .insert_reply_capability_for_test(ReplyCap {
                caller,
                target,
                can_grant: true,
            })
            .unwrap();

        cspace.consume_reply_cap(reply).unwrap();
        let reused = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();

        assert_eq!(reply.slot, reused.slot);
        assert_ne!(reply.slot_generation, reused.slot_generation);
        assert_eq!(
            cspace.lookup(reply),
            Err(CapError::StaleDescriptor {
                slot: reply.slot,
                expected_generation: reused.slot_generation,
                actual_generation: reply.slot_generation,
            })
        );
        assert_eq!(
            cspace.lookup(reused).unwrap().capability,
            endpoint(Rights::READ)
        );
    }

    #[test]
    fn only_reply_cap_can_be_consumed_as_reply() {
        let mut cspace = CapabilitySpace::new();
        let endpoint = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();

        assert_eq!(
            cspace.consume_reply_cap(endpoint),
            Err(CapError::WrongCapability {
                expected: ObjectKind::Reply,
                actual: ObjectKind::Endpoint,
            })
        );
        assert!(cspace.lookup(endpoint).is_ok());
    }
}
