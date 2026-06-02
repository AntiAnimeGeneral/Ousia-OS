use kernel::{
    cap::{Capability, CapabilityDescriptor, CapabilitySpace, UntypedCap},
    object::ObjectTable,
    scheduler::Scheduler,
    state::KernelState,
    tcb::{CpuId, ThreadId},
    thread_action::ThreadTable,
};

pub fn cpu(raw: u32) -> CpuId {
    CpuId::new(raw)
}

pub fn thread(raw: u64) -> ThreadId {
    ThreadId::new(raw)
}

pub fn state_with_untyped(size_bits: u8) -> (KernelState, CapabilityDescriptor) {
    let mut cspace = CapabilitySpace::new();
    let untyped = cspace
        .insert_initial_capability(Capability::Untyped(UntypedCap { size_bits }))
        .unwrap();
    let scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();

    (
        KernelState::from_parts(cspace, ObjectTable::new(), ThreadTable::new(), scheduler),
        untyped,
    )
}
