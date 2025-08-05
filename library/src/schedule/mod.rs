use std::{fmt::Debug, hash::Hash};

use bevy::ecs::{
    change_detection::Res,
    schedule::{
        Chain, InternedSystemSet, IntoScheduleConfigs, IntoSystemSet, Schedulable, Schedule,
        ScheduleConfigTupleMarker, ScheduleConfigs, ScheduleLabel, SystemCondition, SystemSet,
        graph::GraphInfo,
    },
    system::{IntoSystem, ScheduleSystem},
};

use variadics_please::all_tuples;

use crate::meta::RevMeta;
use condition::into_rev_condition;
use system::into_rev_system;

mod condition;
mod system;

#[cfg(test)]
mod test;

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

/// Contains a forward and a backward set that run depending on the current [`RevDirection`] in [`RevMeta`].
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct RevSystems;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`FwdArcSet`]s.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct ForwardSystems;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`BwdCmdArcSet`]s in reverse order.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BackwardSystems;

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

pub trait RevSchedule {
    fn rev_add_systems<Marker>(
        &mut self,
        systems: impl IntoRevScheduleConfigs<ScheduleSystem, Marker>,
    ) -> &mut Self;
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevScheduleConfigs<InternedSystemSet, Marker>,
    ) -> &mut Self;
}

impl RevSchedule for Schedule {
    fn rev_add_systems<Marker>(
        &mut self,
        systems: impl IntoRevScheduleConfigs<ScheduleSystem, Marker>,
    ) -> &mut Self {
        set_base_sets(self);
        let configs = systems.into_rev_configs();
        self.add_systems((
            configs.forward_systems,
            configs.backward_commands,
            configs.backward_systems,
        ));
        self.configure_sets((configs.backward_commands_systems, configs.unified));
        self
    }
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevScheduleConfigs<InternedSystemSet, Marker>,
    ) -> &mut Self {
        set_base_sets(self);
        let configs = sets.into_rev_configs();
        self.configure_sets((
            configs.forward_systems,
            configs.backward_commands,
            configs.backward_systems,
            configs.backward_commands_systems,
            configs.unified,
        ));
        self
    }
}

fn set_base_sets(schedule: &mut Schedule) {
    fn is_forward<const TRUTHY: bool>(meta: Option<Res<RevMeta>>) -> bool {
        meta.and_then(|meta| meta.get_running_direction())
            .is_some_and(|direction| direction.is_forward() == TRUTHY)
    }

    // check needs to be on a non-pub set
    if !schedule.graph().system_sets.contains(ForwardSystems) {
        schedule.configure_sets(
            (
                ForwardSystems.run_if(is_forward::<true>),
                BackwardSystems.run_if(is_forward::<false>),
            )
                .chain() // todo: remove chain to reduce sync points
                .in_set(RevSystems),
        );
    }
}

pub struct RevScheduleConfigs<T: Schedulable> {
    /// contains the ArcSystems for the forward set
    forward_systems: ScheduleConfigs<T>,
    /// contains the BackwardCommands for the backward set
    backward_commands: ScheduleConfigs<T>,
    /// contains the ArcSystems for the backward set
    backward_systems: ScheduleConfigs<T>,
    /// contains the sets that unify the two backward systems
    backward_commands_systems: ScheduleConfigs<InternedSystemSet>,
    /// contains the sets that unify all three systems
    unified: ScheduleConfigs<InternedSystemSet>,
}

pub trait IntoRevScheduleConfigs<
    T: Schedulable<Metadata = GraphInfo, GroupMetadata = Chain>,
    Marker,
