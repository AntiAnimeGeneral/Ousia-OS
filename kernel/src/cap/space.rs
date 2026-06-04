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
//! next integration step is to complete CNode guard/radix lookup, Untyped
//! accounting, typed backing storage, and TCB-embedded IPC/notification queues
//! without changing seL4 baseline authority semantics.

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
    pub const fn new(raw: u64) -> Self {
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
const MAX_MODEL_CNODE_RADIX: u8 = 16;
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
const CSPACE_WORD_BITS: u8 = u64::BITS as u8;

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

    pub const fn with_guard(radix: u8, guard: u64, guard_size: u8) -> Self {
        Self {
            radix,
            guard,
            guard_size,
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
pub struct CNodePath {
    pub root: CapabilityDescriptor,
    pub capptr: u64,
    pub depth: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CNodeLookup {
    pub slot: SlotId,
    pub bits_remaining: u8,
    pub slots_remaining: usize,
    cte: CteRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedCte {
    pub slot: SlotId,
    pub cte: CteRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedCapabilitySlot {
    pub descriptor: CapabilityDescriptor,
    pub cte: CteRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CteRef {
    pub handle: SlotId,
    pub storage: CteStorageRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CteStorageRef {
    Root,
    CNode(CteLocation),
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

impl CteRef {
    pub const fn root(handle: SlotId) -> Self {
        Self {
            handle,
            storage: CteStorageRef::Root,
        }
    }

    pub const fn cnode(handle: SlotId, location: CteLocation) -> Self {
        Self {
            handle,
            storage: CteStorageRef::CNode(location),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetypeResult {
    pub descriptors: Vec<CapabilityDescriptor>,
    pub objects: Vec<ObjectId>,
    pub retyped_objects: Vec<RetypedObject>,
}

impl RetypeResult {
    pub fn retyped_objects(&self) -> impl Iterator<Item = RetypedObject> + '_ {
        self.retyped_objects.iter().copied()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetypedObject {
    pub object: ObjectId,
    pub kind: RetypedObjectKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetypedObjectKind {
    Endpoint,
    Frame,
    CNode { radix: u8, window_start: SlotId },
    Untyped,
    Tcb,
    Notification,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetypeCommitPlan {
    source: ResolvedCapabilitySlot,
    allocation: UntypedAllocationPlan,
    entries: Vec<RetypeCommitEntry>,
    next_slot: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RetypeCommitEntry {
    destination: ResolvedCte,
    object: ObjectId,
    capability: Capability,
    kind: RetypedObjectKind,
}

impl RetypeCommitPlan {
    pub fn objects(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.entries.iter().map(|entry| entry.object)
    }

    pub fn retyped_objects(&self) -> impl Iterator<Item = RetypedObject> + '_ {
        self.entries.iter().map(|entry| RetypedObject {
            object: entry.object,
            kind: entry.kind,
        })
    }
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
    pub fn validate_retype_bounds(&self) -> Result<(), CapError> {
        if let Self::CNode { radix } = self {
            validate_cnode_radix(*radix)?;
        }

        Ok(())
    }

    pub const fn minimum_size_bits(&self) -> u8 {
        match self {
            Self::Endpoint => MODEL_ENDPOINT_SIZE_BITS,
            Self::Frame { .. } => MIN_FRAME_SIZE_BITS,
            Self::CNode { radix } => MODEL_CNODE_SIZE_BITS.saturating_add(*radix),
            Self::Untyped { size_bits } => *size_bits,
            Self::Tcb { .. } => MODEL_TCB_SIZE_BITS,
            Self::Notification => MODEL_NOTIFICATION_SIZE_BITS,
        }
    }

    pub fn validate_rights(&self) -> Result<(), CapError> {
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
    InvalidCteReference {
        slot: SlotId,
    },
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
    RetypeWindowExceedsCNode {
        start: SlotId,
        requested: usize,
        available: usize,
    },
    SlotWindowOverflow {
        start: SlotId,
        count: usize,
    },
    InvalidCNodeDepth {
        depth: u8,
    },
    CNodeGuardMismatch {
        expected_guard: u64,
        actual_guard: u64,
        bits_remaining: u8,
        guard_size: u8,
    },
    CNodeDepthMismatch {
        level_bits: u8,
        bits_remaining: u8,
    },
    CNodeLookupUnresolved {
        slot: SlotId,
        bits_remaining: u8,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KernelObject {
    payload: KernelObjectPayload,
    generation: u64,
    destroyed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum KernelObjectPayload {
    Endpoint,
    Frame,
    CNode { slots: CNodeSlots },
    Untyped,
    Tcb,
    Notification,
    Reply,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CNodeSlots {
    slots: Vec<CNodeSlotEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CNodeSlotEntry {
    handle: SlotId,
    slot: Option<CapabilitySlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CteLocation {
    pub object: ObjectId,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CapabilitySlot {
    object: ObjectId,
    capability: Capability,
    rights: Rights,
    slot_generation: u64,
    object_generation_snapshot: u64,
    parent: Option<CteRef>,
    children: ChildSlots,
    mdb: MdbNode,
    alive: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MdbNode {
    prev: Option<CteRef>,
    next: Option<CteRef>,
    revocable: bool,
    first_badged: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ChildSlots {
    slots: Vec<CteRef>,
}

#[derive(Debug, Default)]
struct CteSlots {
    root_slots: Vec<Option<CapabilitySlot>>,
    cnode_locations: Vec<Option<CteLocation>>,
}

#[derive(Debug, Default)]
struct ObjectStorage {
    objects: Vec<Option<KernelObject>>,
}

#[derive(Debug, Default)]
struct UntypedStorage {
    allocations: Vec<Option<UntypedAllocation>>,
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
    initial_cnode: Option<ObjectId>,
    objects: ObjectStorage,
    untyped_allocations: UntypedStorage,
    slots: CteSlots,
}

impl CapabilitySpace {
    pub fn new() -> Self {
        Self {
            next_object: 1,
            next_slot: 1,
            free_slots: Vec::new(),
            initial_cnode: None,
            objects: ObjectStorage::default(),
            untyped_allocations: UntypedStorage::default(),
            slots: CteSlots::default(),
        }
    }

    pub fn insert_initial_capability(
        &mut self,
        capability: Capability,
    ) -> Result<CapabilityDescriptor, CapError> {
        if matches!(capability, Capability::Reply(_) | Capability::CNode(_)) {
            return Err(CapError::InvalidInitialCapability { capability });
        }

        validate_capability_rights(&capability)?;
        Ok(self.insert_validated_initial_capability(capability))
    }

    pub fn insert_initial_cnode_capability(
        &mut self,
        capability: CNodeCap,
        window_start: SlotId,
    ) -> Result<CapabilityDescriptor, CapError> {
        validate_cnode_radix(capability.radix)?;
        let capability = Capability::CNode(capability);
        validate_capability_rights(&capability)?;
        self.insert_validated_initial_capability_with_cnode_window(capability, window_start)
    }

    #[cfg(test)]
    pub fn insert_reply_capability_for_test(
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
        if let Some(cnode) = self.initial_cnode {
            return self
                .insert_initial_capability_into_cnode(cnode, object, capability, generation);
        }
        self.insert_root_slot(object, capability, generation)
    }

    fn insert_validated_initial_capability_with_cnode_window(
        &mut self,
        capability: Capability,
        window_start: SlotId,
    ) -> Result<CapabilityDescriptor, CapError> {
        let Capability::CNode(cnode) = &capability else {
            unreachable!("initial CNode insertion requires a CNode capability")
        };
        let slot_count = cnode_slot_count(cnode.radix);
        validate_slot_window_bounds(window_start, slot_count)?;
        let window_start = self.available_cnode_window_start(window_start, slot_count)?;
        let descriptor_slot = self.cnode_root_descriptor_slot(window_start, slot_count)?;
        let cnode_slots = CNodeSlots::from_window(window_start, slot_count);
        let (object, generation) =
            self.alloc_object_with_payload(KernelObjectPayload::CNode { slots: cnode_slots });
        let descriptor = self.insert_root_slot_at(descriptor_slot, object, capability, generation);
        self.initial_cnode = Some(object);
        Ok(descriptor)
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
        let source = self.resolve_descriptor_ref(source)?;
        self.insert_derived_capability(source, None, requested_rights, MintParams::None)
    }

    pub fn copy_into(
        &mut self,
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
    ) -> Result<CapabilityDescriptor, CapError> {
        let source = self.resolve_descriptor_ref(source)?;
        let destination = self.empty_cte_for_slot(destination)?;
        self.copy_resolved(source, destination, requested_rights)
    }

    pub fn copy_resolved(
        &mut self,
        source: ResolvedCapabilitySlot,
        destination: ResolvedCte,
        requested_rights: Rights,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.insert_derived_capability(
            source,
            Some(destination),
            requested_rights,
            MintParams::None,
        )
    }

    pub fn mint(
        &mut self,
        source: CapabilityDescriptor,
        requested_rights: Rights,
        params: MintParams,
    ) -> Result<CapabilityDescriptor, CapError> {
        let source = self.resolve_descriptor_ref(source)?;
        self.insert_derived_capability(source, None, requested_rights, params)
    }

    pub fn mint_into(
        &mut self,
        source: CapabilityDescriptor,
        destination: SlotId,
        requested_rights: Rights,
        params: MintParams,
    ) -> Result<CapabilityDescriptor, CapError> {
        let source = self.resolve_descriptor_ref(source)?;
        let destination = self.empty_cte_for_slot(destination)?;
        self.mint_resolved(source, destination, requested_rights, params)
    }

    pub fn mint_resolved(
        &mut self,
        source: ResolvedCapabilitySlot,
        destination: ResolvedCte,
        requested_rights: Rights,
        params: MintParams,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.insert_derived_capability(source, Some(destination), requested_rights, params)
    }

    pub fn retype_untyped(
        &mut self,
        source: CapabilityDescriptor,
        target: RetypeTarget,
    ) -> Result<CapabilityDescriptor, CapError> {
        let plan = self.plan_retype_untyped(source, target)?;
        let result = self.commit_retype_plan(plan)?;
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
        let plan = self.plan_retype_untyped_into(source, target, destination)?;
        self.commit_retype_plan(plan)
    }

    pub fn plan_retype_untyped(
        &self,
        source: CapabilityDescriptor,
        target: RetypeTarget,
    ) -> Result<RetypeCommitPlan, CapError> {
        let destination = RetypeDestination::single(self.next_auto_slot_id());
        self.plan_retype_untyped_into(source, target, destination)
    }

    pub fn plan_retype_untyped_into(
        &self,
        source: CapabilityDescriptor,
        target: RetypeTarget,
        destination: RetypeDestination,
    ) -> Result<RetypeCommitPlan, CapError> {
        let source = self.resolve_descriptor_ref(source)?;
        let (allocation, destinations) =
            self.validate_retype_untyped_into(source, &target, destination)?;
        let mut next_slot = self.next_slot_after_retype_destinations(destination)?;
        let mut entries = Vec::new();
        for (offset, destination) in destinations.into_iter().enumerate() {
            let window_start = self.planned_retype_window_start(&target, &mut next_slot)?;
            let capability = target.clone().into_capability();
            entries.push(RetypeCommitEntry {
                destination,
                object: ObjectId(self.next_object + offset as u64),
                capability,
                kind: self.planned_retype_kind(&target, window_start),
            });
        }

        Ok(RetypeCommitPlan {
            source,
            allocation,
            entries,
            next_slot,
        })
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
        let actual = object.kind();
        if actual != ObjectKind::Reply {
            return Err(CapError::WrongCapability {
                expected: ObjectKind::Reply,
                actual,
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
        let destination = self.alloc_empty_cte();
        self.insert_reply_capability_resolved(reply_object, capability, destination)
    }

    pub fn insert_reply_capability_resolved(
        &mut self,
        reply_object: ObjectId,
        capability: ReplyCap,
        destination: ResolvedCte,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.validate_reply_capability(reply_object, &capability)?;
        let generation = self
            .object(reply_object)
            .expect("validated reply object must remain in CSpace")
            .generation;
        self.reserve_empty_cte_for_insert(destination)?;
        let slot_generation = self.slot_generation_for_cte(destination);
        self.detach_reused_cte(destination);
        self.insert_cte(
            destination.cte,
            CapabilitySlot {
                object: reply_object,
                rights: REPLY_ALLOWED_RIGHTS,
                capability: Capability::Reply(capability),
                slot_generation,
                object_generation_snapshot: generation,
                parent: None,
                children: ChildSlots::new(),
                mdb: MdbNode {
                    revocable: true,
                    first_badged: true,
                    ..MdbNode::default()
                },
                alive: true,
            },
        );
        Ok(CapabilityDescriptor {
            slot: destination.slot,
            slot_generation,
        })
    }

    pub fn move_capability(
        &mut self,
        source: CapabilityDescriptor,
    ) -> Result<CapabilityDescriptor, CapError> {
        let source = self.resolve_descriptor_ref(source)?;
        self.move_capability_to(source, None)
    }

    pub fn move_capability_into(
        &mut self,
        source: CapabilityDescriptor,
        destination: SlotId,
    ) -> Result<CapabilityDescriptor, CapError> {
        let source = self.resolve_descriptor_ref(source)?;
        let destination = self.empty_cte_for_slot(destination)?;
        self.move_resolved(source, destination)
    }

    pub fn move_resolved(
        &mut self,
        source: ResolvedCapabilitySlot,
        destination: ResolvedCte,
    ) -> Result<CapabilityDescriptor, CapError> {
        self.move_capability_to(source, Some(destination))
    }

    fn move_capability_to(
        &mut self,
        source: ResolvedCapabilitySlot,
        destination: Option<ResolvedCte>,
    ) -> Result<CapabilityDescriptor, CapError> {
        if let Some(destination) = destination {
            self.validate_empty_cte(destination)?;
        }
        self.validated_resolved_slot(source)?;

        let destination = match destination {
            Some(destination) => destination,
            None => self.alloc_empty_cte(),
        };
        self.reserve_empty_cte_for_insert(destination)?;
        let destination_generation = self.slot_generation_for_cte(destination);
        self.detach_reused_cte(destination);

        let moved_from = self
            .slot_mut_by_ref(source.cte)
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
                .slot_mut_by_ref(parent)
                .expect("moved capability parent slot must remain in CSpace");
            parent_slot.children.remove(&source.cte);
            parent_slot.children.insert(destination.cte);
        }

        let children = moved.children.clone();
        for child in children.iter() {
            self.slot_mut_by_ref(child)
                .expect("moved capability child slot must remain in CSpace")
                .parent = Some(destination.cte);
        }

        if let Some(prev) = moved.mdb.prev {
            self.slot_mut_by_ref(prev)
                .expect("moved capability previous MDB slot must remain in CSpace")
                .mdb
                .next = Some(destination.cte);
        }
        if let Some(next) = moved.mdb.next {
            self.slot_mut_by_ref(next)
                .expect("moved capability next MDB slot must remain in CSpace")
                .mdb
                .prev = Some(destination.cte);
        }

        self.insert_cte(destination.cte, moved);
        self.push_free_slot_once(source.descriptor.slot);

        Ok(CapabilityDescriptor {
            slot: destination.slot,
            slot_generation: destination_generation,
        })
    }

    pub fn lookup(&self, descriptor: CapabilityDescriptor) -> Result<CapabilityView, CapError> {
        let (slot, object) = self.validated_slot(descriptor)?;

        Ok(CapabilityView {
            object: slot.object,
            object_kind: object.kind(),
            capability: slot.capability.clone(),
            rights: slot.rights,
            descriptor,
            parent: slot.parent.map(|parent| parent.handle),
        })
    }

    pub fn resolve_cnode_path(&self, path: CNodePath) -> Result<CNodeLookup, CapError> {
        let (root_slot, _) = self.validated_slot(path.root)?;
        let Capability::CNode(root) = &root_slot.capability else {
            return Err(CapError::WrongCapability {
                expected: ObjectKind::CNode,
                actual: capability_kind(&root_slot.capability),
            });
        };

        if path.depth == 0 || path.depth > CSPACE_WORD_BITS {
            return Err(CapError::InvalidCNodeDepth { depth: path.depth });
        }

        self.resolve_address_bits(root_slot.object, root, path.capptr, path.depth)
    }

    pub fn lookup_cnode_slot(&self, path: CNodePath) -> Result<SlotId, CapError> {
        let lookup = self.resolve_cnode_path(path)?;
        if lookup.bits_remaining != 0 {
            return Err(CapError::CNodeLookupUnresolved {
                slot: lookup.slot,
                bits_remaining: lookup.bits_remaining,
            });
        }

        Ok(lookup.slot)
    }

    pub fn lookup_cnode_empty_slot(&self, path: CNodePath) -> Result<SlotId, CapError> {
        Ok(self.resolve_cnode_empty_slot(path)?.slot)
    }

    pub fn resolve_cnode_empty_slot(&self, path: CNodePath) -> Result<ResolvedCte, CapError> {
        let lookup = self.lookup_cnode_window(path)?;
        if self.slot_by_ref(lookup.cte).is_some_and(|slot| slot.alive) {
            return Err(CapError::SlotOccupied(lookup.slot));
        }
        Ok(ResolvedCte {
            slot: lookup.slot,
            cte: lookup.cte,
        })
    }

    pub fn lookup_cnode_descriptor(
        &self,
        path: CNodePath,
    ) -> Result<CapabilityDescriptor, CapError> {
        Ok(self.resolve_cnode_descriptor(path)?.descriptor)
    }

    pub fn resolve_cnode_descriptor(
        &self,
        path: CNodePath,
    ) -> Result<ResolvedCapabilitySlot, CapError> {
        let lookup = self.lookup_cnode_window(path)?;
        let slot_ref = self
            .slot_by_ref(lookup.cte)
            .filter(|slot| slot.alive)
            .ok_or(CapError::SlotNotFound(lookup.slot))?;
        let descriptor = CapabilityDescriptor {
            slot: lookup.slot,
            slot_generation: slot_ref.slot_generation,
        };
        Ok(ResolvedCapabilitySlot {
            descriptor,
            cte: lookup.cte,
        })
    }

    pub fn lookup_cnode_window(&self, path: CNodePath) -> Result<CNodeLookup, CapError> {
        let lookup = self.resolve_cnode_path(path)?;
        if lookup.bits_remaining != 0 {
            return Err(CapError::CNodeLookupUnresolved {
                slot: lookup.slot,
                bits_remaining: lookup.bits_remaining,
            });
        }

        Ok(lookup)
    }

    pub fn descriptor_for_live_slot(&self, slot: SlotId) -> Result<CapabilityDescriptor, CapError> {
        let slot_ref = self.live_slot(slot)?;
        Ok(CapabilityDescriptor {
            slot,
            slot_generation: slot_ref.slot_generation,
        })
    }

    pub fn delete(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<CapabilityDeletion, CapError> {
        let resolved = self.resolve_descriptor_ref(descriptor)?;
        self.delete_resolved(resolved)
    }

    pub fn delete_resolved(
        &mut self,
        resolved: ResolvedCapabilitySlot,
    ) -> Result<CapabilityDeletion, CapError> {
        self.validated_resolved_slot(resolved)?;
        let final_object = self
            .is_final_capability(resolved.cte)
            .then(|| {
                self.slot_by_ref(resolved.cte)
                    .filter(|slot| slot.alive)
                    .map(|slot| slot.object)
                    .ok_or(CapError::SlotNotFound(resolved.descriptor.slot))
            })
            .transpose()?;
        self.delete_cte(resolved);
        Ok(CapabilityDeletion { final_object })
    }

    pub fn consume_reply_cap(&mut self, descriptor: CapabilityDescriptor) -> Result<(), CapError> {
        self.validate_consumable_reply_cap(descriptor)?;
        self.delete_slot(descriptor.slot);
        Ok(())
    }

    pub fn validate_consumable_reply_cap(
        &self,
        descriptor: CapabilityDescriptor,
    ) -> Result<(), CapError> {
        self.validate_descriptor(descriptor)?;

        let slot = self.live_slot(descriptor.slot)?;
        if !matches!(slot.capability, Capability::Reply(_)) {
            return Err(CapError::WrongCapability {
                expected: ObjectKind::Reply,
                actual: capability_kind(&slot.capability),
            });
        }

        Ok(())
    }

    pub fn revoke_descendants(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<CapabilityRevocation, CapError> {
        let resolved = self.resolve_descriptor_ref(descriptor)?;
        self.revoke_resolved(resolved)
    }

    pub fn revoke_resolved(
        &mut self,
        resolved: ResolvedCapabilitySlot,
    ) -> Result<CapabilityRevocation, CapError> {
        let (target_slot, _) = self.validated_resolved_slot(resolved)?;
        let target_object = target_slot.object;
        let target = resolved.cte;
        let mut revoked_object_ids = Vec::new();
        let mut final_object_ids = Vec::new();

        loop {
            let Some(next_cte) = self.slot_by_ref(target).and_then(|slot| slot.mdb.next) else {
                break;
            };
            let Some(next_slot) = self.slot_by_ref(next_cte).filter(|slot| slot.alive) else {
                break;
            };
            if !self.is_mdb_parent_of(target, next_cte) {
                break;
            }

            if self.is_final_capability(next_cte) {
                push_unique_object(&mut final_object_ids, next_slot.object);
            }
            if next_slot.object != target_object {
                push_unique_object(&mut revoked_object_ids, next_slot.object);
            }

            self.delete_cte(ResolvedCapabilitySlot {
                descriptor: CapabilityDescriptor {
                    slot: next_cte.handle,
                    slot_generation: 0,
                },
                cte: next_cte,
            });
        }
        for object in &revoked_object_ids {
            self.untyped_allocations.remove(object);
        }
        self.reset_untyped_allocation(target);

        let mut revoked_objects = final_object_ids;
        for object in revoked_object_ids {
            push_unique_object(&mut revoked_objects, object);
        }
        revoked_objects.sort_by_key(|object| object.raw());

        Ok(CapabilityRevocation { revoked_objects })
    }

    pub fn object_has_live_cap(&self, object: ObjectId) -> bool {
        self.root_live_slots()
            .any(|(_, slot)| slot.alive && slot.object == object)
            || self
                .cnode_live_slots()
                .any(|(_, slot)| slot.alive && slot.object == object)
    }

    fn is_final_capability(&self, cte: CteRef) -> bool {
        let Some(slot_ref) = self.slot_by_ref(cte).filter(|slot| slot.alive) else {
            return false;
        };
        if let Some(prev) = slot_ref.mdb.prev
            && let Some(prev_slot) = self.slot_by_ref(prev).filter(|slot| slot.alive)
            && same_object_as(&prev_slot.capability, &slot_ref.capability)
            && prev_slot.object == slot_ref.object
        {
            return false;
        }
        if let Some(next) = slot_ref.mdb.next
            && let Some(next_slot) = self.slot_by_ref(next).filter(|slot| slot.alive)
            && same_object_as(&next_slot.capability, &slot_ref.capability)
            && next_slot.object == slot_ref.object
        {
            return false;
        }

        true
    }

    fn is_mdb_parent_of(&self, parent: CteRef, child: CteRef) -> bool {
        let Some(parent_slot) = self.slot_by_ref(parent).filter(|slot| slot.alive) else {
            return false;
        };
        let Some(child_slot) = self.slot_by_ref(child).filter(|slot| slot.alive) else {
            return false;
        };

        if !child_slot.mdb.revocable {
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
    pub fn bump_generation(&mut self, object: ObjectId) -> Result<u64, CapError> {
        let kernel_object = self.object_mut(object)?;
        kernel_object.generation += 1;
        Ok(kernel_object.generation)
    }

    #[cfg(test)]
    pub fn destroy_object(&mut self, object: ObjectId) -> Result<(), CapError> {
        let kernel_object = self.object_mut(object)?;
        kernel_object.destroyed = true;
        kernel_object.generation += 1;

        let slots_to_remove: Vec<_> = self
            .live_slot_entries()
            .filter_map(|(slot_id, slot)| (slot.object == object).then_some(slot_id))
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
        self.slot(slot).is_some_and(|slot| slot.alive)
    }

    fn slot(&self, slot: SlotId) -> Option<&CapabilitySlot> {
        self.cte_ref_for_slot(slot)
            .and_then(|cte| self.slot_by_ref(cte))
    }

    fn cte_ref_for_slot(&self, slot: SlotId) -> Option<CteRef> {
        if self.slots.has_root_entry(slot) {
            return Some(CteRef::root(slot));
        }
        if let Some(location) = self.slots.cnode_location(slot) {
            return Some(CteRef::cnode(slot, location));
        }

        None
    }

    fn slot_by_ref(&self, cte: CteRef) -> Option<&CapabilitySlot> {
        match cte.storage {
            CteStorageRef::Root => self.slots.get(cte.handle),
            CteStorageRef::CNode(location) => self.cnode_slot(location),
        }
    }

    fn slot_mut_by_ref(&mut self, cte: CteRef) -> Option<&mut CapabilitySlot> {
        match cte.storage {
            CteStorageRef::Root => self.slots.get_mut(cte.handle),
            CteStorageRef::CNode(location) => self.cnode_slot_mut(location),
        }
    }

    fn insert_cte(&mut self, cte: CteRef, value: CapabilitySlot) {
        match cte.storage {
            CteStorageRef::Root => self.slots.insert(cte.handle, value),
            CteStorageRef::CNode(location) => self.cnode_slot_insert(location, value),
        }
    }

    fn root_live_slots(&self) -> impl Iterator<Item = (SlotId, &CapabilitySlot)> {
        self.slots.iter_live()
    }

    #[cfg(test)]
    fn live_slot_entries(&self) -> impl Iterator<Item = (SlotId, &CapabilitySlot)> {
        self.root_live_slots().chain(self.cnode_live_slots())
    }

    fn cnode_live_slots(&self) -> impl Iterator<Item = (SlotId, &CapabilitySlot)> {
        let mut live = Vec::new();
        for object in self.objects.objects.iter().filter_map(Option::as_ref) {
            let KernelObjectPayload::CNode { slots } = &object.payload else {
                continue;
            };
            live.extend(slots.live_entries());
        }
        live.into_iter()
    }

    fn cnode_slot(&self, location: CteLocation) -> Option<&CapabilitySlot> {
        let object = self.objects.get(location.object)?;
        let KernelObjectPayload::CNode { slots } = &object.payload else {
            return None;
        };
        slots.get(location.offset)
    }

    fn cnode_slot_mut(&mut self, location: CteLocation) -> Option<&mut CapabilitySlot> {
        let object = self.objects.get_mut(location.object)?;
        let KernelObjectPayload::CNode { slots } = &mut object.payload else {
            return None;
        };
        slots.get_mut(location.offset)
    }

    fn cnode_slot_insert(&mut self, location: CteLocation, slot: CapabilitySlot) {
        let object = self
            .objects
            .get_mut(location.object)
            .expect("validated CNode location must reference an existing object");
        let KernelObjectPayload::CNode { slots } = &mut object.payload else {
            panic!("validated CNode location must reference CNode payload")
        };
        slots.insert(location.offset, slot);
    }

    fn insert_derived_capability(
        &mut self,
        parent: ResolvedCapabilitySlot,
        destination: Option<ResolvedCte>,
        requested_rights: Rights,
        params: MintParams,
    ) -> Result<CapabilityDescriptor, CapError> {
        if let Some(destination) = destination {
            self.validate_empty_cte(destination)?;
        }
        let (object, object_generation_snapshot, parent_capability) = {
            let (parent_slot, _) = self.validated_resolved_slot(parent)?;
            if !requested_rights.is_subset_of(parent_slot.rights) {
                return Err(CapError::RightsEscalation {
                    parent: parent.descriptor.slot,
                    parent_rights: parent_slot.rights,
                    requested_rights,
                });
            }
            if matches!(parent_slot.capability, Capability::Untyped(_))
                && parent_slot
                    .mdb
                    .next
                    .is_some_and(|next| self.is_mdb_parent_of(parent.cte, next))
            {
                return Err(CapError::CapabilityNotDerivable {
                    parent: parent.descriptor.slot,
                    capability: parent_slot.capability.clone(),
                });
            }
            (
                parent_slot.object,
                parent_slot.object_generation_snapshot,
                parent_slot.capability.clone(),
            )
        };
        let parent_slot_id = parent.descriptor.slot;
        let capability =
            mint_capability(parent_slot_id, &parent_capability, requested_rights, params)?;
        let destination = match destination {
            Some(destination) => destination,
            None => self.alloc_empty_cte(),
        };
        self.reserve_empty_cte_for_insert(destination)?;
        let slot_generation = self.slot_generation_for_cte(destination);
        self.detach_reused_cte(destination);
        let mdb = self.cte_insert_mdb(parent.cte, destination.cte, &capability, &parent_capability);
        self.insert_cte(
            destination.cte,
            CapabilitySlot {
                object,
                capability: capability.clone(),
                rights: capability_rights(&capability),
                slot_generation,
                object_generation_snapshot,
                parent: Some(parent.cte),
                children: ChildSlots::new(),
                mdb,
                alive: true,
            },
        );
        self.slot_mut_by_ref(parent.cte)
            .expect("validated parent slot must remain in CSpace during derivation")
            .children
            .insert(destination.cte);

        Ok(CapabilityDescriptor {
            slot: destination.slot,
            slot_generation,
        })
    }

    fn validate_retype_untyped_into(
        &self,
        source: ResolvedCapabilitySlot,
        target: &RetypeTarget,
        destination: RetypeDestination,
    ) -> Result<(UntypedAllocationPlan, Vec<ResolvedCte>), CapError> {
        if destination.count == 0 {
            return Err(CapError::EmptyRetypeWindow);
        }
        validate_slot_window_bounds(destination.start, destination.count)?;
        destination_window_end(destination)?;
        let mut destinations = Vec::new();
        for offset in 0..destination.count {
            let slot = slot_in_window(destination.start, offset);
            destinations.push(self.empty_cte_for_slot(slot)?);
        }
        let allocation =
            self.validate_retype_untyped_capacity(source, target, destination.count)?;
        Ok((allocation, destinations))
    }

    fn validate_empty_slot(&self, slot: SlotId) -> Result<(), CapError> {
        self.empty_cte_for_slot(slot).map(|_| ())
    }

    fn empty_cte_for_slot(&self, slot: SlotId) -> Result<ResolvedCte, CapError> {
        let cte = self
            .cte_ref_for_slot(slot)
            .unwrap_or_else(|| CteRef::root(slot));
        let resolved = ResolvedCte { slot, cte };
        self.validate_empty_cte(resolved)?;
        Ok(resolved)
    }

    fn validate_empty_cte(&self, cte: ResolvedCte) -> Result<(), CapError> {
        self.validate_resolved_cte_ref(cte)?;
        if self.slot_by_ref(cte.cte).is_some_and(|slot| slot.alive) {
            return Err(CapError::SlotOccupied(cte.slot));
        }
        Ok(())
    }

    fn reserve_empty_cte_for_insert(&mut self, cte: ResolvedCte) -> Result<(), CapError> {
        self.validate_empty_cte(cte)?;
        self.free_slots.retain(|free_slot| *free_slot != cte.slot);
        if cte.slot.raw() >= self.next_slot {
            self.next_slot = cte.slot.raw() + 1;
        }
        Ok(())
    }

    fn alloc_empty_cte(&mut self) -> ResolvedCte {
        let slot = self.alloc_slot_id();
        let cte = self
            .cte_ref_for_slot(slot)
            .unwrap_or_else(|| CteRef::root(slot));
        ResolvedCte { slot, cte }
    }

    fn slot_generation_for_cte(&self, cte: ResolvedCte) -> u64 {
        self.validate_resolved_cte_ref(cte)
            .expect("validated CTE generation lookup requires a coherent CTE reference");
        self.slot_by_ref(cte.cte)
            .map_or(1, |slot| slot.slot_generation + 1)
    }

    fn detach_reused_cte(&mut self, cte: ResolvedCte) {
        self.detach_reused_slot(cte.slot)
    }

    pub fn commit_retype_plan(&mut self, plan: RetypeCommitPlan) -> Result<RetypeResult, CapError> {
        self.validated_resolved_slot(plan.source)?;
        for entry in &plan.entries {
            self.validate_empty_cte(entry.destination)?;
            if let RetypedObjectKind::CNode {
                radix,
                window_start,
            } = entry.kind
            {
                validate_slot_window_bounds(window_start, cnode_slot_count(radix))?;
                self.validate_empty_slot_window(window_start, cnode_slot_count(radix))?;
            }
        }

        let parent_capability = self
            .slot_by_ref(plan.source.cte)
            .expect("validated parent slot must remain in CSpace during retype")
            .capability
            .clone();
        let mut expected_object = self.next_object;
        for entry in &plan.entries {
            assert_eq!(
                entry.object,
                ObjectId(expected_object),
                "retype commit plan object order must match CSpace allocation state"
            );
            expected_object += 1;
        }

        let mut descriptors = Vec::new();
        let mut objects = Vec::new();
        let mut retyped_objects = Vec::new();
        self.untyped_allocations
            .get_mut(plan.allocation.parent_object)
            .expect("validated parent untyped allocation must remain in CSpace")
            .watermark = plan.allocation.next_watermark;
        for entry in plan.entries {
            let object_generation = self.alloc_planned_retyped_object(entry.object, entry.kind);
            if let Capability::Untyped(capability) = &entry.capability {
                self.untyped_allocations.insert(
                    entry.object,
                    UntypedAllocation {
                        size_bits: capability.size_bits,
                        watermark: 0,
                    },
                );
            }

            self.reserve_empty_cte_for_insert(entry.destination)?;
            let slot_generation = self.slot_generation_for_cte(entry.destination);
            self.detach_reused_cte(entry.destination);
            let mdb = self.cte_insert_mdb(
                plan.source.cte,
                entry.destination.cte,
                &entry.capability,
                &parent_capability,
            );
            self.insert_cte(
                entry.destination.cte,
                CapabilitySlot {
                    object: entry.object,
                    rights: capability_rights(&entry.capability),
                    capability: entry.capability,
                    slot_generation,
                    object_generation_snapshot: object_generation,
                    parent: Some(plan.source.cte),
                    children: ChildSlots::new(),
                    mdb,
                    alive: true,
                },
            );
            self.slot_mut_by_ref(plan.source.cte)
                .expect("validated parent slot must remain in CSpace during retype")
                .children
                .insert(entry.destination.cte);

            descriptors.push(CapabilityDescriptor {
                slot: entry.destination.slot,
                slot_generation,
            });
            objects.push(entry.object);
            retyped_objects.push(RetypedObject {
                object: entry.object,
                kind: entry.kind,
            });
        }
        if plan.next_slot > self.next_slot {
            self.next_slot = plan.next_slot;
        }

        Ok(RetypeResult {
            descriptors,
            objects,
            retyped_objects,
        })
    }

    fn planned_retype_window_start(
        &self,
        target: &RetypeTarget,
        next_slot: &mut u64,
    ) -> Result<Option<SlotId>, CapError> {
        let RetypeTarget::CNode { radix } = target else {
            return Ok(None);
        };
        let slot_count = cnode_slot_count(*radix);
        let window_start = SlotId(*next_slot);
        validate_slot_window_bounds(window_start, slot_count)?;
        *next_slot = destination_window_end(RetypeDestination {
            start: window_start,
            count: slot_count,
        })?;
        Ok(Some(window_start))
    }

    fn planned_retype_kind(
        &self,
        target: &RetypeTarget,
        window_start: Option<SlotId>,
    ) -> RetypedObjectKind {
        match target {
            RetypeTarget::Endpoint => RetypedObjectKind::Endpoint,
            RetypeTarget::Frame { .. } => RetypedObjectKind::Frame,
            RetypeTarget::CNode { radix } => RetypedObjectKind::CNode {
                radix: *radix,
                window_start: window_start.expect("planned CNode retype must reserve a window"),
            },
            RetypeTarget::Untyped { .. } => RetypedObjectKind::Untyped,
            RetypeTarget::Tcb { .. } => RetypedObjectKind::Tcb,
            RetypeTarget::Notification => RetypedObjectKind::Notification,
        }
    }

    fn validate_empty_slot_window(&self, start: SlotId, count: usize) -> Result<(), CapError> {
        validate_slot_window_bounds(start, count)?;
        for offset in 0..count {
            self.validate_empty_slot(slot_in_window(start, offset))?;
        }
        Ok(())
    }

    fn validate_retype_untyped_capacity(
        &self,
        source: ResolvedCapabilitySlot,
        target: &RetypeTarget,
        count: usize,
    ) -> Result<UntypedAllocationPlan, CapError> {
        target.validate_retype_bounds()?;

        let (source_size, source_object) = {
            let (parent_slot, _) = self.validated_resolved_slot(source)?;
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
                parent: source.descriptor.slot,
                requested: requested_size,
                source: source_size,
            });
        }

        target.validate_rights()?;

        let allocation = self
            .untyped_allocations
            .get(source_object)
            .expect("validated Untyped cap must have allocation metadata");
        let next_watermark =
            allocation.next_watermark(source.descriptor.slot, requested_size, count)?;
        Ok(UntypedAllocationPlan {
            parent_object: source_object,
            next_watermark,
        })
    }

    fn resolve_address_bits(
        &self,
        root_object: ObjectId,
        root: &CNodeCap,
        capptr: u64,
        depth: u8,
    ) -> Result<CNodeLookup, CapError> {
        let mut node_object = root_object;
        let mut node = root.clone();
        let mut bits_remaining = depth;

        loop {
            let level_bits = node.radix.saturating_add(node.guard_size);
            if level_bits == 0 {
                return Err(CapError::CNodeDepthMismatch {
                    level_bits,
                    bits_remaining,
                });
            }

            let actual_guard = extract_cptr_bits(capptr, bits_remaining, node.guard_size);
            if node.guard_size > bits_remaining || actual_guard != node.guard {
                return Err(CapError::CNodeGuardMismatch {
                    expected_guard: node.guard,
                    actual_guard,
                    bits_remaining,
                    guard_size: node.guard_size,
                });
            }
            if level_bits > bits_remaining {
                return Err(CapError::CNodeDepthMismatch {
                    level_bits,
                    bits_remaining,
                });
            }

            let offset = extract_cptr_bits(capptr, bits_remaining - node.guard_size, node.radix);
            let slot = self.cnode_slot_handle(node_object, offset);
            let location = CteLocation {
                object: node_object,
                offset: usize::try_from(offset)
                    .expect("validated CNode offset must fit host usize"),
            };
            let cte = CteRef::cnode(slot, location);
            let slots_remaining = slots_remaining_in_level(node.radix, offset);
            if bits_remaining == level_bits {
                return Ok(CNodeLookup {
                    slot,
                    bits_remaining: 0,
                    slots_remaining,
                    cte,
                });
            }

            bits_remaining -= level_bits;
            let Some(slot_ref) = self.slot_by_ref(cte).filter(|slot| slot.alive) else {
                return Ok(CNodeLookup {
                    slot,
                    bits_remaining,
                    slots_remaining,
                    cte,
                });
            };
            let Capability::CNode(next_node) = &slot_ref.capability else {
                return Ok(CNodeLookup {
                    slot,
                    bits_remaining,
                    slots_remaining,
                    cte,
                });
            };
            node_object = slot_ref.object;
            node = next_node.clone();
        }
    }

    fn cnode_slot_handle(&self, object: ObjectId, offset: u64) -> SlotId {
        let object_ref = self
            .object(object)
            .expect("CNode cap must reference an existing CSpace object during lookup");
        match &object_ref.payload {
            KernelObjectPayload::CNode { slots } => slots
                .handle(offset)
                .expect("validated CNode radix must keep lookup offset inside owned slots"),
            payload => panic!(
                "CNode cap must reference CNode object payload, found {:?}",
                payload.kind()
            ),
        }
    }

    fn alloc_object(&mut self, kind: ObjectKind) -> (ObjectId, u64) {
        self.alloc_object_with_payload(KernelObjectPayload::from_initial_kind(kind))
    }

    fn alloc_object_with_payload(&mut self, payload: KernelObjectPayload) -> (ObjectId, u64) {
        let object = ObjectId(self.next_object);
        self.next_object += 1;
        let generation = 1;
        self.objects.insert(
            object,
            KernelObject {
                payload,
                generation,
                destroyed: false,
            },
        );
        self.register_cnode_locations(object);
        (object, generation)
    }

    fn alloc_planned_retyped_object(&mut self, object: ObjectId, kind: RetypedObjectKind) -> u64 {
        self.next_object += 1;
        let generation = 1;
        self.objects.insert(
            object,
            KernelObject {
                payload: KernelObjectPayload::from_retyped_kind(kind),
                generation,
                destroyed: false,
            },
        );
        self.register_cnode_locations(object);
        generation
    }

    fn register_cnode_locations(&mut self, object: ObjectId) {
        let Some(KernelObject {
            payload: KernelObjectPayload::CNode { slots },
            ..
        }) = self.objects.get(object)
        else {
            return;
        };
        let handles: Vec<_> = slots.handles().collect();
        for (offset, handle) in handles {
            self.slots
                .insert_cnode_location(handle, CteLocation { object, offset });
        }
    }

    fn insert_root_slot(
        &mut self,
        object: ObjectId,
        capability: Capability,
        generation: u64,
    ) -> CapabilityDescriptor {
        let slot = self.alloc_slot_id();
        self.insert_root_slot_at(slot, object, capability, generation)
    }

    fn insert_root_slot_at(
        &mut self,
        slot: SlotId,
        object: ObjectId,
        capability: Capability,
        generation: u64,
    ) -> CapabilityDescriptor {
        let untyped_size = match &capability {
            Capability::Untyped(capability) => Some(capability.size_bits),
            _ => None,
        };
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
                children: ChildSlots::new(),
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

    fn insert_initial_capability_into_cnode(
        &mut self,
        cnode: ObjectId,
        object: ObjectId,
        capability: Capability,
        generation: u64,
    ) -> CapabilityDescriptor {
        let location = self
            .first_empty_cnode_location(cnode)
            .expect("initial CNode must have an empty slot for bootstrap capabilities");
        let slot = self.cnode_slot_handle(location.object, location.offset as u64);
        let untyped_size = match &capability {
            Capability::Untyped(capability) => Some(capability.size_bits),
            _ => None,
        };
        let rights = capability_rights(&capability);
        let slot_generation = self.slot_generation_for_insert(slot);
        self.cnode_slot_insert(
            location,
            CapabilitySlot {
                object,
                capability,
                rights,
                slot_generation,
                object_generation_snapshot: generation,
                parent: None,
                children: ChildSlots::new(),
                mdb: MdbNode::default(),
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

    fn first_empty_cnode_location(&self, object: ObjectId) -> Option<CteLocation> {
        let object_ref = self.objects.get(object)?;
        let KernelObjectPayload::CNode { slots } = &object_ref.payload else {
            return None;
        };
        slots.first_empty_location(object)
    }

    fn available_cnode_window_start(
        &self,
        preferred: SlotId,
        count: usize,
    ) -> Result<SlotId, CapError> {
        if self.is_cnode_window_available(preferred, count)? {
            return Ok(preferred);
        }

        let mut raw = align_slot_base(self.next_slot, count)?;
        loop {
            let candidate = SlotId(raw);
            if self.is_cnode_window_available(candidate, count)? {
                return Ok(candidate);
            }
            raw = raw
                .checked_add(count as u64)
                .ok_or(CapError::SlotWindowOverflow {
                    start: preferred,
                    count,
                })?;
        }
    }

    fn is_cnode_window_available(&self, start: SlotId, count: usize) -> Result<bool, CapError> {
        validate_slot_window_bounds(start, count)?;
        for offset in 0..count {
            let slot = slot_in_window(start, offset);
            if self.slots.has_root_entry(slot) || self.slots.cnode_location(slot).is_some() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn cnode_root_descriptor_slot(
        &self,
        window_start: SlotId,
        count: usize,
    ) -> Result<SlotId, CapError> {
        let mut raw = destination_window_end(RetypeDestination {
            start: window_start,
            count,
        })?;
        loop {
            let slot = SlotId(raw);
            if self.slot(slot).is_none() && self.slots.cnode_location(slot).is_none() {
                return Ok(slot);
            }
            raw = raw.checked_add(1).ok_or(CapError::SlotWindowOverflow {
                start: window_start,
                count,
            })?;
        }
    }

    fn alloc_slot_id(&mut self) -> SlotId {
        if let Some(slot) = self.free_slots.pop() {
            return slot;
        }

        loop {
            let slot = SlotId(self.next_slot);
            self.next_slot += 1;
            if !self.slot(slot).is_some_and(|slot| slot.alive) {
                return slot;
            }
        }
    }

    fn next_auto_slot_id(&self) -> SlotId {
        if let Some(slot) = self.free_slots.last() {
            return *slot;
        }

        let mut raw = self.next_slot;
        loop {
            let slot = SlotId(raw);
            if !self.slot(slot).is_some_and(|slot| slot.alive) {
                return slot;
            }
            raw += 1;
        }
    }

    fn next_slot_after_retype_destinations(
        &self,
        destination: RetypeDestination,
    ) -> Result<u64, CapError> {
        let after_destination = destination_window_end(destination)?;
        Ok(self.next_slot.max(after_destination))
    }

    fn slot_generation_for_insert(&self, slot: SlotId) -> u64 {
        self.slot(slot).map_or(1, |slot| slot.slot_generation + 1)
    }

    fn validated_slot(
        &self,
        descriptor: CapabilityDescriptor,
    ) -> Result<(&CapabilitySlot, &KernelObject), CapError> {
        let resolved = self.resolve_descriptor_ref(descriptor)?;
        self.validated_resolved_slot(resolved)
    }

    fn validated_resolved_slot(
        &self,
        resolved: ResolvedCapabilitySlot,
    ) -> Result<(&CapabilitySlot, &KernelObject), CapError> {
        self.validate_resolved_cte_ref(ResolvedCte {
            slot: resolved.descriptor.slot,
            cte: resolved.cte,
        })?;
        let slot = self
            .slot_by_ref(resolved.cte)
            .filter(|slot| slot.alive)
            .ok_or(CapError::SlotNotFound(resolved.descriptor.slot))?;
        let object = self.object(slot.object)?;

        if object.destroyed {
            return Err(CapError::ObjectDestroyed(slot.object));
        }

        if resolved.descriptor.slot_generation != slot.slot_generation {
            return Err(CapError::StaleDescriptor {
                slot: resolved.descriptor.slot,
                expected_generation: slot.slot_generation,
                actual_generation: resolved.descriptor.slot_generation,
            });
        }

        if slot.object_generation_snapshot != object.generation {
            return Err(CapError::StaleDescriptor {
                slot: resolved.descriptor.slot,
                expected_generation: object.generation,
                actual_generation: slot.object_generation_snapshot,
            });
        }

        Ok((slot, object))
    }

    fn validate_resolved_cte_ref(&self, resolved: ResolvedCte) -> Result<(), CapError> {
        if resolved.cte.handle != resolved.slot {
            return Err(CapError::InvalidCteReference {
                slot: resolved.slot,
            });
        }

        match resolved.cte.storage {
            CteStorageRef::Root => {
                if self.slots.cnode_location(resolved.slot).is_some() {
                    return Err(CapError::InvalidCteReference {
                        slot: resolved.slot,
                    });
                }
            }
            CteStorageRef::CNode(location) => {
                if self.slots.cnode_location(resolved.slot) != Some(location) {
                    return Err(CapError::InvalidCteReference {
                        slot: resolved.slot,
                    });
                }
            }
        }

        Ok(())
    }

    fn validate_descriptor(&self, descriptor: CapabilityDescriptor) -> Result<(), CapError> {
        self.validated_slot(descriptor).map(|_| ())
    }

    fn resolve_descriptor_ref(
        &self,
        descriptor: CapabilityDescriptor,
    ) -> Result<ResolvedCapabilitySlot, CapError> {
        let cte = self
            .cte_ref_for_slot(descriptor.slot)
            .ok_or(CapError::SlotNotFound(descriptor.slot))?;
        Ok(ResolvedCapabilitySlot { descriptor, cte })
    }

    fn live_slot(&self, slot: SlotId) -> Result<&CapabilitySlot, CapError> {
        let slot_ref = self.slot(slot).ok_or(CapError::SlotNotFound(slot))?;
        if !slot_ref.alive {
            return Err(CapError::SlotNotFound(slot));
        }

        Ok(slot_ref)
    }

    fn object(&self, object: ObjectId) -> Result<&KernelObject, CapError> {
        self.objects
            .get(object)
            .ok_or(CapError::ObjectNotFound(object))
    }

    #[cfg(test)]
    fn object_mut(&mut self, object: ObjectId) -> Result<&mut KernelObject, CapError> {
        self.objects
            .get_mut(object)
            .ok_or(CapError::ObjectNotFound(object))
    }

    fn delete_slot(&mut self, slot: SlotId) {
        let Some(cte) = self.cte_ref_for_slot(slot) else {
            return;
        };
        self.delete_cte(ResolvedCapabilitySlot {
            descriptor: CapabilityDescriptor {
                slot,
                slot_generation: 0,
            },
            cte,
        });
    }

    fn delete_cte(&mut self, resolved: ResolvedCapabilitySlot) {
        let Some(removed) = self.slot_mut_by_ref(resolved.cte) else {
            return;
        };

        if !removed.alive {
            return;
        }

        let parent = removed.parent;
        removed.alive = false;
        let reusable = removed.children.is_empty();
        let _ = removed;
        self.empty_mdb_slot(resolved.cte);
        if reusable {
            self.push_free_slot_once(resolved.descriptor.slot);
        }
        if let Some(parent) = parent {
            self.remove_child_link(parent, resolved.cte);
        }
    }

    fn remove_child_link(&mut self, parent: CteRef, child: CteRef) {
        let Some(parent_slot) = self.slot_mut_by_ref(parent) else {
            return;
        };
        parent_slot.children.remove(&child);
        let parent_is_reusable = !parent_slot.alive && parent_slot.children.is_empty();
        let _ = parent_slot;
        if parent_is_reusable {
            self.push_free_slot_once(parent.handle);
        }
    }

    fn push_free_slot_once(&mut self, slot: SlotId) {
        if !self.free_slots.contains(&slot) {
            self.free_slots.push(slot);
        }
    }

    fn detach_reused_slot(&mut self, slot: SlotId) {
        let Some(old_parent) = self.slot(slot).and_then(|slot| slot.parent) else {
            return;
        };
        let cte = self
            .cte_ref_for_slot(slot)
            .unwrap_or_else(|| CteRef::root(slot));

        if let Some(parent) = self.slot_mut_by_ref(old_parent) {
            parent.children.remove(&cte);
        }
        self.empty_mdb_slot(cte);
    }

    fn cte_insert_mdb(
        &mut self,
        parent: CteRef,
        slot: CteRef,
        new_cap: &Capability,
        parent_cap: &Capability,
    ) -> MdbNode {
        let next = self.slot_by_ref(parent).and_then(|parent| parent.mdb.next);
        let revocable = is_cap_revocable(new_cap, parent_cap);
        let first_badged = is_first_badged_derivation(new_cap, parent_cap);
        if let Some(next) = next {
            self.slot_mut_by_ref(next)
                .expect("parent MDB next slot must remain in CSpace")
                .mdb
                .prev = Some(slot);
        }
        self.slot_mut_by_ref(parent)
            .expect("validated parent slot must remain in CSpace during cteInsert")
            .mdb
            .next = Some(slot);

        MdbNode {
            prev: Some(parent),
            next,
            revocable,
            first_badged,
        }
    }

    fn empty_mdb_slot(&mut self, cte: CteRef) {
        let Some(mdb) = self.slot_by_ref(cte).map(|slot| slot.mdb) else {
            return;
        };
        if let Some(prev) = mdb.prev
            && let Some(prev_slot) = self.slot_mut_by_ref(prev)
        {
            prev_slot.mdb.next = mdb.next;
        }
        if let Some(next) = mdb.next
            && let Some(next_slot) = self.slot_mut_by_ref(next)
        {
            next_slot.mdb.prev = mdb.prev;
            next_slot.mdb.first_badged |= mdb.first_badged;
        }
        if let Some(slot_ref) = self.slot_mut_by_ref(cte) {
            slot_ref.mdb = MdbNode::default();
        }
    }

    fn reset_untyped_allocation(&mut self, cte: CteRef) {
        let Some(slot_ref) = self.slot_by_ref(cte) else {
            return;
        };
        if !matches!(slot_ref.capability, Capability::Untyped(_)) {
            return;
        };

        if let Some(allocation) = self.untyped_allocations.get_mut(slot_ref.object) {
            allocation.watermark = 0;
        }
    }
}

impl KernelObjectPayload {
    fn from_initial_kind(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Endpoint => Self::Endpoint,
            ObjectKind::Frame => Self::Frame,
            ObjectKind::CNode => {
                unreachable!("initial CNode objects require explicit CTE window metadata")
            }
            ObjectKind::Untyped => Self::Untyped,
            ObjectKind::Tcb => Self::Tcb,
            ObjectKind::Notification => Self::Notification,
            ObjectKind::Reply => Self::Reply,
        }
    }

    fn from_retyped_kind(kind: RetypedObjectKind) -> Self {
        match kind {
            RetypedObjectKind::Endpoint => Self::Endpoint,
            RetypedObjectKind::Frame => Self::Frame,
            RetypedObjectKind::CNode {
                radix,
                window_start,
            } => Self::CNode {
                slots: CNodeSlots::from_window(window_start, cnode_slot_count(radix)),
            },
            RetypedObjectKind::Untyped => Self::Untyped,
            RetypedObjectKind::Tcb => Self::Tcb,
            RetypedObjectKind::Notification => Self::Notification,
        }
    }

    const fn kind(&self) -> ObjectKind {
        match self {
            Self::Endpoint => ObjectKind::Endpoint,
            Self::Frame => ObjectKind::Frame,
            Self::CNode { .. } => ObjectKind::CNode,
            Self::Untyped => ObjectKind::Untyped,
            Self::Tcb => ObjectKind::Tcb,
            Self::Notification => ObjectKind::Notification,
            Self::Reply => ObjectKind::Reply,
        }
    }
}

impl CNodeSlots {
    fn from_window(window_start: SlotId, count: usize) -> Self {
        let slots = (0..count)
            .map(|offset| CNodeSlotEntry {
                handle: slot_in_window(window_start, offset),
                slot: None,
            })
            .collect();
        Self { slots }
    }

    fn handle(&self, offset: u64) -> Option<SlotId> {
        let offset = usize::try_from(offset).ok()?;
        self.slots.get(offset).map(|entry| entry.handle)
    }

    fn get(&self, offset: usize) -> Option<&CapabilitySlot> {
        self.slots.get(offset).and_then(|entry| entry.slot.as_ref())
    }

    fn get_mut(&mut self, offset: usize) -> Option<&mut CapabilitySlot> {
        self.slots
            .get_mut(offset)
            .and_then(|entry| entry.slot.as_mut())
    }

    fn first_empty_location(&self, object: ObjectId) -> Option<CteLocation> {
        self.slots
            .iter()
            .position(|entry| entry.slot.is_none())
            .map(|offset| CteLocation { object, offset })
    }

    fn insert(&mut self, offset: usize, slot: CapabilitySlot) {
        let entry = self
            .slots
            .get_mut(offset)
            .expect("validated CNode location must fit owned CTE array");
        entry.slot = Some(slot);
    }

    fn handles(&self) -> impl Iterator<Item = (usize, SlotId)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .map(|(offset, entry)| (offset, entry.handle))
    }

    fn live_entries(&self) -> impl Iterator<Item = (SlotId, &CapabilitySlot)> {
        self.slots
            .iter()
            .filter_map(|entry| entry.slot.as_ref().map(|slot| (entry.handle, slot)))
    }
}

impl KernelObject {
    fn kind(&self) -> ObjectKind {
        self.payload.kind()
    }
}

impl ChildSlots {
    const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn insert(&mut self, cte: CteRef) {
        if !self.slots.contains(&cte) {
            self.slots.push(cte);
        }
    }

    fn remove(&mut self, cte: &CteRef) -> bool {
        let Some(index) = self.slots.iter().position(|child| child == cte) else {
            return false;
        };
        self.slots.remove(index);
        true
    }

    fn clear(&mut self) {
        self.slots.clear();
    }

    fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = CteRef> + '_ {
        self.slots.iter().copied()
    }
}

impl CteSlots {
    fn get(&self, slot: SlotId) -> Option<&CapabilitySlot> {
        self.root_slots
            .get(slot_index(slot))
            .and_then(Option::as_ref)
    }

    fn get_mut(&mut self, slot: SlotId) -> Option<&mut CapabilitySlot> {
        self.root_slots
            .get_mut(slot_index(slot))
            .and_then(Option::as_mut)
    }

    fn has_root_entry(&self, slot: SlotId) -> bool {
        self.root_slots
            .get(slot_index(slot))
            .is_some_and(Option::is_some)
    }

    fn insert(&mut self, slot: SlotId, value: CapabilitySlot) {
        self.ensure_slot(slot);
        self.root_slots[slot_index(slot)] = Some(value);
    }

    fn cnode_location(&self, slot: SlotId) -> Option<CteLocation> {
        self.cnode_locations
            .get(slot_index(slot))
            .and_then(|location| *location)
    }

    fn insert_cnode_location(&mut self, slot: SlotId, location: CteLocation) {
        let index = slot_index(slot);
        if self.cnode_locations.len() <= index {
            self.cnode_locations.resize_with(index + 1, || None);
        }
        self.cnode_locations[index] = Some(location);
    }

    fn ensure_slot(&mut self, slot: SlotId) {
        let index = slot_index(slot);
        if self.root_slots.len() <= index {
            self.root_slots.resize_with(index + 1, || None);
        }
    }

    fn iter_live(&self) -> impl Iterator<Item = (SlotId, &CapabilitySlot)> {
        self.root_slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let raw = u64::try_from(index).ok()?;
                slot.as_ref().map(|slot| (SlotId(raw), slot))
            })
    }
}

impl ObjectStorage {
    fn get(&self, object: ObjectId) -> Option<&KernelObject> {
        self.objects
            .get(object_index(object))
            .and_then(Option::as_ref)
    }

    fn get_mut(&mut self, object: ObjectId) -> Option<&mut KernelObject> {
        self.objects
            .get_mut(object_index(object))
            .and_then(Option::as_mut)
    }

    fn insert(&mut self, object: ObjectId, value: KernelObject) {
        let index = object_index(object);
        if self.objects.len() <= index {
            self.objects.resize_with(index + 1, || None);
        }
        self.objects[index] = Some(value);
    }
}

impl UntypedStorage {
    fn get(&self, object: ObjectId) -> Option<&UntypedAllocation> {
        self.allocations
            .get(object_index(object))
            .and_then(Option::as_ref)
    }

    fn get_mut(&mut self, object: ObjectId) -> Option<&mut UntypedAllocation> {
        self.allocations
            .get_mut(object_index(object))
            .and_then(Option::as_mut)
    }

    fn insert(&mut self, object: ObjectId, value: UntypedAllocation) {
        let index = object_index(object);
        if self.allocations.len() <= index {
            self.allocations.resize_with(index + 1, || None);
        }
        self.allocations[index] = Some(value);
    }

    fn remove(&mut self, object: &ObjectId) -> Option<UntypedAllocation> {
        self.allocations
            .get_mut(object_index(*object))
            .and_then(Option::take)
    }
}

fn slot_index(slot: SlotId) -> usize {
    usize::try_from(slot.raw()).expect("slot id must fit host usize")
}

fn object_index(object: ObjectId) -> usize {
    usize::try_from(object.raw()).expect("object id must fit host usize")
}

fn push_unique_object(objects: &mut Vec<ObjectId>, object: ObjectId) {
    if !objects.contains(&object) {
        objects.push(object);
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

fn validate_cnode_radix(radix: u8) -> Result<(), CapError> {
    if radix == 0 || radix > MAX_MODEL_CNODE_RADIX {
        return Err(CapError::InvalidCNodeDepth { depth: radix });
    }

    Ok(())
}

fn validate_slot_window_bounds(start: SlotId, count: usize) -> Result<(), CapError> {
    let Some(last_offset) = count.checked_sub(1) else {
        return Ok(());
    };
    let Some(last_offset) = u64::try_from(last_offset).ok() else {
        return Err(CapError::SlotWindowOverflow { start, count });
    };
    if start.raw().checked_add(last_offset).is_none() {
        return Err(CapError::SlotWindowOverflow { start, count });
    }

    Ok(())
}

fn destination_window_end(destination: RetypeDestination) -> Result<u64, CapError> {
    let Some(count) = u64::try_from(destination.count).ok() else {
        return Err(CapError::SlotWindowOverflow {
            start: destination.start,
            count: destination.count,
        });
    };
    destination
        .start
        .raw()
        .checked_add(count)
        .ok_or(CapError::SlotWindowOverflow {
            start: destination.start,
            count: destination.count,
        })
}

fn align_slot_base(raw: u64, count: usize) -> Result<u64, CapError> {
    let count_raw = u64::try_from(count).expect("CNode slot count must fit u64");
    if count_raw == 0 {
        return Ok(raw);
    }
    let remainder = raw % count_raw;
    if remainder == 0 {
        return Ok(raw);
    }
    raw.checked_add(count_raw - remainder)
        .ok_or(CapError::SlotWindowOverflow {
            start: SlotId(raw),
            count,
        })
}

fn slot_in_window(start: SlotId, offset: usize) -> SlotId {
    SlotId(
        start
            .raw()
            .checked_add(offset as u64)
            .expect("validated CNode slot window must not overflow"),
    )
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
    if same_region_as(parent_cap, new_cap) {
        return true;
    }

    match (new_cap, parent_cap) {
        (_, Capability::Untyped(_)) | (Capability::Untyped(_), _) => true,
        _ => false,
    }
}

fn is_first_badged_derivation(new_cap: &Capability, parent_cap: &Capability) -> bool {
    match (new_cap, parent_cap) {
        (Capability::Endpoint(new_cap), Capability::Endpoint(parent_cap)) => {
            new_cap.badge != parent_cap.badge
        }
        (Capability::Notification(new_cap), Capability::Notification(parent_cap)) => {
            new_cap.badge != parent_cap.badge
        }
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

fn extract_cptr_bits(capptr: u64, bits_remaining: u8, width: u8) -> u64 {
    if width == 0 {
        return 0;
    }

    let shift = bits_remaining.saturating_sub(width);
    (capptr >> shift) & guard_mask(width)
}

fn slots_remaining_in_level(radix: u8, offset: u64) -> usize {
    if radix >= usize::BITS as u8 {
        return usize::MAX;
    }

    (1usize << radix).saturating_sub(offset as usize)
}

fn cnode_slot_count(radix: u8) -> usize {
    if radix >= usize::BITS as u8 {
        return usize::MAX;
    }

    1usize << radix
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
    fn cnode_path_lookup_resolves_guard_and_radix_bits_to_slot() {
        // Goal: CNode lookup consumes guard then radix bits like seL4 resolveAddressBits.
        // Scope: capability-space CNode path lookup API.
        // Semantics: with depth equal to guard+radix, lookup returns the selected CTE slot.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_cnode_capability(CNodeCap::with_guard(4, 0b10, 2), SlotId::new(32))
            .unwrap();

        assert_eq!(
            cspace.lookup_cnode_slot(CNodePath {
                root,
                capptr: 0b10_0110,
                depth: 6,
            }),
            Ok(SlotId::new(32 + 0b0110))
        );
    }

    #[test]
    fn cnode_path_lookup_rejects_guard_mismatch_before_slot_access() {
        // Goal: guard mismatch is a lookup fault before touching the target CTE.
        // Scope: capability-space CNode path lookup API.
        // Semantics: the error reports expected and actual guard at the current depth.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_cnode_capability(CNodeCap::with_guard(4, 0b10, 2), SlotId::new(0))
            .unwrap();

        assert_eq!(
            cspace.lookup_cnode_slot(CNodePath {
                root,
                capptr: 0b11_0110,
                depth: 6,
            }),
            Err(CapError::CNodeGuardMismatch {
                expected_guard: 0b10,
                actual_guard: 0b11,
                bits_remaining: 6,
                guard_size: 2,
            })
        );
    }

    #[test]
    fn cnode_path_lookup_rejects_depth_shorter_than_level() {
        // Goal: CNode depth mismatch mirrors seL4 when guard+radix exceeds remaining bits.
        // Scope: capability-space CNode path lookup API.
        // Semantics: the lookup fails before producing a target slot.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_cnode_capability(CNodeCap::with_guard(4, 0b10, 2), SlotId::new(0))
            .unwrap();

        assert_eq!(
            cspace.lookup_cnode_slot(CNodePath {
                root,
                capptr: 0b10_011,
                depth: 5,
            }),
            Err(CapError::CNodeDepthMismatch {
                level_bits: 6,
                bits_remaining: 5,
            })
        );
    }

    #[test]
    fn cnode_path_lookup_reports_guard_mismatch_when_guard_exceeds_remaining_bits() {
        // Goal: guard mismatch is reported before depth mismatch when guard bits do not fit.
        // Scope: capability-space CNode path lookup API.
        // Semantics: the lookup fault preserves expected/actual guard diagnostics.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_cnode_capability(CNodeCap::with_guard(4, 0b10, 3), SlotId::new(0))
            .unwrap();

        assert_eq!(
            cspace.lookup_cnode_slot(CNodePath {
                root,
                capptr: 0b10_0110,
                depth: 5,
            }),
            Err(CapError::CNodeGuardMismatch {
                expected_guard: 0b10,
                actual_guard: 0b001,
                bits_remaining: 5,
                guard_size: 3,
            })
        );
    }

    #[test]
    fn cnode_path_lookup_reports_unresolved_non_cnode_slot() {
        // Goal: multi-level lookup stops at the first non-CNode slot with remaining bits.
        // Scope: capability-space CNode path lookup API.
        // Semantics: full slot lookup requires all bits to resolve through CNode caps.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_cnode_capability(CNodeCap::new(4), SlotId::new(0))
            .unwrap();
        let source = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        cspace
            .copy_into(source, SlotId::new(3), Rights::READ)
            .unwrap();

        assert_eq!(
            cspace.lookup_cnode_slot(CNodePath {
                root,
                capptr: 0b0011_1010,
                depth: 8,
            }),
            Err(CapError::CNodeLookupUnresolved {
                slot: SlotId::new(3),
                bits_remaining: 4,
            })
        );
    }

    #[test]
    fn cnode_path_lookup_reports_live_descriptor_or_empty_destination() {
        // Goal: CNode operation preflight consumes resolved CTE facts without caller re-lookup.
        // Scope: capability-space source and destination CNode path helpers.
        // Semantics: source lookup returns the live descriptor, while destination preflight rejects occupied slots.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_cnode_capability(CNodeCap::new(4), SlotId::new(32))
            .unwrap();
        let source = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        let copied = cspace
            .copy_into(source, SlotId::new(33), Rights::READ)
            .unwrap();

        assert_eq!(
            cspace.lookup_cnode_descriptor(CNodePath {
                root,
                capptr: 0b0001,
                depth: 4,
            }),
            Ok(copied)
        );
        assert_eq!(
            cspace.lookup_cnode_empty_slot(CNodePath {
                root,
                capptr: 0b0010,
                depth: 4,
            }),
            Ok(SlotId::new(34))
        );
        assert_eq!(
            cspace.lookup_cnode_empty_slot(CNodePath {
                root,
                capptr: 0b0001,
                depth: 4,
            }),
            Err(CapError::SlotOccupied(SlotId::new(33)))
        );
    }

    #[test]
    fn copied_cnode_cap_uses_object_slot_array() {
        // Goal: CNode storage location belongs to the object, not the copied cap payload.
        // Scope: capability-space CNode copy and path lookup.
        // Semantics: a copied CNode cap resolves paths through the source object's owned CTE slot array.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_cnode_capability(CNodeCap::with_guard(4, 0b10, 2), SlotId::new(48))
            .unwrap();

        let copied_root = cspace.copy(root, Rights::NONE).unwrap();

        assert_eq!(
            cspace.lookup_cnode_slot(CNodePath {
                root: copied_root,
                capptr: 0b10_0101,
                depth: 6,
            }),
            Ok(SlotId::new(48 + 0b0101))
        );
    }

    #[test]
    fn root_capability_can_be_created_and_looked_up() {
        // Goal: initial cap insertion establishes a root capability view.
        // Scope: CapabilitySpace root slot creation and lookup.
        // Semantics: root cap has no parent and preserves object kind, rights, descriptor, and capability data.
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
        // Goal: initial capability insertion enforces object rights policy.
        // Scope: root cap creation before any slot or object is committed.
        // Semantics: invalid rights are rejected at the boundary with the object-specific allowed mask.
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
        // Goal: copying a badged endpoint preserves badge while reducing authority.
        // Scope: CSpace derivation from one endpoint cap.
        // Semantics: child rights cannot exceed parent rights and lineage points at the source slot.
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
    fn copy_into_uses_requested_empty_slot() {
        // Goal: explicit copy destination is honored when the slot is empty.
        // Scope: CSpace copy_into slot allocation boundary.
        // Semantics: derived cap is installed in the requested slot with reduced authority.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(badged_endpoint(Rights::READ | Rights::WRITE, 0x44))
            .unwrap();
        let destination = SlotId::new(30);

        let copy = cspace.copy_into(root, destination, Rights::READ).unwrap();

        assert_eq!(copy.slot, destination);
        assert_eq!(
            cspace.lookup(copy).unwrap().capability,
            badged_endpoint(Rights::READ, 0x44)
        );
    }

    #[test]
    fn copy_into_occupied_destination_fails_before_source_derivation() {
        // Goal: copy_into rejects occupied destinations before mutating derivation state.
        // Scope: explicit slot allocation precheck.
        // Semantics: occupied destination cap remains unchanged and no child is derived.
        let mut cspace = CapabilitySpace::new();
        let source = cspace
            .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE))
            .unwrap();
        let occupied = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();

        assert_eq!(
            cspace.copy_into(source, occupied.slot, Rights::READ),
            Err(CapError::SlotOccupied(occupied.slot))
        );
        assert_eq!(
            cspace.lookup(occupied).unwrap().capability,
            endpoint(Rights::READ)
        );
    }

    #[test]
    fn copy_into_reused_slot_is_removed_from_free_list() {
        // Goal: explicit reuse of a deleted slot removes that slot from implicit allocation.
        // Scope: CSpace free-list ownership after copy_into.
        // Semantics: later implicit insertion cannot allocate the same live slot twice.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE))
            .unwrap();
        let reusable = cspace.copy(root, Rights::READ).unwrap();
        cspace.delete(reusable).unwrap();

        let explicit = cspace.copy_into(root, reusable.slot, Rights::READ).unwrap();
        let implicit = cspace.copy(root, Rights::READ).unwrap();

        assert_eq!(explicit.slot, reusable.slot);
        assert_ne!(implicit.slot, reusable.slot);
        assert!(cspace.lookup(explicit).is_ok());
    }

    #[test]
    fn mint_can_set_endpoint_badge_without_escalating_rights() {
        // Goal: endpoint mint can add a badge while reducing rights.
        // Scope: badge mint derivation for unbadged endpoint caps.
        // Semantics: minted child records badge and parent lineage without authority escalation.
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
        // Goal: notification mint can add a badge while reducing rights.
        // Scope: badge mint derivation for notification caps.
        // Semantics: minted child records badge and parent lineage without authority escalation.
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
        // Goal: badge minting is limited to endpoint and notification capabilities.
        // Scope: mint boundary for non-badge object kinds.
        // Semantics: unsupported cap kinds fail without installing a child cap.
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
        // Goal: badge minting cannot overwrite an existing badge.
        // Scope: mint boundary for already badged endpoint and notification caps.
        // Semantics: existing badge remains authoritative and no child cap is installed.
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
        // Goal: failed mint is transactionally side-effect free for slots and lineage.
        // Scope: mint failure after a deleted child slot becomes reusable.
        // Semantics: reusable slot stays available and source root remains parentless.
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
        // Goal: moving a capability transfers slot identity in the derivation tree.
        // Scope: move_capability and later revoke over moved lineage.
        // Semantics: parent/child links retarget to the new slot and revoke still removes descendants.
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
    fn moving_cnode_owned_cap_retargets_cte_backed_lineage() {
        // Goal: CNode-owned CTEs participate in MDB and child links by CTE identity.
        // Scope: move_capability over a cap installed in a CNode-owned slot and later revoke.
        // Semantics: moving a CNode-owned child retargets descendants without falling back to root slot truth.
        let mut cspace = CapabilitySpace::new();
        let root_cnode = cspace
            .insert_initial_cnode_capability(CNodeCap::new(4), SlotId(0))
            .unwrap();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace
            .copy_into(
                root,
                SlotId(root.slot.raw() + 1),
                Rights::READ | Rights::WRITE,
            )
            .unwrap();
        let grandchild = cspace.copy(child, Rights::READ).unwrap();

        let moved = cspace.move_capability(child).unwrap();

        assert_eq!(cspace.lookup(moved).unwrap().parent, Some(root.slot));
        assert_eq!(cspace.lookup(grandchild).unwrap().parent, Some(moved.slot));
        cspace.revoke_descendants(root).unwrap();

        assert!(cspace.lookup(root_cnode).is_ok());
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
    fn deleted_cnode_owned_cte_reuses_cnode_storage_without_root_shadow() {
        // Goal: the free-list stores facade handles but allocation re-resolves them to CTE storage.
        // Scope: delete of a CNode-owned child followed by auto destination copy.
        // Semantics: the reused descriptor writes the CNode-owned CTE, not a root shadow slot.
        let mut cspace = CapabilitySpace::new();
        cspace
            .insert_initial_cnode_capability(CNodeCap::new(4), SlotId(0))
            .unwrap();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let child = cspace
            .copy_into(root, SlotId(root.slot.raw() + 1), Rights::READ)
            .unwrap();

        cspace.delete(child).unwrap();
        let reused = cspace.copy(root, Rights::WRITE).unwrap();

        assert_eq!(reused.slot, child.slot);
        assert!(!cspace.slots.has_root_entry(reused.slot));
        assert_eq!(
            cspace.lookup(reused).unwrap().capability,
            endpoint(Rights::WRITE)
        );
        assert_eq!(cspace.lookup(reused).unwrap().parent, Some(root.slot));
    }

    #[test]
    fn revoke_copied_typed_cap_removes_copy_descendants() {
        // Goal: typed-copy revoke follows MDB descendants from the selected copy.
        // Scope: revoke_descendants from a copied typed cap.
        // Semantics: descendants below the selected copy are removed while the selected copy stays valid.
        let mut cspace = CapabilitySpace::new();
        let root = cspace
            .insert_initial_capability(endpoint(ENDPOINT_ALLOWED_RIGHTS))
            .unwrap();
        let copy = cspace.copy(root, Rights::READ | Rights::WRITE).unwrap();
        let copy_child = cspace.copy(copy, Rights::READ).unwrap();

        cspace.revoke_descendants(copy).unwrap();

        assert!(cspace.lookup(copy).is_ok());
        assert_eq!(
            cspace.lookup(copy_child),
            Err(CapError::SlotNotFound(copy_child.slot))
        );
    }

    #[test]
    fn untyped_retype_creates_child_object() {
        // Goal: Untyped retype creates a typed child object and capability.
        // Scope: CSpace Untyped allocation into a Frame cap.
        // Semantics: child cap has parent lineage and a distinct object id from the Untyped source.
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
        // Goal: Untyped retype supports kernel object families beyond Frame.
        // Scope: CSpace Untyped allocation into CNode, TCB, and Notification caps.
        // Semantics: each target object kind installs the expected cap shape and policy rights.
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

        assert_eq!(
            cspace.lookup(cnode).unwrap().capability,
            Capability::CNode(CNodeCap::new(4))
        );
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
        // Goal: retype target rights are checked against the target object policy.
        // Scope: Untyped retype precheck before capacity consumption.
        // Semantics: invalid target rights fail without allocating child objects.
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
        // Goal: Untyped sources can derive smaller Untyped children.
        // Scope: Untyped-to-Untyped retype capacity and lineage.
        // Semantics: child size is preserved and child cap records the parent slot.
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
        // Goal: revoking an Untyped child clears that child's allocation state.
        // Scope: revoke_descendants on a derived Untyped cap with an allocated child.
        // Semantics: retyped descendants are deleted and the child Untyped can allocate again.
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
        // Goal: Untyped retype rejects children larger than the source size.
        // Scope: size validation before child object allocation.
        // Semantics: oversized request reports source and requested size without consuming capacity.
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
        // Goal: retype authority is limited to Untyped capabilities.
        // Scope: retype_untyped capability-kind boundary.
        // Semantics: non-Untyped source caps fail with wrong-capability and no allocation.
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
        // Goal: revoking an Untyped source removes all objects allocated from it.
        // Scope: revoke_descendants over multiple retyped child objects.
        // Semantics: source cap remains valid while retyped endpoint/frame children disappear.
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
    fn revoke_descendants_reports_revoked_objects_in_object_id_order() {
        // Goal: revocation reporting remains stable without relying on map or set iteration order.
        // Scope: cap-layer revoke result after MDB traversal has selected descendants.
        // Semantics: finalisation consumers receive each affected object once, sorted by ObjectId.
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(13)).unwrap();
        let first = cspace.retype_untyped(root, RetypeTarget::Endpoint).unwrap();
        let second = cspace
            .retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();
        let first_object = cspace.object_of(first).unwrap();
        let second_object = cspace.object_of(second).unwrap();

        let revocation = cspace.revoke_descendants(root).unwrap();

        assert!(first_object.raw() < second_object.raw());
        assert_eq!(
            revocation.revoked_objects,
            Vec::from([first_object, second_object])
        );
    }

    #[test]
    fn revoke_untyped_descendants_resets_parent_capacity() {
        // Goal: revoking Untyped descendants returns capacity to the parent source.
        // Scope: capacity exhaustion followed by revoke and retype.
        // Semantics: old children are gone or stale, and equivalent allocation can succeed again.
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
        // Goal: revoking a copied Untyped cap does not treat copy lineage as allocation state.
        // Scope: copied Untyped descendant revocation from the root cap.
        // Semantics: copied cap is removed, while parent allocation capacity remains usable.
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
        // Goal: Untyped with live children cannot be copied as a way to reset capacity.
        // Scope: copy precheck over Untyped allocation state.
        // Semantics: copy fails and parent remains capacity-exhausted by its existing child.
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
        // Goal: revoking an Untyped parent removes nested Untyped allocation state.
        // Scope: nested Untyped child with its own retyped object.
        // Semantics: nested child descriptor is invalid and parent capacity can allocate again.
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
        // Goal: Untyped capacity accounting uses model object sizes and alignment.
        // Scope: mixed object allocation from a page-sized Untyped source.
        // Semantics: small object allocation can exhaust aligned capacity for a later Frame.
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
    fn stale_retype_plan_fails_before_untyped_or_slot_mutation() {
        // Goal: retype commit plans are consumed against the exact preflight state.
        // Scope: cap-layer plan/commit boundary.
        // Semantics: if another operation occupies the planned slot first, commit fails without capacity drift.
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(13)).unwrap();
        let endpoint = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        let destination = RetypeDestination {
            start: SlotId(40),
            count: 1,
        };
        let plan = cspace
            .plan_retype_untyped_into(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
                destination,
            )
            .unwrap();
        let planned_object = plan.objects().next().unwrap();

        let occupied = cspace
            .move_capability_into(endpoint, destination.start)
            .unwrap();
        assert_eq!(
            cspace.commit_retype_plan(plan),
            Err(CapError::SlotOccupied(occupied.slot))
        );

        let retry = cspace
            .retype_untyped(
                root,
                RetypeTarget::Frame {
                    rights: Rights::READ,
                },
            )
            .unwrap();
        assert_eq!(
            cspace.lookup(retry).map(|view| view.object),
            Ok(planned_object)
        );
    }

    #[test]
    fn public_resolved_destination_must_match_its_cte_reference() {
        // Goal: public resolved CTE tokens cannot forge a descriptor slot separate from storage.
        // Scope: resolved CTE commit boundary for derived cap insertion.
        // Semantics: mismatch fails before writing either the reported slot or the referenced CTE.
        let mut cspace = CapabilitySpace::new();
        let source = cspace
            .insert_initial_capability(endpoint(Rights::READ | Rights::WRITE))
            .unwrap();
        let destination = ResolvedCte {
            slot: SlotId(30),
            cte: CteRef::root(SlotId(31)),
        };

        assert_eq!(
            cspace.copy_resolved(
                cspace.resolve_descriptor_ref(source).unwrap(),
                destination,
                Rights::READ,
            ),
            Err(CapError::InvalidCteReference { slot: SlotId(30) })
        );
        assert_eq!(
            cspace.lookup(CapabilityDescriptor {
                slot: SlotId(30),
                slot_generation: 1,
            }),
            Err(CapError::SlotNotFound(SlotId(30)))
        );
        assert_eq!(
            cspace.lookup(CapabilityDescriptor {
                slot: SlotId(31),
                slot_generation: 1,
            }),
            Err(CapError::SlotNotFound(SlotId(31)))
        );
    }

    #[test]
    fn public_resolved_source_must_match_its_descriptor_slot() {
        // Goal: public resolved source tokens cannot validate one CTE and mutate lineage under another slot id.
        // Scope: resolved CTE source boundary for move/delete style commits.
        // Semantics: mismatch fails before invalidating the real source cap or touching destination.
        let mut cspace = CapabilitySpace::new();
        let first = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        let second = cspace
            .insert_initial_capability(endpoint(Rights::READ))
            .unwrap();
        let forged_source = ResolvedCapabilitySlot {
            descriptor: CapabilityDescriptor {
                slot: first.slot,
                slot_generation: second.slot_generation,
            },
            cte: CteRef::root(second.slot),
        };
        let destination = ResolvedCte {
            slot: SlotId(40),
            cte: CteRef::root(SlotId(40)),
        };

        assert_eq!(
            cspace.move_resolved(forged_source, destination),
            Err(CapError::InvalidCteReference { slot: first.slot })
        );
        assert!(cspace.lookup(first).is_ok());
        assert!(cspace.lookup(second).is_ok());
        assert_eq!(
            cspace.lookup(CapabilityDescriptor {
                slot: SlotId(40),
                slot_generation: 1,
            }),
            Err(CapError::SlotNotFound(SlotId(40)))
        );
    }

    #[test]
    fn model_sized_kernel_objects_consume_untyped_capacity() {
        // Goal: kernel object model sizes consume Untyped capacity consistently.
        // Scope: TCB and CNode allocation before later Notification allocation.
        // Semantics: larger model-sized objects can exhaust source capacity before smaller requests.
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
        let root = cspace.insert_initial_capability(untyped(7)).unwrap();
        cspace
            .retype_untyped(root, RetypeTarget::CNode { radix: 1 })
            .unwrap();

        assert_eq!(
            cspace.retype_untyped(root, RetypeTarget::Notification),
            Err(CapError::UntypedCapacityExhausted {
                parent: root.slot,
                requested: 5,
                source: 7,
            })
        );
    }

    #[test]
    fn invalid_cnode_radix_fails_before_retype_mutation() {
        // Goal: invalid CNode radix is rejected at the retype boundary.
        // Scope: cap-layer Untyped retype before CNode window planning.
        // Semantics: no object, slot, or Untyped watermark state changes on failure.
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(16)).unwrap();
        let root_object = cspace.object_of(root).unwrap();
        let next_object = cspace.next_object;
        let next_slot = cspace.next_slot;
        let watermark = cspace
            .untyped_allocations
            .get(root_object)
            .unwrap()
            .watermark;

        assert_eq!(
            cspace.retype_untyped(
                root,
                RetypeTarget::CNode {
                    radix: MAX_MODEL_CNODE_RADIX + 1,
                },
            ),
            Err(CapError::InvalidCNodeDepth {
                depth: MAX_MODEL_CNODE_RADIX + 1,
            })
        );

        assert_eq!(cspace.next_object, next_object);
        assert_eq!(cspace.next_slot, next_slot);
        assert_eq!(
            cspace
                .untyped_allocations
                .get(root_object)
                .unwrap()
                .watermark,
            watermark
        );

        let endpoint = cspace.retype_untyped(root, RetypeTarget::Endpoint).unwrap();
        assert_eq!(endpoint.slot, SlotId(next_slot));
        assert_eq!(
            cspace.lookup(endpoint).unwrap().object,
            ObjectId(next_object)
        );
    }

    #[test]
    fn overflowing_retype_destination_window_fails_before_mutation() {
        // Goal: retype destination slot windows cannot wrap around the CSpace slot id range.
        // Scope: cap-layer Untyped retype preflight for caller-provided destination slots.
        // Semantics: overflow fails before object ids, slot cursor, or Untyped watermark move.
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(16)).unwrap();
        let root_object = cspace.object_of(root).unwrap();
        let next_object = cspace.next_object;
        let next_slot = cspace.next_slot;
        let watermark = cspace
            .untyped_allocations
            .get(root_object)
            .unwrap()
            .watermark;

        assert_eq!(
            cspace.retype_untyped_into(
                root,
                RetypeTarget::Endpoint,
                RetypeDestination {
                    start: SlotId(u64::MAX),
                    count: 1,
                },
            ),
            Err(CapError::SlotWindowOverflow {
                start: SlotId(u64::MAX),
                count: 1,
            })
        );
        assert_eq!(cspace.next_object, next_object);
        assert_eq!(cspace.next_slot, next_slot);
        assert_eq!(
            cspace
                .untyped_allocations
                .get(root_object)
                .unwrap()
                .watermark,
            watermark
        );
    }

    #[test]
    fn overflowing_retyped_cnode_reserved_window_fails_before_mutation() {
        // Goal: CNode retype cannot wrap its reserved CTE slot array window.
        // Scope: cap-layer CNode retype planning after destination slot preflight.
        // Semantics: reserved-window overflow fails before object ids, slot cursor, or Untyped watermark move.
        let mut cspace = CapabilitySpace::new();
        let root = cspace.insert_initial_capability(untyped(16)).unwrap();
        let root_object = cspace.object_of(root).unwrap();
        let next_object = cspace.next_object;
        let next_slot = cspace.next_slot;
        let watermark = cspace
            .untyped_allocations
            .get(root_object)
            .unwrap()
            .watermark;

        assert_eq!(
            cspace.retype_untyped_into(
                root,
                RetypeTarget::CNode { radix: 1 },
                RetypeDestination {
                    start: SlotId(u64::MAX - 1),
                    count: 1,
                },
            ),
            Err(CapError::SlotWindowOverflow {
                start: SlotId(u64::MAX),
                count: 2,
            })
        );
        assert_eq!(cspace.next_object, next_object);
        assert_eq!(cspace.next_slot, next_slot);
        assert_eq!(
            cspace
                .untyped_allocations
                .get(root_object)
                .unwrap()
                .watermark,
            watermark
        );
    }

    fn frame_cap(rights: Rights) -> Capability {
        Capability::Frame(FrameCap { rights })
    }

    #[test]
    fn derivation_rejects_rights_outside_object_policy_at_boundary() {
        // Goal: derivation rights checks enforce both parent rights and object policy.
        // Scope: copy boundary for Frame and CNode capabilities.
        // Semantics: requested rights outside parent or object policy fail without child insertion.
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

        let cnode = cspace
            .insert_initial_cnode_capability(CNodeCap::new(4), SlotId::new(0))
            .unwrap();
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
        // Goal: deleting a derived slot does not recursively delete descendants.
        // Scope: delete operation over a typed derivation chain.
        // Semantics: deleted slot is gone, root and grandchild remain valid, and slot is not live.
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
        // Goal: revoking descendants preserves the root of the revocation operation.
        // Scope: revoke_descendants over a typed derivation chain.
        // Semantics: selected descendants disappear while the authority cap remains valid.
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
        // Goal: revocation traverses lineage through deleted intermediate slots.
        // Scope: revoke_descendants after deleting a parent in the derivation path.
        // Semantics: remaining descendants are still found and removed.
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
        // Goal: delete plus revoke does not enqueue the same slot for reuse twice.
        // Scope: slot free-list ownership after leaf deletion and ancestor revocation.
        // Semantics: subsequent allocations receive distinct live slots.
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
        // Goal: object destruction invalidates every cap to that object.
        // Scope: object-generation and slot invalidation across root and derived caps.
        // Semantics: no capability to the destroyed object remains lookup-valid.
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
        // Goal: object generation bumps invalidate existing descriptors.
        // Scope: lookup generation check after object generation change.
        // Semantics: stale descriptor reports expected and actual generation without resolving authority.
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
        // Goal: stale descriptors cannot be used as derivation authority.
        // Scope: derive boundary after object generation bump.
        // Semantics: stale source lookup fails before any child cap is installed.
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
        // Goal: slot generation protects against stale descriptors after slot reuse.
        // Scope: delete, free-list reuse, and lookup generation validation.
        // Semantics: old descriptor is stale while the reused slot's new cap remains valid.
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
        // Goal: deleted slots with live descendants are not reused until revocation clears lineage.
        // Scope: free-list eligibility for deleted intermediate derivation slots.
        // Semantics: allocation avoids the deleted parent while grandchild remains live, then reuses it after revoke.
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
        let reused = cspace
            .insert_initial_capability(frame(Rights::READ))
            .unwrap();
        assert_eq!(reused.slot, child.slot);
    }

    #[test]
    fn notification_capability_derivation_preserves_badge_and_reduces_rights() {
        // Goal: notification derivation preserves notification object semantics while reducing authority.
        // Scope: derive boundary for notification caps.
        // Semantics: child cap remains a notification cap with requested rights only.
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
        // Goal: reply capabilities are single-use authority and cannot derive children.
        // Scope: derive boundary for Reply caps.
        // Semantics: derivation fails with no child cap installed.
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
        // Goal: public initial cap insertion cannot manufacture Reply caps.
        // Scope: root cap creation boundary for Reply authority.
        // Semantics: Reply caps only enter through reply-specific installation paths.
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
    fn public_initial_capability_insertion_rejects_cnode_without_window_metadata() {
        // Goal: bootstrap CNode storage metadata is explicit object state, not a CNode cap default.
        // Scope: root cap creation boundary for initial CNode caps.
        // Semantics: initial CNodes enter through the CNode-specific bootstrap path with CTE window metadata.
        let mut cspace = CapabilitySpace::new();
        let capability = Capability::CNode(CNodeCap::new(4));

        assert_eq!(
            cspace.insert_initial_capability(capability.clone()),
            Err(CapError::InvalidInitialCapability { capability })
        );
    }

    #[test]
    fn initial_cnode_rejects_invalid_radix_before_slot_array_allocation() {
        // Goal: CNode bootstrap does not allocate unbounded or empty CTE slot arrays.
        // Scope: CNode-specific root cap creation boundary.
        // Semantics: invalid radix values fail before any CNode object or slot payload is installed.
        let mut cspace = CapabilitySpace::new();

        assert_eq!(
            cspace.insert_initial_cnode_capability(CNodeCap::new(0), SlotId::new(0)),
            Err(CapError::InvalidCNodeDepth { depth: 0 })
        );
        assert_eq!(cspace.objects.objects.iter().flatten().count(), 0);
        assert_eq!(cspace.slots.root_slots.iter().flatten().count(), 0);
    }

    #[test]
    fn initial_cnode_rejects_overflowing_slot_window_before_mutation() {
        // Goal: CNode bootstrap does not wrap slot ids while expanding CTE slot arrays.
        // Scope: CNode-specific root cap creation boundary.
        // Semantics: overflowing windows fail before any CNode object or slot payload is installed.
        let mut cspace = CapabilitySpace::new();

        assert_eq!(
            cspace.insert_initial_cnode_capability(CNodeCap::new(1), SlotId::new(u64::MAX)),
            Err(CapError::SlotWindowOverflow {
                start: SlotId::new(u64::MAX),
                count: 2,
            })
        );
        assert_eq!(cspace.objects.objects.iter().flatten().count(), 0);
        assert_eq!(cspace.slots.root_slots.iter().flatten().count(), 0);
    }

    #[test]
    fn consuming_reply_cap_invalidates_that_slot() {
        // Goal: consuming a Reply cap removes its slot authority.
        // Scope: consume_reply_cap single-use boundary.
        // Semantics: consumed reply descriptor can no longer be looked up.
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
        // Goal: reply capability installation can reuse an existing Reply runtime object.
        // Scope: insert_reply_capability over a Reply object after seed cap consumption.
        // Semantics: installed cap targets the supplied Reply object and carries new caller metadata.
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
        // Goal: reply cap installation validates target object kind before allocating a slot.
        // Scope: insert_reply_capability failure against a non-Reply object.
        // Semantics: wrong-kind target leaves existing endpoint cap and slot state unchanged.
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
    fn reply_capability_resolved_install_requires_coherent_destination() {
        // Goal: reply cap installation uses the same resolved CTE invariant as other commits.
        // Scope: explicit resolved Reply cap install boundary.
        // Semantics: mismatched destination fails before writing the referenced CTE.
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

        assert_eq!(
            cspace.insert_reply_capability_resolved(
                reply_object,
                ReplyCap {
                    caller: ObjectId::new(101),
                    target: ObjectId::new(201),
                    can_grant: false,
                },
                ResolvedCte {
                    slot: SlotId(70),
                    cte: CteRef::root(SlotId(71)),
                },
            ),
            Err(CapError::InvalidCteReference { slot: SlotId(70) })
        );
        assert_eq!(
            cspace.lookup(CapabilityDescriptor {
                slot: SlotId(71),
                slot_generation: 1,
            }),
            Err(CapError::SlotNotFound(SlotId(71)))
        );
    }

    #[test]
    fn consumed_reply_slot_reuse_rejects_old_descriptor() {
        // Goal: consumed Reply slot reuse makes the old descriptor stale.
        // Scope: consume_reply_cap followed by implicit slot allocation.
        // Semantics: slot generation changes, old reply descriptor is stale, and new cap is valid.
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
        // Goal: consume_reply_cap rejects non-Reply capabilities before deleting slots.
        // Scope: consume_reply_cap kind check.
        // Semantics: wrong-kind cap remains lookup-valid after failure.
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
