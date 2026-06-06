use alloc::vec::Vec;

use crate::{
    error::{KernelError, KernelResult},
    handle::{HandleRights, HandleTable, HandleValue},
    object::{ObjectKind, ObjectManager},
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
        let mut reservation = self.budget.reserve_object()?;
        if self.handles.live_count() == self.handles.capacity() {
            self.budget.release_object();
            return Err(KernelError::NoCapacity);
        }

        let object = match objects.create(kind) {
            Ok(object) => object,
            Err(error) => {
                self.budget.release_object();
                return Err(error);
            }
        };
        let handle = match self.handles.install(objects, object, rights) {
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
