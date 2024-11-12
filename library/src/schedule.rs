use std::fmt::Debug;

use bevy::ecs::{
    change_detection::Res,
    schedule::{InternedSystemSet, IntoSystemSetConfigs, Schedule, SystemSet},
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
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct FwdSysSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// todo
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdCmdSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdSysSet(InternedSystemSet);

/// Subsets of [`BackwardSet`].
///
/// Each contains the system wrapped in `Arc`.
#[derive(SystemSet, Copy, Clone, Hash, PartialEq, Eq)]
struct BwdCmdSysSet(InternedSystemSet);

macro_rules! impl_set_debug {
    ($T: ident) => {
        impl Debug for $T {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let mut f = f.debug_tuple(std::any::type_name::<Self>());
                match self.0.system_type() {
                    None => f.field(&self.0).finish(),
                    Some(id) => f.field(&id).finish(),
                }
            }
        }
    };
}

impl_set_debug!(FwdSysSet);
impl_set_debug!(BwdCmdSet);
impl_set_debug!(BwdSysSet);
impl_set_debug!(BwdCmdSysSet);

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
        // configure sets first because that adds the base configs for the base sets
        self.rev_configure_sets(sets).add_systems(systems)
    }
    fn rev_configure_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self {
        if !self.graph().contains_set(ForwardSet) {
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
                    .chain()
                    .in_set(RevSystemsSet),
            );
        }
        let RevSystemSetConfigs {
            fwd_sys_sets,
            bwd_cmd_sets,
            bwd_sys_sets,
            bwd_cmd_sys_sets,
            condition_sets,
        } = sets.into_rev_configs();
        self.configure_sets((
            fwd_sys_sets,
            bwd_cmd_sets,
            bwd_sys_sets,
            bwd_cmd_sys_sets,
            condition_sets,
        ))
    }
}

#[cfg(test)]
mod test {
    use std::mem::take;

    use bevy::{
        app::{App, FixedUpdate},
        ecs::{
            change_detection::ResMut,
            component::Component,
            event::Event,
            observer::Trigger,
            schedule::IntoSystemSet,
            system::{Commands, Resource},
            world::{DeferredWorld, World},
        },
        utils::default,
    };

    use crate::{
        commands::{RevCommandLog, RevCommands},
        observer::RevEvent,
        world::{RevDeferredWorld, RevWorld},
        RevFrame, RevUpdate,
    };

    use super::*;

