use crate::{
    error::KernelResult,
    handle::{HandleRights, HandleValue, HandleView},
    object::{ObjectKind, ObjectManager},
    process::{ProcessId, ProcessTable},
};

pub enum Syscall {
    CreateObject {
        kind: ObjectKind,
        rights: HandleRights,
    },
    DuplicateHandle {
        source: HandleValue,
        rights: HandleRights,
    },
    CloseHandle {
        handle: HandleValue,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyscallOutcome {
    Handle { handle: HandleValue },
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyscallContext {
    pub process: ProcessId,
}

impl SyscallContext {
    pub const fn new(process: ProcessId) -> Self {
        Self { process }
    }
}

pub struct Kernel {
    pub objects: ObjectManager,
    pub processes: ProcessTable,
}

impl Kernel {
    pub fn new(object_capacity: usize, process_capacity: usize) -> KernelResult<Self> {
        Ok(Self {
            objects: ObjectManager::with_capacity(object_capacity)?,
            processes: ProcessTable::with_capacity(process_capacity)?,
        })
    }

    pub fn create_bootstrap_process(
        &mut self,
        handle_capacity: usize,
        object_quota: usize,
    ) -> KernelResult<ProcessId> {
        self.processes
            .create_bootstrap(&mut self.objects, handle_capacity, object_quota)
    }

    pub fn execute(
        &mut self,
        context: SyscallContext,
        syscall: Syscall,
    ) -> KernelResult<SyscallOutcome> {
        let process = self.processes.get_mut(context.process)?;
        match syscall {
            Syscall::CreateObject { kind, rights } => {
                let handle = process.create_object_handle(&mut self.objects, kind, rights)?;
                Ok(SyscallOutcome::Handle { handle })
            }
            Syscall::DuplicateHandle { source, rights } => {
                let handle = process
                    .handles
                    .duplicate(&mut self.objects, source, rights)?;
                Ok(SyscallOutcome::Handle { handle })
            }
            Syscall::CloseHandle { handle } => {
                process.handles.close(&mut self.objects, handle)?;
                Ok(SyscallOutcome::Closed)
            }
        }
    }

    pub fn lookup_handle(
        &self,
        process: ProcessId,
        handle: HandleValue,
        expected_kind: ObjectKind,
        required_rights: HandleRights,
    ) -> KernelResult<HandleView> {
        self.processes.get(process)?.handles.lookup(
            &self.objects,
            handle,
            expected_kind,
            required_rights,
        )
    }
}
