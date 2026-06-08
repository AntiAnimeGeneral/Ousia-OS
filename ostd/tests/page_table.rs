use ostd::mm::page_table::{
    PageTableIntentError, PageTableRights, PageTableUpdate, PageTableUpdateIntent, PhysicalRange,
    TlbInvalidationIntent, VirtualRange,
};

#[test]
fn page_table_map_intent_records_checked_mapping_facts() {
    // Goal: OSTD exposes architecture-neutral page-table mapping facts.
    // Scope: host test for value types only, not hardware page-table mutation.
    // Semantics: map intent carries checked virtual range, physical range, and rights.
    let virtual_range = VirtualRange::new(0x4000, 0x2000).unwrap();
    let physical_range = PhysicalRange::new(0x8000, 0x2000).unwrap();
    let rights = PageTableRights::READ | PageTableRights::WRITE;

    let intent = PageTableUpdateIntent::map(virtual_range, physical_range, rights).unwrap();

    assert_eq!(intent.virtual_range(), virtual_range);
    assert_eq!(
        intent.update,
        PageTableUpdate::Map {
            virtual_range,
            physical_range,
            rights,
        }
    );
}

#[test]
fn page_table_unmap_and_tlb_intents_record_checked_virtual_range() {
    // Goal: OSTD separates page-table unmap intent from TLB invalidation intent.
    // Scope: host test for value types only, not hardware page-table or TLB mutation.
    // Semantics: both intents carry a checked virtual range without claiming completion.
    let range = VirtualRange::new(0x4000, 0x2000).unwrap();

    let page_table = PageTableUpdateIntent::unmap(range);
    let tlb = TlbInvalidationIntent::new(range);

    assert_eq!(
        page_table.update,
        PageTableUpdate::Unmap {
            virtual_range: range
        }
    );
    assert_eq!(tlb.range, range);
}

#[test]
fn page_table_intents_reject_unaligned_or_empty_ranges() {
    // Goal: OSTD owns hardware page granularity checks before arch page-table work.
    // Scope: pure value construction.
    // Semantics: empty, unaligned, or overflowing ranges cannot become page-table intents.
    assert_eq!(
        VirtualRange::new(0x4000, 0),
        Err(PageTableIntentError::EmptyRange)
    );
    assert_eq!(
        VirtualRange::new(0x4001, 0x1000),
        Err(PageTableIntentError::UnalignedRange)
    );
    assert_eq!(
        PhysicalRange::new(u64::MAX - 0xfff, 0x1000),
        Err(PageTableIntentError::RangeOverflow)
    );
}

#[test]
fn page_table_map_intent_requires_matching_size_and_rights() {
    // Goal: map intent construction establishes facts arch page-table code may trust.
    // Scope: pure OSTD page-table intent construction.
    // Semantics: physical and virtual ranges must cover the same size with non-empty rights.
    let virtual_range = VirtualRange::new(0x4000, 0x2000).unwrap();
    let short_physical_range = PhysicalRange::new(0x8000, 0x1000).unwrap();
    let physical_range = PhysicalRange::new(0x8000, 0x2000).unwrap();

    assert_eq!(
        PageTableUpdateIntent::map(virtual_range, short_physical_range, PageTableRights::READ),
        Err(PageTableIntentError::RangeSizeMismatch)
    );
    assert_eq!(
        PageTableUpdateIntent::map(virtual_range, physical_range, PageTableRights::empty()),
        Err(PageTableIntentError::RightsEmpty)
    );
}
