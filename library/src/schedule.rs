use std::any::TypeId;

use bevy::ecs::{
    change_detection::Res,
    schedule::{InternedSystemSet, IntoSystemSet, IntoSystemSetConfigs, Schedule, SystemSet},
};

use crate::meta::{RevDirection, RevMeta};

mod set_configs;
mod system_configs;

pub use set_configs::*;
pub use system_configs::*;

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct FwdArcSet(TypeId);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BwdArcCmdSet(TypeId);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BwdArcSet(TypeId);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct FwdNonSys(InternedSystemSet);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BwdNonSys(InternedSystemSet);

impl FwdArcSet {
    fn from_set<Marker>(set: impl IntoSystemSet<Marker>) -> InternedSystemSet {
        let set = set.into_system_set().intern();
        match set.system_type() {
            Some(id) => Self(id).intern(),
            None => FwdNonSys(set).intern(),
        }
    }
}

impl BwdArcCmdSet {
    fn from_set<Marker>(set: impl IntoSystemSet<Marker>) -> InternedSystemSet {
        let set = set.into_system_set().intern();
        match set.system_type() {
            Some(id) => Self(id).intern(),
            None => BwdNonSys(set).intern(),
        }
    }
}

impl BwdArcSet {
    fn from_set<Marker>(set: impl IntoSystemSet<Marker>) -> InternedSystemSet {
        let set = set.into_system_set().intern();
        match set.system_type() {
            Some(id) => Self(id).intern(),
            None => BwdNonSys(set).intern(),
        }
    }
}

pub trait RevSchedule {
    fn rev_add_systems<Marker>(&mut self, systems: impl IntoRevSystemConfigs<Marker>) -> &mut Self;
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self;
}

impl RevSchedule for Schedule {
    fn rev_add_systems<Marker>(&mut self, systems: impl IntoRevSystemConfigs<Marker>) -> &mut Self {
        let RevSystemConfigs { systems, sets } = systems.into_rev_configs();
        self.add_systems(systems).rev_configure_sets(sets)
    }
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self {
        if forward_backward_sets_unknown(self) {
            // run conditions return false if RevMeta is missing
            fn if_forward(meta: Res<RevMeta>) -> bool {
                matches!(meta.get_direction(), Some(RevDirection::Forward { .. }))
            }
            fn if_backward(meta: Res<RevMeta>) -> bool {
                matches!(meta.get_direction(), Some(RevDirection::BackwardLog))
            }
            self.configure_sets((
                ForwardSet.run_if(if_forward),
                BackwardSet.run_if(if_backward),
            ));
        }
        let RevSystemSetConfigs {
            fwd_arc_sets,
            bwd_cmd_arc_sets,
            bwd_arc_sets,
        } = sets.into_rev_configs();
        self.configure_sets((
            fwd_arc_sets.in_set(ForwardSet),
            bwd_cmd_arc_sets.in_set(BackwardSet),
            bwd_arc_sets, // subsets of bwd_cmd_arc_sets
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
    // todo: https://github.com/bevyengine/bevy/pull/16206
    let (lower_bound_before, upper_bound_before) = schedule.graph().system_sets().size_hint();
    schedule.configure_sets(ForwardSet);
    let (lower_bound_after, upper_bound_after) = schedule.graph().system_sets().size_hint();

    const EXPECT: &'static str = "ScheduleGraph::system_sets() expected to impl ExactSizeIterator";
    debug_assert_eq!(Some(lower_bound_before), upper_bound_before, "{EXPECT}");
    debug_assert_eq!(Some(lower_bound_after), upper_bound_after, "{EXPECT}");

    lower_bound_before < lower_bound_after
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use bevy::{
        app::{App, Update},
        ecs::{change_detection::ResMut, schedule::ScheduleLabel, system::Resource},
    };

    use crate::{app::{RevApp, RevSystemsPlugin}, RevFrame, RevUpdate};

    use super::*;

    #[test]
    fn forward_backward_sets_unknown_works() {
        let schedule = &mut Schedule::new(RevUpdate);
        assert_eq!(forward_backward_sets_unknown(schedule), true);
        assert_eq!(forward_backward_sets_unknown(schedule), false);
    }

    /*
    todo tests:

    - before/after/chain one system with commands and another system
    - before/after a system with commands and a set with another system
    - run_if
     */

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
            add_rev_meta_sys_in: Some((Update.intern(), None)),
        })
        .init_resource::<Log>()
        .rev_add_systems(RevUpdate, (sys_a, sys_b).rev_chain());

        bevy_mod_debugdump::print_schedule_graph(&mut app, RevUpdate);

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
