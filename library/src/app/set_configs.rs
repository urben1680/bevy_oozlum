use bevy::{
    ecs::schedule::SystemSetConfigs,
    prelude::{Condition, IntoSystemSet, IntoSystemSetConfigs, SystemSet},
    utils::all_tuples,
};
use condition::forward_backward_conditions;

use super::{BackwardCmdsSys, BackwardSys};

mod condition;

pub struct RevSystemSetConfigs {
    /// Configuration(s) of sets or systems in the forward schedule.
    pub(crate) forward_sys: SystemSetConfigs,

    /// Configuration(s) of CommandsBackward in the backward schedule
    /// paired with systems configured with [`Self::backward_sys`].
    pub(crate) backward_cmds_sys: SystemSetConfigs,

    /// Configuration(s) of sets or systems in the backward schedule.
    pub(crate) backward_sys: SystemSetConfigs,
}

pub trait IntoRevSystemSetConfigs<Marker>: Sized {
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevSystemSetConfigs;
    fn rev_in_set(self, set: impl SystemSet) -> RevSystemSetConfigs {
        let set = set.intern();
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.in_set(set),
            backward_cmds_sys: configs.backward_cmds_sys.in_set(BackwardCmdsSys(set)),
            backward_sys: configs.backward_sys.in_set(BackwardSys(set)),
        }
    }
    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let set = set.into_system_set().intern();
        let configs = self.into_rev_configs();
        // example for a system A in self and a system B in set:
        //
        // A forward sys → maybe sync → B forward sys
        //
        // B backward cmds → maybe sync → B backward sys → A backward cmds → maybe sync → A backward sys
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.before(set),
            backward_cmds_sys: configs
                .backward_cmds_sys
                .after_ignore_deferred(BackwardCmdsSys(set)),
            backward_sys: configs.backward_sys,
        }
    }
    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let set = set.into_system_set().intern();
        let configs = self.into_rev_configs();
        // example for a system A in self and a system B in set:
        //
        // B forward sys → maybe sync → A forward sys
        //
        // A backward cmds → maybe sync → A backward sys → B backward cmds → maybe sync → B backward sys
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.after(set),
            backward_cmds_sys: configs
                .backward_cmds_sys
                .before_ignore_deferred(BackwardCmdsSys(set)),
            backward_sys: configs.backward_sys,
        }
    }
    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let set = set.into_system_set().intern();
        let configs = self.into_rev_configs();
        // example for a system A in self and a system B in set:
        //
        // A forward sys → B forward sys
        //
        // B backward cmds → maybe sync → B backward sys ┐
        //                  A backward cmds → maybe sync ┴→ A backward sys
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.before_ignore_deferred(set),
            backward_cmds_sys: configs.backward_cmds_sys,
            backward_sys: configs.backward_sys.after_ignore_deferred(BackwardSys(set)),
        }
    }
    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let set = set.into_system_set().intern();
        let configs = self.into_rev_configs();
        // example for a system A in self and a system B in set:
        //
        // B forward sys → A forward sys
        //
        // A backward cmds → maybe sync → A backward sys ┐
        //                  B backward cmds → maybe sync ┴→ B backward sys
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.after_ignore_deferred(set),
            backward_cmds_sys: configs.backward_cmds_sys,
            backward_sys: configs
                .backward_sys
                .before_ignore_deferred(BackwardSys(set)),
        }
    }
    fn rev_run_if<M>(self, condition: impl Condition<M>) -> RevSystemSetConfigs {
        let (forward_condition, backward_condition) = forward_backward_conditions(condition);
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.run_if(forward_condition),
            backward_cmds_sys: configs.backward_cmds_sys.run_if(backward_condition),
            backward_sys: configs.backward_sys,
        }
    }
    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevSystemSetConfigs {
        let set = set.into_system_set().intern();
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.ambiguous_with(set),
            backward_cmds_sys: configs.backward_cmds_sys, // commands backward systems have no accesses that could be ambigious
            backward_sys: configs.backward_sys.ambiguous_with(BackwardSys(set)),
        }
    }
    fn rev_ambiguous_with_all(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.ambiguous_with_all(),
            backward_cmds_sys: configs.backward_cmds_sys, // commands backward systems have no accesses that could be ambigious
            backward_sys: configs.backward_sys.ambiguous_with_all(),
        }
    }
    fn rev_chain(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        // todo: uncomment when https://github.com/bevyengine/bevy/pull/13919 is landed
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.chain(),
            backward_cmds_sys: configs.backward_cmds_sys.chain/*.chain_ignore_deferred*/(),
            backward_sys: configs.backward_sys,
        }
    }
    fn rev_chain_ignore_deferred(self) -> RevSystemSetConfigs {
        let configs = self.into_rev_configs();
        // todo: uncomment when https://github.com/bevyengine/bevy/pull/13919 landed
        RevSystemSetConfigs {
            forward_sys: configs.forward_sys.chain/*.chain_ignore_deferred*/(),
            backward_cmds_sys: configs.backward_cmds_sys,
            backward_sys: configs.backward_sys.chain/*.chain_ignore_deferred*/(),
        }
    }
}

