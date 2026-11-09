//! How many compile jobs the pane shells are told to run (`[build] jobs`).
//!
//! Cargo and CMake start one job per core by default and never look at the
//! memory, so a machine with many cores and little RAM runs out of memory on
//! a full build. `auto` caps the jobs by both. Pure: the runtime measures the
//! [`Machine`] once and hands it in.

use std::num::NonZeroU16;

/// The variables a new pane shell gets, both set to the same job count.
pub const ENV_KEYS: [&str; 2] = ["CARGO_BUILD_JOBS", "CMAKE_BUILD_PARALLEL_LEVEL"];

/// The memory one compile job is given room for by `auto`.
pub const RAM_PER_JOB: u64 = 2 * 1024 * 1024 * 1024;

/// The `[build] jobs` setting: worked out from the machine, or a fixed count.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BuildJobs {
    #[default]
    Auto,
    Fixed(NonZeroU16),
}

impl BuildJobs {
    /// The value as the config file and the menu write it: `auto` or the
    /// count.
    pub fn id(self) -> String {
        match self {
            Self::Auto => "auto".to_owned(),
            Self::Fixed(n) => n.to_string(),
        }
    }

    /// The next value of the menu row: `auto`, then `1` up to the cores (or
    /// a larger fixed count from the file), then `auto` again. Back from
    /// `auto` is the top count, so the most jobs is one key away.
    pub fn step(self, machine: Machine, forward: bool) -> Self {
        let top = match self {
            Self::Fixed(n) => n.get().max(machine.cores()),
            Self::Auto => machine.cores(),
        };
        let fixed = |n: u16| NonZeroU16::new(n).map_or(Self::Auto, Self::Fixed);
        match (self, forward) {
            (Self::Auto, true) => fixed(1),
            (Self::Auto, false) => fixed(top),
            (Self::Fixed(n), true) if n.get() >= top => Self::Auto,
            (Self::Fixed(n), true) => fixed(n.get() + 1),
            (Self::Fixed(n), false) => fixed(n.get() - 1),
        }
    }
}

/// What `auto` is worked out from. `cores` is never below one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Machine {
    cores: NonZeroU16,
    /// The total memory in bytes, `None` when the system did not say.
    ram: Option<u64>,
}

impl Default for Machine {
    /// One core and unknown memory: `auto` is then one job.
    fn default() -> Self {
        Self::new(1, None)
    }
}

impl Machine {
    /// `cores` below one counts as one; above `u16::MAX` as `u16::MAX`.
    pub fn new(cores: usize, ram: Option<u64>) -> Self {
        let cores = u16::try_from(cores).unwrap_or(u16::MAX);
        Self {
            cores: NonZeroU16::new(cores).unwrap_or(NonZeroU16::MIN),
            ram,
        }
    }

    pub fn cores(self) -> u16 {
        self.cores.get()
    }

    /// The `auto` count: the cores, but no more jobs than the memory holds
    /// at [`RAM_PER_JOB`] each, and never below one. Unknown memory leaves
    /// the cores.
    pub fn auto_jobs(self) -> u16 {
        let by_ram = self.ram.map_or(u64::MAX, |ram| ram / RAM_PER_JOB);
        let jobs = u64::from(self.cores()).min(by_ram).max(1);
        u16::try_from(jobs).unwrap_or(u16::MAX)
    }

    /// The job count `setting` gives on this machine.
    pub fn jobs(self, setting: BuildJobs) -> u16 {
        match setting {
            BuildJobs::Auto => self.auto_jobs(),
            BuildJobs::Fixed(n) => n.get(),
        }
    }
}

/// The variables to set on a new pane shell for `setting`.
pub fn env(setting: BuildJobs, machine: Machine) -> [(String, String); 2] {
    let jobs = machine.jobs(setting).to_string();
    ENV_KEYS.map(|key| (key.to_owned(), jobs.clone()))
}

/// The menu text of the setting: `auto (3)` names the count `auto` gives.
pub fn label(setting: BuildJobs, machine: Machine) -> String {
    match setting {
        BuildJobs::Auto => format!("auto ({})", machine.auto_jobs()),
        BuildJobs::Fixed(n) => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn fixed(n: u16) -> BuildJobs {
        BuildJobs::Fixed(NonZeroU16::new(n).unwrap())
    }

    #[test]
    fn auto_is_capped_by_the_memory_at_two_gib_a_job() {
        // The WSL machine that ran out of memory: 16 cores, 7.6 GiB.
        let wsl = Machine::new(16, Some(7 * GIB + GIB / 2));
        assert_eq!(wsl.auto_jobs(), 3);
        assert_eq!(wsl.jobs(BuildJobs::Auto), 3);
    }

    #[test]
    fn auto_is_capped_by_the_cores_when_memory_is_plenty() {
        assert_eq!(Machine::new(8, Some(64 * GIB)).auto_jobs(), 8);
    }

    #[test]
    fn auto_never_drops_below_one_job() {
        assert_eq!(Machine::new(4, Some(GIB)).auto_jobs(), 1);
        assert_eq!(Machine::new(0, Some(0)).auto_jobs(), 1);
    }

    #[test]
    fn unknown_memory_leaves_the_cores() {
        assert_eq!(Machine::new(6, None).auto_jobs(), 6);
    }

    #[test]
    fn a_fixed_count_wins_over_the_machine() {
        let small = Machine::new(2, Some(GIB));
        assert_eq!(small.jobs(fixed(12)), 12);
    }

    #[test]
    fn env_sets_cargo_and_cmake_to_the_same_count() {
        let machine = Machine::new(16, Some(8 * GIB));
        assert_eq!(
            env(BuildJobs::Auto, machine),
            [
                ("CARGO_BUILD_JOBS".to_owned(), "4".to_owned()),
                ("CMAKE_BUILD_PARALLEL_LEVEL".to_owned(), "4".to_owned()),
            ]
        );
        assert_eq!(env(fixed(16), machine)[0].1, "16");
    }

    #[test]
    fn step_forward_runs_auto_then_one_up_to_the_cores_then_auto() {
        let machine = Machine::new(3, None);
        let mut seen = vec![BuildJobs::Auto];
        for _ in 0..4 {
            seen.push(seen.last().unwrap().step(machine, true));
        }
        assert_eq!(
            seen,
            [
                BuildJobs::Auto,
                fixed(1),
                fixed(2),
                fixed(3),
                BuildJobs::Auto
            ]
        );
    }

    #[test]
    fn step_back_from_auto_is_the_most_jobs() {
        let machine = Machine::new(16, Some(7 * GIB));
        assert_eq!(BuildJobs::Auto.step(machine, false), fixed(16));
        assert_eq!(fixed(1).step(machine, false), BuildJobs::Auto);
        assert_eq!(fixed(5).step(machine, false), fixed(4));
    }

    #[test]
    fn a_count_above_the_cores_from_the_file_stays_reachable() {
        let machine = Machine::new(4, None);
        assert_eq!(fixed(32).step(machine, false), fixed(31));
        assert_eq!(fixed(32).step(machine, true), BuildJobs::Auto);
    }

    #[test]
    fn label_names_the_count_auto_gives() {
        let machine = Machine::new(16, Some(7 * GIB + GIB / 2));
        assert_eq!(label(BuildJobs::Auto, machine), "auto (3)");
        assert_eq!(label(fixed(16), machine), "16");
        assert_eq!(BuildJobs::Auto.id(), "auto");
        assert_eq!(fixed(6).id(), "6");
    }
}
