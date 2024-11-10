use bevy::{
    ecs::schedule::{Condition, IntoSystemSet, IntoSystemSetConfigs, SystemSet, SystemSetConfigs},
    utils::all_tuples,
};
use condition::rev_condition;

use super::{BwdCmdSet, BwdNonSys, BwdSysSet, ForwardSet, FwdNonSys, FwdSysSet};

mod condition;

pub struct RevSystemSetConfigs {
    /// Contains [`FwdArcSet`]s for each system for the [`super::ForwardSet`].
    pub(crate) fwd_sys_sets: SystemSetConfigs,
    /// todo
    pub(crate) bwd_cmd_sets: SystemSetConfigs,
    /// Contains [`BwdArcSet`]s which are subsets of [`BwdArcCmdSet`] with only each `bwd_arc`.
    pub(crate) bwd_sys_sets: SystemSetConfigs,
    /// todo
    pub(crate) cond_sets: SystemSetConfigs,
}

pub trait IntoRevSystemSetConfigs<Marker>: Sized {
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevSystemSetConfigs;
    fn rev_in_set(self, set: impl SystemSet) -> RevSystemSetConfigs {
        let set = set.intern();
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.in_set(FwdNonSys(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets.in_set(BwdNonSys(set)),
            bwd_sys_sets: configs.bwd_sys_sets.in_set(BwdNonSys(set)),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // A forward sys ─> maybe sync ─> B forward sys ─> maybe sync
        //
        // B backward cmds ─> maybe sync ┬> B backward sys ─┐
        //                               └> A backward cmds ┴> maybe sync ─> A backward sys
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.before(FwdSysSet::from_set(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets.after(BwdCmdSet::from_set(set)),
            bwd_sys_sets: configs.bwd_sys_sets.after(BwdSysSet::from_set(set)),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // B forward sys ─> maybe sync ─> A forward sys ─> maybe sync
        //
        // A backward cmds ─> maybe sync ┬> A backward sys ─┐
        //                               └> B backward cmds ┴> maybe sync ─> B backward sys
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.after(FwdSysSet::from_set(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets.before(BwdCmdSet::from_set(set)),
            bwd_sys_sets: configs.bwd_sys_sets.before(BwdSysSet::from_set(set)),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // A forward sys ─> B forward sys ─> maybe sync
        //
        // B backward cmds ┬> maybe sync ─> B backward sys ─┐
        //                 └> A backward cmds ─> maybe sync ┴─> A backward sys
        RevSystemSetConfigs {
            fwd_sys_sets: configs
                .fwd_sys_sets
                .before_ignore_deferred(FwdSysSet::from_set(set)),
            bwd_cmd_sets: configs
                .bwd_cmd_sets
                .after_ignore_deferred(BwdCmdSet::from_set(set)),
            bwd_sys_sets: configs
                .bwd_sys_sets
                .after_ignore_deferred(BwdSysSet::from_set(set)),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // B forward sys ─> A forward sys ─> maybe sync
        //
        // A backward cmds ┬> maybe sync ─> A backward sys ─┐
        //                 └> B backward cmds ─> maybe sync ┴─> B backward sys
        RevSystemSetConfigs {
            fwd_sys_sets: configs
                .fwd_sys_sets
                .after_ignore_deferred(FwdSysSet::from_set(set)),
            bwd_cmd_sets: configs
                .bwd_cmd_sets
                .before_ignore_deferred(BwdCmdSet::from_set(set)),
            bwd_sys_sets: configs
                .bwd_sys_sets
                .before_ignore_deferred(BwdSysSet::from_set(set)),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_run_if<M>(self, condition: impl Condition<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let (condition, set) = rev_condition(condition);
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.in_set(set),
            bwd_cmd_sets: configs.bwd_cmd_sets.in_set(set),
            bwd_sys_sets: configs.bwd_sys_sets.in_set(set),
            cond_sets: (configs.cond_sets, set.run_if(condition)).into_configs(),
        }
    }
    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        RevSystemSetConfigs {
            fwd_sys_sets: configs
                .fwd_sys_sets
                .ambiguous_with(FwdSysSet::from_set(set)),
            bwd_cmd_sets: configs.bwd_cmd_sets, // bwd_cmd have no accesses that could be ambigious
            bwd_sys_sets: configs
                .bwd_sys_sets
                .ambiguous_with(BwdSysSet::from_set(set)),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_ambiguous_with_all(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.ambiguous_with_all(),
            bwd_cmd_sets: configs.bwd_cmd_sets, // bwd_cmd have no accesses that could be ambigious
            bwd_sys_sets: configs.bwd_sys_sets.ambiguous_with_all(),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_chain(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.chain(),
            bwd_cmd_sets: configs.bwd_cmd_sets.chain(),
            bwd_sys_sets: configs.bwd_sys_sets.chain(),
            cond_sets: configs.cond_sets,
        }
    }
    fn rev_chain_ignore_deferred(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_sys_sets: configs.fwd_sys_sets.chain_ignore_deferred(),
            bwd_cmd_sets: configs.bwd_cmd_sets.chain_ignore_deferred(),
            bwd_sys_sets: configs.bwd_sys_sets.chain_ignore_deferred(),
            cond_sets: configs.cond_sets,
        }
    }
}

impl RevSystemSetConfigs {
    /// Split configs to be more readable in impl_into_rev_set_configs! and as partially movable as nested tuples.
    fn split(self) -> (ForwardSetConfig, BackwardSetConfigs) {
        (
            ForwardSetConfig {
                fwd_sys_sets: self.fwd_sys_sets,
                cond_sets: self.cond_sets,
            },
            BackwardSetConfigs {
                bwd_cmd_sets: self.bwd_cmd_sets,
                bwd_sys_sets: self.bwd_sys_sets,
            },
        )
    }
}

struct ForwardSetConfig {
    fwd_sys_sets: SystemSetConfigs,
    cond_sets: SystemSetConfigs,
}

struct BackwardSetConfigs {
    bwd_cmd_sets: SystemSetConfigs,
    bwd_sys_sets: SystemSetConfigs,
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
            fwd_sys_sets: FwdSysSet::from_set(set).into_configs(),
            bwd_cmd_sets: BwdCmdSet::from_set(set).into_configs(),
            bwd_sys_sets: BwdSysSet::from_set(set).into_configs(),
            cond_sets: ForwardSet.into_configs(),
        }
    }
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

                let cond_sets = ($($var.0.cond_sets,)*).into_configs();

                // let [var0, ..., varN]
                //  : [BackwardSetConfigs, ..., BackwardSetConfigs]
                //  = [varN.1, ..., var0.1];
                let mut arr = [$($var.1,)*];
                arr.reverse();
                let [$($var,)*] = arr;

                let bwd_cmd_sets = ($($var.bwd_cmd_sets,)*).into_configs();

                let bwd_sys_sets = ($($var.bwd_sys_sets,)*).into_configs();

                RevSystemSetConfigs {
                    fwd_sys_sets,
                    bwd_cmd_sets,
                    bwd_sys_sets,
                    cond_sets
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_set_configs, 1, 20, T, M, var);
