use bevy::{
    ecs::schedule::SystemSetConfigs,
    prelude::{Condition, IntoSystemSet, IntoSystemSetConfigs, SystemSet},
    utils::all_tuples,
};
use condition::forward_backward_conditions;

use super::{BwdCmdArcSet, BwdArcSet, BwdNonSys, FwdArcSet, FwdNonSys};

mod condition;

pub struct RevSystemSetConfigs {
    /// Contains [`FwdArcSet`]s for each system for the [`super::ForwardSet`].
    pub(crate) fwd_arc_sets: SystemSetConfigs,
    /// Contains [`BwdArcCmdSet`]s that contain `(bwd_cmd, bwd_arc).chain()` for each system for the
    /// [`super::BackwardSet`]. Because that already enforces the syncpoint between each pair members,
    /// these cofigs can always be configured with ignored_deferred. If these forced sync-points are
    /// to be ignored, leave this configs untouched and configure [`Self::bwd_arc_sets`] instead.
    pub(crate) bwd_cmd_arc_sets: SystemSetConfigs,
    /// Contains [`BwdArcSet`]s which are subsets of [`BwdArcCmdSet`] with only each `bwd_arc`.
    pub(crate) bwd_arc_sets: SystemSetConfigs,
}

pub trait IntoRevSystemSetConfigs<Marker>: Sized {
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevSystemSetConfigs;
    fn rev_in_set(self, set: impl SystemSet) -> RevSystemSetConfigs {
        let set = set.intern();
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_arc_sets: configs.fwd_arc_sets.in_set(FwdNonSys(set)),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets.in_set(BwdNonSys(set)),
            bwd_arc_sets: configs.bwd_arc_sets, // already subsets of bwd_cmd_arc_set
        }
    }
    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // A forward sys → maybe sync → B forward sys
        //
        // B backward cmds → maybe sync → B backward sys → A backward cmds → maybe sync → A backward sys
        RevSystemSetConfigs {
            fwd_arc_sets: configs.fwd_arc_sets.before(FwdArcSet::from_set(set)),
            bwd_cmd_arc_sets: configs
                .bwd_cmd_arc_sets
                .after_ignore_deferred(BwdCmdArcSet::from_set(set)),
            bwd_arc_sets: configs.bwd_arc_sets,
        }
    }
    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // B forward sys → maybe sync → A forward sys
        //
        // A backward cmds → maybe sync → A backward sys → B backward cmds → maybe sync → B backward sys
        RevSystemSetConfigs {
            fwd_arc_sets: configs.fwd_arc_sets.after(FwdArcSet::from_set(set)),
            bwd_cmd_arc_sets: configs
                .bwd_cmd_arc_sets
                .before_ignore_deferred(BwdCmdArcSet::from_set(set)),
            bwd_arc_sets: configs.bwd_arc_sets,
        }
    }
    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // A forward sys → B forward sys
        //
        // B backward cmds → maybe sync → B backward sys ┐
        //                  A backward cmds → maybe sync ┴→ A backward sys
        RevSystemSetConfigs {
            fwd_arc_sets: configs
                .fwd_arc_sets
                .before_ignore_deferred(FwdArcSet::from_set(set)),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets,
            bwd_arc_sets: configs
                .bwd_arc_sets
                .after_ignore_deferred(BwdArcSet::from_set(set)),
        }
    }
    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        // example for a system A in self and a system B in set:
        //
        // B forward sys → A forward sys
        //
        // A backward cmds → maybe sync → A backward sys ┐
        //                  B backward cmds → maybe sync ┴→ B backward sys
        RevSystemSetConfigs {
            fwd_arc_sets: configs
                .fwd_arc_sets
                .after_ignore_deferred(FwdArcSet::from_set(set)),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets,
            bwd_arc_sets: configs
                .bwd_arc_sets
                .before_ignore_deferred(BwdArcSet::from_set(set)),
        }
    }
    fn rev_run_if<M>(self, condition: impl Condition<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let (forward_condition, backward_condition) = forward_backward_conditions(condition);
        RevSystemSetConfigs {
            fwd_arc_sets: configs.fwd_arc_sets.run_if(forward_condition),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets.run_if(backward_condition),
            bwd_arc_sets: configs.bwd_arc_sets, // needs none as a subset of bwd_cmd_arc_set
        }
    }
    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        let set = set.into_system_set().intern();
        RevSystemSetConfigs {
            fwd_arc_sets: configs
                .fwd_arc_sets
                .ambiguous_with(FwdArcSet::from_set(set)),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets, // bwd_cmd have no accesses that could be ambigious
            bwd_arc_sets: configs
                .bwd_arc_sets
                .ambiguous_with(BwdArcSet::from_set(set)),
        }
    }
    fn rev_ambiguous_with_all(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            fwd_arc_sets: configs.fwd_arc_sets.ambiguous_with_all(),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets, // bwd_cmd have no accesses that could be ambigious
            bwd_arc_sets: configs.bwd_arc_sets.ambiguous_with_all(),
        }
    }
    fn rev_chain(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        // todo: uncomment when https://github.com/bevyengine/bevy/pull/13919 is landed
        RevSystemSetConfigs {
            fwd_arc_sets: configs.fwd_arc_sets.chain(),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets.chain/*_ignore_deferred*/(), // each cmd and sys pair are chained without ignore_deferred
            bwd_arc_sets: configs.bwd_arc_sets,
        }
    }
    fn rev_chain_ignore_deferred(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        // todo: uncomment when https://github.com/bevyengine/bevy/pull/13919 landed
        RevSystemSetConfigs {
            fwd_arc_sets: configs.fwd_arc_sets.chain/*_ignore_deferred*/(),
            bwd_cmd_arc_sets: configs.bwd_cmd_arc_sets,
            bwd_arc_sets: configs.bwd_arc_sets.chain/*_ignore_deferred*/(),
        }
    }
}

impl RevSystemSetConfigs {
    /// Split configs to be more readable in impl_into_rev_set_configs! and as partially movable as nested tuples.
    fn split(self) -> (ForwardSetConfig, BackwardSetConfigs) {
        (
            ForwardSetConfig {
                fwd_arc_sets: self.fwd_arc_sets,
            },
            BackwardSetConfigs {
                bwd_cmd_arc_sets: self.bwd_cmd_arc_sets,
                bwd_arc_sets: self.bwd_arc_sets,
            },
        )
    }
}

struct ForwardSetConfig {
    fwd_arc_sets: SystemSetConfigs,
}

struct BackwardSetConfigs {
    bwd_cmd_arc_sets: SystemSetConfigs,
    bwd_arc_sets: SystemSetConfigs,
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
            fwd_arc_sets: FwdArcSet::from_set(set).into_configs(),
            bwd_cmd_arc_sets: BwdCmdArcSet::from_set(set).into_configs(),
            bwd_arc_sets: BwdArcSet::from_set(set).into_configs(),
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

                let fwd_arc_sets = ($($var.0.fwd_arc_sets,)*).into_configs();

                // let [var0, ..., varN]
                //  : [BackwardSetConfigs, ..., BackwardSetConfigs]
                //  = [varN.1, ..., var0.1];
                let mut arr = [$($var.1,)*];
                arr.reverse();
                let [$($var,)*] = arr;

                let bwd_cmd_arc_sets = ($($var.bwd_cmd_arc_sets,)*).into_configs();

                let bwd_arc_sets = ($($var.bwd_arc_sets,)*).into_configs();

                RevSystemSetConfigs {
                    fwd_arc_sets,
                    bwd_cmd_arc_sets,
                    bwd_arc_sets,
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_set_configs, 1, 20, T, M, var);
