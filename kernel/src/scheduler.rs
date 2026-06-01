use alloc::collections::{BTreeMap, VecDeque};

use crate::tcb::{CpuId, Tcb, ThreadId, ThreadState};

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

#[derive(Debug, Default)]
struct CpuRunQueue {
    current: Option<ThreadId>,
    ready: VecDeque<ThreadId>,
}

#[derive(Debug)]
pub struct Scheduler {
    cpus: BTreeMap<CpuId, CpuRunQueue>,
    placement: BTreeMap<ThreadId, ThreadPlacement>,
}

impl Scheduler {
    pub fn new(cpus: &[CpuId]) -> Result<Self, SchedulerError> {
        if cpus.len() < 2 {
            return Err(SchedulerError::NotEnoughCpus {
                provided: cpus.len(),
            });
        }

        let mut run_queues = BTreeMap::new();
        for cpu in cpus {
            if run_queues.insert(*cpu, CpuRunQueue::default()).is_some() {
                return Err(SchedulerError::DuplicateCpu { cpu: *cpu });
            }
        }

        Ok(Self {
            cpus: run_queues,
            placement: BTreeMap::new(),
        })
    }

    pub fn enqueue(&mut self, tcb: &Tcb) -> Result<SchedulerAction, SchedulerError> {
        let thread = tcb.id();
        let cpu = tcb.affinity();
        let state = tcb.state();

        if !state.is_runnable() {
            return Err(SchedulerError::ThreadNotRunnable { thread, state });
        }

        if let Some(placement) = self.placement.get(&thread) {
            return Err(SchedulerError::ThreadAlreadyScheduled {
                thread,
                placement: *placement,
            });
        }

        let queue = self
            .cpus
            .get_mut(&cpu)
            .ok_or(SchedulerError::UnknownCpu { cpu })?;
        queue.ready.push_back(thread);
        self.placement
            .insert(thread, ThreadPlacement::Ready { cpu });

        Ok(SchedulerAction::Enqueued { thread, cpu })
    }

    pub fn schedule_next(&mut self, cpu: CpuId) -> Result<SchedulerAction, SchedulerError> {
        let queue = self
            .cpus
            .get_mut(&cpu)
            .ok_or(SchedulerError::UnknownCpu { cpu })?;

        if let Some(current) = queue.current {
            return Err(SchedulerError::CpuAlreadyHasCurrent { cpu, current });
        }

        let Some(next) = queue.ready.pop_front() else {
            return Ok(SchedulerAction::NoRunnableThread { cpu });
        };

        queue.current = Some(next);
        self.placement
            .insert(next, ThreadPlacement::Current { cpu });

        Ok(SchedulerAction::Switched {
            cpu,
            previous: None,
            next,
        })
    }

    pub fn yield_current(&mut self, cpu: CpuId) -> Result<SchedulerAction, SchedulerError> {
        let queue = self
            .cpus
            .get_mut(&cpu)
            .ok_or(SchedulerError::UnknownCpu { cpu })?;

        let Some(previous) = queue.current else {
            return Ok(match queue.ready.pop_front() {
                Some(next) => {
                    queue.current = Some(next);
                    self.placement
                        .insert(next, ThreadPlacement::Current { cpu });
                    SchedulerAction::Switched {
                        cpu,
                        previous: None,
                        next,
                    }
                }
                None => SchedulerAction::NoRunnableThread { cpu },
            });
        };

        if queue.ready.is_empty() {
            return Ok(SchedulerAction::KeptCurrent {
                cpu,
                current: previous,
            });
        }

        queue.current = None;
        queue.ready.push_back(previous);
        self.placement
            .insert(previous, ThreadPlacement::Ready { cpu });

        let next = queue
            .ready
            .pop_front()
            .expect("non-empty ready queue must provide next thread during yield");
        queue.current = Some(next);
        self.placement
            .insert(next, ThreadPlacement::Current { cpu });

        Ok(SchedulerAction::Switched {
            cpu,
            previous: Some(previous),
            next,
        })
    }

