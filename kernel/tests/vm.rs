use kernel::{
    error::KernelError,
    handle::{HandleRights, HandleValue},
    object::{ObjectKind, ObjectPayload, ObjectRef},
    syscall::{Kernel, Syscall, SyscallContext, SyscallOutcome},
    vm::{AddressSpaceObject, MappingPolicy, MemoryObject, VmMapDescriptor},
};

fn handle(outcome: SyscallOutcome) -> HandleValue {
    let SyscallOutcome::Handle { handle } = outcome else {
        panic!("expected handle outcome");
    };
    handle
}

fn create_address_space(
    kernel: &mut Kernel,
    context: SyscallContext,
    rights: HandleRights,
) -> HandleValue {
    handle(
        kernel
            .execute(context, Syscall::CreateAddressSpace { rights })
            .unwrap(),
    )
}

fn create_memory(
    kernel: &mut Kernel,
    context: SyscallContext,
    rights: HandleRights,
) -> HandleValue {
    handle(
        kernel
            .execute(
                context,
                Syscall::CreateMemoryObject {
                    size_bytes: 0x4000,
                    rights,
                },
            )
            .unwrap(),
    )
}

#[test]
fn map_memory_object_records_address_space_mapping() {
    // Goal: AddressSpace owns VM mapping metadata for MemoryObject ranges.
    // Scope: host integration through CreateAddressSpace, CreateMemoryObject, and MapMemoryObject.
    // Semantics: a valid map increments mapping_count and records range/object/rights metadata.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = create_memory(
        &mut kernel,
        context,
        HandleRights::READ | HandleRights::WRITE,
    );
    let memory_view = kernel
        .lookup_handle(
            process,
            memory,
            ObjectKind::MemoryObject,
            HandleRights::READ,
        )
        .unwrap();

    assert_eq!(
        kernel.execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0x1000,
                size_bytes: 0x2000,
                memory_offset: 0x1000,
                rights: HandleRights::READ,
            },
        ),
        Ok(SyscallOutcome::Closed)
    );

    let address_space_view = kernel
        .lookup_handle(
            process,
            address_space,
            ObjectKind::AddressSpace,
            HandleRights::MANAGE,
        )
        .unwrap();
    let ObjectPayload::AddressSpace(address_space_payload) = address_space_view.object.payload
    else {
        panic!("expected address space payload");
    };
    let mappings = address_space_payload.mappings().collect::<Vec<_>>();
    assert_eq!(address_space_payload.mapping_count, 1);
    assert_eq!(address_space_payload.pending_tlb_invalidations.count(), 0);
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].base, 0x1000);
    assert_eq!(mappings[0].size_bytes, 0x2000);
    assert_eq!(mappings[0].memory.id, memory_view.object.id);
    assert_eq!(mappings[0].memory.generation, memory_view.object.generation);
    assert_eq!(mappings[0].memory_offset, 0x1000);
    assert_eq!(mappings[0].rights, HandleRights::READ);
}

#[test]
fn memory_object_records_size_and_mapping_policy() {
    // Goal: MemoryObject owns size and mapping policy, not physical frames.
    // Scope: host integration through CreateMemoryObject and handle lookup.
    // Semantics: a memory object records size and maximum mapping rights.
    let mut kernel = Kernel::new(4, 1).unwrap();
    let process = kernel.create_bootstrap_process(4, 4).unwrap();
    let context = SyscallContext::new(process);
    let memory = create_memory(&mut kernel, context, HandleRights::READ);

    let memory_view = kernel
        .lookup_handle(
            process,
            memory,
            ObjectKind::MemoryObject,
            HandleRights::READ,
        )
        .unwrap();
    let ObjectPayload::MemoryObject(memory_payload) = memory_view.object.payload else {
        panic!("expected memory object payload");
    };

    assert_eq!(memory_payload.size_bytes, 0x4000);
    assert!(
        memory_payload
            .mapping_policy
            .max_rights
            .contains(HandleRights::READ)
    );
    assert!(
        memory_payload
            .mapping_policy
            .max_rights
            .contains(HandleRights::WRITE)
    );
    assert!(
        memory_payload
            .mapping_policy
            .max_rights
            .contains(HandleRights::EXECUTE)
    );
}

