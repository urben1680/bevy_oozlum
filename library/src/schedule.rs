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
            system::{Commands, Resource},
            world::{DeferredWorld, World},
        },
    };

    use crate::{
        commands::{RevCommandLog, RevCommands},
        observer::RevEvent,
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

        SysObsv(T),
        SysObsvObsv(T),
        SysObsvCmd(T),

        SysHook(T),
        SysHookObsv(T),
        SysHookCmd(T),

        SysCmd(T),
        SysCmdHook(T),
        SysCmdObsv(T),
    }

    impl<T> Test<T> {
        fn map<U>(self, map: impl Fn(T) -> U) -> Test<U> {
            match self {
                Test::Sys(value) => Test::Sys(map(value)),

                Test::SysObsv(value) => Test::SysObsv(map(value)),
                Test::SysObsvObsv(value) => Test::SysObsvObsv(map(value)),
                Test::SysObsvCmd(value) => Test::SysObsvCmd(map(value)),

                Test::SysHook(value) => Test::SysHook(map(value)),
                Test::SysHookObsv(value) => Test::SysHookObsv(map(value)),
                Test::SysHookCmd(value) => Test::SysHookCmd(map(value)),

                Test::SysCmd(value) => Test::SysCmd(map(value)),
                Test::SysCmdHook(value) => Test::SysCmdHook(map(value)),
                Test::SysCmdObsv(value) => Test::SysCmdObsv(map(value)),
            }
        }
    }

    #[derive(Clone, Copy)]
    enum TestBundle {
        Regular(u8),
        RegularSyncPoint(u8),
        Exclusive(u8),
    }

    impl IntoIterator for TestBundle {
        type IntoIter = std::vec::IntoIter<Test<u8>>;
        type Item = Test<u8>;
        fn into_iter(self) -> Self::IntoIter {
            match self {
                Self::Regular(n) => vec![Test::Sys(n)],
                Self::RegularSyncPoint(n) => vec![
                    Test::SysObsv(n),
                    Test::SysObsvObsv(n),
                    Test::SysObsvCmd(n),
                    Test::SysHook(n),
                    Test::SysHookObsv(n),
                    Test::SysHookCmd(n),
                    Test::SysCmdHook(n),
                    Test::SysCmdObsv(n),
                    Test::SysCmd(n),
                ],
                Self::Exclusive(n) => vec![
                    Test::Sys(n),
                    Test::SysObsv(n),
                    Test::SysObsvObsv(n),
                    Test::SysObsvCmd(n),
                    Test::SysHook(n),
                    Test::SysHookObsv(n),
                    Test::SysHookCmd(n),
                ],
            }
            .into_iter()
        }
    }

    #[derive(Resource, Default)]
    struct TestLog(Vec<Test<(u8, RevDirection)>>);

    #[derive(Component)]
    struct SysHook(u8);

    #[derive(Component)]
    struct SysCmdHook(u8);

    #[derive(Event, Clone)]
    struct SysObsv(u8);

    #[derive(Event, Clone)]
    struct SysHookObsv(u8);

    #[derive(Event, Clone)]
    struct SysObsvObsv(u8);

    #[derive(Event, Clone)]
    struct SysCmdObsv(u8);

    /// Will add sync point
    fn regular_system<const N: u8>(
        meta: Res<RevMeta>,
        mut log: ResMut<TestLog>,
        mut commands: Commands,
    ) {
        let direction = meta.direction();
        log.0.push(Test::Sys((N, direction)));
        if direction != RevDirection::NotLog {
            return;
        }

        // trigger observer in system
        commands.rev_trigger(SysObsv(N));

        // trigger hook in system
        commands.spawn(SysHook(N));

        // trigger command in system
        commands.rev_queue(system_command::<N>);
    }

    /// Will not add sync point
    fn deferred_system<const N: u8>(mut world: DeferredWorld) {
        let direction = world.resource::<RevMeta>().direction();
        world
            .resource_mut::<TestLog>()
            .0
            .push(Test::Sys((N, direction)));
        if direction != RevDirection::NotLog {
            return;
        }

        // trigger observer in system
        world.rev_trigger(SysObsv(N));
    }

    /// Will not add sync point
    fn exclusive_system<const N: u8>(world: &mut World) {
        let direction = world.resource::<RevMeta>().direction();
        world
            .resource_mut::<TestLog>()
            .0
            .push(Test::Sys((N, direction)));
        if direction != RevDirection::NotLog {
            return;
        }

        // trigger observer in system
        world.rev_trigger(SysObsv(N));

        // trigger hook in system
        world.spawn(SysHook(N));
    }

    fn system_command<const N: u8>(world: &mut World) -> impl RevCommandLog {
        // trigger hook in command
        world.spawn(SysCmdHook(N));

        // trigger observer in command
        world.rev_trigger(SysCmdObsv(N));

        // todo: document that stuff like this belongs right before the return
        world
            .resource_mut::<TestLog>()
            .0
            .push(Test::SysCmd((N, RevDirection::NotLog)));

        |world: &mut World, forward: bool| {
            let direction = match forward {
                true => RevDirection::ForwardLog,
                false => RevDirection::BackwardLog,
            };
            world
                .resource_mut::<TestLog>()
                .0
                .push(Test::SysCmd((N, direction)));
        }
    }

    fn test_run(
        configs: impl FnOnce(&mut Schedule) -> &mut Schedule,
        expected: Vec<Vec<TestBundle>>,
    ) {
        // set up world
        let mut world = World::new();
        world.init_resource::<TestLog>();
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
        world.rev_observe(
            |trigger: Trigger<RevEvent<SysObsv>>, mut world: DeferredWorld| {
                let event = trigger.event();
                let direction = event.direction();
                let n = event.0;

                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::SysObsv((n, direction)));

                if direction != RevDirection::NotLog {
                    return;
                }

                // trigger observer in observer
                world.rev_trigger(SysObsvObsv(n));

                // trigger command in observer
                world.commands().rev_queue(move |world: &mut World| {
                    world
                        .resource_mut::<TestLog>()
                        .0
                        .push(Test::SysObsvCmd((n, RevDirection::NotLog)));

                    move |world: &mut World, forward: bool| {
                        let direction = match forward {
                            true => RevDirection::ForwardLog,
                            false => RevDirection::BackwardLog,
                        };
                        world
                            .resource_mut::<TestLog>()
                            .0
                            .push(Test::SysObsvCmd((n, direction)));
                    }
                });
            },
        );
        world.rev_observe(
            |trigger: Trigger<RevEvent<SysHookObsv>>, mut log: ResMut<TestLog>| {
                let event = trigger.event();
                log.0.push(Test::SysHookObsv((event.0, event.direction())));
            },
        );
        world.rev_observe(
            |trigger: Trigger<RevEvent<SysObsvObsv>>, mut log: ResMut<TestLog>| {
                let event = trigger.event();
                log.0.push(Test::SysObsvObsv((event.0, event.direction())));
            },
        );
        world.rev_observe(
            |trigger: Trigger<RevEvent<SysCmdObsv>>, mut log: ResMut<TestLog>| {
                let event = trigger.event();
                log.0.push(Test::SysCmdObsv((event.0, event.direction())));
            },
        );

        // set up hooks
        world.rev_register_component_hooks::<SysHook>().on_add(
            |direction, mut world, entity, _| {
                let Ok(direction): Result<RevDirection, _> = direction.try_into() else {
                    return;
                };
                let n = world.entity(entity).get::<SysHook>().expect("todo").0;
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::SysHook((n, direction)));

                if direction != RevDirection::NotLog {
                    return;
                }

                // trigger observer in hook
                world.rev_trigger(SysHookObsv(n));

                // trigger command in hook
                world.commands().rev_queue(move |world: &mut World| {
                    world
                        .resource_mut::<TestLog>()
                        .0
                        .push(Test::SysHookCmd((n, RevDirection::NotLog)));

                    move |world: &mut World, forward: bool| {
                        let direction = match forward {
                            true => RevDirection::ForwardLog,
                            false => RevDirection::BackwardLog,
                        };
                        world
                            .resource_mut::<TestLog>()
                            .0
                            .push(Test::SysHookCmd((n, direction)));
                    }
                });
            },
        );
        world.rev_register_component_hooks::<SysCmdHook>().on_add(
            |direction, mut world, entity, _| {
                let Ok(direction) = direction.try_into() else {
                    return;
                };
                let n = world.entity(entity).get::<SysCmdHook>().expect("todo").0;
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::SysCmdHook((n, direction)));
            },
        );

        fn test_step(
            world: &mut World,
            step: usize,
            expected: &Vec<TestBundle>,
            direction: RevDirection,
        ) {
            world.run_schedule(FixedUpdate);
            let actual = take(&mut world.resource_mut::<TestLog>().0);
            let iter = expected
                .iter()
                .flat_map(|bundle| bundle.into_iter())
                .map(|test| test.map(|n| (n, direction)));
            let expected: Vec<_> = if direction.is_forward() {
                iter.collect()
            } else {
                iter.rev().collect()
            };
            assert_eq!(actual, expected, "{direction:?} step #{step}");
        }

        // run tests forward
        for (step, expected) in expected.iter().enumerate() {
            test_step(&mut world, step, expected, RevDirection::NotLog);
        }

        // run tests backward log
        let mut meta = world.resource_mut::<RevMeta>();
        let end_frame = meta.present_world_state();
        assert!(meta.queue_log(RevFrame::new(0)).is_ok());
        for (step, expected) in expected.iter().enumerate().rev() {
            test_step(&mut world, step, expected, RevDirection::BackwardLog);
        }

        // run tests forward log
        let mut meta = world.resource_mut::<RevMeta>();
        assert!(meta.queue_log(end_frame).is_ok());
        for (step, expected) in expected.iter().enumerate() {
            test_step(&mut world, step, expected, RevDirection::ForwardLog);
        }
    }

    #[test]
    fn single_regular_system() {
        test_run(
            |schedule| schedule.rev_add_systems(regular_system::<1>),
            vec![vec![
                TestBundle::Regular(1),
                TestBundle::RegularSyncPoint(1),
            ]],
        );
    }

    #[test]
    fn single_exclusive_system() {
        test_run(
            |schedule| schedule.rev_add_systems(exclusive_system::<1>),
            vec![vec![TestBundle::Exclusive(1)]],
        );
    }

    #[test]
    fn regular_after_regular() {
        test_run(
            |schedule| schedule.rev_add_systems((
                regular_system::<1>,
                regular_system::<2>.rev_after(regular_system::<1>)
            )),
            vec![vec![
                TestBundle::Regular(1),
                TestBundle::RegularSyncPoint(1),
                TestBundle::Regular(2),
                TestBundle::RegularSyncPoint(2),
            ]],
        );
    }

    #[test]
    fn regular_before_regular() {
        test_run(
            |schedule| schedule.rev_add_systems((
                regular_system::<1>.rev_before(regular_system::<2>),
                regular_system::<2>
            )),
            vec![vec![
                TestBundle::Regular(1),
                TestBundle::RegularSyncPoint(1),
                TestBundle::Regular(2),
                TestBundle::RegularSyncPoint(2),
            ]],
        );
    }

    #[test]
    fn regular_chain() {
        test_run(
            |schedule| schedule.rev_add_systems((regular_system::<1>, regular_system::<2>).rev_chain()),
            vec![vec![
                TestBundle::Regular(1),
                TestBundle::RegularSyncPoint(1),
                TestBundle::Regular(2),
                TestBundle::RegularSyncPoint(2),
            ]],
        );
    }
}