>: Sized
{
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevScheduleConfigs<T>;
    fn rev_in_set(self, set: impl SystemSet) -> RevScheduleConfigs<T> {
        let mut configs = self.into_rev_configs();
        configs.rev_in_set_inner(set.intern());
        configs
    }
    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys A -> sync -> sys B -> sync
        // Backward
        //  cmd B -> sync -> sys B -> cmd A -> sync -> sys A
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.before(FwdSysSet(set));
        configs.backward_commands_systems = configs
            .backward_commands_systems
            .after_ignore_deferred(BwdCmdSysSet(set));
        configs
    }
    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys B -> sync -> sys A -> sync
        // Backward
        //  cmd A -> sync -> sys A -> cmd B -> sync -> sys B
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.after(FwdSysSet(set));
        configs.backward_commands_systems = configs
            .backward_commands_systems
            .before_ignore_deferred(BwdCmdSysSet(set));
        configs
    }
    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys A -> sys B -> sync
        // Backward
        //  cmd B -> cmd A -> sync -> sys B -> sys A
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs
            .forward_systems
            .before_ignore_deferred(FwdSysSet(set));
        configs.backward_commands = configs
            .backward_commands
            .after_ignore_deferred(BwdCmdSet(set));
        configs.backward_systems = configs
            .backward_systems
            .after_ignore_deferred(BwdSysSet(set));
        configs
    }
    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys B -> sys A -> sync
        // Backward
        //  cmd A -> cmd B -> sync -> sys A -> sys B
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs
            .forward_systems
            .after_ignore_deferred(FwdSysSet(set));
        configs.backward_commands = configs
            .backward_commands
            .before_ignore_deferred(BwdCmdSet(set));
        configs.backward_systems = configs
            .backward_systems
            .before_ignore_deferred(BwdSysSet(set));
        configs
    }
    fn rev_run_if<M>(self, condition: impl SystemCondition<M>) -> RevScheduleConfigs<T> {
        let mut configs = self.into_rev_configs();
        configs.unified.run_if_dyn(into_rev_condition(condition));
        configs
    }
    fn rev_distributive_run_if<M>(
        self,
        condition: impl SystemCondition<M> + Clone,
    ) -> RevScheduleConfigs<T> {
        fn distribute<M>(
            unified: &mut ScheduleConfigs<InternedSystemSet>,
            condition: impl SystemCondition<M> + Clone,
        ) {
            match unified {
                ScheduleConfigs::ScheduleConfig(_) => {
                    unified.run_if_dyn(into_rev_condition(condition));
                }
                ScheduleConfigs::Configs { configs, .. } => {
                    for config in configs {
                        distribute(config, condition.clone());
                    }
                }
            }
        }

        let mut configs = self.into_rev_configs();
        distribute(&mut configs.unified, condition);
        configs
    }
    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.ambiguous_with(FwdSysSet(set));
        configs.backward_systems = configs.backward_systems.ambiguous_with(BwdSysSet(set));
        configs
    }
    fn rev_ambiguous_with_all(self) -> RevScheduleConfigs<T> {
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.ambiguous_with_all();
        configs.backward_systems = configs.backward_systems.ambiguous_with_all();
        configs
    }
    fn rev_chain(self) -> RevScheduleConfigs<T> {
        // Example for systems A and B in self:
        // Forward
        //  sys A -> sync -> sys B -> sync
        // Backward
        //  cmd B -> sync -> sys B -> cmd A -> sync -> sys A
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.chain();
        configs.backward_commands_systems =
            configs.backward_commands_systems.chain_ignore_deferred();
        configs
    }
    fn rev_chain_ignore_deferred(self) -> RevScheduleConfigs<T> {
        // Example for systems A and B in self:
        // Forward
        //  sys A -> sys B -> sync
        // Backward
        //  cmd B -> cmd A -> sync -> sys B -> sys A
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.chain_ignore_deferred();
        configs.backward_commands = configs.backward_commands.chain_ignore_deferred();
        configs.backward_systems = configs.backward_systems.chain_ignore_deferred();
        configs
    }
}

impl<T: Schedulable<Metadata = GraphInfo, GroupMetadata = Chain>> IntoRevScheduleConfigs<T, ()>
    for RevScheduleConfigs<T>
{
    fn into_rev_configs(self) -> RevScheduleConfigs<T> {
        self
    }
}