    pub fn block_current(&mut self, cpu: CpuId) -> Result<SchedulerAction, SchedulerError> {
        let queue = self
            .cpus
            .get_mut(&cpu)
            .ok_or(SchedulerError::UnknownCpu { cpu })?;

        let Some(thread) = queue.current.take() else {
            return Ok(SchedulerAction::NoRunnableThread { cpu });
        };

        self.placement.remove(&thread);
        Ok(SchedulerAction::BlockedCurrent { cpu, thread })
    }

    pub fn current(&self, cpu: CpuId) -> Result<Option<ThreadId>, SchedulerError> {
        self.cpus
            .get(&cpu)
            .map(|queue| queue.current)
            .ok_or(SchedulerError::UnknownCpu { cpu })
    }

    pub fn ready_len(&self, cpu: CpuId) -> Result<usize, SchedulerError> {
        self.cpus
            .get(&cpu)
            .map(|queue| queue.ready.len())
            .ok_or(SchedulerError::UnknownCpu { cpu })
    }

    pub fn placement(&self, thread: ThreadId) -> Option<ThreadPlacement> {
        self.placement.get(&thread).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KernelErrorCode;

    fn cpu(raw: u32) -> CpuId {
        CpuId::new(raw)
    }

    fn thread(raw: u64, affinity: CpuId, state: ThreadState) -> Tcb {
        let mut tcb = Tcb::new(ThreadId::new(raw), affinity);
        tcb.set_state(state);
        tcb
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
        assert_eq!(scheduler.ready_len(cpu(1)), Ok(1));
        assert_eq!(scheduler.ready_len(cpu(0)), Ok(0));
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
        assert_eq!(scheduler.ready_len(cpu(1)), Ok(1));
    }

    #[test]
    fn schedule_next_picks_fifo_ready_thread_per_cpu() {
        let mut scheduler = scheduler();
        let first = thread(1, cpu(0), ThreadState::Restart);
        let second = thread(2, cpu(0), ThreadState::Restart);

        scheduler.enqueue(&first).unwrap();
        scheduler.enqueue(&second).unwrap();

        assert_eq!(
            scheduler.schedule_next(cpu(0)),
            Ok(SchedulerAction::Switched {
                cpu: cpu(0),
                previous: None,
                next: ThreadId::new(1),
            })
        );
        assert_eq!(scheduler.current(cpu(0)), Ok(Some(ThreadId::new(1))));
        assert_eq!(scheduler.ready_len(cpu(0)), Ok(1));
        assert_eq!(
            scheduler.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Current { cpu: cpu(0) })
        );
    }

