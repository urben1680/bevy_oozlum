use std::fmt::Debug;

use bevy::{
    ecs::{
        change_detection::Res,
        schedule::{InternedSystemSet, IntoSystemSetConfigs, Schedule, ScheduleLabel, SystemSet},
    },
    prelude::IntoSystemSet,
};

use crate::meta::RevMeta;

mod condition;
mod set_configs;
mod system;
mod system_configs;

#[cfg(test)]
mod test;

pub use set_configs::*;
pub use system_configs::*;

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

/// Contains a forward and a backward set that run depending on the current [`RevDirection`] in [`RevMeta`].
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct RevSystemsSet;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`FwdArcSet`]s.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct ForwardSet;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`BwdCmdArcSet`]s in reverse order.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BackwardSet;

/// Subsets of [`ForwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct FwdSysSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// todo
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdCmdSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdSysSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdCmdSysSet(InternedSystemSet);

// todo: warn about cases where a system is actually a reversible system despite having no logic in the backward direction
// - triggers reversible commands
// - triggers commands that trigger reversible hooks/observers
pub fn forward_set<M>(into_set: impl IntoSystemSet<M>) -> impl SystemSet {
    FwdSysSet(into_set.into_system_set().intern())
}

pub trait RevSchedule {
    fn rev_add_systems<Marker>(&mut self, systems: impl IntoRevSystemConfigs<Marker>) -> &mut Self;
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self;
}

impl RevSchedule for Schedule {
    fn rev_add_systems<Marker>(&mut self, systems: impl IntoRevSystemConfigs<Marker>) -> &mut Self {
        let RevSystemConfigs { systems, sets } = systems.into_rev_configs();
        // configure sets first because that adds the base configs for the base sets
        self.rev_configure_sets(sets).add_systems(systems)
    }
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self {
        // check needs to be on a non-pub set that is also not used by the `forward_set` function
        if !self.graph().contains_set(ForwardSet) {
            fn is_forward<const TRUE: bool>(meta: Option<Res<RevMeta>>) -> bool {
                meta.and_then(|meta| meta.get_direction())
                    .is_some_and(|direction| direction.is_forward() == TRUE)
            }
            self.configure_sets(
                (
                    ForwardSet.run_if(is_forward::<true>),
                    BackwardSet.run_if(is_forward::<false>),
                )
                    .chain() // todo: remove chain to reduce sync points
                    .in_set(RevSystemsSet),
            );
        }
        let RevSystemSetConfigs {
            fwd_sys_sets,
            bwd_cmd_sets,
            bwd_sys_sets,
            bwd_cmd_sys_sets,
            condition_sets,
        } = sets.into_rev_configs();
        self.configure_sets((
            fwd_sys_sets,
            bwd_cmd_sets,
            bwd_sys_sets,
            bwd_cmd_sys_sets,
            condition_sets,
        ))
    }
}

macro_rules! error_per_flag {
    ($flag:expr, $($arg:tt)+) => ({
        if !*$flag {
            bevy::log::error!($($arg)+);
            *$flag = true;
        }
        core::default::Default::default()
    });
}

use error_per_flag;