#[test]
fn vm_prepare_map_does_not_publish_mapping_until_commit() {
    // Goal: VM mapping follows a plan-before-publish transaction boundary.
    // Scope: direct AddressSpaceObject prepare_map and reservation commit calls.
    // Semantics: prepare_map returns an exclusive reservation without publishing mapping metadata.
    let mut address_space = AddressSpaceObject::new();
    let memory = ObjectRef {
        id: kernel::object::ObjectId::new(7),
        generation: kernel::object::ObjectGeneration::INITIAL,
    };
    let memory_object = MemoryObject::new(
        0x4000,
        MappingPolicy::new(HandleRights::READ | HandleRights::WRITE),
    );

    let reservation = address_space
        .prepare_map(
            memory,
            memory_object,
            VmMapDescriptor {
                base: 0x1000,
                size_bytes: 0x1000,
                memory_offset: 0,
                rights: HandleRights::READ,
            },
        )
        .unwrap();

    reservation.commit();
    assert_eq!(address_space.mapping_count, 1);
    assert_eq!(address_space.pending_tlb_invalidations.count(), 0);
    let mapping = address_space.mappings().next().unwrap();
    assert_eq!(mapping.base, 0x1000);
    assert_eq!(mapping.memory, memory);
}

#[test]
fn dropping_vm_map_reservation_leaves_address_space_unchanged() {
    // Goal: abandoning a prepared VM map has no owner-state side effect.
    // Scope: direct AddressSpaceObject prepare_map followed by dropping the reservation.
    // Semantics: an uncommitted reservation does not publish mapping metadata or TLB work.
    let mut address_space = AddressSpaceObject::new();
    let memory = ObjectRef {
        id: kernel::object::ObjectId::new(9),
        generation: kernel::object::ObjectGeneration::INITIAL,
    };
    let memory_object = MemoryObject::new(0x4000, MappingPolicy::new(HandleRights::READ));
    let descriptor = VmMapDescriptor {
        base: 0x1000,
        size_bytes: 0x1000,
        memory_offset: 0,
        rights: HandleRights::READ,
    };

    let reservation = address_space
        .prepare_map(memory, memory_object, descriptor)
        .unwrap();
    drop(reservation);

    assert_eq!(address_space.mapping_count, 0);
    assert_eq!(address_space.pending_tlb_invalidations.count(), 0);
    assert!(address_space.mappings().next().is_none());
}

#[test]
fn dropping_vm_unmap_reservation_leaves_address_space_unchanged() {
    // Goal: abandoning a prepared VM unmap has no owner-state side effect.
    // Scope: direct AddressSpaceObject map commit, prepare_unmap, then drop reservation.
    // Semantics: an uncommitted unmap reservation preserves mapping metadata and TLB work.
    let mut address_space = AddressSpaceObject::new();
    let memory = ObjectRef {
        id: kernel::object::ObjectId::new(10),
        generation: kernel::object::ObjectGeneration::INITIAL,
    };
    let memory_object = MemoryObject::new(0x4000, MappingPolicy::new(HandleRights::READ));
    let descriptor = VmMapDescriptor {
        base: 0x2000,
        size_bytes: 0x1000,
        memory_offset: 0,
        rights: HandleRights::READ,
    };
    address_space
        .prepare_map(memory, memory_object, descriptor)
        .unwrap()
        .commit();

    let reservation = address_space.prepare_unmap(0x2000, 0x1000).unwrap();
    assert_eq!(reservation.page_table().virtual_range().base, 0x2000);
    assert_eq!(reservation.tlb_invalidation().range.base, 0x2000);
    drop(reservation);

    assert_eq!(address_space.mapping_count, 1);
    assert_eq!(address_space.pending_tlb_invalidations.count(), 0);
    let mapping = address_space.mappings().next().unwrap();
    assert_eq!(mapping.base, 0x2000);
}

#[test]
fn vm_prepare_map_failure_leaves_address_space_unchanged() {
    // Goal: VM validation failures happen before AddressSpace owner mutation.
    // Scope: direct prepare_map with a MemoryObject range overflow.
    // Semantics: InvalidArgument leaves the mapping set empty.
    let mut address_space = AddressSpaceObject::new();
    let memory = ObjectRef {
        id: kernel::object::ObjectId::new(8),
        generation: kernel::object::ObjectGeneration::INITIAL,
    };
    let memory_object = MemoryObject::new(0x1000, MappingPolicy::new(HandleRights::READ));

    let result = address_space.prepare_map(
        memory,
        memory_object,
        VmMapDescriptor {
            base: 0,
            size_bytes: 0x2000,
            memory_offset: 0,
            rights: HandleRights::READ,
        },
    );
    assert!(matches!(result, Err(KernelError::InvalidArgument)));

    assert_eq!(address_space.mapping_count, 0);
    assert_eq!(address_space.pending_tlb_invalidations.count(), 0);
    assert!(address_space.mappings().next().is_none());
}

#[test]
fn overlapping_map_is_rejected_without_mutating_address_space() {
    // Goal: VM range overlap is rejected before mapping owner mutation.
    // Scope: two MapMemoryObject calls against the same AddressSpace.
    // Semantics: InvalidArgument leaves the existing mapping set unchanged.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = create_memory(&mut kernel, context, HandleRights::READ);
    kernel
        .execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0x1000,
                size_bytes: 0x1000,
                memory_offset: 0,
                rights: HandleRights::READ,
            },
        )
        .unwrap();

    assert_eq!(
        kernel.execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0x1800,
                size_bytes: 0x1000,
                memory_offset: 0,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::InvalidArgument)
    );
    assert_eq!(mapping_count(&kernel, process, address_space), 1);
}

