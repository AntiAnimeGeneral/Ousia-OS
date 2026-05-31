use core::alloc::Layout;
use core::ops::Range;

pub const PAGE_SIZE: usize = 4096;
pub type Paddr = usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameAllocError {
    EmptyRegion,
    UnalignedRegion,
    InvalidLayout,
    TooManyRegions,
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

    #[test]
    fn rejects_empty_or_unaligned_regions() {
        assert_eq!(
            FrameRange::new(0x1000, 0x1000),
            Err(FrameAllocError::EmptyRegion)
        );
        assert_eq!(
            FrameRange::new(0x1001, 0x3000),
            Err(FrameAllocError::UnalignedRegion)
        );
        assert_eq!(
            FrameRange::new(0x1000, 0x3001),
            Err(FrameAllocError::UnalignedRegion)
        );
    }

    #[test]
    fn allocates_page_aligned_ranges_monotonically() {
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
        let range = FrameRange::new(0x1000, 0x9000).unwrap();
        let mut allocator = EarlyFrameAllocator::new(range);

        let allocated = allocator
            .allocate(Layout::from_size_align(PAGE_SIZE, 0x4000).unwrap())
            .unwrap();
        assert_eq!(allocated.as_range(), 0x4000..0x5000);
    }

    #[test]
    fn reports_exhaustion_without_advancing() {
        let range = FrameRange::new(0x1000, 0x3000).unwrap();
        let mut allocator = EarlyFrameAllocator::new(range);

        assert_eq!(
            allocator.allocate(Layout::from_size_align(0x3000, PAGE_SIZE).unwrap()),
            Err(FrameAllocError::Exhausted)
        );
        assert_eq!(allocator.allocated(), 0x1000..0x1000);
    }

    #[test]
    fn rejects_zero_sized_allocations() {
        let range = FrameRange::new(0x1000, 0x3000).unwrap();
        let mut allocator = EarlyFrameAllocator::new(range);

        assert_eq!(
            allocator.allocate(Layout::from_size_align(0, PAGE_SIZE).unwrap()),
            Err(FrameAllocError::InvalidLayout)
        );
    }

    #[test]
    fn memory_map_allocator_walks_ranges_in_order() {
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
        let map = normalize_memory_regions::<2>(&[
            MemoryRegion::new(0x1001, 0x1fff, MemoryRegionKind::Usable),
            MemoryRegion::new(0x2000, 0x4000, MemoryRegionKind::Reserved),
        ])
        .unwrap();

        assert!(map.is_empty());
    }

    #[test]
    fn normalizing_reports_capacity_exhaustion() {
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
        let map = normalize_memory_regions_with_reserved::<2>(
            &[MemoryRegion::new(0x1000, 0x3000, MemoryRegionKind::Usable)],
            &[FrameRange::new(0x0000, 0x4000).unwrap()],
        )
        .unwrap();

        assert!(map.is_empty());
    }

    #[test]
    fn reserved_ranges_can_be_unsorted_and_overlapping() {
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
        assert_eq!(
            normalize_memory_regions_with_reserved::<1>(
                &[MemoryRegion::new(0x1000, 0x9000, MemoryRegionKind::Usable,)],
                &[FrameRange::new(0x3000, 0x5000).unwrap()],
            ),
            Err(FrameAllocError::TooManyRegions)
        );
    }

    #[test]
    fn normalized_map_can_create_allocator() {
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
        let map = NormalizedMemoryMap::<2>::empty();
        assert_eq!(
            map.try_into_allocator().unwrap_err(),
            FrameAllocError::Exhausted
        );
    }
}