    #[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
    struct TestSet(u8);

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
        NonExclusive(u8),
        NonExclusiveSyncPoint(u8),
        Exclusive(u8),
    }

    impl IntoIterator for TestBundle {
        type IntoIter = std::vec::IntoIter<Test<u8>>;
        type Item = Test<u8>;
        fn into_iter(self) -> Self::IntoIter {
            match self {
                Self::NonExclusive(n) => vec![Test::Sys(n)],
                Self::NonExclusiveSyncPoint(n) => vec![
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
    fn non_exclusive_system<const N: u8>(
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

    fn test_run<C: for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>(
        configs: Vec<C>,
        expected: Vec<Vec<TestBundle>>,
    ) {
        for (variant, config) in configs.into_iter().enumerate() {
            // set up world
            let mut world = World::new();
            world.init_resource::<TestLog>();
            world.insert_resource(RevMeta::new(None, 0, false));
            let mut schedule = Schedule::new(FixedUpdate);
            schedule.add_systems(RevMeta::update_world);
            let err = schedule.initialize(&mut world).err();
            assert!(err.is_none(), "{:?}", err.unwrap());
            world.add_schedule(schedule);

            // set up reversible schedule
            let mut schedule = Schedule::new(RevUpdate);
            config(&mut schedule);
            let err = schedule.initialize(&mut world).err();
            assert!(err.is_none(), "{:?}", err.unwrap());
            world.add_schedule(schedule);

            // set up observers
            world.rev_add_observer(
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
            world.rev_add_observer(
                |trigger: Trigger<RevEvent<SysHookObsv>>, mut log: ResMut<TestLog>| {
                    let event = trigger.event();
                    log.0.push(Test::SysHookObsv((event.0, event.direction())));
                },
            );
            world.rev_add_observer(
                |trigger: Trigger<RevEvent<SysObsvObsv>>, mut log: ResMut<TestLog>| {
                    let event = trigger.event();
                    log.0.push(Test::SysObsvObsv((event.0, event.direction())));
                },
            );
            world.rev_add_observer(
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

            fn test_step<C: for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>(
                world: &mut World,
                config: &C,
                variant: usize,
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
                if actual != expected {
                    let mut schedule = Schedule::new(RevUpdate);
                    config(&mut schedule);
                    let mut app = App::new();
                    app.add_schedule(schedule);
                    let graph =
                        bevy_mod_debugdump::schedule_graph_dot(&mut app, RevUpdate, &default());
                    panic!("log mismatch! config #{variant}, {direction:?}, step #{step}\nexpected:\n{expected:?}\nactual:\n{actual:?}\n\n{graph}")
                }
            }

            // run tests forward
            for (step, expected) in expected.iter().enumerate() {
                test_step(
                    &mut world,
                    &config,
                    variant,
                    step,
                    expected,
                    RevDirection::NotLog,
                );
            }

            // run tests backward log
            let mut meta = world.resource_mut::<RevMeta>();
            let end_frame = meta.present_world_state();
            assert!(meta.queue_log(RevFrame::new(0)).is_ok());
            for (step, expected) in expected.iter().enumerate().rev() {
                test_step(
                    &mut world,
                    &config,
                    variant,
                    step,
                    expected,
                    RevDirection::BackwardLog,
                );
            }

            // run tests forward log
            let mut meta = world.resource_mut::<RevMeta>();
            assert!(meta.queue_log(end_frame).is_ok());
            for (step, expected) in expected.iter().enumerate() {
                test_step(
                    &mut world,
                    &config,
                    variant,
                    step,
                    expected,
                    RevDirection::ForwardLog,
                );
            }
        }
    }

    fn a_then_b(
        a_exclusive: bool,
        b_exclusive: bool,
        ignore_deferred: bool,
    ) -> Vec<Box<dyn for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>> {
        let sys_a: fn() -> RevSystemConfigs;
        let sys_b: fn() -> RevSystemConfigs;

        let set_a: InternedSystemSet;
        let set_b: InternedSystemSet;

        let sys_after: fn(RevSystemConfigs, InternedSystemSet) -> RevSystemConfigs;
        let sys_before: fn(RevSystemConfigs, InternedSystemSet) -> RevSystemConfigs;
        let sys_chain: fn(RevSystemConfigs) -> RevSystemConfigs;

        let set_after: fn(RevSystemSetConfigs, InternedSystemSet) -> RevSystemSetConfigs;
        let set_before: fn(RevSystemSetConfigs, InternedSystemSet) -> RevSystemSetConfigs;
        let set_chain: fn(RevSystemSetConfigs) -> RevSystemSetConfigs;

        if a_exclusive {
            sys_a = || exclusive_system::<1>.into_rev_configs();
            set_a = exclusive_system::<1>.into_system_set().intern();
        } else {
            sys_a = || non_exclusive_system::<1>.into_rev_configs();
            set_a = non_exclusive_system::<1>.into_system_set().intern();
        };
        if b_exclusive {
            sys_b = || exclusive_system::<2>.into_rev_configs();
            set_b = exclusive_system::<2>.into_system_set().intern();
        } else {
            sys_b = || non_exclusive_system::<2>.into_rev_configs();
            set_b = non_exclusive_system::<2>.into_system_set().intern();
        };
        if ignore_deferred {
            sys_after = |sys, set| sys.rev_after_ignore_deferred(set);
            sys_before = |sys, set| sys.rev_before_ignore_deferred(set);
            sys_chain = |sys| sys.rev_chain_ignore_deferred();

            set_after = |sys, set| sys.rev_after_ignore_deferred(set);
            set_before = |sys, set| sys.rev_before_ignore_deferred(set);
            set_chain = |sys| sys.rev_chain_ignore_deferred();
        } else {
            sys_after = |sys, set| sys.rev_after(set);
            sys_before = |sys, set| sys.rev_before(set);
            sys_chain = |sys| sys.rev_chain();

            set_after = |sys, set| sys.rev_after(set);
            set_before = |sys, set| sys.rev_before(set);
            set_chain = |sys| sys.rev_chain();
        }
        vec![
            // #0 system after system
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((sys_a(), sys_after(sys_b(), set_a)))
            }),
            // #1 system after system (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((sys_after(sys_b(), set_a), sys_a()))
            }),
            // #2 set after system
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((sys_a(), sys_b().rev_in_set(TestSet(2))))
                    .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_a))
            }),
            // #3 set after system (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((sys_b().rev_in_set(TestSet(2)), sys_a()))
                    .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_a))
            }),
            // #4 system after set
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a().rev_in_set(TestSet(1)),
                    sys_after(sys_b(), TestSet(1).intern()),
                ))
            }),
            // #5 system after set (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_after(sys_b(), TestSet(1).intern()),
                    sys_a().rev_in_set(TestSet(1)),
                ))
            }),
            // #6 set after set
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a().rev_in_set(TestSet(1)),
                        sys_b().rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(set_after(
                        TestSet(2).into_rev_configs(),
                        TestSet(1).intern(),
                    ))
            }),
            // #6 set after set (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b().rev_in_set(TestSet(2)),
                        sys_a().rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(set_after(
                        TestSet(2).into_rev_configs(),
                        TestSet(1).intern(),
                    ))
            }),
            // #7 system before system
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((sys_before(sys_a(), set_b), sys_b()))
            }),
            // #8 system before system (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((sys_b(), sys_before(sys_a(), set_b)))
            }),
            // #9 set before system
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((sys_a().rev_in_set(TestSet(1)), sys_b()))
                    .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_b))
            }),
            // #10 set before system (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((sys_b(), sys_a().rev_in_set(TestSet(1))))
                    .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_b))
            }),
            // #11 system before set
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_before(sys_a(), TestSet(2).intern()),
                    sys_b().rev_in_set(TestSet(2)),
                ))
            }),
            // #12 system before set (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b().rev_in_set(TestSet(2)),
                    sys_before(sys_a(), TestSet(2).intern()),
                ))
            }),
            // #13 set before set
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a().rev_in_set(TestSet(1)),
                        sys_b().rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(set_before(
                        TestSet(1).into_rev_configs(),
                        TestSet(2).intern(),
                    ))
            }),
            // #14 set before set (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b().rev_in_set(TestSet(2)),
                        sys_a().rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(set_before(
                        TestSet(1).into_rev_configs(),
                        TestSet(2).intern(),
                    ))
            }),
            // #15 system chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems(sys_chain((sys_a(), sys_b()).into_rev_configs()))
            }),
            // #16 set chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a().rev_in_set(TestSet(1)),
                        sys_b().rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(set_chain((TestSet(1), TestSet(2)).into_rev_configs()))
            }),
            // #17 set chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b().rev_in_set(TestSet(2)),
                        sys_a().rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(set_chain((TestSet(1), TestSet(2)).into_rev_configs()))
            }),
        ]
    }

    #[test]
    fn single_non_exclusive_system() {
        fn configs(schedule: &mut Schedule) -> &mut Schedule {
            schedule.rev_add_systems(non_exclusive_system::<1>)
        }
        test_run(
            vec![configs],
            vec![vec![
                TestBundle::NonExclusive(1),
                TestBundle::NonExclusiveSyncPoint(1),
            ]],
        );
    }

    #[test]
    fn single_exclusive_system() {
        fn configs(schedule: &mut Schedule) -> &mut Schedule {
            schedule.rev_add_systems(exclusive_system::<1>)
        }
        test_run(vec![configs], vec![vec![TestBundle::Exclusive(1)]]);
    }

    #[test]
    fn non_exclusive_then_non_exclusive() {
        test_run(
            a_then_b(false, false, false),
            vec![vec![
                TestBundle::NonExclusive(1),
                TestBundle::NonExclusiveSyncPoint(1),
                TestBundle::NonExclusive(2),
                TestBundle::NonExclusiveSyncPoint(2),
            ]],
        )
    }

    #[test]
    fn exclusive_then_non_exclusive() {
        test_run(
            a_then_b(true, false, false),
            vec![vec![
                TestBundle::Exclusive(1),
                TestBundle::NonExclusive(2),
                TestBundle::NonExclusiveSyncPoint(2),
            ]],
        )
    }

    #[test]
    fn non_exclusive_then_exclusive() {
        test_run(
            a_then_b(false, true, false),
            vec![vec![
                TestBundle::NonExclusive(1),
                TestBundle::NonExclusiveSyncPoint(1),
                TestBundle::Exclusive(2),
            ]],
        )
    }

    #[test]
    fn exclusive_then_exclusive() {
        test_run(
            a_then_b(true, true, false),
            vec![vec![TestBundle::Exclusive(1), TestBundle::Exclusive(2)]],
        )
    }

    #[test]
    fn non_exclusive_then_non_exclusive_ignore_deferred() {
        test_run(
            a_then_b(false, false, true),
            vec![vec![
                TestBundle::NonExclusive(1),
                TestBundle::NonExclusive(2),
                TestBundle::NonExclusiveSyncPoint(1),
                TestBundle::NonExclusiveSyncPoint(2),
            ]],
        )
    }

    #[test]
    fn exclusive_then_non_exclusive_ignore_deferred() {
        test_run(
            a_then_b(true, false, true),
            vec![vec![
                TestBundle::Exclusive(1),
                TestBundle::NonExclusive(2),
                TestBundle::NonExclusiveSyncPoint(2),
            ]],
        )
    }

    #[test]
    fn non_exclusive_then_exclusive_ignore_deferred() {
        // Problem: exclusive system triggert irgendwo world.flush oä
        test_run(
            a_then_b(false, true, true),
            vec![vec![
                TestBundle::NonExclusive(1),
                TestBundle::Exclusive(2),
                TestBundle::NonExclusiveSyncPoint(1),
            ]],
        )
    }

    #[test]
    fn exclusive_then_exclusive_ignore_deferred() {
        test_run(
            a_then_b(true, true, true),
            vec![vec![TestBundle::Exclusive(1), TestBundle::Exclusive(2)]],
        )
    }

    #[test]
    fn run_if() {
        fn at_2(meta: Res<RevMeta>) -> bool {
            let now: usize = meta.present_world_state().into();
            now == 2
        }
        fn config0(schedule: &mut Schedule) -> &mut Schedule {
            schedule.rev_add_systems(non_exclusive_system::<1>.rev_run_if(at_2))
        }
        fn config1(schedule: &mut Schedule) -> &mut Schedule {
            schedule
                .rev_add_systems(non_exclusive_system::<1>.rev_in_set(TestSet(1)))
                .rev_configure_sets(TestSet(1).rev_run_if(at_2))
        }
        test_run(
            vec![config0, config1],
            vec![
                vec![], // does not run at 1
                vec![
                    TestBundle::NonExclusive(1),
                    TestBundle::NonExclusiveSyncPoint(1),
                ],
                vec![], // does not run at 3
            ],
        );
    }
}