#[test]
fn map_beyond_memory_size_is_rejected_without_mutation() {
    // Goal: MemoryObject range bounds are checked before AddressSpace mutation.
    // Scope: MapMemoryObject with memory_offset + size beyond MemoryObject size.
    // Semantics: InvalidArgument leaves mapping_count unchanged.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = create_memory(&mut kernel, context, HandleRights::READ);

    assert_eq!(
        kernel.execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0,
                size_bytes: 0x2000,
                memory_offset: 0x3000,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::InvalidArgument)
    );
    assert_eq!(mapping_count(&kernel, process, address_space), 0);
}

#[test]
fn missing_memory_rights_reject_map_without_mutation() {
    // Goal: mapping rights derive from the MemoryObject handle rights.
    // Scope: MapMemoryObject requesting WRITE from a read-only memory handle.
    // Semantics: MissingRights leaves the AddressSpace mapping set unchanged.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = create_memory(&mut kernel, context, HandleRights::READ);

    assert_eq!(
        kernel.execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0,
                size_bytes: 0x1000,
                memory_offset: 0,
                rights: HandleRights::WRITE,
            },
        ),
        Err(KernelError::MissingRights)
    );
    assert_eq!(mapping_count(&kernel, process, address_space), 0);
}

#[test]
fn invalid_mapping_descriptors_are_rejected_before_mutation() {
    // Goal: malformed VM mapping descriptors fail before AddressSpace mutation.
    // Scope: zero-size, overflow, unaligned range/offset, and non-VM rights.
    // Semantics: every invalid descriptor leaves mapping_count unchanged.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = create_memory(
        &mut kernel,
        context,
        HandleRights::READ | HandleRights::TRANSFER,
    );

    for (base, size_bytes, memory_offset, rights) in [
        (0, 0, 0, HandleRights::READ),
        (u64::MAX, 1, 0, HandleRights::READ),
        (0, 1, u64::MAX, HandleRights::READ),
        (1, 0x1000, 0, HandleRights::READ),
        (0, 1, 0, HandleRights::READ),
        (0, 0x1000, 1, HandleRights::READ),
        (0, 0x1000, 0, HandleRights::TRANSFER),
        (0, 0x1000, 0, HandleRights::empty()),
    ] {
        assert_eq!(
            kernel.execute(
                context,
                Syscall::MapMemoryObject {
                    address_space,
                    memory,
                    base,
                    size_bytes,
                    memory_offset,
                    rights,
                },
            ),
            Err(KernelError::InvalidArgument)
        );
        assert_eq!(mapping_count(&kernel, process, address_space), 0);
    }
}

#[test]
fn zero_size_unmap_is_rejected_before_mutation() {
    // Goal: malformed unmap descriptors do not touch AddressSpace metadata.
    // Scope: UnmapAddressRange with size zero after one valid mapping exists.
    // Semantics: InvalidArgument leaves the mapping intact.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = create_memory(&mut kernel, context, HandleRights::READ);
    kernel
        .execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0x1000,
                size_bytes: 0x1000,
                memory_offset: 0,
                rights: HandleRights::READ,
            },
        )
        .unwrap();

    assert_eq!(
        kernel.execute(
            context,
            Syscall::UnmapAddressRange {
                address_space,
                base: 0x1000,
                size_bytes: 0,
            },
        ),
        Err(KernelError::InvalidArgument)
    );
    assert_eq!(mapping_count(&kernel, process, address_space), 1);
}

#[test]
fn unmap_removes_exact_mapping_only() {
    // Goal: unmap commits only an exact mapped range removal.
    // Scope: map one range, reject partial unmap, then unmap the exact range.
    // Semantics: partial unmap leaves mapping_count unchanged; exact unmap removes it.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = create_memory(&mut kernel, context, HandleRights::READ);
    kernel
        .execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0x4000,
                size_bytes: 0x2000,
                memory_offset: 0,
                rights: HandleRights::READ,
            },
        )
        .unwrap();

    assert_eq!(
        kernel.execute(
            context,
            Syscall::UnmapAddressRange {
                address_space,
                base: 0x4000,
                size_bytes: 0x1000,
            },
        ),
        Err(KernelError::InvalidArgument)
    );
    assert_eq!(mapping_count(&kernel, process, address_space), 1);
    assert_eq!(
        kernel.execute(
            context,
            Syscall::UnmapAddressRange {
                address_space,
                base: 0x4000,
                size_bytes: 0x2000,
            },
        ),
        Ok(SyscallOutcome::Closed)
    );
    let address_space_view = kernel
        .lookup_handle(
            process,
            address_space,
            ObjectKind::AddressSpace,
            HandleRights::MANAGE,
        )
        .unwrap();
    let ObjectPayload::AddressSpace(address_space_payload) = address_space_view.object.payload
    else {
        panic!("expected address space payload");
    };
    assert_eq!(address_space_payload.mapping_count, 0);
    assert_eq!(address_space_payload.pending_tlb_invalidations.count(), 1);
}

