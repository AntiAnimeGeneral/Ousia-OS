use alloc::{boxed::Box, vec::Vec};

use crate::{
    cap::{ObjectId, ObjectKind, SlotId},
    ipc::Endpoint,
    notification::Notification,
    reply::Reply,
    thread::tcb::ThreadId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelObjectKind {
    Endpoint,
    Frame,
    CNode,
    Notification,
    Reply,
    Tcb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelObjectRef {
    Endpoint,
    Frame {
        size_bits: u8,
    },
    CNode {
        radix: u8,
        slots: usize,
        window_start: SlotId,
    },
    Notification,
    Reply,
    Tcb {
        thread: Option<ThreadId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectTableError {
    ObjectNotFound {
        object: ObjectId,
    },
    WrongObjectType {
        object: ObjectId,
        expected: KernelObjectKind,
        actual: KernelObjectKind,
    },
    ObjectIdAlreadyBound {
        object: ObjectId,
    },
    ThreadObjectNotFound {
        thread: ThreadId,
    },
    TcbObjectUnbound {
        object: ObjectId,
    },
    ThreadObjectAlreadyBound {
        thread: ThreadId,
    },
    ObjectTableFull {
        capacity: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameObject {
    size_bits: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CNodeObject {
    radix: u8,
    window_start: SlotId,
}

impl FrameObject {
    pub const fn new(size_bits: u8) -> Self {
        Self { size_bits }
    }

    pub const fn size_bits(self) -> u8 {
        self.size_bits
    }
}

impl CNodeObject {
    pub const fn new(radix: u8, window_start: SlotId) -> Self {
        Self {
            radix,
            window_start,
        }
    }

    pub const fn radix(self) -> u8 {
        self.radix
    }

    pub const fn slots(self) -> usize {
        if self.radix >= usize::BITS as u8 {
            return usize::MAX;
        }

        1usize << self.radix
    }

    pub const fn window_start(self) -> SlotId {
        self.window_start
    }
}

#[derive(Debug)]
pub struct ObjectTable {
    objects: Box<[Option<ObjectSlot>]>,
}

#[derive(Debug)]
struct ObjectSlot {
    object: ObjectId,
    value: KernelObject,
}

#[derive(Debug)]
enum KernelObject {
    Endpoint(Endpoint),
    Frame(FrameObject),
    CNode(CNodeObject),
    Notification(Notification),
    Reply(Reply),
    Tcb { thread: Option<ThreadId> },
}

impl KernelObjectKind {
    pub const fn from_cap_object_kind(kind: ObjectKind) -> Option<Self> {
        match kind {
            ObjectKind::Endpoint => Some(Self::Endpoint),
            ObjectKind::Frame => Some(Self::Frame),
            ObjectKind::CNode => Some(Self::CNode),
            ObjectKind::Notification => Some(Self::Notification),
            ObjectKind::Reply => Some(Self::Reply),
            ObjectKind::Tcb => Some(Self::Tcb),
            ObjectKind::Untyped => None,
        }
    }
}

impl KernelObjectRef {
    pub const fn kind(self) -> KernelObjectKind {
        match self {
            Self::Endpoint => KernelObjectKind::Endpoint,
            Self::Frame { .. } => KernelObjectKind::Frame,
            Self::CNode { .. } => KernelObjectKind::CNode,
            Self::Notification => KernelObjectKind::Notification,
            Self::Reply => KernelObjectKind::Reply,
            Self::Tcb { .. } => KernelObjectKind::Tcb,
        }
    }
}

impl KernelObject {
    const fn as_ref(&self) -> KernelObjectRef {
        match self {
            Self::Endpoint(_) => KernelObjectRef::Endpoint,
            Self::Frame(frame) => KernelObjectRef::Frame {
                size_bits: frame.size_bits(),
            },
            Self::CNode(cnode) => KernelObjectRef::CNode {
                radix: cnode.radix(),
                slots: cnode.slots(),
                window_start: cnode.window_start(),
            },
            Self::Notification(_) => KernelObjectRef::Notification,
            Self::Reply(_) => KernelObjectRef::Reply,
            Self::Tcb { thread } => KernelObjectRef::Tcb { thread: *thread },
        }
    }

    const fn kind(&self) -> KernelObjectKind {
        self.as_ref().kind()
    }
}

impl ObjectTable {
    pub const DEFAULT_CAPACITY: usize = 512;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let mut objects = Vec::with_capacity(capacity);
        fill_reserved_object_slots(&mut objects, capacity);
        Self {
            objects: objects.into_boxed_slice(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.objects.len()
    }

    pub fn insert_endpoint(
        &mut self,
        object: ObjectId,
        endpoint: Endpoint,
    ) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.insert_object(ObjectSlot {
            object,
            value: KernelObject::Endpoint(endpoint),
        })
    }

    pub fn validate_unbound(&self, object: ObjectId) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)
    }

    pub fn validate_insert_batch<I>(&self, objects: I) -> Result<(), ObjectTableError>
    where
        I: IntoIterator<Item = ObjectId>,
        I::IntoIter: Clone,
    {
        let objects = objects.into_iter();
        for object in objects.clone() {
            self.ensure_unbound(object)?;
        }

        let mut remaining = objects.clone();
        while let Some(object) = remaining.next() {
            if remaining.clone().any(|other| other == object) {
                return Err(ObjectTableError::ObjectIdAlreadyBound { object });
            }
        }
        if self.free_slot_count() < objects.count() {
            return Err(ObjectTableError::ObjectTableFull {
                capacity: self.capacity(),
            });
        }
        Ok(())
    }

    pub fn insert_frame(
        &mut self,
        object: ObjectId,
        frame: FrameObject,
    ) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.insert_object(ObjectSlot {
            object,
            value: KernelObject::Frame(frame),
        })
    }

    pub fn insert_cnode(
        &mut self,
        object: ObjectId,
        cnode: CNodeObject,
    ) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.insert_object(ObjectSlot {
            object,
            value: KernelObject::CNode(cnode),
        })
    }

    pub fn insert_notification(
        &mut self,
        object: ObjectId,
        notification: Notification,
    ) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.insert_object(ObjectSlot {
            object,
            value: KernelObject::Notification(notification),
        })
    }

    pub fn insert_reply(&mut self, object: ObjectId, reply: Reply) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.insert_object(ObjectSlot {
            object,
            value: KernelObject::Reply(reply),
        })
    }

    pub fn insert_tcb(&mut self, object: ObjectId) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.insert_object(ObjectSlot {
            object,
            value: KernelObject::Tcb { thread: None },
        })
    }

    pub fn bind_tcb(&mut self, object: ObjectId, thread: ThreadId) -> Result<(), ObjectTableError> {
        if self.tcb_object_for_thread(thread).is_ok() {
            return Err(ObjectTableError::ThreadObjectAlreadyBound { thread });
        }
        match self.object_mut(object) {
            Some(KernelObject::Tcb { thread: binding }) => {
                if binding.is_some() {
                    return Err(ObjectTableError::ObjectIdAlreadyBound { object });
                }
                *binding = Some(thread);
                Ok(())
            }
            Some(object_ref) => Err(ObjectTableError::WrongObjectType {
                object,
                expected: KernelObjectKind::Tcb,
                actual: object_ref.kind(),
            }),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn get(&self, object: ObjectId) -> Result<KernelObjectRef, ObjectTableError> {
        self.object(object)
            .map(KernelObject::as_ref)
            .ok_or(ObjectTableError::ObjectNotFound { object })
    }

    pub fn remove_inert(&mut self, object: ObjectId) -> Option<KernelObjectRef> {
        match self.object(object)?.kind() {
            KernelObjectKind::Frame | KernelObjectKind::CNode => self
                .remove_object(object)
                .map(|object_ref| object_ref.as_ref()),
            KernelObjectKind::Endpoint
            | KernelObjectKind::Notification
            | KernelObjectKind::Reply
            | KernelObjectKind::Tcb => None,
        }
    }

    pub fn remove_finalised(&mut self, object: ObjectId) -> Option<KernelObjectRef> {
        self.remove_object(object)
            .map(|object_ref| object_ref.as_ref())
    }

    pub fn expect_kind(
        &self,
        object: ObjectId,
        expected: KernelObjectKind,
    ) -> Result<KernelObjectRef, ObjectTableError> {
        let object_ref = self.get(object)?;
        let actual = object_ref.kind();
        if actual != expected {
            return Err(ObjectTableError::WrongObjectType {
                object,
                expected,
                actual,
            });
        }
        Ok(object_ref)
    }

    pub fn endpoint(&self, object: ObjectId) -> Result<&Endpoint, ObjectTableError> {
        match self.object(object) {
            Some(KernelObject::Endpoint(endpoint)) => Ok(endpoint),
            Some(object_ref) => Err(Self::wrong_type(
                object,
                KernelObjectKind::Endpoint,
                object_ref.kind(),
            )),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn endpoint_mut(&mut self, object: ObjectId) -> Result<&mut Endpoint, ObjectTableError> {
        match self.object_mut(object) {
            Some(KernelObject::Endpoint(endpoint)) => Ok(endpoint),
            Some(object_ref) => Err(Self::wrong_type(
                object,
                KernelObjectKind::Endpoint,
                object_ref.kind(),
            )),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn frame(&self, object: ObjectId) -> Result<FrameObject, ObjectTableError> {
        match self.object(object) {
            Some(KernelObject::Frame(frame)) => Ok(*frame),
            Some(object_ref) => Err(Self::wrong_type(
                object,
                KernelObjectKind::Frame,
                object_ref.kind(),
            )),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn notification(&self, object: ObjectId) -> Result<&Notification, ObjectTableError> {
        match self.object(object) {
            Some(KernelObject::Notification(notification)) => Ok(notification),
            Some(object_ref) => Err(Self::wrong_type(
                object,
                KernelObjectKind::Notification,
                object_ref.kind(),
            )),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn notification_mut(
        &mut self,
        object: ObjectId,
    ) -> Result<&mut Notification, ObjectTableError> {
        match self.object_mut(object) {
            Some(KernelObject::Notification(notification)) => Ok(notification),
            Some(object_ref) => Err(Self::wrong_type(
                object,
                KernelObjectKind::Notification,
                object_ref.kind(),
            )),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn reply(&self, object: ObjectId) -> Result<&Reply, ObjectTableError> {
        match self.object(object) {
            Some(KernelObject::Reply(reply)) => Ok(reply),
            Some(object_ref) => Err(Self::wrong_type(
                object,
                KernelObjectKind::Reply,
                object_ref.kind(),
            )),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn reply_mut(&mut self, object: ObjectId) -> Result<&mut Reply, ObjectTableError> {
        match self.object_mut(object) {
            Some(KernelObject::Reply(reply)) => Ok(reply),
            Some(object_ref) => Err(Self::wrong_type(
                object,
                KernelObjectKind::Reply,
                object_ref.kind(),
            )),
            None => Err(ObjectTableError::ObjectNotFound { object }),
        }
    }

    pub fn with_endpoint_and_reply_mut<T>(
        &mut self,
        endpoint: ObjectId,
        reply: ObjectId,
        f: impl FnOnce(&mut Endpoint, &mut Reply) -> T,
    ) -> Result<T, ObjectTableError> {
        self.expect_kind(endpoint, KernelObjectKind::Endpoint)?;
        self.expect_kind(reply, KernelObjectKind::Reply)?;

        let Some((endpoint_ref, reply_ref)) = self.two_objects_mut(endpoint, reply) else {
            return Err(ObjectTableError::ObjectNotFound { object: endpoint });
        };
        match (endpoint_ref, reply_ref) {
            (Some(KernelObject::Endpoint(endpoint_ref)), Some(KernelObject::Reply(reply_ref))) => {
                Ok(f(endpoint_ref, reply_ref))
            }
            (Some(object_ref), Some(KernelObject::Reply(_))) => Err(Self::wrong_type(
                endpoint,
                KernelObjectKind::Endpoint,
                object_ref.kind(),
            )),
            (Some(KernelObject::Endpoint(_)), Some(object_ref)) => Err(Self::wrong_type(
                reply,
                KernelObjectKind::Reply,
                object_ref.kind(),
            )),
            (Some(object_ref), _) => Err(Self::wrong_type(
                endpoint,
                KernelObjectKind::Endpoint,
                object_ref.kind(),
            )),
            (_, Some(object_ref)) => Err(Self::wrong_type(
                reply,
                KernelObjectKind::Reply,
                object_ref.kind(),
            )),
            (None, _) => Err(ObjectTableError::ObjectNotFound { object: endpoint }),
        }
    }

    pub fn tcb_thread(&self, object: ObjectId) -> Result<ThreadId, ObjectTableError> {
        match self.expect_kind(object, KernelObjectKind::Tcb)? {
            KernelObjectRef::Tcb {
                thread: Some(thread),
            } => Ok(thread),
            KernelObjectRef::Tcb { thread: None } => {
                Err(ObjectTableError::TcbObjectUnbound { object })
            }
            KernelObjectRef::Endpoint
            | KernelObjectRef::Frame { .. }
            | KernelObjectRef::CNode { .. }
            | KernelObjectRef::Notification
            | KernelObjectRef::Reply => {
                unreachable!("expect_kind returned a non-TCB object for TCB expectation")
            }
        }
    }

    pub fn tcb_object_for_thread(&self, thread: ThreadId) -> Result<ObjectId, ObjectTableError> {
        self.objects
            .iter()
            .filter_map(Option::as_ref)
            .find_map(|slot| match slot.value {
                KernelObject::Tcb {
                    thread: Some(bound),
                } if bound == thread => Some(slot.object),
                _ => None,
            })
            .ok_or(ObjectTableError::ThreadObjectNotFound { thread })
    }

    fn ensure_unbound(&self, object: ObjectId) -> Result<(), ObjectTableError> {
        if self.object(object).is_some() {
            return Err(ObjectTableError::ObjectIdAlreadyBound { object });
        }

        Ok(())
    }

    fn object(&self, object: ObjectId) -> Option<&KernelObject> {
        self.objects
            .iter()
            .filter_map(Option::as_ref)
            .find(|slot| slot.object == object)
            .map(|slot| &slot.value)
    }

    fn object_mut(&mut self, object: ObjectId) -> Option<&mut KernelObject> {
        self.objects
            .iter_mut()
            .filter_map(Option::as_mut)
            .find(|slot| slot.object == object)
            .map(|slot| &mut slot.value)
    }

    fn remove_object(&mut self, object: ObjectId) -> Option<KernelObject> {
        let slot = self
            .objects
            .iter_mut()
            .find(|slot| matches!(slot, Some(slot) if slot.object == object))?;
        Some(
            slot.take()
                .expect("matched object slot must be occupied")
                .value,
        )
    }

    fn two_objects_mut(
        &mut self,
        first: ObjectId,
        second: ObjectId,
    ) -> Option<(Option<&mut KernelObject>, Option<&mut KernelObject>)> {
        let first_index = self
            .objects
            .iter()
            .position(|slot| matches!(slot, Some(slot) if slot.object == first))?;
        let second_index = self
            .objects
            .iter()
            .position(|slot| matches!(slot, Some(slot) if slot.object == second))?;
        if first_index == second_index {
            return None;
        }
        if first_index < second_index {
            let (left, right) = self.objects.split_at_mut(second_index);
            return Some((
                Some(
                    &mut left[first_index]
                        .as_mut()
                        .expect("first object index must remain occupied")
                        .value,
                ),
                Some(
                    &mut right[0]
                        .as_mut()
                        .expect("second object index must remain occupied")
                        .value,
                ),
            ));
        }
        let (left, right) = self.objects.split_at_mut(first_index);
        Some((
            Some(
                &mut right[0]
                    .as_mut()
                    .expect("first object index must remain occupied")
                    .value,
            ),
            Some(
                &mut left[second_index]
                    .as_mut()
                    .expect("second object index must remain occupied")
                    .value,
            ),
        ))
    }

    fn insert_object(&mut self, object: ObjectSlot) -> Result<(), ObjectTableError> {
        let capacity = self.capacity();
        let slot = self
            .objects
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(ObjectTableError::ObjectTableFull { capacity })?;
        *slot = Some(object);
        Ok(())
    }

    fn free_slot_count(&self) -> usize {
        self.objects.iter().filter(|slot| slot.is_none()).count()
    }

    const fn wrong_type(
        object: ObjectId,
        expected: KernelObjectKind,
        actual: KernelObjectKind,
    ) -> ObjectTableError {
        ObjectTableError::WrongObjectType {
            object,
            expected,
            actual,
        }
    }
}

fn fill_reserved_object_slots(objects: &mut Vec<Option<ObjectSlot>>, capacity: usize) {
    assert!(
        objects.capacity() >= capacity,
        "ObjectTable::with_capacity allocates fixed backing before filling object slots"
    );
    objects.resize_with(capacity, || None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(raw: u64) -> ObjectId {
        ObjectId::new(raw)
    }

    fn thread(raw: u64) -> ThreadId {
        ThreadId::new(raw)
    }

    #[test]
    fn table_rejects_duplicate_object_ids() {
        // Goal: ObjectTable owns object-id uniqueness across all object kinds.
        // Scope: insert boundary before replacing or aliasing an existing object.
        // Semantics: duplicate id insertion fails and preserves the original object owner.
        let mut table = ObjectTable::new();
        table.insert_endpoint(object(1), Endpoint::new()).unwrap();

        assert_eq!(
            table.insert_notification(object(1), Notification::new()),
            Err(ObjectTableError::ObjectIdAlreadyBound { object: object(1) })
        );
    }

    #[test]
    fn bounded_table_rejects_capacity_overflow_without_leaking_slots() {
        // Goal: ObjectTable insertion has an explicit fixed-capacity failure boundary.
        // Scope: bounded runtime object storage insertion and removal.
        // Semantics: a full table rejects new objects, and removing an object frees its slot for reuse.
        let mut table = ObjectTable::with_capacity(1);

        table.insert_endpoint(object(1), Endpoint::new()).unwrap();
        assert_eq!(
            table.insert_notification(object(2), Notification::new()),
            Err(ObjectTableError::ObjectTableFull { capacity: 1 })
        );

        assert_eq!(
            table.remove_finalised(object(1)),
            Some(KernelObjectRef::Endpoint)
        );
        table
            .insert_notification(object(2), Notification::new())
            .unwrap();
        assert_eq!(table.get(object(2)), Ok(KernelObjectRef::Notification));
    }

    #[test]
    fn batch_validation_rejects_duplicate_planned_objects() {
        // Goal: batch runtime-object preflight rejects duplicate object ids before commit.
        // Scope: ObjectTable preflight used by Untyped retype executor paths.
        // Semantics: a duplicate planned object id is rejected even when capacity is available.
        let table = ObjectTable::with_capacity(2);

        assert_eq!(
            table.validate_insert_batch([object(1), object(1)]),
            Err(ObjectTableError::ObjectIdAlreadyBound { object: object(1) })
        );
    }

    #[test]
    fn cnode_object_is_tracked_as_kernel_object() {
        // Goal: ObjectTable records CNode metadata in the generic object view.
        // Scope: CNode insertion and typed accessor mismatch reporting.
        // Semantics: CNode radix/slot metadata is visible, while Endpoint access reports wrong type.
        let mut table = ObjectTable::new();
        table
            .insert_cnode(object(2), CNodeObject::new(4, SlotId::new(32)))
            .unwrap();

        assert_eq!(
            table.get(object(2)),
            Ok(KernelObjectRef::CNode {
                radix: 4,
                slots: 16,
                window_start: SlotId::new(32),
            })
        );
        assert_eq!(
            table.endpoint(object(2)).map(|_| ()),
            Err(ObjectTableError::WrongObjectType {
                object: object(2),
                expected: KernelObjectKind::Endpoint,
                actual: KernelObjectKind::CNode,
            })
        );
    }

    #[test]
    fn frame_object_is_tracked_as_kernel_object() {
        // Goal: ObjectTable records Frame metadata in both typed and generic views.
        // Scope: Frame insertion and typed accessor mismatch reporting.
        // Semantics: Frame size is preserved, while Endpoint access reports wrong type.
        let mut table = ObjectTable::new();
        table.insert_frame(object(3), FrameObject::new(12)).unwrap();

        assert_eq!(table.frame(object(3)), Ok(FrameObject::new(12)));
        assert_eq!(
            table.get(object(3)),
            Ok(KernelObjectRef::Frame { size_bits: 12 })
        );
        assert_eq!(
            table.endpoint(object(3)).map(|_| ()),
            Err(ObjectTableError::WrongObjectType {
                object: object(3),
                expected: KernelObjectKind::Endpoint,
                actual: KernelObjectKind::Frame,
            })
        );
    }

    #[test]
    fn tcb_binding_keeps_thread_state_outside_object_table() {
        // Goal: ObjectTable binds TCB object identity without owning TCB runtime state.
        // Scope: TCB object binding and generic object view.
        // Semantics: object lookup exposes the thread binding, while thread state remains ThreadTable-owned.
        let mut table = ObjectTable::new();
        table.insert_tcb(object(10)).unwrap();
        table.bind_tcb(object(10), thread(1)).unwrap();

        assert_eq!(table.tcb_thread(object(10)), Ok(thread(1)));
        assert_eq!(
            table.get(object(10)),
            Ok(KernelObjectRef::Tcb {
                thread: Some(thread(1))
            })
        );
    }

    #[test]
    fn unbound_tcb_object_has_no_thread_binding() {
        // Goal: unbound TCB objects remain valid objects but cannot resolve to a thread.
        // Scope: TCB object insertion before KernelState binds runtime thread state.
        // Semantics: generic object view succeeds, while tcb_thread reports the missing binding.
        let mut table = ObjectTable::new();
        table.insert_tcb(object(10)).unwrap();

        assert_eq!(
            table.get(object(10)),
            Ok(KernelObjectRef::Tcb { thread: None })
        );
        assert_eq!(
            table.tcb_thread(object(10)),
            Err(ObjectTableError::TcbObjectUnbound { object: object(10) })
        );
    }

    #[test]
    fn tcb_thread_binding_is_unique_and_lookupable_by_thread() {
        // Goal: preserve the one-to-one TCB object/thread binding contract.
        // Scope: ObjectTable binding boundary and reverse lookup API.
        // Semantics: a bound thread resolves to its TCB object; duplicate thread
        // binding and unknown thread lookup fail without rebinding another TCB.
        let mut table = ObjectTable::new();
        table.insert_tcb(object(10)).unwrap();
        table.insert_tcb(object(11)).unwrap();

        table.bind_tcb(object(10), thread(1)).unwrap();

        assert_eq!(table.tcb_object_for_thread(thread(1)), Ok(object(10)));
        assert_eq!(
            table.bind_tcb(object(11), thread(1)),
            Err(ObjectTableError::ThreadObjectAlreadyBound { thread: thread(1) })
        );
        assert_eq!(
            table.tcb_object_for_thread(thread(2)),
            Err(ObjectTableError::ThreadObjectNotFound { thread: thread(2) })
        );
        assert_eq!(
            table.tcb_thread(object(11)),
            Err(ObjectTableError::TcbObjectUnbound { object: object(11) })
        );
    }

    #[test]
    fn endpoint_reply_mutation_keeps_reply_on_closure_error() {
        // Goal: keep ObjectTable ownership stable when a caller reports an error
        // after receiving mutable endpoint and reply references.
        // Scope: dual-object mutable access API used by endpoint IPC paths.
        // Semantics: closure failure is not an ObjectTable failure and must not
        // remove either object from the table.
        let mut table = ObjectTable::new();
        table.insert_endpoint(object(1), Endpoint::new()).unwrap();
        table.insert_reply(object(2), Reply::new()).unwrap();

        let result = table.with_endpoint_and_reply_mut(object(1), object(2), |_, _| {
            Err::<(), &'static str>("caller failed")
        });

        assert_eq!(result, Ok(Err("caller failed")));
        assert!(table.endpoint(object(1)).is_ok());
        assert!(table.reply(object(2)).is_ok());
    }

    #[test]
    fn endpoint_reply_mutation_rejects_same_object_before_dual_lookup() {
        // Goal: prevent duplicate-key dual lookup from becoming a panic path.
        // Scope: ObjectTable dual-object mutable access boundary.
        // Semantics: one object cannot satisfy both endpoint and reply roles, so
        // same-object input fails as wrong type before requesting two references.
        let mut table = ObjectTable::new();
        table.insert_endpoint(object(1), Endpoint::new()).unwrap();

        assert_eq!(
            table.with_endpoint_and_reply_mut(object(1), object(1), |_, _| ()),
            Err(ObjectTableError::WrongObjectType {
                object: object(1),
                expected: KernelObjectKind::Reply,
                actual: KernelObjectKind::Endpoint,
            })
        );
    }

    #[test]
    fn wrong_type_reports_expected_and_actual_kind() {
        // Goal: typed ObjectTable access reports both requested and stored object kinds.
        // Scope: object type discrimination at typed accessor boundary.
        // Semantics: wrong-type lookup does not hide the actual runtime object kind.
        let mut table = ObjectTable::new();
        table.insert_reply(object(3), Reply::new()).unwrap();

        assert_eq!(
            table.endpoint(object(3)).map(|_| ()),
            Err(ObjectTableError::WrongObjectType {
                object: object(3),
                expected: KernelObjectKind::Endpoint,
                actual: KernelObjectKind::Reply,
            })
        );
    }
}
