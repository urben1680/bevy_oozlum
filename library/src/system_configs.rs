use bevy::{
    ecs::schedule::{Condition, IntoSystemConfigs, IntoSystemSet, SystemConfigs, SystemSet},
    utils::all_tuples,
};

use crate::set_configs::{IntoRevSystemSetConfigs, RevSystemSetConfigs};

mod system;

pub struct RevSystemConfigs {
    /// Unconfigured system(s) for the forward schedule.
    ///
    /// Configuration is done at [`Self::set_configs.forward_sys`](RevSystemSetConfigs::forward_sys) instead.
    pub(crate) forward: SystemConfigs,

    /// Unconfigured system(s) for the backward schedule.
    ///
    /// Configuration is done at [`Self::set_configs.forward_sys`](RevSystemSetConfigs::backward_cmds_sys) and
    /// [`Self::set_configs.forward_sys`](RevSystemSetConfigs::backward_sys) instead.
    pub(crate) backward: SystemConfigs,

    /// Actual configuration of the systems.
    pub(crate) set_configs: RevSystemSetConfigs,
}

pub trait IntoRevSystemConfigs<Marker>
where
    Self: Sized,
{
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevSystemConfigs;

    fn rev_in_set(self, set: impl SystemSet) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_in_set(set);
        configs
    }

    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_before(set);
        configs
    }

    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_after(set);
        configs
    }

    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_before_ignore_deferred(set);
        configs
    }

    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_after_ignore_deferred(set);
        configs
    }

    fn rev_run_if<M>(self, condition: impl Condition<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_run_if(condition);
        configs
    }

    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_ambiguous_with(set);
        configs
    }

    fn rev_ambiguous_with_all(self) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_ambiguous_with_all();
        configs
    }

    fn rev_chain(self) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_chain();
        configs
    }

    fn rev_chain_ignore_deferred(self) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.set_configs = configs.set_configs.rev_chain_ignore_deferred();
        configs
    }
}

impl IntoRevSystemConfigs<()> for RevSystemConfigs {
    fn into_rev_configs(self) -> RevSystemConfigs {
        self
    }
}

macro_rules! impl_into_rev_system_configs {
    ($(($T: ident, $M: ident, $var: ident)),*) => {
        impl<$($T, $M),*> IntoRevSystemConfigs<($($M,)*)> for ($($T,)*)
        where
            $($T: IntoRevSystemConfigs<$M>,)*
        {
            fn into_rev_configs(self) -> RevSystemConfigs {
                // let (var0, ..., var9)
                //  : (impl IntoRevSystemConfigs, ..., impl IntoRevSystemConfigs)
                //  = self;
                let ($($var,)*) = self;

                // let (var0, ..., var9)
                //  : (RevSystemConfigs, ..., RevSystemConfigs)
                //  = (var0.into_rev_configs(), ..., var9.into_rev_configs());
                let ($($var,)*) = ($($var.into_rev_configs(),)*);

                let forward = ($($var.forward,)*).into_configs();

                // would need to be inverted if it was configured further, but that happens with set_configs instead
                let backward = ($($var.backward,)*).into_configs();

                let set_configs = ($($var.set_configs,)*).into_rev_configs();


                RevSystemConfigs {
                    forward,
                    backward,
                    set_configs
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_system_configs, 1, 20, T, M, var);
