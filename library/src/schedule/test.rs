use std::{
    mem::take,
    num::NonZeroU32,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use bevy::{
    app::FixedUpdate,
    ecs::{
        change_detection::{Res, ResMut},
        component::Component,
        event::Event,
        observer::Trigger,
        schedule::{common_conditions::run_once, IntoSystemSet},
        system::{Commands, IntoSystem, Resource},
        world::{DeferredWorld, World},
    },
};

use crate::{
    frame::RevFrame,
    meta::RevDirection,
    schedule::RevUpdate,
    undo_redo::{BuffersUndoRedo, UndoRedo, UndoRedoBuffer, UndoRedoDirection},
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

impl UndoRedo for Test<u8> {
    fn undo(&mut self, world: &mut World) {
        world
            .resource_mut::<TestLog>()
            .0
            .push(self.map(|n| (n, RevDirection::BackwardLog)));
    }
    fn redo(&mut self, world: &mut World) {
        world
            .resource_mut::<TestLog>()
            .0
            .push(self.map(|n| (n, RevDirection::FORWARD_LOG)));
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
    direction: RevDirection,
    mut log: ResMut<TestLog>,
    commands: Commands,
) {
    log.0.push(Test::Sys((N, direction)));

    non_exclusive_system_commands_only::<N>(direction, commands);
}

fn non_exclusive_system_commands_only<const N: u8>(
    direction: RevDirection,
    mut commands: Commands,
) {
    if direction != RevDirection::NOT_LOG {
        return;
    }

    // trigger observer in system
    commands.trigger(SysObsv(N));

    // trigger hook in system
    commands.spawn(SysHook(N));

    // trigger command in system
    commands.queue(system_command::<N>);
}

/// Will not add sync point
fn exclusive_system<const N: u8>(world: &mut World) {
    let direction = world.resource::<RevMeta>().direction();
    world
        .resource_mut::<TestLog>()
        .0
        .push(Test::Sys((N, direction)));
    if direction != RevDirection::NOT_LOG {
        return;
    }

    // trigger observer in system
    world.trigger(SysObsv(N));

    // trigger hook in system
    world.spawn(SysHook(N));
}

fn system_command<const N: u8>(world: &mut World) {
    // trigger hook in command
    world.spawn(SysCmdHook(N));

    // trigger observer in command
    world.trigger(SysCmdObsv(N));

    // todo: document that stuff like this belongs right before the return
    world
        .resource_mut::<TestLog>()
        .0
        .push(Test::SysCmd((N, RevDirection::NOT_LOG)));

    world.buffer_undo_redo(Test::SysCmd(N));
}

fn test_run<C: for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>(
    configs: Vec<C>,
    expected: Vec<Vec<TestBundle>>,
) {
    for (variant, config) in configs.into_iter().enumerate() {
        for apply_final_deferred in [true, false] {
            test_run_variant(variant, &config, apply_final_deferred, &expected);
        }
    }
}

fn test_run_variant<C: for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>(
    variant: usize,
    config: &C,
    apply_final_deferred: bool,
    expected: &Vec<Vec<TestBundle>>,
) {
    // set up world
    let mut world = World::new();
    world.init_resource::<TestLog>();
    world.insert_resource(RevMeta::new(None, None, false));

    // set up schedules
    let mut schedule = Schedule::new(FixedUpdate);
    schedule.add_systems(RevMeta::run_rev_update);
    let err = schedule.initialize(&mut world).err();
    assert!(err.is_none(), "{:?}", err.unwrap());
    world.add_schedule(schedule);

    let mut schedule = Schedule::new(RevUpdate);
    config(&mut schedule);
    schedule.set_apply_final_deferred(apply_final_deferred);
    let err = schedule.initialize(&mut world).err();
    assert!(err.is_none(), "{:?}", err.unwrap());
    world.add_schedule(schedule);

    // set up observers
    world.add_observer(|event: Trigger<SysObsv>, mut world: DeferredWorld| {
        let n = event.0;

        world
            .resource_mut::<TestLog>()
            .0
            .push(Test::SysObsv((n, RevDirection::NOT_LOG)));

        // trigger observer in observer
        world.trigger::<SysObsvObsv>(SysObsvObsv(n));

        // trigger command in observer
        world.commands().queue(move |world: &mut World| {
            world
                .resource_mut::<TestLog>()
                .0
                .push(Test::SysObsvCmd((n, RevDirection::NOT_LOG)));

            world.buffer_undo_redo(Test::SysObsvCmd(n));
        });

        // buffer reversible observer
        world.buffer_undo_redo(Test::SysObsv(n));
    });
    world.add_observer(
        |event: Trigger<SysHookObsv>,
         mut log: ResMut<TestLog>,
         mut buffer: ResMut<UndoRedoBuffer>| {
            let n = event.0;
            log.0.push(Test::SysHookObsv((n, RevDirection::NOT_LOG)));
            buffer.buffer_undo_redo(Test::SysHookObsv(n));
        },
    );
    world.add_observer(
        |event: Trigger<SysObsvObsv>,
         mut log: ResMut<TestLog>,
         mut buffer: ResMut<UndoRedoBuffer>| {
            let n = event.0;
            log.0.push(Test::SysObsvObsv((n, RevDirection::NOT_LOG)));
            buffer.buffer_undo_redo(Test::SysObsvObsv(n));
        },
    );
    world.add_observer(
        |event: Trigger<SysCmdObsv>,
         mut log: ResMut<TestLog>,
         mut buffer: ResMut<UndoRedoBuffer>| {
            let n = event.0;
            log.0.push(Test::SysCmdObsv((n, RevDirection::NOT_LOG)));
            buffer.buffer_undo_redo(Test::SysCmdObsv(n));
        },
    );

    // set up hooks
    world
        .register_component_hooks::<SysHook>()
        .on_add(|mut world, entity, _| {
            let n = world.entity(entity).get::<SysHook>().unwrap().0;
            world
                .resource_mut::<TestLog>()
                .0
                .push(Test::SysHook((n, RevDirection::NOT_LOG)));

            // trigger observer in hook
            world.trigger::<SysHookObsv>(SysHookObsv(n));

            // trigger command in hook
            world.commands().queue(move |world: &mut World| {
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(Test::SysHookCmd((n, RevDirection::NOT_LOG)));

                world.buffer_undo_redo(Test::SysHookCmd(n));
            });

            // buffer reversible hook
            world.buffer_undo_redo(Test::SysHook(n));
        });
    world
        .register_component_hooks::<SysCmdHook>()
        .on_add(|mut world, entity, _| {
            let n = world.entity(entity).get::<SysCmdHook>().expect("todo").0;
            world
                .resource_mut::<TestLog>()
                .0
                .push(Test::SysCmdHook((n, RevDirection::NOT_LOG)));

            // buffer reversible hook
            world.buffer_undo_redo(Test::SysCmdHook(n));
        });

    fn test_step(
        world: &mut World,
        variant: usize,
        apply_final_deferred: bool,
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
        assert_eq!(actual, expected, "log mismatch! config #{variant}, apply_final_deferred {apply_final_deferred}, {direction:?}, step #{step}");
    }

    // run tests forward
    for (step, expected) in expected.iter().enumerate() {
        test_step(
            &mut world,
            variant,
            apply_final_deferred,
            step,
            expected,
            RevDirection::NOT_LOG,
        );
    }

    // run tests backward log
    let mut meta = world.resource_mut::<RevMeta>();
    let end_frame = meta.present_world_state();
    assert!(
        meta.queue_log(RevFrame::checked_new(0)).is_ok(),
        "{meta:#?}"
    );
    for (step, expected) in expected.iter().enumerate().rev() {
        test_step(
            &mut world,
            variant,
            apply_final_deferred,
            step,
            expected,
            RevDirection::BackwardLog,
        );
    }

    // run tests forward log
    let mut meta = world.resource_mut::<RevMeta>();
    assert!(meta.queue_log(end_frame).is_ok(), "{meta:#?}");
    for (step, expected) in expected.iter().enumerate() {
        test_step(
            &mut world,
            variant,
            apply_final_deferred,
            step,
            expected,
            RevDirection::FORWARD_LOG,
        );
    }
}

fn a_then_b(
    a_exclusive: bool,
    b_exclusive: bool,
    ignore_deferred: bool,
) -> Vec<Box<dyn for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>> {
    fn noop<const N: u8>() {}

    let sys_a: fn() -> RevSystemConfigs;
    let sys_a_pipe_noop: fn() -> RevSystemConfigs;
    let noop_pipe_sys_a: fn() -> RevSystemConfigs;

    let sys_b: fn() -> RevSystemConfigs;
    let sys_b_pipe_noop: fn() -> RevSystemConfigs;
    let noop_pipe_sys_b: fn() -> RevSystemConfigs;

    let set_sys_a: InternedSystemSet;
    let set_noop_a = noop::<3>.into_system_set().intern();
    let set_sys_b: InternedSystemSet;
    let set_noop_b = noop::<4>.into_system_set().intern();

    let sys_after: fn(RevSystemConfigs, InternedSystemSet) -> RevSystemConfigs;
    let sys_before: fn(RevSystemConfigs, InternedSystemSet) -> RevSystemConfigs;
    let sys_chain: fn(RevSystemConfigs) -> RevSystemConfigs;

    let set_after: fn(RevSystemSetConfigs, InternedSystemSet) -> RevSystemSetConfigs;
    let set_before: fn(RevSystemSetConfigs, InternedSystemSet) -> RevSystemSetConfigs;
    let set_chain: fn(RevSystemSetConfigs) -> RevSystemSetConfigs;

    if a_exclusive {
        sys_a = || exclusive_system::<1>.into_rev_configs();
        sys_a_pipe_noop = || exclusive_system::<1>.pipe(noop::<3>).into_rev_configs();
        noop_pipe_sys_a = || noop::<3>.pipe(exclusive_system::<1>).into_rev_configs();
        set_sys_a = exclusive_system::<1>.into_system_set().intern();
    } else {
        sys_a = || non_exclusive_system::<1>.into_rev_configs();
        sys_a_pipe_noop = || non_exclusive_system::<1>.pipe(noop::<3>).into_rev_configs();
        noop_pipe_sys_a = || noop::<3>.pipe(non_exclusive_system::<1>).into_rev_configs();
        set_sys_a = non_exclusive_system::<1>.into_system_set().intern();
    };

    if b_exclusive {
        sys_b = || exclusive_system::<2>.into_rev_configs();
        sys_b_pipe_noop = || exclusive_system::<2>.pipe(noop::<4>).into_rev_configs();
        noop_pipe_sys_b = || noop::<4>.pipe(exclusive_system::<2>).into_rev_configs();
        set_sys_b = exclusive_system::<2>.into_system_set().intern();
    } else {
        sys_b = || non_exclusive_system::<2>.into_rev_configs();
        sys_b_pipe_noop = || non_exclusive_system::<2>.pipe(noop::<4>).into_rev_configs();
        noop_pipe_sys_b = || noop::<4>.pipe(non_exclusive_system::<2>).into_rev_configs();
        set_sys_b = non_exclusive_system::<2>.into_system_set().intern();
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
            schedule.rev_add_systems((sys_a(), sys_after(sys_b(), set_sys_a)))
        }),
        // #1 system after system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_after(sys_b(), set_sys_a), sys_a()))
        }),
        // #2 system after system-noop pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_a_pipe_noop(), sys_after(sys_b(), set_sys_a)))
        }),
        // #3 system after system-noop pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_after(sys_b(), set_sys_a), sys_a_pipe_noop()))
        }),
        // #4 system after system-noop pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_a_pipe_noop(), sys_after(sys_b(), set_noop_a)))
        }),
        // #5 system after system-noop pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_after(sys_b(), set_noop_a), sys_a_pipe_noop()))
        }),
        // #6 system after noop-system pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((noop_pipe_sys_a(), sys_after(sys_b(), set_sys_a)))
        }),
        // #7 system after noop-system pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_after(sys_b(), set_sys_a), noop_pipe_sys_a()))
        }),
        // #8 system after noop-system pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((noop_pipe_sys_a(), sys_after(sys_b(), set_noop_a)))
        }),
        // #9 system after noop-system pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_after(sys_b(), set_noop_a), noop_pipe_sys_a()))
        }),
        // #10 set after system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a(), sys_b().rev_in_set(TestSet(2))))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_sys_a))
        }),
        // #11 set after system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b().rev_in_set(TestSet(2)), sys_a()))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_sys_a))
        }),
        // #12 set after system-noop pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a_pipe_noop(), sys_b().rev_in_set(TestSet(2))))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_sys_a))
        }),
        // #13 set after system-noop pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b().rev_in_set(TestSet(2)), sys_a_pipe_noop()))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_sys_a))
        }),
        // #14 set after system-noop pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a_pipe_noop(), sys_b().rev_in_set(TestSet(2))))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_noop_a))
        }),
        // #15 set after system-noop pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b().rev_in_set(TestSet(2)), sys_a_pipe_noop()))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_noop_a))
        }),
        // #16 system after set
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_a().rev_in_set(TestSet(1)),
                sys_after(sys_b(), TestSet(1).intern()),
            ))
        }),
        // #17 system after set (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_after(sys_b(), TestSet(1).intern()),
                sys_a().rev_in_set(TestSet(1)),
            ))
        }),
        // #18 set after set
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
        // #19 set after set (flipped)
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
        // #20 system before system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_sys_b), sys_b()))
        }),
        // #21 system before system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_b(), sys_before(sys_a(), set_sys_b)))
        }),
        // #22 system before system-noop pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_sys_b), sys_b_pipe_noop()))
        }),
        // #23 system before system-noop pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_b_pipe_noop(), sys_before(sys_a(), set_sys_b)))
        }),
        // #24 system before system-noop pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_noop_b), sys_b_pipe_noop()))
        }),
        // #25 system before system-noop pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_b_pipe_noop(), sys_before(sys_a(), set_noop_b)))
        }),
        // #26 system before noop-system pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_sys_b), noop_pipe_sys_b()))
        }),
        // #27 system before noop-system pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((noop_pipe_sys_b(), sys_before(sys_a(), set_sys_b)))
        }),
        // #28 system before noop-system pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_noop_b), noop_pipe_sys_b()))
        }),
        // #29 system before noop-system pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((noop_pipe_sys_b(), sys_before(sys_a(), set_noop_b)))
        }),
        // #30 set before system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), sys_b()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #31 set before system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #32 set before system-noop pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), sys_b_pipe_noop()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #33 set before system-noop pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b_pipe_noop(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #34 set before system-noop pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), sys_b_pipe_noop()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #35 set before system-noop pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b_pipe_noop(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #36 set before noop-system pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), noop_pipe_sys_b()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #37 set before noop-system pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((noop_pipe_sys_b(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #38 set before noop-system pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), noop_pipe_sys_b()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #39 set before noop-system pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((noop_pipe_sys_b(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #40 system before set
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_before(sys_a(), TestSet(2).intern()),
                sys_b().rev_in_set(TestSet(2)),
            ))
        }),
        // #41 system before set (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_b().rev_in_set(TestSet(2)),
                sys_before(sys_a(), TestSet(2).intern()),
            ))
        }),
        // #42 set before set
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
        // #43 set before set (flipped)
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
        // #44 system chain
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems(sys_chain((sys_a(), sys_b()).into_rev_configs()))
        }),
        // #45 set chain
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((
                    sys_a().rev_in_set(TestSet(1)),
                    sys_b().rev_in_set(TestSet(2)),
                ))
                .rev_configure_sets(set_chain((TestSet(1), TestSet(2)).into_rev_configs()))
        }),
        // #46 set chain (flipped)
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
#[ignore] // TODO: Find cause and maybe report an issue at bevy.
fn non_exclusive_then_exclusive_ignore_deferred() {
    // For some reason bevy adds a sync point between non_exclusive::<1> and fwd exclusive::<2> in ForwardSet.
    // The implemention is simple here, using the vanilla before_ignore_deferred in the case of config #0.
    // The sync point disappears if the BackwardSet is empty.
    // Chaining ForwardSet and BackwardSet has no effect.
    // bevy_mod_debugdump does not show other causes for this sync point.
    // All configs from a_then_b are affected, though not the flipped variants.
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
fn pipe_commands() {
    fn configs(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(
            non_exclusive_system_commands_only::<1>.pipe(non_exclusive_system_commands_only::<2>),
        )
    }
    test_run(
        vec![configs],
        vec![vec![
            TestBundle::NonExclusiveSyncPoint(1),
            TestBundle::NonExclusiveSyncPoint(2),
        ]],
    )
}

#[test]
fn run_if() {
    fn at_2(meta: Res<RevMeta>) -> bool {
        let now: u32 = meta.present_world_state().into();
        now == 2
    }
    fn at_2_once() -> impl Fn(Res<RevMeta>) -> bool + Clone {
        let was_2 = Arc::new(AtomicBool::new(false));
        move |meta| {
            if was_2.load(Ordering::Relaxed) {
                return false;
            }
            let at_2 = at_2(meta);
            was_2.store(at_2, Ordering::Relaxed);
            at_2
        }
    }
    fn config0(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(non_exclusive_system::<1>.rev_run_if(at_2))
    }
    fn config1(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(non_exclusive_system::<1>.rev_distributive_run_if(at_2))
    }
    fn config2(schedule: &mut Schedule) -> &mut Schedule {
        schedule
            .rev_add_systems(non_exclusive_system::<1>.rev_in_set(TestSet(1)))
            .rev_configure_sets(TestSet(1).rev_run_if(at_2))
    }
    fn config3(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(
            (non_exclusive_system::<1>, exclusive_system::<2>)
                .rev_chain()
                .rev_distributive_run_if(at_2_once()),
        )
    }
    fn config4(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(
            (non_exclusive_system::<1>, exclusive_system::<2>)
                .rev_distributive_run_if(at_2_once())
                .rev_chain(),
        )
    }
    test_run(
        vec![config0, config1, config2, config3, config4],
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

#[test]
fn finalize_undo_redo() {
    #[derive(Resource, Copy, Clone, Debug, PartialEq)]
    enum TestCommand {
        Applied,
        Finalized,
    }

    fn system(mut commands: Commands) {
        commands.queue(|world: &mut World| {
            world.insert_resource(TestCommand::Applied);
            world.buffer_undo_redo(|world: &mut World, finalize: UndoRedoDirection| {
                if finalize == UndoRedoDirection::FinalizeRedone {
                    world.insert_resource(TestCommand::Finalized);
                }
            });
        });
    }

    // setup world
    let mut world = World::new();
    world.insert_resource(RevMeta::new(
        NonZeroU32::new(1),
        Some(RevFrame::checked_new(RevMeta::MAX_WORLD_STATES - 2)),
        false,
    ));

    // setup schedules
    let mut schedule = Schedule::new(FixedUpdate);
    schedule.add_systems(RevMeta::run_rev_update);
    let err = schedule.initialize(&mut world).err();
    assert!(err.is_none(), "{:?}", err.unwrap());
    world.add_schedule(schedule);

    let mut schedule = Schedule::new(RevUpdate);
    schedule.rev_add_systems(system.rev_run_if(run_once));
    let err = schedule.initialize(&mut world).err();
    assert!(err.is_none(), "{:?}", err.unwrap());
    world.add_schedule(schedule);

    // first updates issues command that sets the Applied resource
    world.run_schedule(FixedUpdate);
    assert_eq!(world.remove_resource(), Some(TestCommand::Applied));

    // no system run and no event to check the commands log because log start is still <= MAX_WORLD_STATES
    world.run_schedule(FixedUpdate);
    assert_eq!(world.remove_resource::<TestCommand>(), None);

    // no system run but an event to check the commands log because log start is MAX_WORLD_STATES + 1
    world.run_schedule(FixedUpdate);
    assert_eq!(world.remove_resource(), Some(TestCommand::Finalized));
}
