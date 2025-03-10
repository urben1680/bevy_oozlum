use bevy::ecs::schedule::{
    Condition, IntoSystemConfigs, IntoSystemSet, NodeConfigs, SystemConfigs, SystemSet,
};

use variadics_please::all_tuples;

use super::set_configs::{IntoRevSystemSetConfigs, RevSystemSetConfigs};

pub struct RevSystemConfigs {
    pub(crate) systems: SystemConfigs,
    pub(crate) sets: RevSystemSetConfigs,
}

pub trait IntoRevSystemConfigs<Marker>
where
    Self: Sized,
{
    #[doc(hidden)]
    fn into_rev_configs(self) -> RevSystemConfigs;

    fn rev_in_set(self, set: impl SystemSet) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_in_set(set);
        configs
    }
    fn rev_before<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_before(set);
        configs
    }
    fn rev_after<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_after(set);
        configs
    }
    fn rev_before_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_before_ignore_deferred(set);
        configs
    }
    fn rev_after_ignore_deferred<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_after_ignore_deferred(set);
        configs
    }
    fn rev_run_if<M>(self, condition: impl Condition<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_run_if(condition);
        configs
    }
    fn rev_distributive_run_if<M>(self, condition: impl Condition<M> + Clone) -> RevSystemConfigs {
        fn distribute<M>(
            set_configs: &mut RevSystemSetConfigs,
            nodes: &mut SystemConfigs,
            condition: impl Condition<M> + Clone,
        ) {
            let nodes = match nodes {
                NodeConfigs::Configs { configs, .. } => configs,
                NodeConfigs::NodeConfig(_) => {
                    unreachable!(
                        "first iteration: `configs.systems` is always `(fwd_sys, bwd_cmd, bwd_sys)` or further nested tuples.\n
                        next iterations: would not have been called if this is a NodeConfig as these cause a break of recursion."
                    )
                }
            };
            if matches!(nodes.get(0), Some(NodeConfigs::NodeConfig(_))) {
                // detected fwd_sys of single system from `(fwd_sys, bwd_cmd, bwd_sys).into_configs()`
                set_configs.rev_run_if_inner(condition);
                return;
            }
            for node in nodes {
                distribute(set_configs, node, condition.clone());
            }
        }

        let mut configs: RevSystemConfigs = self.into_rev_configs();
        distribute(&mut configs.sets, &mut configs.systems, condition);
        configs
    }
    fn rev_ambiguous_with<M>(self, set: impl IntoSystemSet<M>) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_ambiguous_with(set);
        configs
    }
    fn rev_ambiguous_with_all(self) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_ambiguous_with_all();
        configs
    }
    fn rev_chain(self) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_chain();
        configs
    }
    fn rev_chain_ignore_deferred(self) -> RevSystemConfigs {
        let mut configs = self.into_rev_configs();
        configs.sets = configs.sets.rev_chain_ignore_deferred();
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

                let systems = ($($var.systems,)*).into_configs();

                let sets = ($($var.sets,)*).into_rev_configs();

                RevSystemConfigs {
                    systems,
                    sets
                }
            }
        }
    };
}

all_tuples!(impl_into_rev_system_configs, 1, 20, T, M, var);
