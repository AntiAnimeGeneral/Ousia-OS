#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct CpuId(u32);

impl CpuId {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuSet {
    AllActive,
    One(CpuId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuTopologyError {
    EmptyTopology,
    TooManyCpus,
    DuplicateCpu,
    UnknownCpu,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopologySnapshot<const N: usize> {
    active: [Option<CpuId>; N],
    active_count: usize,
}

impl<const N: usize> TopologySnapshot<N> {
    pub fn from_active(cpus: &[CpuId]) -> Result<Self, CpuTopologyError> {
        if cpus.is_empty() {
            return Err(CpuTopologyError::EmptyTopology);
        }
        if cpus.len() > N {
            return Err(CpuTopologyError::TooManyCpus);
        }

        let mut active = [None; N];
        for (index, cpu) in cpus.iter().copied().enumerate() {
            if active[..index].contains(&Some(cpu)) {
                return Err(CpuTopologyError::DuplicateCpu);
            }
            active[index] = Some(cpu);
        }

        Ok(Self {
            active,
            active_count: cpus.len(),
        })
    }

    pub const fn active_count(self) -> usize {
        self.active_count
    }

    pub fn active_cpus(&self) -> impl Iterator<Item = CpuId> + '_ {
        self.active
            .iter()
            .take(self.active_count)
            .filter_map(|cpu| *cpu)
    }

    pub fn expand(self, set: CpuSet) -> Result<TargetCpuSet<N>, CpuTopologyError> {
        match set {
            CpuSet::AllActive => Ok(TargetCpuSet {
                cpus: self.active,
                count: self.active_count,
            }),
            CpuSet::One(cpu) => {
                if !self.active_cpus().any(|active| active == cpu) {
                    return Err(CpuTopologyError::UnknownCpu);
                }
                let mut cpus = [None; N];
                cpus[0] = Some(cpu);
                Ok(TargetCpuSet { cpus, count: 1 })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetCpuSet<const N: usize> {
    cpus: [Option<CpuId>; N],
    count: usize,
}

impl<const N: usize> TargetCpuSet<N> {
    pub const fn count(self) -> usize {
        self.count
    }

    pub fn cpus(&self) -> impl Iterator<Item = CpuId> + '_ {
        self.cpus.iter().take(self.count).filter_map(|cpu| *cpu)
    }

    pub fn contains(self, cpu: CpuId) -> bool {
        self.cpus().any(|target| target == cpu)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct CpuGeneration(u64);

impl CpuGeneration {
    pub const INITIAL: Self = Self(0);

    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuId, CpuSet, CpuTopologyError, TopologySnapshot};

    #[test]
    fn topology_rejects_empty_active_set() {
        // Goal: topology snapshots represent real active CPU evidence.
        // Scope: construct an empty OSTD topology snapshot.
        // Semantics: empty snapshots are rejected instead of standing in for single-core defaults.
        assert_eq!(
            TopologySnapshot::<4>::from_active(&[]),
            Err(CpuTopologyError::EmptyTopology)
        );
    }

    #[test]
    fn topology_rejects_duplicate_active_cpu() {
        // Goal: active CPU snapshots keep one identity per CPU.
        // Scope: construct a snapshot with a repeated CpuId.
        // Semantics: duplicates are rejected before a target set can be expanded.
        assert_eq!(
            TopologySnapshot::<4>::from_active(&[CpuId::new(1), CpuId::new(1)]),
            Err(CpuTopologyError::DuplicateCpu)
        );
    }

    #[test]
    fn topology_rejects_more_active_cpus_than_storage() {
        // Goal: topology import fails before overflowing bounded target storage.
        // Scope: construct a two-slot snapshot from three active CPUs.
        // Semantics: capacity exhaustion is reported before any truncated topology can escape.
        assert_eq!(
            TopologySnapshot::<2>::from_active(&[CpuId::new(1), CpuId::new(2), CpuId::new(3),]),
            Err(CpuTopologyError::TooManyCpus)
        );
    }

    #[test]
    fn all_active_expands_to_snapshot_cpus() {
        // Goal: CpuSet::AllActive is anchored to topology evidence, not a CPU 0 shortcut.
        // Scope: expand AllActive against a three-CPU snapshot.
        // Semantics: the target set contains exactly the active CPUs from the snapshot.
        let topology =
            TopologySnapshot::<4>::from_active(&[CpuId::new(2), CpuId::new(4), CpuId::new(6)])
                .unwrap();

        let target = topology.expand(CpuSet::AllActive).unwrap();

        assert_eq!(target.count(), 3);
        assert!(target.contains(CpuId::new(2)));
        assert!(target.contains(CpuId::new(4)));
        assert!(target.contains(CpuId::new(6)));
        assert!(!target.contains(CpuId::new(0)));
    }

    #[test]
    fn one_cpu_target_requires_active_cpu() {
        // Goal: single-CPU targets are validated against active topology.
        // Scope: expand One for present and absent CPUs.
        // Semantics: present CPUs produce a one-entry target; absent CPUs fail before dispatch.
        let topology = TopologySnapshot::<4>::from_active(&[CpuId::new(3), CpuId::new(5)]).unwrap();

        let target = topology.expand(CpuSet::One(CpuId::new(5))).unwrap();
        assert_eq!(target.count(), 1);
        assert!(target.contains(CpuId::new(5)));
        assert!(!target.contains(CpuId::new(3)));

        assert_eq!(
            topology.expand(CpuSet::One(CpuId::new(7))),
            Err(CpuTopologyError::UnknownCpu)
        );
    }
}
