use ostd::mm::page_table::{PageTableUpdateIntent, TlbInvalidationIntent, VirtualRange};

#[test]
fn page_table_and_tlb_intents_record_virtual_range() {
    // Goal: OSTD exposes architecture-neutral page-table/TLB boundary facts.
    // Scope: host test for value types only, not hardware page-table mutation.
    // Semantics: intents carry the range kernel VM may request without claiming completion.
    let range = VirtualRange::new(0x4000, 0x2000);

    let page_table = PageTableUpdateIntent::new(range);
    let tlb = TlbInvalidationIntent::new(range);

    assert_eq!(page_table.range, range);
    assert_eq!(tlb.range, range);
}
