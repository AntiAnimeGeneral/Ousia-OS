use alloc::vec::Vec;

use crate::{
    error::KernelResult,
    handle::{HandleRights, HandleValue, HandleView},
    object::{MAX_CHANNEL_MESSAGE_BYTES, ObjectKind, ObjectManager},
    process::{ProcessId, ProcessTable},
};

pub enum Syscall {
    CreateObject {
        kind: ObjectKind,
        rights: HandleRights,
    },
    CreateMemoryObject {
        size_bytes: u64,
        rights: HandleRights,
    },
    CreateChannelPair {
        max_messages: usize,
        rights: HandleRights,
    },
    ChannelSend {
        channel: HandleValue,
        bytes: Vec<u8>,
        handles: Vec<HandleValue>,
    },
    ChannelRecv {
        channel: HandleValue,
    },
    DuplicateHandle {
        source: HandleValue,
        rights: HandleRights,
    },
    CloseHandle {
        handle: HandleValue,
    },
    RevokeDescendants {
        root: HandleValue,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyscallOutcome {
    Handle {
        handle: HandleValue,
    },
    HandlePair {
        first: HandleValue,
        second: HandleValue,
    },
    Message {
        bytes: [u8; MAX_CHANNEL_MESSAGE_BYTES],
        byte_len: usize,
        handles: Vec<HandleValue>,
    },
    Closed,
    Revoked {
        count: usize,
    },
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
            Syscall::CreateMemoryObject { size_bytes, rights } => {
                let handle =
                    process.create_memory_object_handle(&mut self.objects, size_bytes, rights)?;
                Ok(SyscallOutcome::Handle { handle })
            }
            Syscall::CreateChannelPair {
                max_messages,
                rights,
            } => {
                let (first, second) =
                    process.create_channel_pair_handles(&mut self.objects, max_messages, rights)?;
                Ok(SyscallOutcome::HandlePair { first, second })
            }
            Syscall::ChannelSend {
                channel,
                bytes,
                handles,
            } => {
                process.send_channel_message(&mut self.objects, channel, &bytes, &handles)?;
                Ok(SyscallOutcome::Closed)
            }
            Syscall::ChannelRecv { channel } => {
                let message = process.recv_channel_message(&mut self.objects, channel)?;
                Ok(SyscallOutcome::Message {
                    bytes: message.bytes,
                    byte_len: message.byte_len,
                    handles: message.handles,
                })
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
            Syscall::RevokeDescendants { root } => {
                let count = process
                    .handles
                    .revoke_descendants(&mut self.objects, root)?;
                Ok(SyscallOutcome::Revoked { count })
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
