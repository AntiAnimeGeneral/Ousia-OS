use alloc::vec;

use kernel::{
    error::KernelError,
    handle::{HandleRights, HandleValue},
    object::ObjectKind,
    syscall::{Kernel, Syscall, SyscallContext, SyscallOutcome},
};

extern crate alloc;

fn handle(outcome: SyscallOutcome) -> HandleValue {
    let SyscallOutcome::Handle { handle } = outcome else {
        panic!("expected handle outcome");
    };
    handle
}

fn channel_pair(outcome: SyscallOutcome) -> (HandleValue, HandleValue) {
    let SyscallOutcome::HandlePair { first, second } = outcome else {
        panic!("expected channel pair outcome");
    };
    (first, second)
}

#[test]
fn channel_send_recv_transfers_bytes() {
    // Goal: channel endpoints exchange bounded bytes through the syscall boundary.
    // Scope: host integration through CreateChannelPair, ChannelSend, and ChannelRecv.
    // Semantics: send writes to the peer queue; recv dequeues the same bytes.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 2,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );

    assert_eq!(
        kernel.execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![1, 2, 3],
                handles: vec![],
            },
        ),
        Ok(SyscallOutcome::Closed)
    );

    let SyscallOutcome::Message {
        bytes,
        byte_len,
        handles,
    } = kernel
        .execute(context, Syscall::ChannelRecv { channel: right })
        .unwrap()
    else {
        panic!("expected message outcome");
    };
    assert_eq!(&bytes[..byte_len], &[1, 2, 3]);
    assert!(handles.is_empty());
}

#[test]
fn channel_send_moves_transfer_handles_until_recv_reinstalls_them() {
    // Goal: channel handle transfer moves authority into the queued message.
    // Scope: host integration through ChannelSend and ChannelRecv with one transferred handle.
    // Semantics: sender handle becomes stale; receiver gets a fresh handle to the same object.
    let mut kernel = Kernel::new(8, 1).unwrap();
    let process = kernel.create_bootstrap_process(8, 8).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 2,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let event = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );

    kernel
        .execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![9],
                handles: vec![event],
            },
        )
        .unwrap();
    assert_eq!(
        kernel.lookup_handle(process, event, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::InvalidHandle)
    );

    let SyscallOutcome::Message { handles, .. } = kernel
        .execute(context, Syscall::ChannelRecv { channel: right })
        .unwrap()
    else {
        panic!("expected message outcome");
    };
    let [received] = handles.as_slice() else {
        panic!("expected one received handle");
    };
    assert!(
        kernel
            .lookup_handle(process, *received, ObjectKind::Event, HandleRights::READ)
            .is_ok()
    );
}

#[test]
fn channel_full_send_does_not_move_transfer_handle() {
    // Goal: queue capacity failure happens before transferring handles out of the sender table.
    // Scope: host integration through two sends to a one-slot channel.
    // Semantics: NoCapacity leaves the second transfer handle live and the queued message intact.
    let mut kernel = Kernel::new(10, 1).unwrap();
    let process = kernel.create_bootstrap_process(10, 10).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 1,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let first_event = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let second_event = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );

    kernel
        .execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![1],
                handles: vec![first_event],
            },
        )
        .unwrap();
    assert_eq!(
        kernel.execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![2],
                handles: vec![second_event],
            },
        ),
        Err(KernelError::NoCapacity)
    );

    assert!(
        kernel
            .lookup_handle(process, second_event, ObjectKind::Event, HandleRights::READ)
            .is_ok()
    );
    let SyscallOutcome::Message {
        bytes, byte_len, ..
    } = kernel
        .execute(context, Syscall::ChannelRecv { channel: right })
        .unwrap()
    else {
        panic!("expected message outcome");
    };
    assert_eq!(&bytes[..byte_len], &[1]);
}

#[test]
fn recv_capacity_failure_keeps_message_queued() {
    // Goal: recv preflights destination handle capacity before dequeuing a message.
    // Scope: host integration with a transferred handle and a full handle table at receive time.
    // Semantics: NoCapacity leaves the message queued so a later recv can succeed.
    let mut kernel = Kernel::new(7, 1).unwrap();
    let process = kernel.create_bootstrap_process(5, 7).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 1,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let event = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    kernel
        .execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![7],
                handles: vec![event],
            },
        )
        .unwrap();
    let filler = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );
    let _second_filler = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );

    assert_eq!(
        kernel.execute(context, Syscall::ChannelRecv { channel: right }),
        Err(KernelError::NoCapacity)
    );
    kernel
        .execute(context, Syscall::CloseHandle { handle: filler })
        .unwrap();
    let SyscallOutcome::Message {
        bytes,
        byte_len,
        handles,
    } = kernel
        .execute(context, Syscall::ChannelRecv { channel: right })
        .unwrap()
    else {
        panic!("expected message outcome");
    };
    assert_eq!(&bytes[..byte_len], &[7]);
    assert_eq!(handles.len(), 1);
}

