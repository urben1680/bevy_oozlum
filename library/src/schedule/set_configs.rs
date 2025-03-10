use std::sync::atomic::{AtomicU32, Ordering};

use bevy::ecs::schedule::{
    Condition, IntoSystemSet, IntoSystemSetConfigs, SystemSet, SystemSetConfigs,
};

use variadics_please::all_tuples;

use super::{
    condition::add_condition, BackwardSet, BwdCmdSet, BwdCmdSysSet, BwdSysSet, ForwardSet,
    FwdSysSet,
};

pub struct RevSystemSetConfigs {
    /// Contains [`FwdArcSet`]s for each system for the [`super::ForwardSet`].
    pub(crate) fwd_sys_sets: SystemSetConfigs,
    /// todo
    pub(crate) bwd_cmd_sets: SystemSetConfigs,
    /// Contains [`BwdArcSet`]s which are subsets of [`BwdArcCmdSet`] with only each `bwd_arc`.
    pub(crate) bwd_sys_sets: SystemSetConfigs,
    /// todo
    pub(crate) bwd_cmd_sys_sets: SystemSetConfigs,
    /// todo
    pub(crate) condition_sets: SystemSetConfigs,
}

impl RevSystemSetConfigs {
    pub(super) fn rev_run_if_inner<M>(&mut self, condition: impl Condition<M>) {
        let set = add_condition(&mut self.condition_sets, condition);
        self.fwd_sys_sets.in_set_inner(set);
        self.bwd_cmd_sys_sets.in_set_inner(set);
    }
}

