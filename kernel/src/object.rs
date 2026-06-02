use alloc::collections::BTreeMap;

use crate::{
    cap::{ObjectId, ObjectKind},
    ipc::Endpoint,
    notification::Notification,
    reply::Reply,
    tcb::ThreadId,
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
    Frame { size_bits: u8 },
    CNode,
    Notification,
    Reply,
    Tcb { thread: Option<ThreadId> },
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameObject {
    size_bits: u8,
}

impl FrameObject {
    pub const fn new(size_bits: u8) -> Self {
        Self { size_bits }
    }

    pub const fn size_bits(self) -> u8 {
        self.size_bits
    }
}

#[derive(Debug, Default)]
pub struct ObjectTable {
    endpoints: BTreeMap<ObjectId, Endpoint>,
    frames: BTreeMap<ObjectId, FrameObject>,
    cnodes: BTreeMap<ObjectId, ()>,
    notifications: BTreeMap<ObjectId, Notification>,
    replies: BTreeMap<ObjectId, Reply>,
    tcbs: BTreeMap<ObjectId, Option<ThreadId>>,
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
            Self::CNode => KernelObjectKind::CNode,
            Self::Notification => KernelObjectKind::Notification,
            Self::Reply => KernelObjectKind::Reply,
            Self::Tcb { .. } => KernelObjectKind::Tcb,
        }
    }
}