#[test]
fn mapping_table_capacity_failure_leaves_existing_mappings() {
    // Goal: fixed AddressSpace mapping capacity is checked before mutation.
    // Scope: fill all mapping slots, then attempt one more non-overlapping mapping.
    // Semantics: NoCapacity leaves the existing mapping_count unchanged.
    let mut kernel = Kernel::new(6, 1).unwrap();
    let process = kernel.create_bootstrap_process(6, 6).unwrap();
    let context = SyscallContext::new(process);
    let address_space = create_address_space(&mut kernel, context, HandleRights::MANAGE);
    let memory = handle(
        kernel
            .execute(
                context,
                Syscall::CreateMemoryObject {
                    size_bytes: 0x9000,
                    rights: HandleRights::READ,
                },
            )
            .unwrap(),
    );

    for index in 0..8 {
        kernel
            .execute(
                context,
                Syscall::MapMemoryObject {
                    address_space,
                    memory,
                    base: index * 0x1000,
                    size_bytes: 0x1000,
                    memory_offset: index * 0x1000,
                    rights: HandleRights::READ,
                },
            )
            .unwrap();
    }

    assert_eq!(
        kernel.execute(
            context,
            Syscall::MapMemoryObject {
                address_space,
                memory,
                base: 0x8000,
                size_bytes: 0x1000,
                memory_offset: 0x8000,
                rights: HandleRights::READ,
            },
        ),
        Err(KernelError::NoCapacity)
    );
    assert_eq!(mapping_count(&kernel, process, address_space), 8);
}

#[test]
fn pending_tlb_capacity_failure_leaves_address_space_unchanged() {
    // Goal: TLB pending-work capacity is reserved before unmap metadata changes.
    // Scope: direct AddressSpaceObject map/unmap reservations with fixed pending TLB storage full.
    // Semantics: pending TLB capacity does not block metadata-only map, but NoCapacity blocks unmap.
    let mut address_space = AddressSpaceObject::new();
    let memory = ObjectRef {
        id: kernel::object::ObjectId::new(11),
        generation: kernel::object::ObjectGeneration::INITIAL,
    };
    let memory_object = MemoryObject::new(0x9000, MappingPolicy::new(HandleRights::READ));

    for index in 0..8 {
        address_space
            .prepare_map(
                memory,
                memory_object,
                VmMapDescriptor {
                    base: index * 0x1000,
                    size_bytes: 0x1000,
                    memory_offset: index * 0x1000,
                    rights: HandleRights::READ,
                },
            )
            .unwrap()
            .commit();
        address_space
            .prepare_unmap(index * 0x1000, 0x1000)
            .unwrap()
            .commit();
    }

    address_space
        .prepare_map(
            memory,
            memory_object,
            VmMapDescriptor {
                base: 0,
                size_bytes: 0x1000,
                memory_offset: 0,
                rights: HandleRights::READ,
            },
        )
        .unwrap()
        .commit();
    assert_eq!(address_space.mapping_count, 1);
    assert_eq!(address_space.pending_tlb_invalidations.count(), 8);
    assert_mapping_bases(&address_space, &[0]);

    let unmap_result = address_space.prepare_unmap(0, 0x1000);
    assert!(matches!(unmap_result, Err(KernelError::NoCapacity)));
    assert_eq!(address_space.mapping_count, 1);
    assert_eq!(address_space.pending_tlb_invalidations.count(), 8);
    assert_mapping_bases(&address_space, &[0]);
}

fn assert_mapping_bases(address_space: &AddressSpaceObject, expected: &[u64]) {
    let mappings = address_space.mappings().collect::<Vec<_>>();
    assert_eq!(mappings.len(), expected.len());
    for base in expected {
        assert!(mappings.iter().any(|mapping| mapping.base == *base));
    }
}

fn mapping_count(
    kernel: &Kernel,
    process: kernel::process::ProcessId,
    address_space: HandleValue,
) -> usize {
    let view = kernel
        .lookup_handle(
            process,
            address_space,
            ObjectKind::AddressSpace,
            HandleRights::MANAGE,
        )
        .unwrap();
    let ObjectPayload::AddressSpace(address_space_payload) = view.object.payload else {
        panic!("expected address space payload");
    };
    address_space_payload.mapping_count
}