pub trait IntoRevSystemSetConfigs<Marker>: Sized {
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevSystemSetConfigs;
    fn rev_in_set(self, set: impl SystemSet) -> RevSystemSetConfigs {
        let set = set.intern();
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.in_set(FwdSysSet(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets.in_set(BwdCmdSet(set)),
            bwd_sys_sets: configs.bwd_sys_sets.in_set(BwdSysSet(set)),
            bwd_cmd_sys_sets: configs.bwd_cmd_sys_sets.in_set(BwdCmdSysSet(set)),
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys A -> sync -> sys B -> sync
        // Backward
        //  cmd B -> sync -> sys B -> cmd A -> sync -> sys A
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.before(FwdSysSet(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets,
            bwd_sys_sets: configs.bwd_sys_sets,
            bwd_cmd_sys_sets: configs
                .bwd_cmd_sys_sets
                .after_ignore_deferred(BwdCmdSysSet(set)),
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys B -> sync -> sys A -> sync
        // Backward
        //  cmd A -> sync -> sys A -> cmd B -> sync -> sys B
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.after(FwdSysSet(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets,
            bwd_sys_sets: configs.bwd_sys_sets,
            bwd_cmd_sys_sets: configs
                .bwd_cmd_sys_sets
                .before_ignore_deferred(BwdCmdSysSet(set)),
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys A -> sys B -> sync
        // Backward
        //  cmd B -> cmd A -> sync -> sys B -> sys A
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.before_ignore_deferred(FwdSysSet(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets.after_ignore_deferred(BwdCmdSet(set)),
            bwd_sys_sets: configs.bwd_sys_sets.after_ignore_deferred(BwdSysSet(set)),
            bwd_cmd_sys_sets: configs.bwd_cmd_sys_sets,
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        // Example for a system A in self and a system B in set:
        // Forward
        //  sys B -> sys A -> sync
        // Backward
        //  cmd A -> cmd B -> sync -> sys A -> sys B
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.after_ignore_deferred(FwdSysSet(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets.before_ignore_deferred(BwdCmdSet(set)),
            bwd_sys_sets: configs.bwd_sys_sets.before_ignore_deferred(BwdSysSet(set)),
            bwd_cmd_sys_sets: configs.bwd_cmd_sys_sets,
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_run_if<M>(self, condition: impl Condition<M>) -> RevSystemSetConfigs {
        let mut configs = self.into_rev_configs();
        configs.rev_run_if_inner(condition);
        configs
    }
    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.ambiguous_with(FwdSysSet(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets, // bwd_cmd have no accesses that could be ambigious
            bwd_sys_sets: configs.bwd_sys_sets.ambiguous_with(BwdSysSet(set)),
            bwd_cmd_sys_sets: configs.bwd_cmd_sys_sets, // adding config would be redundant
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_ambiguous_with_all(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.ambiguous_with_all(),
            bwd_cmd_sets: configs.bwd_cmd_sets, // bwd_cmd have no accesses that could be ambigious
            bwd_sys_sets: configs.bwd_sys_sets.ambiguous_with_all(),
            bwd_cmd_sys_sets: configs.bwd_cmd_sys_sets, // adding config would be redundant
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_chain(self) -> RevSystemSetConfigs {
        // Example for systems A, B and C in self:
        // Forward
        //  sys A -> sync -> sys B -> sync -> sys C -> sync
        // Backward
        //  cmd C -> sync -> sys C -> cmd B -> sync -> sys B -> cmd A -> sync -> sys A
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.chain(),
            bwd_cmd_sets: configs.bwd_cmd_sets,
            bwd_sys_sets: configs.bwd_sys_sets,
            bwd_cmd_sys_sets: configs.bwd_cmd_sys_sets.chain_ignore_deferred(),
            condition_sets: configs.condition_sets,
        }
    }
    fn rev_chain_ignore_deferred(self) -> RevSystemSetConfigs {
        // Example for systems A, B and C in self:
        // Forward
        //  sys A -> sys B -> sys C -> sync
        // Backward
        //  cmd C -> cmd B -> cmd A -> sync -> sys C -> sys B -> sys A
        let configs = self.into_rev_configs();
        #[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
        struct ChainIgnoreDeferred(u32);
        static ID: AtomicU32 = AtomicU32::new(0);
        let set = ChainIgnoreDeferred(ID.fetch_add(1, Ordering::Relaxed)).intern();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.chain_ignore_deferred(),
            bwd_cmd_sets: configs.bwd_cmd_sets.chain_ignore_deferred().in_set(set),
            bwd_sys_sets: configs.bwd_sys_sets.chain_ignore_deferred().after(set),
            bwd_cmd_sys_sets: configs.bwd_cmd_sys_sets,
            condition_sets: (configs.condition_sets, set.in_set(BackwardSet)).into_configs(), // todo rename field
        }
    }
}

impl IntoRevSystemSetConfigs<()> for RevSystemSetConfigs {
    fn into_rev_configs(self) -> RevSystemSetConfigs {
        self
    }
}

impl<S: SystemSet> IntoRevSystemSetConfigs<()> for S {
    fn into_rev_configs(self) -> RevSystemSetConfigs {
        let set = self.intern();
        RevSystemSetConfigs {
            fwd_sys_sets: FwdSysSet(set).into_configs(),
            bwd_cmd_sets: BwdCmdSet(set).into_configs(),
            bwd_sys_sets: BwdSysSet(set).into_configs(),
            bwd_cmd_sys_sets: BwdCmdSysSet(set).into_configs(),
            condition_sets: ForwardSet.into_configs(),
        }
    }
}

impl RevSystemSetConfigs {
    /// Split configs to be more readable in impl_into_rev_set_configs! and as partially movable as nested tuples.
    fn split(self) -> (ForwardSetConfig, BackwardSetConfigs) {
        (
            ForwardSetConfig {
                fwd_sys_sets: self.fwd_sys_sets,
                condition_sets: self.condition_sets,
            },
            BackwardSetConfigs {
                bwd_cmd_sets: self.bwd_cmd_sets,
                bwd_sys_sets: self.bwd_sys_sets,
                bwd_cmd_sys_sets: self.bwd_cmd_sys_sets,
            },
        )
    }
}

struct ForwardSetConfig {
    fwd_sys_sets: SystemSetConfigs,
    condition_sets: SystemSetConfigs,
}

struct BackwardSetConfigs {
    bwd_cmd_sets: SystemSetConfigs,
    bwd_sys_sets: SystemSetConfigs,
    bwd_cmd_sys_sets: SystemSetConfigs,
}

macro_rules! impl_into_rev_set_configs {
    ($(($T: ident, $M: ident, $var: ident)),*) => {
        impl<$($T, $M),*> IntoRevSystemSetConfigs<($($M,)*)> for ($($T,)*)
        where
            $($T: IntoRevSystemSetConfigs<$M>,)*
        {
            fn into_rev_configs(self) -> RevSystemSetConfigs {
                // let (var0, ..., varN)
                //  : (impl IntoRevSystemSetConfigs, ..., impl IntoRevSystemSetConfigs)
                //  = self;
                let ($($var,)*) = self;

                // let (var0, ..., varN)
                //  : ((ForwardSetConfig, BackwardSetConfigs), ..., (ForwardSetConfig, BackwardSetConfigs))
                //  = (var0.into_rev_configs().split(), ..., varN.into_rev_configs().split());
                let ($($var,)*) = ($($var.into_rev_configs().split(),)*);

                let fwd_sys_sets = ($($var.0.fwd_sys_sets,)*).into_configs();

                let condition_sets = ($($var.0.condition_sets,)*).into_configs();

                // let [var0, ..., varN]
                //  : [BackwardSetConfigs, ..., BackwardSetConfigs]
                //  = [varN.1, ..., var0.1];
                let mut arr = [$($var.1,)*];
                arr.reverse();
                let [$($var,)*] = arr;

                let bwd_cmd_sets = ($($var.bwd_cmd_sets,)*).into_configs();

                let bwd_sys_sets = ($($var.bwd_sys_sets,)*).into_configs();

                let bwd_cmd_sys_sets = ($($var.bwd_cmd_sys_sets,)*).into_configs();

                RevSystemSetConfigs {
                    fwd_sys_sets,
                    bwd_cmd_sets,
                    bwd_sys_sets,
                    bwd_cmd_sys_sets,
                    condition_sets
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_set_configs, 1, 20, T, M, var);
