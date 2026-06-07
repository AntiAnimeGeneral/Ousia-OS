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
    MemoryObject(u64),
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
