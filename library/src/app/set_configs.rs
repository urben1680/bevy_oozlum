use bevy::{
    ecs::schedule::SystemSetConfigs,
    prelude::{Condition, IntoSystemSet, IntoSystemSetConfigs, SystemSet},
};
use condition::forward_backward_conditions;

use super::{BackwardCmdsSys, BackwardSys};

mod condition;

pub struct RevSystemSetConfigs {
    pub(crate) forward_sys: SystemSetConfigs,
    pub(crate) backward_cmds_sys: SystemSetConfigs,
    pub(crate) backward_sys: SystemSetConfigs,
}

pub trait IntoRevSystemSetConfigs: Sized {
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

impl IntoRevSystemSetConfigs for RevSystemSetConfigs {
    fn into_rev_configs(self) -> RevSystemSetConfigs {
        self
    }
}

impl<S: SystemSet> IntoRevSystemSetConfigs for S {
    fn into_rev_configs(self) -> RevSystemSetConfigs {
        let set = self.intern();
        RevSystemSetConfigs {
            forward_sys: set.into_configs(),
            backward_cmds_sys: BackwardCmdsSys(set).into_configs(),
            backward_sys: BackwardSys(set).into_configs(),
        }
    }
}
