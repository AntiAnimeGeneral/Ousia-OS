use alloc::vec::Vec;

use crate::tcb::{CpuId, Tcb, ThreadId, ThreadState};

mod sealed {
    pub trait Sealed {}
}

/// Read-only thread view required by scheduler enqueue paths.
///
/// Implementing this trait does not mean the thread is runnable. Runnable
/// semantics remain owned by the concrete seL4-like `ThreadState` state
/// machine. The trait is sealed so only kernel-owned thread state sources can
/// represent schedulable input.
pub trait ThreadScheduleView: sealed::Sealed {
    fn id(&self) -> ThreadId;

    fn affinity(&self) -> CpuId;

    fn state(&self) -> ThreadState;
}

impl sealed::Sealed for Tcb {}

impl ThreadScheduleView for Tcb {
    fn id(&self) -> ThreadId {
        self.id()
    }

    fn affinity(&self) -> CpuId {
        self.affinity()
    }

    fn state(&self) -> ThreadState {
        self.state()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadPlacement {
    Ready { cpu: CpuId },
    Current { cpu: CpuId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchedulerAction {
    Enqueued {
        thread: ThreadId,
        cpu: CpuId,
    },
    Switched {
        cpu: CpuId,
        previous: Option<ThreadId>,
        next: ThreadId,
    },
    KeptCurrent {
        cpu: CpuId,
        current: ThreadId,
    },
    BlockedCurrent {
        cpu: CpuId,
        thread: ThreadId,
    },
    NoRunnableThread {
        cpu: CpuId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchedulerError {
    NotEnoughCpus {
        provided: usize,
    },
    DuplicateCpu {
        cpu: CpuId,
    },
    UnknownCpu {
        cpu: CpuId,
    },
    ThreadAffinityMismatch {
        thread: ThreadId,
        expected_cpu: CpuId,
        actual_cpu: CpuId,
    },
    ThreadNotRunnable {
        thread: ThreadId,
        state: ThreadState,
    },
    ThreadAlreadyScheduled {
        thread: ThreadId,
        placement: ThreadPlacement,
    },
    CpuAlreadyHasCurrent {
        cpu: CpuId,
        current: ThreadId,
    },
}

#[derive(Debug)]
pub struct PerCpuRunQueue {
    cpu: CpuId,
    current: Option<ThreadId>,
    ready: [ReadyLane; READY_LANES],
    ready_bitmap: u64,
}

#[derive(Debug)]
pub struct Scheduler {
    run_queues: Vec<PerCpuRunQueue>,
}

#[derive(Debug, Default)]
struct ReadyLane {
    threads: Vec<ThreadId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadySelector {
    priority: usize,
    domain: usize,
}

const READY_LANES: usize = 1;
const DEFAULT_SELECTOR: ReadySelector = ReadySelector {
    priority: 0,
    domain: 0,
};

impl PerCpuRunQueue {
    pub const fn new(cpu: CpuId) -> Self {
        Self {
            cpu,
            current: None,
            ready: [ReadyLane::new()],
            ready_bitmap: 0,
        }
    }

    pub const fn cpu(&self) -> CpuId {
        self.cpu
    }

    fn validate_enqueue_fields(
        &self,
        thread: ThreadId,
        actual_cpu: CpuId,
        state: ThreadState,
    ) -> Result<(), SchedulerError> {
        if actual_cpu != self.cpu {
            return Err(SchedulerError::ThreadAffinityMismatch {
                thread,
                expected_cpu: self.cpu,
                actual_cpu,
            });
        }

        if !state.is_runnable() {
            return Err(SchedulerError::ThreadNotRunnable { thread, state });
        }

        if let Some(placement) = self.placement(thread) {
            return Err(SchedulerError::ThreadAlreadyScheduled { thread, placement });
        }

        Ok(())
    }

    fn enqueue<T: ThreadScheduleView>(
        &mut self,
        thread_view: &T,
    ) -> Result<SchedulerAction, SchedulerError> {
        let thread = thread_view.id();
        let actual_cpu = thread_view.affinity();
        let state = thread_view.state();

        self.validate_enqueue_fields(thread, actual_cpu, state)?;

        self.push_ready(DEFAULT_SELECTOR, thread);
        Ok(SchedulerAction::Enqueued {
            thread,
            cpu: self.cpu,
        })
    }

    fn enqueue_validated(&mut self, thread: ThreadId) -> SchedulerAction {
        self.push_ready(DEFAULT_SELECTOR, thread);
        SchedulerAction::Enqueued {
            thread,
            cpu: self.cpu,
        }
    }

    pub fn schedule_next(&mut self) -> Result<SchedulerAction, SchedulerError> {
        if let Some(current) = self.current {
            return Err(SchedulerError::CpuAlreadyHasCurrent {
                cpu: self.cpu,
                current,
            });
        }

        let Some(next) = self.pop_next_ready() else {
            return Ok(SchedulerAction::NoRunnableThread { cpu: self.cpu });
        };

        self.current = Some(next);
        Ok(SchedulerAction::Switched {
            cpu: self.cpu,
            previous: None,
            next,
        })
    }

    pub fn yield_current(&mut self) -> SchedulerAction {
        let Some(previous) = self.current else {
            return match self.pop_next_ready() {
                Some(next) => {
                    self.current = Some(next);
                    SchedulerAction::Switched {
                        cpu: self.cpu,
                        previous: None,
                        next,
                    }
                }
                None => SchedulerAction::NoRunnableThread { cpu: self.cpu },
            };
        };

        if self.ready_bitmap == 0 {
            return SchedulerAction::KeptCurrent {
                cpu: self.cpu,
                current: previous,
            };
        }

        self.current = None;
        self.push_ready(DEFAULT_SELECTOR, previous);
        let next = self
            .pop_next_ready()
            .expect("non-empty ready bitmap must provide next thread during yield");
        self.current = Some(next);

        SchedulerAction::Switched {
            cpu: self.cpu,
            previous: Some(previous),
            next,
        }
    }

    pub fn block_current(&mut self) -> SchedulerAction {
        let Some(thread) = self.current.take() else {
            return SchedulerAction::NoRunnableThread { cpu: self.cpu };
        };

        SchedulerAction::BlockedCurrent {
            cpu: self.cpu,
            thread,
        }
    }

    pub const fn current(&self) -> Option<ThreadId> {
        self.current
    }

    pub fn ready_len(&self) -> usize {
        self.ready.iter().map(ReadyLane::len).sum()
    }

    pub fn placement(&self, thread: ThreadId) -> Option<ThreadPlacement> {
        if self.current == Some(thread) {
            return Some(ThreadPlacement::Current { cpu: self.cpu });
        }

        self.ready
            .iter()
            .any(|lane| lane.contains(thread))
            .then_some(ThreadPlacement::Ready { cpu: self.cpu })
    }

    fn remove_thread(&mut self, thread: ThreadId) -> Option<ThreadPlacement> {
        if self.current == Some(thread) {
            self.current = None;
            return Some(ThreadPlacement::Current { cpu: self.cpu });
        }

        for lane_index in 0..READY_LANES {
            if self.ready[lane_index].remove(thread) {
                self.update_lane_bitmap(lane_index);
                return Some(ThreadPlacement::Ready { cpu: self.cpu });
            }
        }
        None
    }

    fn push_ready(&mut self, selector: ReadySelector, thread: ThreadId) {
        let lane = selector.lane();
        self.ready[lane].push(thread);
        self.ready_bitmap |= 1 << lane;
    }

    fn pop_next_ready(&mut self) -> Option<ThreadId> {
        let lane = self.next_ready_lane()?;
        let thread = self.ready[lane].pop_front();
        self.update_lane_bitmap(lane);
        thread
    }

    fn next_ready_lane(&self) -> Option<usize> {
        if self.ready_bitmap == 0 {
            return None;
        }
        Some(self.ready_bitmap.trailing_zeros() as usize)
    }

    fn update_lane_bitmap(&mut self, lane: usize) {
        if self.ready[lane].is_empty() {
            self.ready_bitmap &= !(1 << lane);
        } else {
            self.ready_bitmap |= 1 << lane;
        }
    }
}

impl Scheduler {
    pub fn new(cpus: &[CpuId]) -> Result<Self, SchedulerError> {
        if cpus.len() < 2 {
            return Err(SchedulerError::NotEnoughCpus {
                provided: cpus.len(),
            });
        }

        let mut run_queues = Vec::new();
        for cpu in cpus {
            if run_queues
                .iter()
                .any(|queue: &PerCpuRunQueue| queue.cpu() == *cpu)
            {
                return Err(SchedulerError::DuplicateCpu { cpu: *cpu });
            }
            run_queues.push(PerCpuRunQueue::new(*cpu));
        }

        Ok(Self { run_queues })
    }

    pub fn run_queue(&self, cpu: CpuId) -> Result<&PerCpuRunQueue, SchedulerError> {
        self.run_queues
            .iter()
            .find(|queue| queue.cpu() == cpu)
            .ok_or(SchedulerError::UnknownCpu { cpu })
    }

    pub fn run_queue_mut(&mut self, cpu: CpuId) -> Result<&mut PerCpuRunQueue, SchedulerError> {
        self.run_queues
            .iter_mut()
            .find(|queue| queue.cpu() == cpu)
            .ok_or(SchedulerError::UnknownCpu { cpu })
    }

    pub fn schedule_next(&mut self, cpu: CpuId) -> Result<SchedulerAction, SchedulerError> {
        self.run_queue_mut(cpu)?.schedule_next()
    }

    pub fn yield_current(&mut self, cpu: CpuId) -> Result<SchedulerAction, SchedulerError> {
        Ok(self.run_queue_mut(cpu)?.yield_current())
    }

    pub fn block_current(&mut self, cpu: CpuId) -> Result<SchedulerAction, SchedulerError> {
        Ok(self.run_queue_mut(cpu)?.block_current())
    }

    pub fn enqueue<T: ThreadScheduleView>(
        &mut self,
        thread_view: &T,
    ) -> Result<SchedulerAction, SchedulerError> {
        let thread = thread_view.id();

        if let Some(placement) = self.placement(thread) {
            return Err(SchedulerError::ThreadAlreadyScheduled { thread, placement });
        }

        self.run_queue_mut(thread_view.affinity())?
            .enqueue(thread_view)
    }

    pub(crate) fn validate_enqueue_fields(
        &self,
        thread: ThreadId,
        cpu: CpuId,
        state: ThreadState,
    ) -> Result<(), SchedulerError> {
        if let Some(placement) = self.placement(thread) {
            return Err(SchedulerError::ThreadAlreadyScheduled { thread, placement });
        }

        self.run_queue(cpu)?
            .validate_enqueue_fields(thread, cpu, state)
    }

    pub(crate) fn enqueue_validated(&mut self, thread: ThreadId, cpu: CpuId) -> SchedulerAction {
        self.run_queue_mut(cpu)
            .expect("validated scheduler enqueue must target a known CPU")
            .enqueue_validated(thread)
    }

    pub fn placement(&self, thread: ThreadId) -> Option<ThreadPlacement> {
        self.run_queues
            .iter()
            .find_map(|queue| queue.placement(thread))
    }

    pub fn remove_thread(&mut self, thread: ThreadId) -> Option<ThreadPlacement> {
        self.run_queues
            .iter_mut()
            .find_map(|queue| queue.remove_thread(thread))
    }
}

impl ReadyLane {
    const fn new() -> Self {
        Self {
            threads: Vec::new(),
        }
    }

    fn push(&mut self, thread: ThreadId) {
        self.threads.push(thread);
    }

    fn pop_front(&mut self) -> Option<ThreadId> {
        if self.threads.is_empty() {
            return None;
        }
        Some(self.threads.remove(0))
    }

    fn remove(&mut self, thread: ThreadId) -> bool {
        let Some(index) = self.threads.iter().position(|ready| *ready == thread) else {
            return false;
        };
        self.threads.remove(index);
        true
    }

    fn contains(&self, thread: ThreadId) -> bool {
        self.threads.contains(&thread)
    }

    fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    fn len(&self) -> usize {
        self.threads.len()
    }
}

impl ReadySelector {
    const fn lane(self) -> usize {
        let _ = self.priority;
        let _ = self.domain;
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KernelErrorCode;

    struct FakeThread {
        id: ThreadId,
        affinity: CpuId,
        state: ThreadState,
    }

    impl sealed::Sealed for FakeThread {}

    impl ThreadScheduleView for FakeThread {
        fn id(&self) -> ThreadId {
            self.id
        }

        fn affinity(&self) -> CpuId {
            self.affinity
        }

        fn state(&self) -> ThreadState {
            self.state
        }
    }

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64, affinity: CpuId, state: ThreadState) -> Tcb {
        let mut tcb = Tcb::new(ThreadId::new(raw), affinity);
        tcb.set_state(state);
        tcb
    }

    fn fake_thread(raw: u64, affinity: CpuId, state: ThreadState) -> FakeThread {
        FakeThread {
            id: ThreadId::new(raw),
            affinity,
            state,
        }
    }

    fn scheduler() -> Scheduler {
        Scheduler::new(&[cpu(0), cpu(1)]).unwrap()
    }

    #[test]
    fn scheduler_requires_multi_core_topology() {
        assert_eq!(
            Scheduler::new(&[]).unwrap_err(),
            SchedulerError::NotEnoughCpus { provided: 0 }
        );
        assert_eq!(
            Scheduler::new(&[cpu(0)]).unwrap_err(),
            SchedulerError::NotEnoughCpus { provided: 1 }
        );
        assert_eq!(
            Scheduler::new(&[cpu(0), cpu(0)]).unwrap_err(),
            SchedulerError::DuplicateCpu { cpu: cpu(0) }
        );
    }

    #[test]
    fn topology_exposes_per_cpu_run_queues() {
        let mut scheduler = scheduler();

        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().cpu(), cpu(0));
        assert_eq!(
            scheduler.run_queue(cpu(9)).unwrap_err(),
            SchedulerError::UnknownCpu { cpu: cpu(9) }
        );
        assert_eq!(
            scheduler.run_queue_mut(cpu(9)).unwrap_err(),
            SchedulerError::UnknownCpu { cpu: cpu(9) }
        );
    }

    #[test]
    fn enqueue_uses_tcb_affinity_and_requires_runnable_state() {
        let mut scheduler = scheduler();
        let tcb = thread(1, cpu(1), ThreadState::Restart);

        assert_eq!(
            scheduler.enqueue(&tcb),
            Ok(SchedulerAction::Enqueued {
                thread: ThreadId::new(1),
                cpu: cpu(1),
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 1);
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(
            scheduler.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Ready { cpu: cpu(1) })
        );

        let blocked = thread(2, cpu(1), ThreadState::BlockedOnReply);
        assert_eq!(
            scheduler.enqueue(&blocked),
            Err(SchedulerError::ThreadNotRunnable {
                thread: ThreadId::new(2),
                state: ThreadState::BlockedOnReply,
            })
        );
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 1);
    }

    #[test]
    fn ready_bitmap_tracks_non_empty_ready_lane() {
        // Goal: scheduler readiness is represented by the seL4-style bitmap shape.
        // Scope: unit test for the first priority/domain lane.
        // Semantics: enqueue sets the lane bit and schedule clears it after the lane drains.
        let mut scheduler = scheduler();
        let tcb = thread(21, cpu(0), ThreadState::Restart);

        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_bitmap, 0);
        scheduler.enqueue(&tcb).unwrap();
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_bitmap, 1);

        assert_eq!(
            scheduler.schedule_next(cpu(0)),
            Ok(SchedulerAction::Switched {
                cpu: cpu(0),
                previous: None,
                next: ThreadId::new(21),
            })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_bitmap, 0);
    }

    #[test]
    fn enqueue_accepts_thread_schedule_view_without_full_tcb() {
        let mut scheduler = scheduler();
        let runnable = fake_thread(11, cpu(0), ThreadState::Restart);

        assert_eq!(
            scheduler.enqueue(&runnable),
            Ok(SchedulerAction::Enqueued {
                thread: ThreadId::new(11),
                cpu: cpu(0),
            })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 1);

        let blocked = fake_thread(12, cpu(0), ThreadState::BlockedOnReply);
        assert_eq!(
            scheduler.enqueue(&blocked),
            Err(SchedulerError::ThreadNotRunnable {
                thread: ThreadId::new(12),
                state: ThreadState::BlockedOnReply,
            })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 1);
        assert_eq!(scheduler.placement(ThreadId::new(12)), None);
    }

    #[test]
    fn enqueue_unknown_cpu_fails_without_side_effects() {
        let mut scheduler = scheduler();
        let tcb = thread(13, cpu(9), ThreadState::Restart);

        assert_eq!(
            scheduler.enqueue(&tcb),
            Err(SchedulerError::UnknownCpu { cpu: cpu(9) })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 0);
        assert_eq!(scheduler.placement(ThreadId::new(13)), None);
    }

    #[test]
    fn local_run_queue_rejects_wrong_affinity_without_side_effects() {
        let mut scheduler = scheduler();
        let tcb = thread(1, cpu(1), ThreadState::Restart);

        assert_eq!(
            scheduler.run_queue_mut(cpu(0)).unwrap().enqueue(&tcb),
            Err(SchedulerError::ThreadAffinityMismatch {
                thread: ThreadId::new(1),
                expected_cpu: cpu(0),
                actual_cpu: cpu(1),
            })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 0);
        assert_eq!(scheduler.placement(ThreadId::new(1)), None);
    }

    #[test]
    fn schedule_next_picks_fifo_ready_thread_per_cpu() {
        let mut queue = PerCpuRunQueue::new(cpu(0));
        let first = thread(1, cpu(0), ThreadState::Restart);
        let second = thread(2, cpu(0), ThreadState::Restart);

        queue.enqueue(&first).unwrap();
        queue.enqueue(&second).unwrap();

        assert_eq!(
            queue.schedule_next(),
            Ok(SchedulerAction::Switched {
                cpu: cpu(0),
                previous: None,
                next: ThreadId::new(1),
            })
        );
        assert_eq!(queue.current(), Some(ThreadId::new(1)));
        assert_eq!(queue.ready_len(), 1);
        assert_eq!(
            queue.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Current { cpu: cpu(0) })
        );
    }

    #[test]
    fn yielding_current_round_robins_with_same_cpu_ready_queue() {
        let mut queue = PerCpuRunQueue::new(cpu(0));
        let first = thread(1, cpu(0), ThreadState::Restart);
        let second = thread(2, cpu(0), ThreadState::Restart);

        queue.enqueue(&first).unwrap();
        queue.enqueue(&second).unwrap();
        queue.schedule_next().unwrap();

        assert_eq!(
            queue.yield_current(),
            SchedulerAction::Switched {
                cpu: cpu(0),
                previous: Some(ThreadId::new(1)),
                next: ThreadId::new(2),
            }
        );
        assert_eq!(queue.current(), Some(ThreadId::new(2)));
        assert_eq!(queue.ready_len(), 1);
        assert_eq!(
            queue.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Ready { cpu: cpu(0) })
        );
    }

    #[test]
    fn blocking_current_removes_thread_from_local_run_queue() {
        let mut queue = PerCpuRunQueue::new(cpu(0));
        let tcb = thread(1, cpu(0), ThreadState::Restart);

        queue.enqueue(&tcb).unwrap();
        queue.schedule_next().unwrap();

        assert_eq!(
            queue.block_current(),
            SchedulerAction::BlockedCurrent {
                cpu: cpu(0),
                thread: ThreadId::new(1),
            }
        );
        assert_eq!(queue.current(), None);
        assert_eq!(queue.placement(ThreadId::new(1)), None);
    }

    #[test]
    fn duplicate_thread_is_rejected_without_side_effects() {
        let mut scheduler = scheduler();
        let mut tcb = thread(2, cpu(0), ThreadState::Restart);

        scheduler.enqueue(&tcb).unwrap();

        assert_eq!(
            scheduler.enqueue(&tcb),
            Err(SchedulerError::ThreadAlreadyScheduled {
                thread: ThreadId::new(2),
                placement: ThreadPlacement::Ready { cpu: cpu(0) },
            })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 1);

        tcb.set_affinity(cpu(1));
        assert_eq!(
            scheduler.enqueue(&tcb),
            Err(SchedulerError::ThreadAlreadyScheduled {
                thread: ThreadId::new(2),
                placement: ThreadPlacement::Ready { cpu: cpu(0) },
            })
        );
        assert_eq!(scheduler.run_queue(cpu(0)).unwrap().ready_len(), 1);
        assert_eq!(scheduler.run_queue(cpu(1)).unwrap().ready_len(), 0);
    }

    #[test]
    fn schedule_next_rejects_cpu_with_current_without_side_effects() {
        let mut queue = PerCpuRunQueue::new(cpu(0));
        let first = thread(1, cpu(0), ThreadState::Restart);
        let second = thread(2, cpu(0), ThreadState::Restart);

        queue.enqueue(&first).unwrap();
        queue.enqueue(&second).unwrap();
        queue.schedule_next().unwrap();

        assert_eq!(
            queue.schedule_next(),
            Err(SchedulerError::CpuAlreadyHasCurrent {
                cpu: cpu(0),
                current: ThreadId::new(1),
            })
        );
        assert_eq!(queue.current(), Some(ThreadId::new(1)));
        assert_eq!(queue.ready_len(), 1);
        assert_eq!(
            queue.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Current { cpu: cpu(0) })
        );
        assert_eq!(
            queue.placement(ThreadId::new(2)),
            Some(ThreadPlacement::Ready { cpu: cpu(0) })
        );
    }

    #[test]
    fn scheduler_operation_failures_collapse_to_boundary_error_codes() {
        let mut scheduler = Scheduler::new(&[cpu(0), cpu(1)]).unwrap();
        let inactive = thread(1, cpu(0), ThreadState::Inactive);
        let wrong_cpu = thread(2, cpu(1), ThreadState::Restart);
        let current = thread(3, cpu(0), ThreadState::Restart);
        let queued = thread(4, cpu(0), ThreadState::Restart);

        assert_eq!(
            scheduler.run_queue(cpu(9)).unwrap_err().error_code(),
            KernelErrorCode::InvalidArgument
        );
        assert_eq!(
            scheduler
                .run_queue_mut(cpu(0))
                .unwrap()
                .enqueue(&wrong_cpu)
                .unwrap_err()
                .error_code(),
            KernelErrorCode::InvalidArgument
        );
        assert_eq!(
            scheduler
                .run_queue_mut(cpu(0))
                .unwrap()
                .enqueue(&inactive)
                .unwrap_err()
                .error_code(),
            KernelErrorCode::IllegalOperation
        );

        scheduler.enqueue(&current).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();
        scheduler.enqueue(&queued).unwrap();

        assert_eq!(
            scheduler.schedule_next(cpu(0)).unwrap_err().error_code(),
            KernelErrorCode::IllegalOperation
        );
        assert_eq!(
            scheduler.placement(ThreadId::new(3)),
            Some(ThreadPlacement::Current { cpu: cpu(0) })
        );
        assert_eq!(
            scheduler.placement(ThreadId::new(4)),
            Some(ThreadPlacement::Ready { cpu: cpu(0) })
        );
    }
}