#[test]
fn duplicate_transfer_handle_is_rejected_before_move() {
    // Goal: repeated handle transfer input is rejected before owner mutation.
    // Scope: host integration through ChannelSend with duplicate handle values.
    // Semantics: InvalidArgument leaves the handle live and no message queued.
    let mut kernel = Kernel::new(8, 1).unwrap();
    let process = kernel.create_bootstrap_process(8, 8).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 1,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let event = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );

    assert_eq!(
        kernel.execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![1],
                handles: vec![event, event],
            },
        ),
        Err(KernelError::InvalidArgument)
    );
    assert!(
        kernel
            .lookup_handle(process, event, ObjectKind::Event, HandleRights::READ)
            .is_ok()
    );
    assert_eq!(
        kernel.execute(context, Syscall::ChannelRecv { channel: right }),
        Err(KernelError::WouldBlock)
    );
}

#[test]
fn transferred_derived_handle_remains_revocable_after_recv() {
    // Goal: channel transfer preserves process-local derivation lineage.
    // Scope: duplicate, transfer through channel, receive, then revoke descendants.
    // Semantics: root revoke removes the reinstalled descendant handle.
    let mut kernel = Kernel::new(10, 1).unwrap();
    let process = kernel.create_bootstrap_process(10, 10).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 1,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let root = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::DUPLICATE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let child = handle(
        kernel
            .execute(
                context,
                Syscall::DuplicateHandle {
                    source: root,
                    rights: HandleRights::READ | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );

    kernel
        .execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![],
                handles: vec![child],
            },
        )
        .unwrap();
    let SyscallOutcome::Message { handles, .. } = kernel
        .execute(context, Syscall::ChannelRecv { channel: right })
        .unwrap()
    else {
        panic!("expected message outcome");
    };
    let [received] = handles.as_slice() else {
        panic!("expected one received handle");
    };

    assert_eq!(
        kernel.execute(context, Syscall::RevokeDescendants { root }),
        Ok(SyscallOutcome::Revoked { count: 1 })
    );
    assert_eq!(
        kernel.lookup_handle(process, *received, ObjectKind::Event, HandleRights::READ),
        Err(KernelError::InvalidHandle)
    );
}

#[test]
fn send_after_peer_destroy_reports_peer_closed() {
    // Goal: peer finalization maps channel send failure to PeerClosed.
    // Scope: ObjectManager destroy on peer endpoint followed by ChannelSend.
    // Semantics: send observes peer_closed before validating the dead peer object.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 1,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let right_view = kernel
        .lookup_handle(
            process,
            right,
            ObjectKind::ChannelEndpoint,
            HandleRights::READ,
        )
        .unwrap();
    kernel
        .objects
        .destroy(right_view.object.id, right_view.object.generation)
        .unwrap();

    assert_eq!(
        kernel.execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![1],
                handles: vec![],
            },
        ),
        Err(KernelError::PeerClosed)
    );
}

#[test]
fn destroy_non_empty_channel_is_rejected_without_dropping_message() {
    // Goal: channel finalization cannot leak queued transferred authority.
    // Scope: ObjectManager destroy on endpoint with a queued handle-bearing message.
    // Semantics: WouldBlock leaves the message available to receive.
    let mut kernel = Kernel::new(8, 1).unwrap();
    let process = kernel.create_bootstrap_process(8, 8).unwrap();
    let context = SyscallContext::new(process);
    let (left, right) = channel_pair(
        kernel
            .execute(
                context,
                Syscall::CreateChannelPair {
                    max_messages: 1,
                    rights: HandleRights::READ | HandleRights::WRITE | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    let event = handle(
        kernel
            .execute(
                context,
                Syscall::CreateObject {
                    kind: ObjectKind::Event,
                    rights: HandleRights::READ | HandleRights::TRANSFER,
                },
            )
            .unwrap(),
    );
    kernel
        .execute(
            context,
            Syscall::ChannelSend {
                channel: left,
                bytes: vec![3],
                handles: vec![event],
            },
        )
        .unwrap();
    let right_view = kernel
        .lookup_handle(
            process,
            right,
            ObjectKind::ChannelEndpoint,
            HandleRights::READ,
        )
        .unwrap();

    assert_eq!(
        kernel
            .objects
            .destroy(right_view.object.id, right_view.object.generation),
        Err(KernelError::WouldBlock)
    );
    let SyscallOutcome::Message {
        bytes,
        byte_len,
        handles,
    } = kernel
        .execute(context, Syscall::ChannelRecv { channel: right })
        .unwrap()
    else {
        panic!("expected message outcome");
    };
    assert_eq!(&bytes[..byte_len], &[3]);
    assert_eq!(handles.len(), 1);
}
