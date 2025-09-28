use crate::meta::RevMeta;
use bevy_ecs::{
    change_detection::Res,
    schedule::{
        Chain, InternedSystemSet, IntoScheduleConfigs, IntoSystemSet, Schedulable, Schedule,
        ScheduleConfigTupleMarker, ScheduleConfigs, ScheduleLabel, SystemCondition, SystemSet,
        graph::GraphInfo,
    },
    system::{IntoSystem, ScheduleSystem},
};
use condition::into_rev_condition;
use core::{fmt::Debug, hash::Hash};
use system::into_rev_system;
use variadics_please::all_tuples;

mod condition;
mod system;

#[cfg(test)]
mod test;

/// The schedule that is run by [`RevMeta::run_rev_update`]. All reversible systems go in here,
/// directly or indirectly in schedules that are run within this.
///
/// Reversible systems in a schedule are automatically added to the [`RevSystems`] set so other,
/// non-reversible systems can be ordered to them while ignoring the reversed order of
/// [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog).
#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

/// Contains a forward and a backward set that run depending on the current
/// [`RevMeta::running_direction`].
///
/// Do not use this set when using the `rev_*` configuration methods. This is intended for use in
/// the regular bevy configuraton methods.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct RevSystems;

/// Subset of [`RevSystems`].
///
/// Contains all [`RevSystem::<T, true>`](system::RevSystem).
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct ForwardSystems;

/// Subset of [`RevSystems`].
///
/// Contains all [`BackwardDeferred`](system::BackwardDeferred) and
/// [`RevSystem::<T, false>`](system::RevSystem).
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BackwardSystems;

/// Subsets of [`RevSystems`].
///
/// Each value of this set contains the specific [`RevSystem::<T, true>`](system::RevSystem) of a
/// system.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct ForwardSystemSet(InternedSystemSet);

/// Subsets of [`RevSystems`].
///
/// Each value of this set contains the specific [`BackwardDeferred`](system::BackwardDeferred) of a
/// system.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BackwardDeferredSet(InternedSystemSet);

/// Subsets of [`RevSystems`].
///
/// Each value of this set contains the specific [`RevSystem::<T, false>`](system::RevSystem) of a
/// system.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BackwardSystemSet(InternedSystemSet);

/// Subsets of [`RevSystems`].
///
/// Each value of this set contains the specific [`BackwardDeferred`](system::BackwardDeferred) and
/// [`RevSystem::<T, false>`](system::RevSystem) of a system.
#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct BackwardDeferredAndSystemSet(InternedSystemSet);

/// Extension trait for [`Schedule`] for adding reversible systems and configurations.
pub trait RevSchedule {
    /// Reversible version of [`Schedule::add_systems`].
    ///
    /// This wraps every passed system `T` in an `Arc<Mutex<T>>` that is shared for:
    /// - A new system `F` that runs at [`RevDirection::Forward`]
    /// - A new system `B` that runs at [`RevDirection::BackwardLog`]
    ///
    /// Systems can read the direction value from [`RevMeta::running_direction`].
    ///
    /// Additionally, another new system per `T` is added that runs at [`RevDirection::BackwardLog`]
    /// but before `B` which undoes deferred actions such as commands. This way, `B` will start with
    /// the [`World`] state that was present when `F` finished but did not have its deferred actions
    /// applied yet.
    ///
    /// This third system is noop for exclusive systems or when `T` has no such deferred parameters
    /// like [`Commands`].
    ///
    /// Configurations that order the systems will be reversed for the `B` variants.
    ///
    /// [`RevDirection::Forward`]: crate::meta::RevDirection::Forward
    /// [`RevDirection::BackwardLog`]: crate::meta::RevDirection::BackwardLog
    /// [`World`]: bevy_ecs::world::World
    /// [`Commands`]: bevy_ecs::system::Commands
    fn rev_add_systems<Marker>(
        &mut self,
        systems: impl IntoRevScheduleConfigs<ScheduleSystem, Marker>,
    ) -> &mut Self;

    /// Reversible version of [`Schedule::configure_sets`].
    ///
    /// Configurations that order the systems or sets will be reversed for when
    /// [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) is
    /// [running](`RevMeta::running_direction`).
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
            configs.backward_deferred,
            configs.backward_systems,
        ));
        self.configure_sets((configs.backward_deferred_and_systems, configs.conditioned));
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
            configs.backward_deferred,
            configs.backward_systems,
            configs.backward_deferred_and_systems,
            configs.conditioned,
        ));
        self
    }
}

