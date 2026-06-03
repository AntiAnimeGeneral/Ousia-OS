use core::alloc::Layout;
use core::ops::Range;

use spin::Mutex;

pub const PAGE_SIZE: usize = 4096;
pub type Paddr = usize;
pub const EARLY_FRAME_REGIONS: usize = 32;

static EARLY_FRAME_ALLOCATOR: Mutex<EarlyFrameAllocatorState<EARLY_FRAME_REGIONS>> =
    Mutex::new(EarlyFrameAllocatorState::uninitialized());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameAllocError {
    EmptyRegion,
    UnalignedRegion,
    InvalidLayout,
    TooManyRegions,
    AlreadyInitialized,
    Uninitialized,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRegionKind {
    Usable,
    Reserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRegion {
    start: Paddr,
    end: Paddr,
    kind: MemoryRegionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameRange {
    start: Paddr,
    end: Paddr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizedMemoryMap<const N: usize> {
    ranges: [Option<FrameRange>; N],
    len: usize,
}

impl MemoryRegion {
    pub const fn new(start: Paddr, end: Paddr, kind: MemoryRegionKind) -> Self {
        Self { start, end, kind }
    }

    pub const fn start(self) -> Paddr {
        self.start
    }

    pub const fn end(self) -> Paddr {
        self.end
    }

    pub const fn kind(self) -> MemoryRegionKind {
        self.kind
    }
}

impl FrameRange {
    pub const fn new(start: Paddr, end: Paddr) -> Result<Self, FrameAllocError> {
        if start >= end {
            return Err(FrameAllocError::EmptyRegion);
        }
        if !is_page_aligned(start) || !is_page_aligned(end) {
            return Err(FrameAllocError::UnalignedRegion);
        }
        Ok(Self { start, end })
    }

    pub const fn start(self) -> Paddr {
        self.start
    }

    pub const fn end(self) -> Paddr {
        self.end
    }

    pub const fn len(self) -> usize {
        self.end - self.start
    }

    pub const fn as_range(self) -> Range<Paddr> {
        self.start..self.end
    }
}

impl<const N: usize> NormalizedMemoryMap<N> {
    pub const fn empty() -> Self {
        Self {
            ranges: [None; N],
            len: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn range(&self, index: usize) -> Option<FrameRange> {
        if index >= self.len {
            return None;
        }
        self.ranges[index]
    }

    fn push(&mut self, range: FrameRange) -> Result<(), FrameAllocError> {
        if self.len == N {
            return Err(FrameAllocError::TooManyRegions);
        }
        self.ranges[self.len] = Some(range);
        self.len += 1;
        Ok(())
    }

    pub const fn try_into_allocator(self) -> Result<EarlyMemoryMapAllocator<N>, FrameAllocError> {
        if self.len == 0 {
            return Err(FrameAllocError::Exhausted);
        }

        let mut ranges = [FrameRange {
            start: 0,
            end: PAGE_SIZE,
        }; N];
        let mut index = 0;
        while index < self.len {
            match self.ranges[index] {
                Some(range) => ranges[index] = range,
                None => return Err(FrameAllocError::Exhausted),
            }
            index += 1;
        }

        Ok(EarlyMemoryMapAllocator::new_active(ranges, self.len))
    }
}

pub fn normalize_memory_regions<const N: usize>(
    regions: &[MemoryRegion],
) -> Result<NormalizedMemoryMap<N>, FrameAllocError> {
    normalize_memory_regions_with_reserved(regions, &[])
}

pub fn normalize_memory_regions_with_reserved<const N: usize>(
    regions: &[MemoryRegion],
    reserved: &[FrameRange],
) -> Result<NormalizedMemoryMap<N>, FrameAllocError> {
    let mut map = NormalizedMemoryMap::empty();
    for region in regions {
        if region.kind != MemoryRegionKind::Usable {
            continue;
        }
        if let Some(range) = normalize_usable_region(*region) {
            push_available_range(&mut map, range, reserved)?;
        }
    }
    Ok(map)
}

pub fn init_early_frame_allocator_from_regions(
    regions: &[MemoryRegion],
    reserved: &[FrameRange],
) -> Result<(), FrameAllocError> {
    EARLY_FRAME_ALLOCATOR
        .lock()
        .init_from_regions(regions, reserved)
}

pub fn try_allocate_early_frame(layout: Layout) -> Result<FrameRange, FrameAllocError> {
    EARLY_FRAME_ALLOCATOR.lock().allocate(layout)
}

fn push_available_range<const N: usize>(
    map: &mut NormalizedMemoryMap<N>,
    range: FrameRange,
    reserved: &[FrameRange],
) -> Result<(), FrameAllocError> {
    let mut cursor = range.start;
    while cursor < range.end {
        if let Some(end) = covering_reserved_end(cursor, range.end, reserved) {
            cursor = end;
            continue;
        }

        let end = next_reserved_start(cursor, range.end, reserved).unwrap_or(range.end);
        map.push(FrameRange { start: cursor, end })?;
        cursor = end;
    }

    Ok(())
}

fn covering_reserved_end(cursor: Paddr, limit: Paddr, reserved: &[FrameRange]) -> Option<Paddr> {
    let mut end = cursor;
    for range in reserved {
        if range.start <= cursor && range.end > end {
            end = range.end.min(limit);
        }
    }

    if end == cursor { None } else { Some(end) }
}

fn next_reserved_start(cursor: Paddr, limit: Paddr, reserved: &[FrameRange]) -> Option<Paddr> {
    let mut start = limit;
    for range in reserved {
        if range.end <= cursor || range.start <= cursor || range.start >= start {
            continue;
        }
        start = range.start.min(limit);
    }

    if start == limit { None } else { Some(start) }
}

#[derive(Debug)]
pub struct EarlyFrameAllocator {
    range: FrameRange,
    next: Paddr,
}

#[derive(Debug)]
pub struct EarlyMemoryMapAllocator<const N: usize> {
    ranges: [FrameRange; N],
    next: [Paddr; N],
    active: usize,
}

#[derive(Debug)]
pub struct EarlyFrameAllocatorState<const N: usize> {
    allocator: Option<EarlyMemoryMapAllocator<N>>,
}

impl EarlyFrameAllocator {
    pub const fn new(range: FrameRange) -> Self {
        Self {
            range,
            next: range.start,
        }
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<FrameRange, FrameAllocError> {
        let request = normalize_layout(layout)?;
        let allocated =
            allocate_from_range(self.next, self.range.end, request.size(), request.align())
                .ok_or(FrameAllocError::Exhausted)?;

        self.next = allocated.end;
        Ok(allocated)
    }

    pub const fn remaining(&self) -> usize {
        self.range.end - self.next
    }

    pub const fn allocated(&self) -> Range<Paddr> {
        self.range.start..self.next
    }
}

impl<const N: usize> EarlyMemoryMapAllocator<N> {
    pub const fn new(ranges: [FrameRange; N]) -> Self {
        let mut next = [0; N];
        let mut index = 0;
        while index < N {
            next[index] = ranges[index].start;
            index += 1;
        }
        Self {
            ranges,
            next,
            active: N,
        }
    }

    const fn new_active(ranges: [FrameRange; N], active: usize) -> Self {
        let mut next = [0; N];
        let mut index = 0;
        while index < N {
            next[index] = ranges[index].start;
            index += 1;
        }
        Self {
            ranges,
            next,
            active,
        }
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<FrameRange, FrameAllocError> {
        let request = normalize_layout(layout)?;
        for index in 0..self.active {
            if let Some(allocated) = allocate_from_range(
                self.next[index],
                self.ranges[index].end,
                request.size(),
                request.align(),
            ) {
                self.next[index] = allocated.end;
                return Ok(allocated);
            }
        }
        Err(FrameAllocError::Exhausted)
    }

    pub const fn allocated_in(&self, index: usize) -> Range<Paddr> {
        self.ranges[index].start..self.next[index]
    }

    pub const fn range(&self, index: usize) -> FrameRange {
        self.ranges[index]
    }

    pub const fn active_ranges(&self) -> usize {
        self.active
    }
}

impl<const N: usize> EarlyFrameAllocatorState<N> {
    pub const fn uninitialized() -> Self {
        Self { allocator: None }
    }

    pub fn init_from_regions(
        &mut self,
        regions: &[MemoryRegion],
        reserved: &[FrameRange],
    ) -> Result<(), FrameAllocError> {
        if self.allocator.is_some() {
            return Err(FrameAllocError::AlreadyInitialized);
        }

        let map = normalize_memory_regions_with_reserved(regions, reserved)?;
        self.allocator = Some(map.try_into_allocator()?);
        Ok(())
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<FrameRange, FrameAllocError> {
        self.allocator
            .as_mut()
            .ok_or(FrameAllocError::Uninitialized)?
            .allocate(layout)
    }

    pub const fn is_initialized(&self) -> bool {
        self.allocator.is_some()
    }
}

fn allocate_from_range(next: Paddr, limit: Paddr, size: usize, align: usize) -> Option<FrameRange> {
    let start = align_up(next, align)?;
    let end = start.checked_add(size)?;
    if end > limit {
        return None;
    }

    FrameRange::new(start, end).ok()
}

fn normalize_layout(layout: Layout) -> Result<Layout, FrameAllocError> {
    if layout.size() == 0 {
        return Err(FrameAllocError::InvalidLayout);
    }
    let align = layout.align().max(PAGE_SIZE);
    let size = align_up(layout.size(), PAGE_SIZE).ok_or(FrameAllocError::InvalidLayout)?;
    Layout::from_size_align(size, align).map_err(|_| FrameAllocError::InvalidLayout)
}

fn normalize_usable_region(region: MemoryRegion) -> Option<FrameRange> {
    let start = align_up(region.start, PAGE_SIZE)?;
    let end = align_down(region.end, PAGE_SIZE);
    if start >= end {
        return None;
    }
    FrameRange::new(start, end).ok()
}

const fn is_page_aligned(value: usize) -> bool {
    value % PAGE_SIZE == 0
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    debug_assert!(align.is_power_of_two());
    let mask = align - 1;
    value.checked_add(mask).map(|value| value & !mask)
}

const fn align_down(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::empty_region(0x1000, 0x1000, FrameAllocError::EmptyRegion)]
    #[case::unaligned_start(0x1001, 0x3000, FrameAllocError::UnalignedRegion)]
    #[case::unaligned_end(0x1000, 0x3001, FrameAllocError::UnalignedRegion)]
    fn frame_range_rejects_invalid_boundaries(
        #[case] start: Paddr,
        #[case] end: Paddr,
        #[case] expected: FrameAllocError,
    ) {
        // Goal: FrameRange establishes the page-aligned non-empty range invariant.
        // Scope: pure FrameRange construction without allocator state changes.
        // Semantics: invalid boundaries fail at construction and never enter allocator state.
        assert_eq!(FrameRange::new(start, end), Err(expected));
    }

    #[rstest]
    #[case::oversized_request(Layout::from_size_align(0x3000, PAGE_SIZE).unwrap(), FrameAllocError::Exhausted)]
    #[case::zero_sized_request(Layout::from_size_align(0, PAGE_SIZE).unwrap(), FrameAllocError::InvalidLayout)]
    fn frame_allocator_rejects_invalid_requests_without_advancing(
        #[case] layout: Layout,
        #[case] expected: FrameAllocError,
    ) {
        // Goal: EarlyFrameAllocator rejects invalid requests before committing allocation state.
        // Scope: single-range host unit test for allocation prechecks.
        // Semantics: exhausted and invalid layouts both leave the allocation cursor unchanged.
        let range = FrameRange::new(0x1000, 0x3000).unwrap();
        let mut allocator = EarlyFrameAllocator::new(range);

        assert_eq!(allocator.allocate(layout), Err(expected));
        assert_eq!(allocator.allocated(), 0x1000..0x1000);
    }

    #[test]
    fn allocates_page_aligned_ranges_monotonically() {
        // Goal: EarlyFrameAllocator hands out page-aligned frames monotonically.
        // Scope: single contiguous FrameRange allocation state.
        // Semantics: each successful allocation advances the cursor and updates remaining capacity.
        let range = FrameRange::new(0x1000, 0x5000).unwrap();
        let mut allocator = EarlyFrameAllocator::new(range);

        let first = allocator.allocate(Layout::new::<u8>()).unwrap();
        assert_eq!(first.as_range(), 0x1000..0x2000);

        let second = allocator
            .allocate(Layout::from_size_align(0x1800, PAGE_SIZE).unwrap())
            .unwrap();
        assert_eq!(second.as_range(), 0x2000..0x4000);

        assert_eq!(allocator.allocated(), 0x1000..0x4000);
        assert_eq!(allocator.remaining(), 0x1000);
    }

    #[test]
    fn honors_larger_alignment() {
        // Goal: EarlyFrameAllocator honors alignments larger than the page size.
        // Scope: single-range allocation alignment handling.
        // Semantics: allocator skips forward to the requested alignment before committing.
        let range = FrameRange::new(0x1000, 0x9000).unwrap();
        let mut allocator = EarlyFrameAllocator::new(range);

        let allocated = allocator
            .allocate(Layout::from_size_align(PAGE_SIZE, 0x4000).unwrap())
            .unwrap();
        assert_eq!(allocated.as_range(), 0x4000..0x5000);
    }

    #[test]
    fn memory_map_allocator_walks_ranges_in_order() {
        // Goal: memory-map allocator consumes normalized ranges in order.
        // Scope: multi-range allocator cursor behavior.
        // Semantics: exhausting one range advances to the next without rewinding prior allocations.
        let mut allocator = EarlyMemoryMapAllocator::new([
            FrameRange::new(0x1000, 0x3000).unwrap(),
            FrameRange::new(0x8000, 0xc000).unwrap(),
        ]);

        let first = allocator
            .allocate(Layout::from_size_align(0x2000, PAGE_SIZE).unwrap())
            .unwrap();
        assert_eq!(first.as_range(), 0x1000..0x3000);

        let second = allocator.allocate(Layout::new::<u8>()).unwrap();
        assert_eq!(second.as_range(), 0x8000..0x9000);
        assert_eq!(allocator.allocated_in(0), 0x1000..0x3000);
        assert_eq!(allocator.allocated_in(1), 0x8000..0x9000);
    }

    #[test]
    fn memory_map_allocator_honors_alignment_per_range() {
        // Goal: memory-map allocation alignment is evaluated per candidate range.
        // Scope: multi-range allocator selection with an oversized alignment.
        // Semantics: an unsuitable first range remains untouched while a later aligned range is used.
        let mut allocator = EarlyMemoryMapAllocator::new([
            FrameRange::new(0x1000, 0x5000).unwrap(),
            FrameRange::new(0x8000, 0xc000).unwrap(),
        ]);

        let first = allocator
            .allocate(Layout::from_size_align(PAGE_SIZE, 0x8000).unwrap())
            .unwrap();
        assert_eq!(first.as_range(), 0x8000..0x9000);
        assert_eq!(allocator.allocated_in(0), 0x1000..0x1000);
        assert_eq!(allocator.allocated_in(1), 0x8000..0x9000);
    }

    #[test]
    fn memory_map_allocator_exhaustion_does_not_advance_ranges() {
        // Goal: memory-map allocator failure has no partial cursor side effects.
        // Scope: multi-range allocation request larger than every usable range.
        // Semantics: exhaustion leaves every range's allocated prefix unchanged.
        let mut allocator = EarlyMemoryMapAllocator::new([
            FrameRange::new(0x1000, 0x2000).unwrap(),
            FrameRange::new(0x4000, 0x5000).unwrap(),
        ]);

        assert_eq!(
            allocator.allocate(Layout::from_size_align(0x3000, PAGE_SIZE).unwrap()),
            Err(FrameAllocError::Exhausted)
        );
        assert_eq!(allocator.allocated_in(0), 0x1000..0x1000);
        assert_eq!(allocator.allocated_in(1), 0x4000..0x4000);
    }

    #[test]
    fn normalizes_only_usable_page_ranges() {
        // Goal: memory-map normalization keeps only page-aligned usable regions.
        // Scope: host unit test for mixed usable/reserved firmware map input.
        // Semantics: reserved regions and sub-page fragments never enter allocator input.
        let map = normalize_memory_regions::<4>(&[
            MemoryRegion::new(0x1003, 0x3fff, MemoryRegionKind::Usable),
            MemoryRegion::new(0x4000, 0x9000, MemoryRegionKind::Reserved),
            MemoryRegion::new(0x9001, 0xa000, MemoryRegionKind::Usable),
            MemoryRegion::new(0xa000, 0xc000, MemoryRegionKind::Usable),
        ])
        .unwrap();

        assert_eq!(map.len(), 2);
        assert_eq!(map.range(0).unwrap().as_range(), 0x2000..0x3000);
        assert_eq!(map.range(1).unwrap().as_range(), 0xa000..0xc000);
        assert_eq!(map.range(2), None);
    }

    #[test]
    fn normalizing_empty_usable_result_is_allowed() {
        // Goal: normalization accepts maps with no usable page range.
        // Scope: host unit test for memory map normalization without allocator creation.
        // Semantics: empty usable output is valid map data, distinct from allocator exhaustion.
        let map = normalize_memory_regions::<2>(&[
            MemoryRegion::new(0x1001, 0x1fff, MemoryRegionKind::Usable),
            MemoryRegion::new(0x2000, 0x4000, MemoryRegionKind::Reserved),
        ])
        .unwrap();

        assert!(map.is_empty());
    }

    #[test]
    fn normalizing_reports_capacity_exhaustion() {
        // Goal: normalization reports when usable ranges exceed destination map capacity.
        // Scope: host unit test for NormalizedMemoryMap capacity during region normalization.
        // Semantics: capacity exhaustion is reported before silently dropping usable ranges.
        assert_eq!(
            normalize_memory_regions::<1>(&[
                MemoryRegion::new(0x1000, 0x2000, MemoryRegionKind::Usable),
                MemoryRegion::new(0x3000, 0x4000, MemoryRegionKind::Usable),
            ]),
            Err(FrameAllocError::TooManyRegions)
        );
    }

    #[test]
    fn normalizing_subtracts_reserved_ranges() {
        // Goal: reserved ranges are subtracted from usable memory before allocation.
        // Scope: normalization with explicit reserved intervals.
        // Semantics: usable output excludes reserved spans and keeps remaining segments ordered.
        let map = normalize_memory_regions_with_reserved::<4>(
            &[MemoryRegion::new(0x1000, 0x9000, MemoryRegionKind::Usable)],
            &[
                FrameRange::new(0x3000, 0x5000).unwrap(),
                FrameRange::new(0x8000, 0x9000).unwrap(),
            ],
        )
        .unwrap();

        assert_eq!(map.len(), 2);
        assert_eq!(map.range(0).unwrap().as_range(), 0x1000..0x3000);
        assert_eq!(map.range(1).unwrap().as_range(), 0x5000..0x8000);
    }

    #[test]
    fn reserved_ranges_can_cover_usable_range_edges() {
        // Goal: reserved subtraction handles overlaps at usable-region edges.
        // Scope: normalization with reserved ranges extending beyond usable bounds.
        // Semantics: only the uncovered interior segment remains usable.
        let map = normalize_memory_regions_with_reserved::<2>(
            &[MemoryRegion::new(0x1000, 0x9000, MemoryRegionKind::Usable)],
            &[
                FrameRange::new(0x0000, 0x3000).unwrap(),
                FrameRange::new(0x7000, 0xa000).unwrap(),
            ],
        )
        .unwrap();

        assert_eq!(map.len(), 1);
        assert_eq!(map.range(0).unwrap().as_range(), 0x3000..0x7000);
    }

    #[test]
    fn reserved_ranges_can_remove_all_usable_memory() {
        // Goal: reserved subtraction may remove all usable memory without error.
        // Scope: normalization where reserved ranges fully cover usable input.
        // Semantics: empty normalized output is valid map data.
        let map = normalize_memory_regions_with_reserved::<2>(
            &[MemoryRegion::new(0x1000, 0x3000, MemoryRegionKind::Usable)],
            &[FrameRange::new(0x0000, 0x4000).unwrap()],
        )
        .unwrap();

        assert!(map.is_empty());
    }

    #[test]
    fn reserved_ranges_can_be_unsorted_and_overlapping() {
        // Goal: reserved subtraction is independent of reserved-range order and overlap shape.
        // Scope: normalization over unsorted overlapping reserved intervals.
        // Semantics: effective reserved union is removed before producing ordered usable segments.
        let map = normalize_memory_regions_with_reserved::<4>(
            &[MemoryRegion::new(0x1000, 0xa000, MemoryRegionKind::Usable)],
            &[
                FrameRange::new(0x7000, 0x9000).unwrap(),
                FrameRange::new(0x3000, 0x5000).unwrap(),
                FrameRange::new(0x4000, 0x8000).unwrap(),
            ],
        )
        .unwrap();

        assert_eq!(map.len(), 2);
        assert_eq!(map.range(0).unwrap().as_range(), 0x1000..0x3000);
        assert_eq!(map.range(1).unwrap().as_range(), 0x9000..0xa000);
    }

    #[test]
    fn reports_capacity_exhaustion_after_reserved_subtraction() {
        // Goal: reserved subtraction can split one usable region into more ranges than the map can hold.
        // Scope: host unit test for normalization capacity after reserved-range filtering.
        // Semantics: split-range overflow is reported instead of dropping the later usable segment.
        assert_eq!(
            normalize_memory_regions_with_reserved::<1>(
                &[MemoryRegion::new(0x1000, 0x9000, MemoryRegionKind::Usable)],
                &[FrameRange::new(0x3000, 0x5000).unwrap()],
            ),
            Err(FrameAllocError::TooManyRegions)
        );
    }

    #[test]
    fn normalized_map_can_create_allocator() {
        // Goal: a non-empty normalized memory map can become an allocation source.
        // Scope: bridge from normalization output to EarlyMemoryMapAllocator.
        // Semantics: allocator preserves range order and advances through normalized ranges.
        let map = normalize_memory_regions::<4>(&[
            MemoryRegion::new(0x1003, 0x3fff, MemoryRegionKind::Usable),
            MemoryRegion::new(0x8000, 0xa000, MemoryRegionKind::Usable),
        ])
        .unwrap();
        let mut allocator = map.try_into_allocator().unwrap();

        assert_eq!(allocator.active_ranges(), 2);
        assert_eq!(allocator.range(0).as_range(), 0x2000..0x3000);
        assert_eq!(allocator.range(1).as_range(), 0x8000..0xa000);

        let first = allocator.allocate(Layout::new::<u8>()).unwrap();
        assert_eq!(first.as_range(), 0x2000..0x3000);

        let second = allocator.allocate(Layout::new::<u8>()).unwrap();
        assert_eq!(second.as_range(), 0x8000..0x9000);
    }

    #[test]
    fn empty_normalized_map_cannot_create_allocator() {
        // Goal: allocator creation rejects normalized maps with no active range.
        // Scope: NormalizedMemoryMap to allocator conversion boundary.
        // Semantics: absence of usable memory is reported as exhaustion, not an empty allocator.
        let map = NormalizedMemoryMap::<2>::empty();
        assert_eq!(
            map.try_into_allocator().unwrap_err(),
            FrameAllocError::Exhausted
        );
    }

    #[test]
    fn early_frame_state_rejects_allocation_before_init() {
        // Goal: global early-frame state rejects allocation before initialization.
        // Scope: EarlyFrameAllocatorState pre-init boundary.
        // Semantics: uninitialized state remains uninitialized after allocation failure.
        let mut state = EarlyFrameAllocatorState::<2>::uninitialized();

        assert!(!state.is_initialized());
        assert_eq!(
            state.allocate(Layout::new::<u8>()),
            Err(FrameAllocError::Uninitialized)
        );
    }

    #[test]
    fn early_frame_state_initializes_from_regions_once() {
        // Goal: early-frame state accepts exactly one initialization.
        // Scope: EarlyFrameAllocatorState initialization boundary.
        // Semantics: successful init establishes allocator state; later init attempts fail.
        let mut state = EarlyFrameAllocatorState::<4>::uninitialized();

        state
            .init_from_regions(
                &[MemoryRegion::new(0x1000, 0x9000, MemoryRegionKind::Usable)],
                &[FrameRange::new(0x3000, 0x5000).unwrap()],
            )
            .unwrap();

        assert!(state.is_initialized());
        assert_eq!(
            state.init_from_regions(
                &[MemoryRegion::new(0x1000, 0x9000, MemoryRegionKind::Usable)],
                &[],
            ),
            Err(FrameAllocError::AlreadyInitialized)
        );
    }

    #[test]
    fn early_frame_state_allocates_only_after_reserved_ranges() {
        // Goal: early-frame state allocates only from memory left after reserved subtraction.
        // Scope: initialized EarlyFrameAllocatorState over firmware regions and reserved ranges.
        // Semantics: allocation starts after reserved boot/platform memory.
        let mut state = EarlyFrameAllocatorState::<4>::uninitialized();

        state
            .init_from_regions(
                &[MemoryRegion::new(0x1000, 0x9000, MemoryRegionKind::Usable)],
                &[FrameRange::new(0x1000, 0x5000).unwrap()],
            )
            .unwrap();

        let allocated = state.allocate(Layout::new::<u8>()).unwrap();
        assert_eq!(allocated.as_range(), 0x5000..0x6000);
    }

    #[test]
    fn early_frame_state_reports_no_available_memory() {
        // Goal: early-frame initialization fails when reserved ranges cover all usable memory.
        // Scope: EarlyFrameAllocatorState init path with empty normalized allocator input.
        // Semantics: failed init leaves the global state uninitialized.
        let mut state = EarlyFrameAllocatorState::<2>::uninitialized();

        assert_eq!(
            state.init_from_regions(
                &[MemoryRegion::new(0x1000, 0x3000, MemoryRegionKind::Usable)],
                &[FrameRange::new(0x1000, 0x3000).unwrap()],
            ),
            Err(FrameAllocError::Exhausted)
        );
        assert!(!state.is_initialized());
    }
}