impl RevSystemSetConfigs {
    /// Split configs to be more readable in macros and as partially movable as nested tuples.
    pub(crate) fn split(self) -> (ForwardSetConfig, BackwardSetConfigs) {
        (
            ForwardSetConfig {
                forward_sys: self.forward_sys,
            },
            BackwardSetConfigs {
                backward_cmds_sys: self.backward_cmds_sys,
                backward_sys: self.backward_sys,
            },
        )
    }
}

pub(crate) struct ForwardSetConfig {
    pub(crate) forward_sys: SystemSetConfigs,
}

pub(crate) struct BackwardSetConfigs {
    pub(crate) backward_cmds_sys: SystemSetConfigs,
    pub(crate) backward_sys: SystemSetConfigs,
}

impl IntoRevSystemSetConfigs<RevSystemSetConfigs> for RevSystemSetConfigs {
    fn into_rev_configs(self) -> RevSystemSetConfigs {
        self
    }
}

impl<S: SystemSet> IntoRevSystemSetConfigs<()> for S {
    fn into_rev_configs(self) -> RevSystemSetConfigs {
        let set = self.intern();
        RevSystemSetConfigs {
            forward_sys: set.into_configs(),
            backward_cmds_sys: BackwardCmdsSys(set).into_configs(),
            backward_sys: BackwardSys(set).into_configs(),
        }
    }
}

/// Inverts tuples of expressions.
///
/// `invert_tuple!(a.foo(), b.bar())` extends to `(b.bar(), a.foo())`.
///
/// This is expensive as the steps grow quadratically with the number of arguments.
///
/// See https://veykril.github.io/tlborm/decl-macros/patterns/tt-muncher.html.
macro_rules! invert_tuple {
    // Initialize the empty accumulator so that callers don't have to pass that in.
    ($($input:expr),* $(,)?) => {
        invert_tuple!(@ACC [] [$($input,)*])
    };
    // Recursive step, each new identifier is prepended to the left of the
    // previous tokens.
    (@ACC [$($acc:tt)*] [$fst:expr , $($rest:tt)*]) => {
        invert_tuple!(@ACC [$fst, $($acc)*] [$($rest)*])
    };
    // Final step, finalize the accumulator, producing an actual tuple.
    (@ACC [$($acc:tt)*] []) => {
        ($($acc)*)
    };
}

macro_rules! impl_into_rev_set_configs {
    ($(($T: ident, $M: ident, $var: ident)),*) => {
        impl<$($T, $M),*> IntoRevSystemSetConfigs<($($M,)*)> for ($($T,)*)
        where
            $($T: IntoRevSystemSetConfigs<$M>,)*
        {
            fn into_rev_configs(self) -> RevSystemSetConfigs {
                // let (var0, ..., var9)
                //  : (impl IntoRevSystemSetConfigs, ..., impl IntoRevSystemSetConfigs)
                //  = self;
                let ($($var,)*) = self;

                // let (var0, ..., var9)
                //  : ((ForwardSetConfig, BackwardSetConfigs), ..., (ForwardSetConfig, BackwardSetConfigs))
                //  = (var0.into_rev_configs().split(), ..., var9.into_rev_configs().split());
                let ($($var,)*) = ($($var.into_rev_configs().split(),)*);

                let forward_sys = ($($var.0.forward_sys,)*).into_configs();

                // let (var0, ..., var9)
                //  : (BackwardSetConfigs, ..., BackwardSetConfigs)
                //  = (var9, ..., var0);
                let ($($var,)*) = invert_tuple!($($var.1,)*);

                let backward_cmds_sys = ($($var.backward_cmds_sys,)*).into_configs();

                let backward_sys = ($($var.backward_sys,)*).into_configs();

                RevSystemSetConfigs {
                    forward_sys,
                    backward_cmds_sys,
                    backward_sys,
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_set_configs, 1, 20, T, M, var);
