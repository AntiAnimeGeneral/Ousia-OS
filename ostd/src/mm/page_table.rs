use super::frame::{FrameRange, PAGE_SIZE};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageTableIntentError {
    EmptyRange,
    UnalignedRange,
    RangeOverflow,
    RangeSizeMismatch,
    RightsEmpty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualRange {
    pub base: u64,
    pub size_bytes: u64,
}

impl VirtualRange {
    pub fn new(base: u64, size_bytes: u64) -> Result<Self, PageTableIntentError> {
        validate_page_range(base, size_bytes)?;
        Ok(Self { base, size_bytes })
    }

    pub const fn end(self) -> u64 {
        self.base + self.size_bytes
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct PageTableRights: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageTableUpdate {
    Map {
        virtual_range: VirtualRange,
        frame_range: FrameRange,
        rights: PageTableRights,
    },
    Unmap {
        virtual_range: VirtualRange,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageTableUpdateIntent {
    // TODO(ostd-page-table): this is the architecture-neutral boundary shape for
    // a future page-table update, not proof that page-table nodes, owned frames,
    // PTEs or cursor locks exist. Replace or extend it when arch-owned page-table
    // preparation returns real owner evidence and tests cover failed preparation
    // without kernel VM metadata publication.
    pub update: PageTableUpdate,
}

impl PageTableUpdateIntent {
    pub fn map(
        virtual_range: VirtualRange,
        frame_range: FrameRange,
        rights: PageTableRights,
    ) -> Result<Self, PageTableIntentError> {
        if rights.is_empty() {
            return Err(PageTableIntentError::RightsEmpty);
        }
        if virtual_range.size_bytes != frame_range.len() as u64 {
            return Err(PageTableIntentError::RangeSizeMismatch);
        }
        Ok(Self {
            update: PageTableUpdate::Map {
                virtual_range,
                frame_range,
                rights,
            },
        })
    }

    pub const fn unmap(virtual_range: VirtualRange) -> Self {
        Self {
            update: PageTableUpdate::Unmap { virtual_range },
        }
    }

    pub const fn virtual_range(self) -> VirtualRange {
        match self.update {
            PageTableUpdate::Map { virtual_range, .. } => virtual_range,
            PageTableUpdate::Unmap { virtual_range } => virtual_range,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TlbInvalidationIntent {
    // TODO(ostd-tlb): this is the architecture-neutral invalidation intent, not a
    // completed shootdown. The final OSTD boundary needs CPU target/generation,
    // ordering barriers and completion semantics owned outside kernel VM policy.
    pub range: VirtualRange,
}

impl TlbInvalidationIntent {
    pub const fn new(range: VirtualRange) -> Self {
        Self { range }
    }
}

fn validate_page_range(base: u64, size_bytes: u64) -> Result<(), PageTableIntentError> {
    if size_bytes == 0 {
        return Err(PageTableIntentError::EmptyRange);
    }
    if !is_page_aligned(base) || !is_page_aligned(size_bytes) {
        return Err(PageTableIntentError::UnalignedRange);
    }
    if base.checked_add(size_bytes).is_none() {
        return Err(PageTableIntentError::RangeOverflow);
    }
    Ok(())
}

fn is_page_aligned(value: u64) -> bool {
    value.is_multiple_of(PAGE_SIZE as u64)
}
