#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualRange {
    pub base: u64,
    pub size_bytes: u64,
}

impl VirtualRange {
    pub const fn new(base: u64, size_bytes: u64) -> Self {
        Self { base, size_bytes }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageTableUpdateIntent {
    // TODO(ostd-page-table): this is the architecture-neutral boundary shape for
    // a future page-table update, not proof that page-table nodes, frames or PTEs
    // exist. Replace or extend it when arch-owned page-table preparation returns
    // real owner evidence and tests cover failed preparation without kernel VM
    // metadata publication.
    pub range: VirtualRange,
}

impl PageTableUpdateIntent {
    pub const fn new(range: VirtualRange) -> Self {
        Self { range }
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
