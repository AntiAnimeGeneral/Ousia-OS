use kernel::{
    error::KernelError,
    memory::frame::{FrameAllocator, FrameOwner, FrameRange, FrameState, PAGE_SIZE},
};

fn allocator() -> FrameAllocator {
    FrameAllocator::from_available_ranges(&[
        FrameRange::new(0x1000, 0x3000).unwrap(),
        FrameRange::new(0x8000, 0x9000).unwrap(),
    ])
    .unwrap()
}

#[test]
fn runtime_frame_allocator_imports_available_ranges_as_free_metadata() {
    // Goal: runtime frame metadata owns normalized frame availability after OSTD boot filtering.
    // Scope: FrameAllocator construction from page-aligned available ranges.
    // Semantics: every imported frame starts free and no allocation happens during import.
    let allocator = allocator();

    assert_eq!(allocator.capacity(), 3);
    assert_eq!(allocator.free_count(), 3);
}

#[test]
fn reserving_frame_publishes_owner_and_physical_address() {
    // Goal: frame reservation publishes a single owner for the allocated physical frame.
    // Scope: one successful runtime frame reservation.
    // Semantics: free count drops by one and the selected frame records its owner.
    let mut allocator = allocator();

    let frame = allocator.reserve_one(FrameOwner::Kernel).unwrap();

    assert_eq!(frame.paddr(), 0x1000);
    assert_eq!(allocator.free_count(), 2);
    assert_eq!(
        allocator.state(frame.id()),
        Ok(FrameState::Allocated {
            owner: FrameOwner::Kernel,
        })
    );
}

#[test]
fn exhaustion_does_not_mutate_allocator_state() {
    // Goal: frame exhaustion is a recoverable memory error with no hidden state mutation.
    // Scope: reserve every frame, then attempt one extra reservation.
    // Semantics: NoMemory leaves the allocator free count and existing allocations unchanged.
    let mut allocator = allocator();
    let first = allocator.reserve_one(FrameOwner::Kernel).unwrap();
    let second = allocator.reserve_one(FrameOwner::Kernel).unwrap();
    let third = allocator.reserve_one(FrameOwner::Kernel).unwrap();

    assert_eq!(
        allocator.reserve_one(FrameOwner::Kernel),
        Err(KernelError::NoMemory)
    );

    assert_eq!(allocator.free_count(), 0);
    assert_eq!(
        allocator.state(first.id()),
        Ok(FrameState::Allocated {
            owner: first.owner(),
        })
    );
    assert_eq!(
        allocator.state(second.id()),
        Ok(FrameState::Allocated {
            owner: second.owner()
        })
    );
    assert_eq!(
        allocator.state(third.id()),
        Ok(FrameState::Allocated {
            owner: third.owner(),
        })
    );
}

#[test]
fn double_free_preserves_free_state_after_generation_advance() {
    // Goal: frame release tokens cannot be replayed after a successful free.
    // Scope: free the same allocator-issued frame token twice.
    // Semantics: the replay fails as stale and leaves the frame free.
    let mut allocator = allocator();
    let frame = allocator.reserve_one(FrameOwner::Process(7)).unwrap();

    assert_eq!(allocator.free(frame), Ok(()));
    assert_eq!(allocator.free(frame), Err(KernelError::StaleHandle));
    assert_eq!(allocator.state(frame.id()), Ok(FrameState::Free));
}

#[test]
fn freed_frame_rejects_stale_reference_after_generation_advance() {
    // Goal: frame generation rejects stale frame references after reuse eligibility changes.
    // Scope: free a frame, then attempt to free the same reference again.
    // Semantics: the second free fails as stale and leaves the frame free.
    let mut allocator = allocator();
    let frame = allocator.reserve_one(FrameOwner::MemoryObject(9)).unwrap();

    assert_eq!(allocator.free(frame), Ok(()));
    assert_eq!(allocator.free(frame), Err(KernelError::StaleHandle));
    assert_eq!(allocator.state(frame.id()), Ok(FrameState::Free));
    assert_eq!(allocator.free_count(), 3);
}

#[test]
fn invalid_frame_ranges_are_rejected_before_metadata_publication() {
    // Goal: malformed runtime frame ranges fail before allocator metadata exists.
    // Scope: FrameRange construction and empty allocator import.
    // Semantics: invalid descriptors return stable errors instead of creating partial metadata.
    assert_eq!(
        FrameRange::new(0x1001, 0x2000),
        Err(KernelError::InvalidArgument)
    );
    assert_eq!(
        FrameRange::new(0x2000, 0x2000),
        Err(KernelError::InvalidArgument)
    );
    assert_eq!(
        FrameAllocator::from_available_ranges(&[]).map(|allocator| allocator.capacity()),
        Err(KernelError::NoMemory)
    );
    assert_eq!(PAGE_SIZE, 4096);
}

#[test]
fn overlapping_available_ranges_are_rejected_before_metadata_publication() {
    // Goal: runtime frame metadata never publishes the same physical frame twice.
    // Scope: allocator import with overlapping normalized-range candidates.
    // Semantics: InvalidArgument rejects the import before any FrameAllocator is created.
    assert_eq!(
        FrameAllocator::from_available_ranges(&[
            FrameRange::new(0x1000, 0x3000).unwrap(),
            FrameRange::new(0x2000, 0x4000).unwrap(),
        ])
        .map(|allocator| allocator.capacity()),
        Err(KernelError::InvalidArgument)
    );
}