impl<S: SystemSet> IntoRevScheduleConfigs<InternedSystemSet, ()> for S {
    fn into_rev_configs(self) -> RevScheduleConfigs<InternedSystemSet> {
        let set = self.intern();
        RevScheduleConfigs {
            forward_systems: FwdSysSet(set).in_set(set),
            backward_commands: BwdCmdSet(set).into_configs(),
            backward_systems: BwdSysSet(set).into_configs(),
            backward_commands_systems: BwdCmdSysSet(set).in_set(set),
            unified: set.into_configs(),
        }
    }
}

impl<F, Marker> IntoRevScheduleConfigs<ScheduleSystem, (Marker,)> for F
where
    F: IntoSystem<(), (), Marker>,
{
    fn into_rev_configs(self) -> RevScheduleConfigs<ScheduleSystem> {
        into_rev_system(self)
    }
}

impl<T: Schedulable<Metadata = GraphInfo, GroupMetadata = Chain>> RevScheduleConfigs<T> {
    pub fn rev_in_set_inner(&mut self, set: InternedSystemSet) {
        self.forward_systems.in_set_inner(FwdSysSet(set).intern());
        self.backward_commands.in_set_inner(BwdCmdSet(set).intern());
        self.backward_systems.in_set_inner(BwdSysSet(set).intern());
        self.backward_commands_systems
            .in_set_inner(BwdCmdSysSet(set).intern());
    }
    fn split(self) -> (ForwardConfigs<T>, BackwardConfigs<T>) {
        (
            ForwardConfigs {
                forward_systems: self.forward_systems,
                unified: self.unified,
            },
            BackwardConfigs {
                backward_commands: self.backward_commands,
                backward_systems: self.backward_systems,
                backward_commands_systems: self.backward_commands_systems,
            },
        )
    }
}

struct ForwardConfigs<T: Schedulable> {
    forward_systems: ScheduleConfigs<T>,
    unified: ScheduleConfigs<InternedSystemSet>,
}

struct BackwardConfigs<T: Schedulable> {
    backward_commands: ScheduleConfigs<T>,
    backward_systems: ScheduleConfigs<T>,
    backward_commands_systems: ScheduleConfigs<InternedSystemSet>,
}

macro_rules! impl_into_rev_schedule_configs {
    ($(($T: ident, $M: ident, $var: ident)),*) => {
        impl<S, $($T, $M),*> IntoRevScheduleConfigs<S, (ScheduleConfigTupleMarker, $($M,)*)> for ($($T,)*)
        where
            S: Schedulable<Metadata = GraphInfo, GroupMetadata = Chain>,
            $($T: IntoRevScheduleConfigs<S, $M>,)*
        {
            fn into_rev_configs(self) -> RevScheduleConfigs<S> {
                // let (var0, ..., varN)
                //  : (impl IntoRevScheduleConfigs, ..., impl IntoRevScheduleConfigs)
                //  = self;
                let ($($var,)*) = self;

                // let (var0, ..., varN)
                //  : ((ForwardConfigs, BackwardConfigs), ..., (ForwardConfigs, BackwardConfigs))
                //  = (var0.into_rev_configs().split(), ..., varN.into_rev_configs().split());
                let ($($var,)*) = ($($var.into_rev_configs().split(),)*);

                let forward_systems = ($($var.0.forward_systems,)*).into_configs();
                let unified = ($($var.0.unified,)*).into_configs();

                // let [var0, ..., varN]
                //  : [BackwardConfigs, ..., BackwardConfigs]
                //  = [varN.1, ..., var0.1];
                let mut backward_configs = [$($var.1,)*];
                backward_configs.reverse();
                let [$($var,)*] = backward_configs;

                let backward_commands = ($($var.backward_commands,)*).into_configs();
                let backward_systems = ($($var.backward_systems,)*).into_configs();
                let backward_commands_systems = ($($var.backward_commands_systems,)*).into_configs();

                RevScheduleConfigs {
                    forward_systems,
                    backward_commands,
                    backward_systems,
                    backward_commands_systems,
                    unified
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_schedule_configs, 1, 20, T, M, var);

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
