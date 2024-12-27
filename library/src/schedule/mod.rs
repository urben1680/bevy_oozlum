use std::fmt::Debug;

use bevy::ecs::schedule::{InternedSystemSet, IntoSystemSetConfigs, Schedule, SystemSet};

use crate::meta::RevDirection;

mod condition;
mod set_configs;
mod system;
mod system_configs;

#[cfg(test)]
mod test;

pub use set_configs::*;
pub use system_configs::*;

/// Contains a forward and a backward set that run depending on the current [`RevDirection`] in [`RevMeta`].
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevSystemsSet;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`FwdArcSet`]s.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSet;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`BwdCmdArcSet`]s in reverse order.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSet;

/// Subsets of [`ForwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct FwdSysSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// todo
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdCmdSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdSysSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdCmdSysSet(InternedSystemSet);

macro_rules! impl_set_debug {
    ($T: ident) => {
        impl Debug for $T {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let mut f = f.debug_tuple(std::any::type_name::<Self>());
                match self.0.system_type() {
                    None => f.field(&self.0).finish(),
                    Some(id) => f.field(&id).finish(),
                }
            }
        }
    };
}

impl_set_debug!(FwdSysSet);
impl_set_debug!(BwdCmdSet);
impl_set_debug!(BwdSysSet);
impl_set_debug!(BwdCmdSysSet);

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
        if !self.graph().contains_set(ForwardSet) {
            // run conditions return false if RevMeta is missing or not in a running RevDirection
            fn if_forward(direction: RevDirection) -> bool {
                matches!(direction, RevDirection::Forward { .. })
            }
            fn if_backward(direction: RevDirection) -> bool {
                matches!(direction, RevDirection::BackwardLog)
            }
            self.configure_sets(
                (
                    ForwardSet.run_if(if_forward),
                    BackwardSet.run_if(if_backward),
                )
                    .chain()
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
