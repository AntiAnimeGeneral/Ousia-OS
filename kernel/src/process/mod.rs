use alloc::vec::Vec;

use crate::{
    error::{KernelError, KernelResult},
    handle::{HandleRights, HandleTable, HandleValue},
    memory::frame::{FrameAllocator, FrameOwner},
    object::{
        ChannelMessage, MAX_CHANNEL_MESSAGE_BYTES, ObjectKind, ObjectManager, ObjectPayload,
        ObjectRef,
    },
    vm::{MappingPolicy, MemoryObject},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ProcessId(u64);

impl ProcessId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceBudget {
    remaining_objects: usize,
}

impl ResourceBudget {
    pub const fn new(object_quota: usize) -> Self {
        Self {
            remaining_objects: object_quota,
        }
    }

    pub const fn remaining_objects(self) -> usize {
        self.remaining_objects
    }

    fn reserve_object(&mut self) -> KernelResult<ObjectReservation> {
        if self.remaining_objects == 0 {
            return Err(KernelError::QuotaExceeded);
        }
        self.remaining_objects -= 1;
        Ok(ObjectReservation { committed: false })
    }

    fn release_object(&mut self) {
        self.remaining_objects = self.remaining_objects.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObjectReservation {
    committed: bool,
}

impl ObjectReservation {
    fn commit(&mut self) {
        self.committed = true;
    }
}

pub struct Process {
    pub id: ProcessId,
    pub handles: HandleTable,
    pub budget: ResourceBudget,
    address_space: Option<HandleValue>,
    thread_count: usize,
}

impl Process {
    pub fn new(id: ProcessId, handle_capacity: usize, object_quota: usize) -> KernelResult<Self> {
        Ok(Self {
            id,
            handles: HandleTable::with_capacity(handle_capacity)?,
            budget: ResourceBudget::new(object_quota),
            address_space: None,
            thread_count: 0,
        })
    }

    pub const fn address_space(&self) -> Option<HandleValue> {
        self.address_space
    }

    pub const fn thread_count(&self) -> usize {
        self.thread_count
    }

    pub fn create_object_handle(
        &mut self,
        objects: &mut ObjectManager,
        kind: ObjectKind,
        rights: HandleRights,
    ) -> KernelResult<HandleValue> {
        self.create_preflighted_object_handle(
            objects,
            rights,
            ObjectManager::payload_for_kind(kind)?,
        )
    }

    pub fn create_memory_object_handle(
        &mut self,
        objects: &mut ObjectManager,
        frames: &mut FrameAllocator,
        size_bytes: u64,
        rights: HandleRights,
    ) -> KernelResult<HandleValue> {
        MemoryObject::validate_size(size_bytes)?;
        let mut reservation = self.budget.reserve_object()?;
        let handle_slot = match self.handles.reserve_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };
        let object_entry = match objects.reserve_entry() {
            Ok(entry) => entry,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };
        let owner = FrameOwner::MemoryObject(object_entry.object_id().raw());
        let frame_range = match frames.reserve_contiguous(owner, size_bytes) {
            Ok(range) => range,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };
        let payload = ObjectPayload::MemoryObject(MemoryObject::new(
            size_bytes,
            MappingPolicy::new(HandleRights::READ | HandleRights::WRITE | HandleRights::EXECUTE),
            frame_range,
        )?);

        let object = match objects.commit_reserved(object_entry, payload) {
            Ok(object) => object,
            Err(error) => {
                let _ = frames.free_range(frame_range, owner);
                self.budget.release_object();
                return Err(error);
            }
        };
        let handle = match self
            .handles
            .install_reserved(objects, handle_slot, object, rights)
        {
            Ok(handle) => handle,
            Err(error) => {
                let _ = frames.free_range(frame_range, owner);
                objects
                    .destroy_unpublished_memory_object(object.id, object.generation)
                    .expect(
                        "unpublished MemoryObject entry must be removable after frame rollback",
                    );
                self.budget.release_object();
                return Err(error);
            }
        };
        reservation.commit();
        Ok(handle)
    }

    pub fn create_address_space_handle(
        &mut self,
        objects: &mut ObjectManager,
        rights: HandleRights,
    ) -> KernelResult<HandleValue> {
        self.create_preflighted_object_handle(
            objects,
            rights,
            ObjectManager::payload_for_kind(ObjectKind::AddressSpace)?,
        )
    }

    pub fn map_memory_object(
        &mut self,
        objects: &mut ObjectManager,
        address_space: HandleValue,
        memory: HandleValue,
        base: u64,
        size_bytes: u64,
        memory_offset: u64,
        rights: HandleRights,
    ) -> KernelResult<()> {
        let address_space = self.handles.lookup(
            objects,
            address_space,
            ObjectKind::AddressSpace,
            HandleRights::MANAGE,
        )?;
        let memory = self
            .handles
            .lookup(objects, memory, ObjectKind::MemoryObject, rights)?;
        objects.map_memory_object(
            ObjectRef {
                id: address_space.object.id,
                generation: address_space.object.generation,
            },
            ObjectRef {
                id: memory.object.id,
                generation: memory.object.generation,
            },
            base,
            size_bytes,
            memory_offset,
            rights,
        )
    }

    pub fn unmap_address_range(
        &mut self,
        objects: &mut ObjectManager,
        frames: &mut FrameAllocator,
        address_space: HandleValue,
        base: u64,
        size_bytes: u64,
    ) -> KernelResult<()> {
        let address_space = self.handles.lookup(
            objects,
            address_space,
            ObjectKind::AddressSpace,
            HandleRights::MANAGE,
        )?;
        objects.unmap_address_range(
            frames,
            ObjectRef {
                id: address_space.object.id,
                generation: address_space.object.generation,
            },
            base,
            size_bytes,
        )
    }

    pub fn create_channel_pair_handles(
        &mut self,
        objects: &mut ObjectManager,
        frames: &mut FrameAllocator,
        max_messages: usize,
        rights: HandleRights,
    ) -> KernelResult<(HandleValue, HandleValue)> {
        let mut first_reservation = self.budget.reserve_object()?;
        let mut second_reservation = match self.budget.reserve_object() {
            Ok(reservation) => reservation,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };
        if self.handles.capacity() - self.handles.live_count() < 2 {
            self.budget.release_object();
            self.budget.release_object();
            return Err(KernelError::NoCapacity);
        }

        let (first, second) = match objects.create_channel_pair(max_messages) {
            Ok(pair) => pair,
            Err(error) => {
                self.budget.release_object();
                self.budget.release_object();
                return Err(error);
            }
        };
        let first_handle = match self.handles.install(objects, first, rights) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = objects.destroy(first.id, first.generation);
                let _ = objects.destroy(second.id, second.generation);
                self.budget.release_object();
                self.budget.release_object();
                return Err(error);
            }
        };
        let second_handle = match self.handles.install(objects, second, rights) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = self.handles.close(objects, frames, first_handle);
                let _ = objects.destroy(first.id, first.generation);
                let _ = objects.destroy(second.id, second.generation);
                self.budget.release_object();
                self.budget.release_object();
                return Err(error);
            }
        };
        first_reservation.commit();
        second_reservation.commit();
        Ok((first_handle, second_handle))
    }

    pub fn send_channel_message(
        &mut self,
        objects: &mut ObjectManager,
        channel: HandleValue,
        bytes: &[u8],
        handles: &[HandleValue],
    ) -> KernelResult<()> {
        let channel_view = self.handles.lookup(
            objects,
            channel,
            ObjectKind::ChannelEndpoint,
            HandleRights::WRITE,
        )?;
        let peer = objects.channel_peer(channel_view.object.id, channel_view.object.generation)?;
        objects.ensure_channel_can_enqueue(peer.id, peer.generation)?;

        let mut entries = Vec::new();
        entries
            .try_reserve_exact(handles.len())
            .map_err(|_| KernelError::NoMemory)?;
        for (index, handle) in handles.iter().enumerate() {
            if handles[..index].contains(handle) {
                return Err(KernelError::InvalidArgument);
            }
            let kind = self.handles.peek_kind(*handle)?;
            let entry = self
                .handles
                .lookup(objects, *handle, kind, HandleRights::TRANSFER)?;
            entries.push(entry.entry);
        }
        let message = ChannelMessage::new(bytes, &entries)?;

        let mut moved = Vec::new();
        moved
            .try_reserve_exact(handles.len())
            .map_err(|_| KernelError::NoMemory)?;
        for handle in handles {
            moved.push(self.handles.remove_for_transfer(objects, *handle)?);
        }
        if let Err(error) = objects.enqueue_channel_message(peer.id, peer.generation, message) {
            for entry in moved {
                let _ = self.handles.install_entry(entry);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn recv_channel_message(
        &mut self,
        objects: &mut ObjectManager,
        channel: HandleValue,
    ) -> KernelResult<ReceivedMessage> {
        let channel_view = self.handles.lookup(
            objects,
            channel,
            ObjectKind::ChannelEndpoint,
            HandleRights::READ,
        )?;
        let handle_count = objects.next_channel_message_handle_count(
            channel_view.object.id,
            channel_view.object.generation,
        )?;
        if self.handles.capacity() - self.handles.live_count() < handle_count {
            return Err(KernelError::NoCapacity);
        }
        let message = objects
            .dequeue_channel_message(channel_view.object.id, channel_view.object.generation)?;
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(message.handle_count)
            .map_err(|_| KernelError::NoMemory)?;
        for entry in message.handle_entries() {
            let handle = self
                .handles
                .install_entry(entry)
                .expect("recv preflight reserved enough handle slots for transferred handles");
            handles.push(handle);
        }
        Ok(ReceivedMessage {
            bytes: message.bytes,
            byte_len: message.byte_len,
            handles,
        })
    }

    fn create_preflighted_object_handle(
        &mut self,
        objects: &mut ObjectManager,
        rights: HandleRights,
        payload: ObjectPayload,
    ) -> KernelResult<HandleValue> {
        let mut reservation = self.budget.reserve_object()?;
        let handle_slot = match self.handles.reserve_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };
        let object_entry = match objects.reserve_entry() {
            Ok(entry) => entry,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };

        let object = match objects.commit_reserved(object_entry, payload) {
            Ok(object) => object,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };
        let handle = match self
            .handles
            .install_reserved(objects, handle_slot, object, rights)
        {
            Ok(handle) => handle,
            Err(error) => {
                let _ = objects.destroy(object.id, object.generation);
                self.budget.release_object();
                return Err(error);
            }
        };
        reservation.commit();
        Ok(handle)
    }
}

pub struct ReceivedMessage {
    pub bytes: [u8; MAX_CHANNEL_MESSAGE_BYTES],
    pub byte_len: usize,
    pub handles: Vec<HandleValue>,
}

impl ReceivedMessage {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.byte_len]
    }
}

