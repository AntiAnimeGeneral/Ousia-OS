use alloc::vec::Vec;
use core::ops::Range;

use crate::error::{KernelError, KernelResult};

pub const PAGE_SIZE: u64 = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FrameId(u64);

impl FrameId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FrameGeneration(u64);

impl FrameGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn raw(self) -> u64 {
        self.0
    }

    fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("frame generation exhausted before frame id reuse"),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameOwner {
    Kernel,
    Process(u64),
    MemoryObject { object: u64, generation: u64 },
    PageTable(u64),
    Device(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameState {
    Free,
    Reserved,
    Allocated { owner: FrameOwner },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameRef {
    id: FrameId,
    generation: FrameGeneration,
    paddr: u64,
    owner: FrameOwner,
}

impl FrameRef {
    pub const fn id(self) -> FrameId {
        self.id
    }

    pub const fn paddr(self) -> u64 {
        self.paddr
    }

    pub const fn owner(self) -> FrameOwner {
        self.owner
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameRange {
    pub start: u64,
    pub end: u64,
}

impl FrameRange {
    pub const fn new(start: u64, end: u64) -> KernelResult<Self> {
        if start >= end || !is_page_aligned(start) || !is_page_aligned(end) {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self { start, end })
    }

    pub const fn len(self) -> u64 {
        self.end - self.start
    }

    pub const fn frame_count(self) -> u64 {
        self.len() / PAGE_SIZE
    }

    pub const fn as_range(self) -> Range<u64> {
        self.start..self.end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrameEntry {
    paddr: u64,
    generation: FrameGeneration,
    state: FrameState,
}

pub struct FrameAllocator {
    frames: Vec<FrameEntry>,
}

impl FrameAllocator {
    pub fn from_available_ranges(ranges: &[FrameRange]) -> KernelResult<Self> {
        ensure_non_overlapping(ranges)?;
        let frame_count = total_frame_count(ranges)?;
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(frame_count)
            .map_err(|_| KernelError::NoMemory)?;

        for range in ranges {
            let mut paddr = range.start;
            while paddr < range.end {
                // `frames` was reserved to the exact total frame count above.
                frames.push(FrameEntry {
                    paddr,
                    generation: FrameGeneration::INITIAL,
                    state: FrameState::Free,
                });
                paddr = paddr
                    .checked_add(PAGE_SIZE)
                    .expect("validated frame range overflowed during metadata construction");
            }
        }

        if frames.is_empty() {
            return Err(KernelError::NoMemory);
        }
        frames.sort_by_key(|entry| entry.paddr);

        Ok(Self { frames })
    }

    pub fn capacity(&self) -> usize {
        self.frames.len()
    }

    pub fn free_count(&self) -> usize {
        self.frames
            .iter()
            .filter(|entry| entry.state == FrameState::Free)
            .count()
    }

    pub fn reserve_one(&mut self, owner: FrameOwner) -> KernelResult<FrameRef> {
        let Some(index) = self
            .frames
            .iter()
            .position(|entry| entry.state == FrameState::Free)
        else {
            return Err(KernelError::NoMemory);
        };

        self.frames[index].state = FrameState::Allocated { owner };
        Ok(FrameRef {
            id: FrameId::new(index as u64),
            generation: self.frames[index].generation,
            paddr: self.frames[index].paddr,
            owner,
        })
    }

    pub fn reserve_contiguous(
        &mut self,
        owner: FrameOwner,
        size_bytes: u64,
    ) -> KernelResult<FrameRange> {
        let frame_count = frame_count_for_size(size_bytes)?;
        let Some(start_index) = self.contiguous_free_run(frame_count) else {
            return Err(KernelError::NoMemory);
        };
        let start = self.frames[start_index].paddr;
        let end = start
            .checked_add(size_bytes)
            .ok_or(KernelError::InvalidArgument)?;

        for index in start_index..start_index + frame_count {
            self.frames[index].state = FrameState::Allocated { owner };
        }
        FrameRange::new(start, end)
    }

    pub fn free(&mut self, frame: FrameRef) -> KernelResult<()> {
        let index = self.index(frame.id)?;
        let entry = &mut self.frames[index];
        if entry.generation != frame.generation {
            return Err(KernelError::StaleHandle);
        }
        match entry.state {
            FrameState::Allocated { owner } if owner == frame.owner => {
                entry.state = FrameState::Free;
                entry.generation = entry.generation.next();
                Ok(())
            }
            FrameState::Allocated { .. } => Err(KernelError::MissingRights),
            FrameState::Free | FrameState::Reserved => Err(KernelError::InvalidArgument),
        }
    }

    pub fn free_range(&mut self, range: FrameRange, owner: FrameOwner) -> KernelResult<()> {
        let frame_count =
            usize::try_from(range.frame_count()).map_err(|_| KernelError::NoCapacity)?;
        let matching_count = self
            .frames
            .iter()
            .filter(|entry| range.start <= entry.paddr && entry.paddr < range.end)
            .count();
        if matching_count != frame_count {
            return Err(KernelError::InvalidArgument);
        }
        if self
            .frames
            .iter()
            .filter(|entry| range.start <= entry.paddr && entry.paddr < range.end)
            .any(|entry| entry.state != FrameState::Allocated { owner })
        {
            return Err(KernelError::MissingRights);
        }

        for entry in self
            .frames
            .iter_mut()
            .filter(|entry| range.start <= entry.paddr && entry.paddr < range.end)
        {
            entry.state = FrameState::Free;
            entry.generation = entry.generation.next();
        }
        Ok(())
    }

    pub fn state(&self, id: FrameId) -> KernelResult<FrameState> {
        Ok(self.frames[self.index(id)?].state)
    }

    fn index(&self, id: FrameId) -> KernelResult<usize> {
        let index = id.raw() as usize;
        if index >= self.frames.len() {
            return Err(KernelError::InvalidArgument);
        }
        Ok(index)
    }

    fn contiguous_free_run(&self, frame_count: usize) -> Option<usize> {
        if frame_count == 0 || frame_count > self.frames.len() {
            return None;
        }
        self.frames.windows(frame_count).position(|window| {
            window.iter().enumerate().all(|(offset, entry)| {
                entry.state == FrameState::Free
                    && entry.paddr
                        == window[0]
                            .paddr
                            .checked_add((offset as u64) * PAGE_SIZE)
                            .expect("frame run offset overflowed validated allocator metadata")
            })
        })
    }
}

fn frame_count_for_size(size_bytes: u64) -> KernelResult<usize> {
    if size_bytes == 0 || !is_page_aligned(size_bytes) {
        return Err(KernelError::InvalidArgument);
    }
    usize::try_from(size_bytes / PAGE_SIZE).map_err(|_| KernelError::NoCapacity)
}

fn total_frame_count(ranges: &[FrameRange]) -> KernelResult<usize> {
    let mut total = 0usize;
    for range in ranges {
        total = total
            .checked_add(usize::try_from(range.frame_count()).map_err(|_| KernelError::NoCapacity)?)
            .ok_or(KernelError::NoCapacity)?;
    }
    Ok(total)
}

fn ensure_non_overlapping(ranges: &[FrameRange]) -> KernelResult<()> {
    for (index, range) in ranges.iter().enumerate() {
        for other in ranges.iter().skip(index + 1) {
            if range.start < other.end && other.start < range.end {
                return Err(KernelError::InvalidArgument);
            }
        }
    }
    Ok(())
}

const fn is_page_aligned(value: u64) -> bool {
    value % PAGE_SIZE == 0
}
