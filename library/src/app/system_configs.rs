use bevy::ecs::schedule::{Condition, IntoSystemSet, SystemConfigs, SystemSet};

use super::set_configs::{IntoRevSystemSetConfigs, RevSystemSetConfigs};

pub mod system;

pub struct RevSystemConfigs {
    pub(crate) forward: SystemConfigs,
    pub(crate) backward: SystemConfigs,
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
        configs.set_configs = configs.set_configs.rev_before_ignore_deferred(set);
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