fn set_base_sets(schedule: &mut Schedule) {
    fn is_forward<const TRUTHY: bool>(meta: Option<Res<RevMeta>>) -> bool {
        meta.and_then(|meta| meta.get_running_direction())
            .is_some_and(|direction| direction.is_forward() == TRUTHY)
    }

    // check needs to be on a non-pub set so user code cannot make this unreliable
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

/// Reversible variant of [`ScheduleConfigs<T>`].
pub struct RevScheduleConfigs<T: Schedulable> {
    /// Configurations of [`RevSystem::<_, true>`](system::RevSystem)s or
    /// [`ForwardSystemSet`]s depending on `T`.
    forward_systems: ScheduleConfigs<T>,

    /// Configurations of [`BackwardDeferred`](system::BackwardDeferred)s or
    /// [`BackwardDeferredSet`]s depending on `T`.
    backward_deferred: ScheduleConfigs<T>,

    /// Configurations of [`RevSystem::<_, false>`](system::RevSystem)s or
    /// [`BackwardSystemSet`]s depending on `T`.
    backward_systems: ScheduleConfigs<T>,

    /// Configurations of [`BackwardDeferredAndSystemSet`]s.
    backward_deferred_and_systems: ScheduleConfigs<InternedSystemSet>,

    /// Contains [`AtomicSet`](system::AtomicSet)s and default sets of a reversible system or custom
    /// sets depending on `T`.
    ///
    /// Used for run conditions.
    conditioned: ScheduleConfigs<InternedSystemSet>,
}

/// Reversible variant of [`IntoScheduleConfigs`].
pub trait IntoRevScheduleConfigs<
    T: Schedulable<Metadata = GraphInfo, GroupMetadata = Chain>,
    Marker,
>: Sized
{
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevScheduleConfigs<T>;

    /// Reversible variant of [`IntoScheduleConfigs::in_set`].
    fn rev_in_set(self, set: impl SystemSet) -> RevScheduleConfigs<T> {
        let mut configs = self.into_rev_configs();
        configs.rev_in_set_inner(set.intern());
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::before`].
    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        //
        // Forward
        //  system A -> sync -> system B -> sync
        // Backward
        //  deferred B -> sync -> system B -> deferred A -> sync -> system A
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.before(ForwardSystemSet(set));
        configs.backward_deferred_and_systems = configs
            .backward_deferred_and_systems
            .after_ignore_deferred(BackwardDeferredAndSystemSet(set));
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::after`].
    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        //
        // Forward
        //  system B -> sync -> system A -> sync
        // Backward
        //  deferred A -> sync -> system A -> deferred B -> sync -> system B
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.after(ForwardSystemSet(set));
        configs.backward_deferred_and_systems = configs
            .backward_deferred_and_systems
            .before_ignore_deferred(BackwardDeferredAndSystemSet(set));
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::before_ignore_deferred`].
    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        //
        // Forward
        //  system A -> system B -> sync
        // Backward
        //  deferred B -> deferred A -> sync -> system B -> system A
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs
            .forward_systems
            .before_ignore_deferred(ForwardSystemSet(set));
        configs.backward_deferred = configs
            .backward_deferred
            .after_ignore_deferred(BackwardDeferredSet(set));
        configs.backward_systems = configs
            .backward_systems
            .after_ignore_deferred(BackwardSystemSet(set));
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::after_ignore_deferred`].
    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        // Example for a system A in self and a system B in set:
        //
        // Forward
        //  system B -> system A -> sync
        // Backward
        //  deferred A -> deferred B -> sync -> system A -> system B
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs
            .forward_systems
            .after_ignore_deferred(ForwardSystemSet(set));
        configs.backward_deferred = configs
            .backward_deferred
            .before_ignore_deferred(BackwardDeferredSet(set));
        configs.backward_systems = configs
            .backward_systems
            .before_ignore_deferred(BackwardSystemSet(set));
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::run_if`].
    fn rev_run_if<M>(self, condition: impl SystemCondition<M>) -> RevScheduleConfigs<T> {
        let mut configs = self.into_rev_configs();
        configs
            .conditioned
            .run_if_dyn(into_rev_condition(condition));
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::distributive_run_if`].
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
        distribute(&mut configs.conditioned, condition);
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::ambiguous_with`].
    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevScheduleConfigs<T> {
        let set = set.into_system_set().intern();
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs
            .forward_systems
            .ambiguous_with(ForwardSystemSet(set));
        configs.backward_systems = configs
            .backward_systems
            .ambiguous_with(BackwardSystemSet(set));
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::ambiguous_with_all`].
    fn rev_ambiguous_with_all(self) -> RevScheduleConfigs<T> {
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.ambiguous_with_all();
        configs.backward_systems = configs.backward_systems.ambiguous_with_all();
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::chain`].
    fn rev_chain(self) -> RevScheduleConfigs<T> {
        // Example for systems A and B in self:
        //
        // Forward
        //  system A -> sync -> system B -> sync
        // Backward
        //  deferred B -> sync -> system B -> deferred A -> sync -> system A
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.chain();
        configs.backward_deferred_and_systems = configs
            .backward_deferred_and_systems
            .chain_ignore_deferred();
        configs
    }

    /// Reversible variant of [`IntoScheduleConfigs::chain_ignore_deferred`].
    fn rev_chain_ignore_deferred(self) -> RevScheduleConfigs<T> {
        // Example for systems A and B in self:
        //
        // Forward
        //  system A -> system B -> sync
        // Backward
        //  deferred B -> deferred A -> sync -> system B -> system A
        let mut configs = self.into_rev_configs();
        configs.forward_systems = configs.forward_systems.chain_ignore_deferred();
        configs.backward_deferred = configs.backward_deferred.chain_ignore_deferred();
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
            forward_systems: ForwardSystemSet(set).in_set(set),
            backward_deferred: BackwardDeferredSet(set).in_set(set),
            backward_systems: BackwardSystemSet(set).in_set(set),
            backward_deferred_and_systems: BackwardDeferredAndSystemSet(set).in_set(set),
            conditioned: set.into_configs(),
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
    /// Reversible variant of [`ScheduleConfigs::in_set_inner`].
    pub fn rev_in_set_inner(&mut self, set: InternedSystemSet) {
        self.forward_systems
            .in_set_inner(ForwardSystemSet(set).intern());
        self.backward_deferred
            .in_set_inner(BackwardDeferredSet(set).intern());
        self.backward_systems
            .in_set_inner(BackwardSystemSet(set).intern());
        self.backward_deferred_and_systems
            .in_set_inner(BackwardDeferredAndSystemSet(set).intern());
    }

    /// Split configuration for [`impl_into_rev_schedule_configs`].
    fn split(self) -> (ForwardConfigs<T>, BackwardConfigs<T>) {
        (
            ForwardConfigs {
                forward_systems: self.forward_systems,
                conditioned: self.conditioned,
            },
            BackwardConfigs {
                backward_deferred: self.backward_deferred,
                backward_systems: self.backward_systems,
                backward_deferred_and_systems: self.backward_deferred_and_systems,
            },
        )
    }
}

struct ForwardConfigs<T: Schedulable> {
    forward_systems: ScheduleConfigs<T>,

    /// Not strongly required to be here and not in [`BackwardConfigs`].
    conditioned: ScheduleConfigs<InternedSystemSet>,
}

struct BackwardConfigs<T: Schedulable> {
    backward_deferred: ScheduleConfigs<T>,
    backward_systems: ScheduleConfigs<T>,
    backward_deferred_and_systems: ScheduleConfigs<InternedSystemSet>,
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
                let conditioned = ($($var.0.conditioned,)*).into_configs();

                // let [var0, ..., varN]
                //  : [BackwardConfigs, ..., BackwardConfigs]
                //  = [varN.1, ..., var0.1];
                let mut backward_configs = [$($var.1,)*];
                backward_configs.reverse();
                let [$($var,)*] = backward_configs;

                let backward_deferred = ($($var.backward_deferred,)*).into_configs();
                let backward_systems = ($($var.backward_systems,)*).into_configs();
                let backward_deferred_and_systems = ($($var.backward_deferred_and_systems,)*).into_configs();

                RevScheduleConfigs {
                    forward_systems,
                    backward_deferred,
                    backward_systems,
                    backward_deferred_and_systems,
                    conditioned
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_schedule_configs, 1, 20, T, M, var);