    #[test]
    fn yielding_current_round_robins_with_same_cpu_ready_queue() {
        let mut scheduler = scheduler();
        let first = thread(1, cpu(0), ThreadState::Restart);
        let second = thread(2, cpu(0), ThreadState::Restart);

        scheduler.enqueue(&first).unwrap();
        scheduler.enqueue(&second).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();

        assert_eq!(
            scheduler.yield_current(cpu(0)),
            Ok(SchedulerAction::Switched {
                cpu: cpu(0),
                previous: Some(ThreadId::new(1)),
                next: ThreadId::new(2),
            })
        );
        assert_eq!(scheduler.current(cpu(0)), Ok(Some(ThreadId::new(2))));
        assert_eq!(scheduler.ready_len(cpu(0)), Ok(1));
        assert_eq!(
            scheduler.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Ready { cpu: cpu(0) })
        );
    }

    #[test]
    fn blocking_current_removes_thread_from_scheduler() {
        let mut scheduler = scheduler();
        let tcb = thread(1, cpu(0), ThreadState::Restart);

        scheduler.enqueue(&tcb).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();

        assert_eq!(
            scheduler.block_current(cpu(0)),
            Ok(SchedulerAction::BlockedCurrent {
                cpu: cpu(0),
                thread: ThreadId::new(1),
            })
        );
        assert_eq!(scheduler.current(cpu(0)), Ok(None));
        assert_eq!(scheduler.placement(ThreadId::new(1)), None);
    }

    #[test]
    fn enqueue_rejects_unknown_cpu_and_duplicate_thread_without_side_effects() {
        let mut scheduler = scheduler();
        let unknown = thread(1, cpu(9), ThreadState::Restart);

        assert_eq!(
            scheduler.enqueue(&unknown),
            Err(SchedulerError::UnknownCpu { cpu: cpu(9) })
        );
        assert_eq!(scheduler.placement(ThreadId::new(1)), None);

        let tcb = thread(2, cpu(0), ThreadState::Restart);
        scheduler.enqueue(&tcb).unwrap();

        assert_eq!(
            scheduler.enqueue(&tcb),
            Err(SchedulerError::ThreadAlreadyScheduled {
                thread: ThreadId::new(2),
                placement: ThreadPlacement::Ready { cpu: cpu(0) },
            })
        );
        assert_eq!(scheduler.ready_len(cpu(0)), Ok(1));
    }

    #[test]
    fn schedule_next_rejects_cpu_with_current_without_side_effects() {
        let mut scheduler = scheduler();
        let first = thread(1, cpu(0), ThreadState::Restart);
        let second = thread(2, cpu(0), ThreadState::Restart);

        scheduler.enqueue(&first).unwrap();
        scheduler.enqueue(&second).unwrap();
        scheduler.schedule_next(cpu(0)).unwrap();

        assert_eq!(
            scheduler.schedule_next(cpu(0)),
            Err(SchedulerError::CpuAlreadyHasCurrent {
                cpu: cpu(0),
                current: ThreadId::new(1),
            })
        );
        assert_eq!(scheduler.current(cpu(0)), Ok(Some(ThreadId::new(1))));
        assert_eq!(scheduler.ready_len(cpu(0)), Ok(1));
        assert_eq!(
            scheduler.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Current { cpu: cpu(0) })
        );
        assert_eq!(
            scheduler.placement(ThreadId::new(2)),
            Some(ThreadPlacement::Ready { cpu: cpu(0) })
        );
    }

    #[test]
    fn unknown_cpu_operations_do_not_change_existing_queues() {
        let mut scheduler = scheduler();
        let tcb = thread(1, cpu(0), ThreadState::Restart);

        scheduler.enqueue(&tcb).unwrap();

        assert_eq!(
            scheduler.schedule_next(cpu(9)),
            Err(SchedulerError::UnknownCpu { cpu: cpu(9) })
        );
        assert_eq!(
            scheduler.yield_current(cpu(9)),
            Err(SchedulerError::UnknownCpu { cpu: cpu(9) })
        );
        assert_eq!(
            scheduler.block_current(cpu(9)),
            Err(SchedulerError::UnknownCpu { cpu: cpu(9) })
        );
        assert_eq!(scheduler.current(cpu(0)), Ok(None));
        assert_eq!(scheduler.ready_len(cpu(0)), Ok(1));
        assert_eq!(
            scheduler.placement(ThreadId::new(1)),
            Some(ThreadPlacement::Ready { cpu: cpu(0) })
        );
    }

    #[test]
    fn scheduler_errors_map_to_kernel_error_codes() {
        assert_eq!(
            SchedulerError::UnknownCpu { cpu: cpu(9) }.error_code(),
            KernelErrorCode::InvalidArgument
        );
        assert_eq!(
            SchedulerError::ThreadNotRunnable {
                thread: ThreadId::new(1),
                state: ThreadState::Inactive,
            }
            .error_code(),
            KernelErrorCode::IllegalOperation
        );
        assert_eq!(
            SchedulerError::CpuAlreadyHasCurrent {
                cpu: cpu(0),
                current: ThreadId::new(1),
            }
            .error_code(),
            KernelErrorCode::IllegalOperation
        );
    }
}
