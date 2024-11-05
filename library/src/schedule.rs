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

/// Contains a forward and a backward set that run depending on the current [`RevDirection`] in [`RevMeta`].
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevSystemsSet;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`FwdArcSet`]s.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSet;

/// Subset of [`RevSystemsSet`].
///
/// Contains [`BwdCmdArcSet`]s in reverse order.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSet;

/// Subsets of [`ForwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct FwdArcSet(TypeId);

/// Subsets of [`ForwardSet`].
///
/// Each contains a non-system set.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct FwdNonSys(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// Each contains the [`BwdArcSet`] `sys_n` and a command log `cmd_n` in this configuration:
///
/// `(cmd_n, sys_n).chain()`
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BwdCmdArcSet(TypeId);

/// Subsets of [`BackwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BwdArcSet(TypeId);

/// Subsets of [`BackwardSet`].
///
/// Each contains a non-system set.
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BwdNonSys(InternedSystemSet);

impl FwdArcSet {
    fn from_set<Marker>(set: impl IntoSystemSet<Marker>) -> InternedSystemSet {
        let set = set.into_system_set();
        match set.system_type() {
            Some(id) => Self(id).intern(),
            None => FwdNonSys(set.intern()).intern(),
        }
    }
}

impl BwdCmdArcSet {
    fn from_set<Marker>(set: impl IntoSystemSet<Marker>) -> InternedSystemSet {
        let set = set.into_system_set();
        match set.system_type() {
            Some(id) => Self(id).intern(),
            None => BwdNonSys(set.intern()).intern(),
        }
    }
}