pub struct ProcessTable {
    processes: Vec<Option<Process>>,
    capacity: usize,
}

impl ProcessTable {
    pub fn with_capacity(capacity: usize) -> KernelResult<Self> {
        let mut processes = Vec::new();
        processes
            .try_reserve_exact(capacity)
            .map_err(|_| KernelError::NoMemory)?;
        processes.resize_with(capacity, || None);
        Ok(Self {
            processes,
            capacity,
        })
    }

    pub fn create_bootstrap(
        &mut self,
        objects: &mut ObjectManager,
        handle_capacity: usize,
        object_quota: usize,
    ) -> KernelResult<ProcessId> {
        let index = self
            .processes
            .iter()
            .position(Option::is_none)
            .ok_or(KernelError::NoCapacity)?;
        let id = ProcessId::new(index as u64);
        let mut process = Process::new(id, handle_capacity, object_quota)?;
        process.create_object_handle(
            objects,
            ObjectKind::Process,
            HandleRights::READ | HandleRights::MANAGE,
        )?;
        self.processes[index] = Some(process);
        Ok(id)
    }

    pub fn get(&self, id: ProcessId) -> KernelResult<&Process> {
        let index = self.index(id)?;
        self.processes[index]
            .as_ref()
            .ok_or(KernelError::InvalidHandle)
    }

    pub fn get_mut(&mut self, id: ProcessId) -> KernelResult<&mut Process> {
        let index = self.index(id)?;
        self.processes[index]
            .as_mut()
            .ok_or(KernelError::InvalidHandle)
    }

    fn index(&self, id: ProcessId) -> KernelResult<usize> {
        let index = id.raw() as usize;
        if index >= self.capacity {
            return Err(KernelError::InvalidHandle);
        }
        Ok(index)
    }
}
