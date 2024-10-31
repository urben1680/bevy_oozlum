use bevy::ecs::{
    change_detection::Res,
    schedule::{IntoSystemSetConfigs, Schedule, SystemSet},
};

use crate::{
    meta::{RevDirection, RevMeta},
    set_configs::RevSystemSetConfigs,
    system_configs::RevSystemConfigs,
};

use super::{set_configs::IntoRevSystemSetConfigs, system_configs::IntoRevSystemConfigs};

pub trait RevSchedule {
    fn rev_add_systems<Marker>(&mut self, systems: impl IntoRevSystemConfigs<Marker>) -> &mut Self;
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self;
}

impl RevSchedule for Schedule {
    fn rev_add_systems<Marker>(&mut self, systems: impl IntoRevSystemConfigs<Marker>) -> &mut Self {
        let RevSystemConfigs {
            forward,
            backward,
            set_configs,
        } = systems.into_rev_configs();
        self.add_systems((forward, backward))
            .rev_configure_sets(set_configs)
    }
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self {
        if forward_backward_sets_unknown(self) {
            fn run_forward(meta: Res<RevMeta>) -> bool {
                matches!(meta.get_direction(), Some(RevDirection::Forward { .. }))
            }
            fn run_backward(meta: Res<RevMeta>) -> bool {
                matches!(meta.get_direction(), Some(RevDirection::BackwardLog))
            }
            self.configure_sets((
                ForwardSet.run_if(run_forward),
                BackwardSet.run_if(run_backward),
            ));
        }
        let RevSystemSetConfigs {
            forward_sys,
            backward_cmds_sys,
            backward_sys,
        } = sets.into_rev_configs();
        self.configure_sets((
            forward_sys.in_set(ForwardSet),
            (backward_cmds_sys, backward_sys).in_set(BackwardSet),
        ))
    }
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSet;

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSet;

fn forward_backward_sets_unknown(schedule: &mut Schedule) -> bool {
    // ScheduleGraph::system_sets() does not return an `impl ExactSizeIterator` but it is one actually.
    // Manually searching the sets for `ForwardSet`/`BackwardSet` would be O(n) per call of this method,
    // which itself is assumed to be called many times. So instead this impl relies on `size_hint` being
    // accurate to see if adding one of the two sets increases the size.
    // todo: upstream a `ScheduleGraph::contains_system_set` method
    let (lower_bound_before, upper_bound_before) = schedule.graph().system_sets().size_hint();
    schedule.configure_sets(ForwardSet);
    let (lower_bound_after, upper_bound_after) = schedule.graph().system_sets().size_hint();

    if cfg!(debug_assertions) {
        const EXPECT: &'static str =
            "ScheduleGraph::system_sets() expected to be ExactSizeIterator";
        let upper_bound_before = upper_bound_before.expect(EXPECT);
        let upper_bound_after = upper_bound_after.expect(EXPECT);
        assert_eq!(lower_bound_before, upper_bound_before, "{EXPECT}");
        assert_eq!(lower_bound_after, upper_bound_after, "{EXPECT}");
    }

    upper_bound_before < upper_bound_after
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use bevy::{
        app::{App, Update}, ecs::{change_detection::ResMut, schedule::ScheduleLabel, system::Resource}
    };

    use crate::{app::RevApp, RevFrame, RevSystemsPlugin, RevUpdate};

    use super::*;

    #[test]
    fn forward_backward_sets_unknown_works() {
        let schedule = &mut Schedule::new(RevUpdate);
        assert_eq!(forward_backward_sets_unknown(schedule), true);
        assert_eq!(forward_backward_sets_unknown(schedule), false);
    }

    #[test]
    fn reversible_systems_works() {
        #[derive(PartialEq, Debug)]
        enum Sys {
            A,
            B,
        }

        #[derive(Resource, Default)]
        struct Log(Vec<Sys>);

        fn sys_a(mut res: ResMut<Log>) {
            res.0.push(Sys::A)
        }

        fn sys_b(mut res: ResMut<Log>) {
            res.0.push(Sys::B)
        }

        let mut app = App::new();

        app.add_plugins(RevSystemsPlugin {
            rev_meta: Some(RevMeta::new(NonZeroUsize::new(2), 0, false)),
            add_rev_meta_sys_in: Some((Update.intern(), None))
        })
            .init_resource::<Log>()
            .rev_add_systems(RevUpdate, (sys_a, sys_b).rev_chain());

        app.update();
        let log = app
            .world_mut()
            .resource_mut::<Log>()
            .0
            .drain(..)
            .collect::<Vec<_>>();
        assert_eq!(log, [Sys::A, Sys::B]);

        app.world_mut()
            .resource_mut::<RevMeta>()
            .queue_log(RevFrame::new(0))
            .expect("in log");

        app.update();
        let log = app.world_mut().remove_resource::<Log>().unwrap().0;
        assert_eq!(log, [Sys::B, Sys::A]);
    }
}
