use core::alloc::Layout;
use core::ops::Range;

pub const PAGE_SIZE: usize = 4096;
pub type Paddr = usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameAllocError {
    EmptyRegion,
    UnalignedRegion,
    InvalidLayout,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameRange {
    start: Paddr,
    end: Paddr,
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

#[derive(Debug)]
pub struct EarlyFrameAllocator {
    range: FrameRange,
    next: Paddr,
}

#[derive(Debug)]
pub struct EarlyMemoryMapAllocator<const N: usize> {
    ranges: [FrameRange; N],
    next: [Paddr; N],
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
        Self { ranges, next }
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<FrameRange, FrameAllocError> {
        let request = normalize_layout(layout)?;
        for index in 0..N {
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

const fn is_page_aligned(value: usize) -> bool {
    value % PAGE_SIZE == 0
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    debug_assert!(align.is_power_of_two());
    let mask = align - 1;
    value.checked_add(mask).map(|value| value & !mask)
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
}
