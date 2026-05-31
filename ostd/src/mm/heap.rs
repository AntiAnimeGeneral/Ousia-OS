use core::alloc::Layout;
use core::sync::atomic::{AtomicBool, Ordering};

use linked_list_allocator::LockedHeap;

use crate::boot::{early_println, wait_forever};

pub const EARLY_HEAP_SIZE: usize = 1024 * 1024;

#[global_allocator]
static GLOBAL_ALLOCATOR: LockedHeap = LockedHeap::empty();

static EARLY_HEAP_INITIALIZED: AtomicBool = AtomicBool::new(false);

#[repr(C, align(4096))]
struct EarlyHeap([u8; EARLY_HEAP_SIZE]);

#[unsafe(link_section = ".bss.heap")]
static mut EARLY_HEAP: EarlyHeap = EarlyHeap([0; EARLY_HEAP_SIZE]);

pub fn init_early_heap() {
    if EARLY_HEAP_INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    let heap_start = core::ptr::addr_of_mut!(EARLY_HEAP).cast::<u8>();
    // SAFETY: This fixed early heap is a private static region that lives for
    // the whole kernel lifetime and is initialized once before kernel code uses
    // allocation-backed data structures.
    unsafe {
        GLOBAL_ALLOCATOR.lock().init(heap_start, EARLY_HEAP_SIZE);
    }
}

#[alloc_error_handler]
fn handle_alloc_error(_layout: Layout) -> ! {
    early_println("Ousia kernel heap allocation failed");
    wait_forever()
}