impl BwdArcSet {
    fn from_set<Marker>(set: impl IntoSystemSet<Marker>) -> InternedSystemSet {
        let set = set.into_system_set();
        match set.system_type() {
            Some(id) => Self(id).intern(),
            None => BwdNonSys(set.intern()).intern(),
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
            self.configure_sets(
                (
                    ForwardSet.run_if(if_forward),
                    BackwardSet.run_if(if_backward),
                )
                    .in_set(RevSystemsSet),
            );
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
    use std::mem::take;

    use bevy::{
        app::FixedUpdate,
        ecs::{
            change_detection::ResMut,
            component::Component,
            event::Event,
            observer::Trigger,
            system::Resource,
            world::{DeferredWorld, World},
        },
    };

    use crate::{
        commands::{observer::RevEvent, RevCommands},
        world::{RevDeferredWorld, RevWorld},
        RevFrame, RevUpdate,
    };

    use super::*;

    #[test]
    fn forward_backward_sets_unknown_works() {
        let schedule = &mut Schedule::new(RevUpdate);
        assert_eq!(forward_backward_sets_unknown(schedule), true);
        assert_eq!(forward_backward_sets_unknown(schedule), false);
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Test<T> {
        Sys(T),

        HookBySys(T),
        HookByCmd(T),

        ObsvBySys(T),
        ObsvByCmd(T),
        ObsvByHook(T),
        ObsvByObsv(T),

        CmdBySys(T),
        CmdByCmd(T),
        CmdByHook(T),
        CmdByObsv(T),
    }

    impl<T> Test<T> {
        fn map<U>(self, map: impl Fn(T) -> U) -> Test<U> {
            match self {
                Test::Sys(value) => Test::Sys(map(value)),

                Test::HookBySys(value) => Test::HookBySys(map(value)),
                Test::HookByCmd(value) => Test::HookByCmd(map(value)),

                Test::ObsvBySys(value) => Test::ObsvBySys(map(value)),
                Test::ObsvByCmd(value) => Test::ObsvByCmd(map(value)),
                Test::ObsvByHook(value) => Test::ObsvByHook(map(value)),
                Test::ObsvByObsv(value) => Test::ObsvByObsv(map(value)),

                Test::CmdBySys(value) => Test::CmdBySys(map(value)),
                Test::CmdByCmd(value) => Test::CmdByCmd(map(value)),
                Test::CmdByHook(value) => Test::CmdByHook(map(value)),
                Test::CmdByObsv(value) => Test::CmdByObsv(map(value)),
            }
        }
    }

    #[derive(Resource)]
    struct TestLog(Vec<Test<(u8, RevDirection)>>);

    #[derive(Component)]
    struct HookBySys(u8);

    #[derive(Component)]
    struct HookByCmd(u8);

    #[derive(Event, Clone)]
    struct ObsvBySys(u8);

    #[derive(Event, Clone)]
    struct ObsvByCmd(u8);

    #[derive(Event, Clone)]
    struct ObsvByHook(u8);

    #[derive(Event, Clone)]
    struct ObsvByObsv(u8);

    fn test_sys<const N: u8>(world: &mut World) {
        let direction = world.resource::<RevMeta>().direction();
        world
            .resource_mut::<TestLog>()
            .0
            .push(Test::Sys((N, direction)));
        if direction != RevDirection::NotLog {
            return;
        }

        // trigger hook in system
        world.spawn(HookBySys(N));

        // trigger observer in system
        world.rev_trigger(ObsvBySys(N));

        // trigger command in system
        world.commands().rev_queue(|world: &mut World| {
            world
                .resource_mut::<TestLog>()
                .0
                .push(Test::CmdBySys((N, RevDirection::NotLog)));

            // trigger hook in command
            world.spawn(HookByCmd(N));

            // trigger observer in command
            world.rev_trigger(ObsvByCmd(N));

            // trigger command in command
            world.commands().rev_queue(|world: &mut World| {
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::CmdByCmd((N, RevDirection::NotLog)));

                |world: &mut World, forward: bool| {
                    let direction = match forward {
                        true => RevDirection::ForwardLog,
                        false => RevDirection::BackwardLog,
                    };
                    world
                        .resource_mut::<TestLog>()
                        .0
                        .push(Test::CmdByCmd((N, direction)));
                }
            });

            |world: &mut World, forward: bool| {
                let direction = match forward {
                    true => RevDirection::ForwardLog,
                    false => RevDirection::BackwardLog,
                };
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::CmdBySys((N, direction)));
            }
        });
    }

    fn test_run(configs: impl FnOnce(&mut Schedule), tests: Vec<Vec<Test<u8>>>) {
        // set up world
        let mut world = World::new();
        world.insert_resource(RevMeta::new(None, 0, false));
        let mut schedule = Schedule::new(FixedUpdate);
        schedule.add_systems(RevMeta::update_world);
        assert!(schedule.initialize(&mut world).is_ok());
        world.add_schedule(schedule);

        // set up reversible schedule
        let mut schedule = Schedule::new(RevUpdate);
        configs(&mut schedule);
        assert!(schedule.initialize(&mut world).is_ok());
        world.add_schedule(schedule);

        // set up observers
        world.observe(
            |trigger: Trigger<RevEvent<ObsvBySys>>, mut world: DeferredWorld| {
                let event = trigger.event();
                let n = event.0;
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::ObsvBySys((n, event.direction())));

                // trigger observer in observer
                world.rev_trigger(ObsvByHook(n));

                // trigger command in observer
                world.commands().rev_queue(move |world: &mut World| {
                    world
                        .resource_mut::<TestLog>()
                        .0
                        .push(Test::CmdByObsv((n, RevDirection::NotLog)));

                    move |world: &mut World, forward: bool| {
                        let direction = match forward {
                            true => RevDirection::ForwardLog,
                            false => RevDirection::BackwardLog,
                        };
                        world
                            .resource_mut::<TestLog>()
                            .0
                            .push(Test::CmdByObsv((n, direction)));
                    }
                });
            },
        );
        world.observe(
            |trigger: Trigger<RevEvent<ObsvByCmd>>, mut log: ResMut<TestLog>| {
                let event = trigger.event();
                log.0.push(Test::ObsvByCmd((event.0, event.direction())));
            },
        );
        world.observe(
            |trigger: Trigger<RevEvent<ObsvByHook>>, mut log: ResMut<TestLog>| {
                let event = trigger.event();
                log.0.push(Test::ObsvByHook((event.0, event.direction())));
            },
        );
        world.observe(
            |trigger: Trigger<RevEvent<ObsvByObsv>>, mut log: ResMut<TestLog>| {
                let event = trigger.event();
                log.0.push(Test::ObsvByObsv((event.0, event.direction())));
            },
        );

        // set up hooks
        world.rev_register_component_hooks::<HookBySys>().on_add(
            |direction, mut world, entity, _| {
                let Ok(direction): Result<RevDirection, _> = direction.try_into() else {
                    return;
                };
                let n = world.entity(entity).get::<HookBySys>().expect("todo").0;
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::HookBySys((n, direction)));

                if direction != RevDirection::NotLog {
                    return;
                }

                // trigger observer in hook
                world.rev_trigger(ObsvByHook(n));

                // trigger command in hook
                world.commands().rev_queue(move |world: &mut World| {
                    world
                        .resource_mut::<TestLog>()
                        .0
                        .push(Test::CmdByHook((n, RevDirection::NotLog)));

                    move |world: &mut World, forward: bool| {
                        let direction = match forward {
                            true => RevDirection::ForwardLog,
                            false => RevDirection::BackwardLog,
                        };
                        world
                            .resource_mut::<TestLog>()
                            .0
                            .push(Test::CmdByHook((n, direction)));
                    }
                });
            },
        );
        world.rev_register_component_hooks::<HookByCmd>().on_add(
            |direction, mut world, entity, _| {
                let Ok(direction) = direction.try_into() else {
                    return;
                };
                let n = world.entity(entity).get::<HookByCmd>().expect("todo").0;
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::HookByCmd((n, direction)));
            },
        );

        // run tests forward
        for (i, step) in tests.iter().enumerate() {
            let tests = step
                .iter()
                .map(|test| test.map(|n| (n, RevDirection::NotLog)))
                .collect::<Vec<_>>();
            world.run_schedule(FixedUpdate);
            let log = &mut world.resource_mut::<TestLog>().0;
            let log = take(log);
            assert_eq!(tests, log, "\nforward step: {i}");
        }

        // run tests backward log
        let mut meta = world.resource_mut::<RevMeta>();
        let end_frame = meta.present_world_state();
        assert!(meta.queue_log(RevFrame::new(0)).is_ok());
        for (i, step) in tests.iter().enumerate().rev() {
            let tests = step
                .iter()
                .rev()
                .map(|test| test.map(|n| (n, RevDirection::BackwardLog)))
                .collect::<Vec<_>>();
            world.run_schedule(FixedUpdate);
            let log = &mut world.resource_mut::<TestLog>().0;
            let log = take(log);
            assert_eq!(tests, log, "\nbackward log step: {i}");
        }

        // run tests forward log
        let mut meta = world.resource_mut::<RevMeta>();
        assert!(meta.queue_log(end_frame).is_ok());
        for (i, step) in tests.iter().enumerate() {
            let tests = step
                .iter()
                .map(|test| test.map(|n| (n, RevDirection::ForwardLog)))
                .collect::<Vec<_>>();
            world.run_schedule(FixedUpdate);
            let log = &mut world.resource_mut::<TestLog>().0;
            let log = take(log);
            assert_eq!(tests, log, "\nforward log step: {i}");
        }
    }
}
