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
//! - `revoke_descendants` invalidates descendants while keeping the named slot.
//! - `destroy_object` invalidates every capability that targets the object.
//! - `slot_generation` prevents ABA when a slot is safely reused.
//! - `object_generation_snapshot` rejects descriptors after object generation
//!   changes.
//! - Dead slots with descendants are not reused, so lineage remains available
//!   for later revocation.
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
    pub const fn raw(self) -> u64 {
        self.0
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    pub rights: Rights,
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
pub enum ObjectKind {
    Endpoint,
    Frame,
    CNode,
    Untyped,
    Tcb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityDescriptor {
    pub slot: SlotId,
    pub slot_generation: u64,
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
    alive: bool,
}

#[derive(Debug, Default)]
pub struct CapabilitySpace {
    next_object: u64,
    next_slot: u64,
    free_slots: Vec<SlotId>,
    objects: BTreeMap<ObjectId, KernelObject>,
    slots: BTreeMap<SlotId, CapabilitySlot>,
}

impl CapabilitySpace {
    pub fn new() -> Self {
        Self {
            next_object: 1,
            next_slot: 1,
            free_slots: Vec::new(),
            objects: BTreeMap::new(),
            slots: BTreeMap::new(),
        }
    }

    pub fn create_object(&mut self, capability: Capability) -> CapabilityDescriptor {
        let kind = capability_kind(&capability);
        let (object, generation) = self.alloc_object(kind);
        self.insert_root_slot(object, capability, generation)
    }

    pub fn derive(
        &mut self,
        parent: CapabilityDescriptor,
        requested_rights: Rights,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.validate_descriptor(parent)?;

        let parent_slot = self.live_slot(parent.slot)?;
        if !requested_rights.is_subset_of(parent_slot.rights) {
            return Err(CapError::RightsEscalation {
                parent: parent.slot,
                parent_rights: parent_slot.rights,
                requested_rights,
            });
        }

        let object = parent_slot.object;
        let object_generation_snapshot = parent_slot.object_generation_snapshot;
        let parent_capability = parent_slot.capability.clone();
        let parent_slot_id = parent.slot;
        let slot = self.alloc_slot_id();
        let slot_generation = self.slot_generation_for_insert(slot);
        self.detach_reused_slot(slot);
        let capability = derive_capability(parent_slot_id, &parent_capability, requested_rights)?;
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
                alive: true,
            },
        );
        self.slots
            .get_mut(&parent_slot_id)
            .ok_or(CapError::SlotNotFound(parent_slot_id))?
            .children
            .insert(slot);

        Ok(CapabilityDescriptor {
            slot,
            slot_generation,
        })
    }

    pub fn lookup(&self, descriptor: CapabilityDescriptor) -> Result<CapabilityView, CapError> {
        self.validate_descriptor(descriptor)?;

        let slot = self.live_slot(descriptor.slot)?;
        let object = self.object(slot.object)?;

        Ok(CapabilityView {
            object: slot.object,
            object_kind: object.kind.clone(),
            capability: slot.capability.clone(),
            rights: slot.rights,
            descriptor,
            parent: slot.parent,
        })
    }

    pub fn delete(&mut self, descriptor: CapabilityDescriptor) -> Result<(), CapError> {
        self.validate_descriptor(descriptor)?;
        self.delete_slot(descriptor.slot);
        Ok(())
    }

    pub fn revoke_descendants(&mut self, descriptor: CapabilityDescriptor) -> Result<(), CapError> {
        self.validate_descriptor(descriptor)?;

        let descendants = self.collect_descendants(descriptor.slot);
        for slot in descendants {
            self.delete_slot(slot);
        }

        Ok(())
    }

    pub fn bump_generation(&mut self, object: ObjectId) -> Result<u64, CapError> {
        let kernel_object = self.object_mut(object)?;
        kernel_object.generation += 1;
        Ok(kernel_object.generation)
    }

    pub fn destroy_object(&mut self, object: ObjectId) -> Result<(), CapError> {
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

    pub fn object_of(&self, descriptor: CapabilityDescriptor) -> Result<ObjectId, CapError> {
        self.validate_descriptor(descriptor)?;
        let slot = self.live_slot(descriptor.slot)?;
        Ok(slot.object)
    }

    pub fn slot_exists(&self, slot: SlotId) -> bool {
        self.slots.get(&slot).is_some_and(|slot| slot.alive)
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
                alive: true,
            },
        );

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

    fn validate_descriptor(&self, descriptor: CapabilityDescriptor) -> Result<(), CapError> {
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

        Ok(())
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

    fn object_mut(&mut self, object: ObjectId) -> Result<&mut KernelObject, CapError> {
        self.objects
            .get_mut(&object)
            .ok_or(CapError::ObjectNotFound(object))
    }

    fn collect_descendants(&self, slot: SlotId) -> Vec<SlotId> {
        let mut descendants = Vec::new();
        self.collect_descendants_into(slot, &mut descendants);
        descendants
    }

    fn collect_descendants_into(&self, slot: SlotId, descendants: &mut Vec<SlotId>) {
        let Some(parent) = self.slots.get(&slot) else {
            return;
        };

        for child in &parent.children {
            descendants.push(*child);
            self.collect_descendants_into(*child, descendants);
        }
    }

    fn delete_slot(&mut self, slot: SlotId) {
        let Some(removed) = self.slots.get_mut(&slot) else {
            return;
        };

        removed.alive = false;
        if removed.children.is_empty() {
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
    }
}

fn capability_kind(capability: &Capability) -> ObjectKind {
    match capability {
        Capability::Endpoint(_) => ObjectKind::Endpoint,
        Capability::Frame(_) => ObjectKind::Frame,
        Capability::CNode(_) => ObjectKind::CNode,
        Capability::Untyped(_) => ObjectKind::Untyped,
        Capability::Tcb(_) => ObjectKind::Tcb,
    }
}

fn capability_rights(capability: &Capability) -> Rights {
    match capability {
        Capability::Endpoint(cap) => cap.rights,
        Capability::Frame(cap) => cap.rights,
        Capability::CNode(cap) => cap.rights,
        Capability::Untyped(_) => Rights::NONE,
        Capability::Tcb(cap) => cap.rights,
    }
}

fn derive_capability(
    parent_slot: SlotId,
    parent: &Capability,
    requested_rights: Rights,
) -> Result<Capability, CapError> {
    match parent {
        Capability::Endpoint(cap) => {
            if !requested_rights.is_subset_of(cap.rights) {
                return Err(CapError::RightsEscalation {
                    parent: parent_slot,
                    parent_rights: cap.rights,
                    requested_rights,
                });
            }
            Ok(Capability::Endpoint(EndpointCap {
                badge: cap.badge,
                rights: requested_rights,
            }))
        }
        Capability::Frame(cap) => {
            if !requested_rights.is_subset_of(cap.rights) {
                return Err(CapError::RightsEscalation {
                    parent: parent_slot,
                    parent_rights: cap.rights,
                    requested_rights,
                });
            }
            Ok(Capability::Frame(FrameCap {
                rights: requested_rights,
            }))
        }
        Capability::CNode(cap) => {
            if !requested_rights.is_subset_of(cap.rights) {
                return Err(CapError::RightsEscalation {
                    parent: parent_slot,
                    parent_rights: cap.rights,
                    requested_rights,
                });
            }
            Ok(Capability::CNode(CNodeCap {
                rights: requested_rights,
            }))
        }
        Capability::Untyped(cap) => Ok(Capability::Untyped(UntypedCap {
            size_bits: cap.size_bits,
        })),
        Capability::Tcb(cap) => {
            if !requested_rights.is_subset_of(cap.rights) {
                return Err(CapError::RightsEscalation {
                    parent: parent_slot,
                    parent_rights: cap.rights,
                    requested_rights,
                });
            }
            Ok(Capability::Tcb(TcbCap {
                rights: requested_rights,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(rights: Rights) -> Capability {
        Capability::Endpoint(EndpointCap { badge: 0, rights })
    }

    fn frame(rights: Rights) -> Capability {
        Capability::Frame(FrameCap { rights })
    }

    fn tcb(rights: Rights) -> Capability {
        Capability::Tcb(TcbCap { rights })
    }

    #[test]
    fn root_capability_can_be_created_and_looked_up() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.create_object(endpoint(Rights::ALL));

        let view = cspace.lookup(root).unwrap();

        assert_eq!(view.object_kind, ObjectKind::Endpoint);
        assert_eq!(view.capability, endpoint(Rights::ALL));
        assert_eq!(view.rights, Rights::ALL);
        assert_eq!(view.descriptor, root);
        assert_eq!(view.parent, None);
    }

    #[test]
    fn derived_capability_can_only_reduce_rights() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.create_object(endpoint(Rights::READ | Rights::WRITE));

        let read_only = cspace.derive(root, Rights::READ).unwrap();
        let view = cspace.lookup(read_only).unwrap();

        assert_eq!(view.capability, endpoint(Rights::READ));
        assert_eq!(view.rights, Rights::READ);
        assert_eq!(view.parent, Some(root.slot));
    }

    #[test]
    fn derivation_cannot_escalate_rights() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.create_object(endpoint(Rights::READ));

        let err = cspace
            .derive(root, Rights::READ | Rights::WRITE)
            .unwrap_err();

        assert_eq!(
            err,
            CapError::RightsEscalation {
                parent: root.slot,
                parent_rights: Rights::READ,
                requested_rights: Rights::READ | Rights::WRITE,
            }
        );
    }

    #[test]
    fn deleting_a_slot_only_invalidates_that_slot() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.create_object(endpoint(Rights::ALL));
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
        let root = cspace.create_object(endpoint(Rights::ALL));
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
        let root = cspace.create_object(endpoint(Rights::ALL));
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
    fn destroying_object_invalidates_all_related_capabilities() {
        let mut cspace = CapabilitySpace::new();
        let root = cspace.create_object(frame(Rights::ALL));
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
        let root = cspace.create_object(frame(Rights::ALL));
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
        let root = cspace.create_object(frame(Rights::ALL));
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
        let first = cspace.create_object(endpoint(Rights::READ));

        cspace.delete(first).unwrap();
        let second = cspace.create_object(tcb(Rights::MANAGE));

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
        let root = cspace.create_object(endpoint(Rights::ALL));
        let child = cspace.derive(root, Rights::READ | Rights::WRITE).unwrap();
        let grandchild = cspace.derive(child, Rights::READ).unwrap();

        cspace.delete(child).unwrap();
        let next = cspace.create_object(tcb(Rights::MANAGE));

        assert_ne!(child.slot, next.slot);
        assert!(cspace.lookup(grandchild).is_ok());

        cspace.revoke_descendants(root).unwrap();

        assert_eq!(
            cspace.lookup(grandchild),
            Err(CapError::SlotNotFound(grandchild.slot))
        );
    }
}