impl ObjectTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_endpoint(
        &mut self,
        object: ObjectId,
        endpoint: Endpoint,
    ) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.endpoints.insert(object, endpoint);
        Ok(())
    }

    pub fn validate_unbound(&self, object: ObjectId) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)
    }

    pub fn insert_frame(
        &mut self,
        object: ObjectId,
        frame: FrameObject,
    ) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.frames.insert(object, frame);
        Ok(())
    }

    pub fn insert_cnode(&mut self, object: ObjectId) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.cnodes.insert(object, ());
        Ok(())
    }

    pub fn insert_notification(
        &mut self,
        object: ObjectId,
        notification: Notification,
    ) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.notifications.insert(object, notification);
        Ok(())
    }

    pub fn insert_reply(&mut self, object: ObjectId, reply: Reply) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.replies.insert(object, reply);
        Ok(())
    }

    pub fn insert_tcb(&mut self, object: ObjectId) -> Result<(), ObjectTableError> {
        self.ensure_unbound(object)?;
        self.tcbs.insert(object, None);
        Ok(())
    }

    pub fn bind_tcb(&mut self, object: ObjectId, thread: ThreadId) -> Result<(), ObjectTableError> {
        if self.tcbs.values().any(|bound| *bound == Some(thread)) {
            return Err(ObjectTableError::ThreadObjectAlreadyBound { thread });
        }
        let binding = self
            .tcbs
            .get_mut(&object)
            .ok_or(ObjectTableError::ObjectNotFound { object })?;
        if binding.is_some() {
            return Err(ObjectTableError::ObjectIdAlreadyBound { object });
        }
        *binding = Some(thread);
        Ok(())
    }

    pub fn get(&self, object: ObjectId) -> Result<KernelObjectRef, ObjectTableError> {
        if self.endpoints.contains_key(&object) {
            return Ok(KernelObjectRef::Endpoint);
        }
        if let Some(frame) = self.frames.get(&object) {
            return Ok(KernelObjectRef::Frame {
                size_bits: frame.size_bits(),
            });
        }
        if self.cnodes.contains_key(&object) {
            return Ok(KernelObjectRef::CNode);
        }
        if self.notifications.contains_key(&object) {
            return Ok(KernelObjectRef::Notification);
        }
        if self.replies.contains_key(&object) {
            return Ok(KernelObjectRef::Reply);
        }
        if let Some(thread) = self.tcbs.get(&object) {
            return Ok(KernelObjectRef::Tcb { thread: *thread });
        }

        Err(ObjectTableError::ObjectNotFound { object })
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
        self.endpoints
            .get(&object)
            .ok_or_else(|| self.missing_or_wrong(object, KernelObjectKind::Endpoint))
    }

    pub fn endpoint_mut(&mut self, object: ObjectId) -> Result<&mut Endpoint, ObjectTableError> {
        if !self.endpoints.contains_key(&object) {
            return Err(self.missing_or_wrong(object, KernelObjectKind::Endpoint));
        }
        Ok(self
            .endpoints
            .get_mut(&object)
            .expect("checked endpoint object must exist"))
    }

    pub fn frame(&self, object: ObjectId) -> Result<FrameObject, ObjectTableError> {
        self.frames
            .get(&object)
            .copied()
            .ok_or_else(|| self.missing_or_wrong(object, KernelObjectKind::Frame))
    }

    pub fn notification(&self, object: ObjectId) -> Result<&Notification, ObjectTableError> {
        self.notifications
            .get(&object)
            .ok_or_else(|| self.missing_or_wrong(object, KernelObjectKind::Notification))
    }

    pub fn notification_mut(
        &mut self,
        object: ObjectId,
    ) -> Result<&mut Notification, ObjectTableError> {
        if !self.notifications.contains_key(&object) {
            return Err(self.missing_or_wrong(object, KernelObjectKind::Notification));
        }
        Ok(self
            .notifications
            .get_mut(&object)
            .expect("checked notification object must exist"))
    }

    pub fn reply(&self, object: ObjectId) -> Result<&Reply, ObjectTableError> {
        self.replies
            .get(&object)
            .ok_or_else(|| self.missing_or_wrong(object, KernelObjectKind::Reply))
    }

    pub fn reply_mut(&mut self, object: ObjectId) -> Result<&mut Reply, ObjectTableError> {
        if !self.replies.contains_key(&object) {
            return Err(self.missing_or_wrong(object, KernelObjectKind::Reply));
        }
        Ok(self
            .replies
            .get_mut(&object)
            .expect("checked reply object must exist"))
    }

    pub fn endpoint_and_reply_mut(
        &mut self,
        endpoint: ObjectId,
        reply: ObjectId,
    ) -> Result<(&mut Endpoint, &mut Reply), ObjectTableError> {
        if !self.endpoints.contains_key(&endpoint) {
            return Err(self.missing_or_wrong(endpoint, KernelObjectKind::Endpoint));
        }
        if !self.replies.contains_key(&reply) {
            return Err(self.missing_or_wrong(reply, KernelObjectKind::Reply));
        }

        let endpoint = self
            .endpoints
            .get_mut(&endpoint)
            .expect("checked endpoint object must exist");
        let reply = self
            .replies
            .get_mut(&reply)
            .expect("checked reply object must exist");
        Ok((endpoint, reply))
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
            | KernelObjectRef::CNode
            | KernelObjectRef::Notification
            | KernelObjectRef::Reply => {
                unreachable!("expect_kind returned a non-TCB object for TCB expectation")
            }
        }
    }

    pub fn tcb_object_for_thread(&self, thread: ThreadId) -> Result<ObjectId, ObjectTableError> {
        self.tcbs
            .iter()
            .find_map(|(object, bound)| (*bound == Some(thread)).then_some(*object))
            .ok_or(ObjectTableError::ThreadObjectNotFound { thread })
    }

    fn ensure_unbound(&self, object: ObjectId) -> Result<(), ObjectTableError> {
        if self.endpoints.contains_key(&object)
            || self.frames.contains_key(&object)
            || self.cnodes.contains_key(&object)
            || self.notifications.contains_key(&object)
            || self.replies.contains_key(&object)
            || self.tcbs.contains_key(&object)
        {
            return Err(ObjectTableError::ObjectIdAlreadyBound { object });
        }

        Ok(())
    }

    fn missing_or_wrong(&self, object: ObjectId, expected: KernelObjectKind) -> ObjectTableError {
        match self.get(object) {
            Ok(object_ref) => ObjectTableError::WrongObjectType {
                object,
                expected,
                actual: object_ref.kind(),
            },
            Err(error) => error,
        }
    }
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
        let mut table = ObjectTable::new();
        table.insert_endpoint(object(1), Endpoint::new()).unwrap();

        assert_eq!(
            table.insert_notification(object(1), Notification::new()),
            Err(ObjectTableError::ObjectIdAlreadyBound { object: object(1) })
        );
    }

    #[test]
    fn cnode_object_is_tracked_as_kernel_object() {
        let mut table = ObjectTable::new();
        table.insert_cnode(object(2)).unwrap();

        assert_eq!(table.get(object(2)), Ok(KernelObjectRef::CNode));
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
    fn wrong_type_reports_expected_and_actual_kind() {
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
