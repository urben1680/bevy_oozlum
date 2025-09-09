use std::mem::take;

use bevy::{
    app::{App, FixedUpdate},
    ecs::{
        change_detection::ResMut,
        observer::On,
        schedule::{ApplyDeferred, IntoSystemSet, LogLevel, ScheduleBuildSettings},
        system::IntoSystem,
        world::{DeferredWorld, World},
    },
};

use crate::{
    meta::RevDirection,
    panic_on_error_events,
    schedule::RevUpdate,
    undo_redo::{BuffersUndoRedo, UndoRedoBuffer},
};

use super::*;

pub(super) fn test_run<C: for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>(
    configs: Vec<C>,
    expected: Vec<Vec<Test>>,
) {
    panic_on_error_events();
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
    expected: &Vec<Vec<Test>>,
) {
    // set up world
    let mut world = World::new();
    world.init_resource::<TestLog>();
    world.insert_resource(RevMeta::new(None, false));

    // set up schedules
    let mut schedule = Schedule::new(FixedUpdate);
    schedule.add_systems(RevMeta::try_run_rev_update);
    let err = schedule.initialize(&mut world).err();
    assert!(
        err.is_none(),
        "FixedUpdate init fail: {:?}\nconfig: {variant}\napply_final_deferred: {apply_final_deferred}",
        err.unwrap()
    );
    world.add_schedule(schedule);

    let mut schedule = Schedule::new(RevUpdate);
    let settings = schedule.get_build_settings();
    schedule.set_build_settings(ScheduleBuildSettings {
        hierarchy_detection: LogLevel::Error,
        ..settings
    });
    config(&mut schedule);
    schedule.set_apply_final_deferred(apply_final_deferred);
    let err = schedule.initialize(&mut world).err();
    assert!(
        err.is_none(),
        "RevUpdate init fail: {:?}\nconfig: {variant}\napply_final_deferred: {apply_final_deferred}",
        err.unwrap()
    );
    world.add_schedule(schedule);

    // set up observers
    world.add_observer(|event: On<SysObsv>, mut world: DeferredWorld| {
        let n = event.0;

        world
            .resource_mut::<TestLog>()
            .0
            .push(LogEntry::SysObsv((n, RevDirection::NOT_LOG)));

        // trigger observer in observer
        world.trigger(SysObsvObsv(n));

        let now = world.resource::<RevMeta>().non_log_now().unwrap();

        // trigger command in observer
        world.commands().queue(move |world: &mut World| {
            world
                .resource_mut::<TestLog>()
                .0
                .push(LogEntry::SysObsvCmd((n, RevDirection::NOT_LOG)));

            let test = LogEntry::SysObsvCmd(n);
            world.buffer_undo_redo(now, test);
        });

        // buffer reversible observer
        let test = LogEntry::SysObsv(n);
        world.buffer_undo_redo(now, test);
    });
    world.add_observer(
        |event: On<SysHookObsv>,
         mut log: ResMut<TestLog>,
         meta: Res<RevMeta>,
         mut buffer: ResMut<UndoRedoBuffer>| {
            let n = event.0;
            log.0
                .push(LogEntry::SysHookObsv((n, RevDirection::NOT_LOG)));
            let test = LogEntry::SysHookObsv(n);
            let now = meta.non_log_now().unwrap();
            buffer.buffer_undo_redo(now, test);
        },
    );
    world.add_observer(
        |event: On<SysObsvObsv>,
         mut log: ResMut<TestLog>,
         meta: Res<RevMeta>,
         mut buffer: ResMut<UndoRedoBuffer>| {
            let n = event.0;
            log.0
                .push(LogEntry::SysObsvObsv((n, RevDirection::NOT_LOG)));
            let test = LogEntry::SysObsvObsv(n);
            let now = meta.non_log_now().unwrap();
            buffer.buffer_undo_redo(now, test);
        },
    );
    world.add_observer(
        |event: On<SysCmdObsv>,
         mut log: ResMut<TestLog>,
         meta: Res<RevMeta>,
         mut buffer: ResMut<UndoRedoBuffer>| {
            let n = event.0;
            log.0.push(LogEntry::SysCmdObsv((n, RevDirection::NOT_LOG)));
            let test = LogEntry::SysCmdObsv(n);
            let now = meta.non_log_now().unwrap();
            buffer.buffer_undo_redo(now, test);
        },
    );

    // set up hooks
    world
        .register_component_hooks::<SysHook>()
        .on_add(|mut world, hook| {
            let n = world.entity(hook.entity).get::<SysHook>().unwrap().0;
            world
                .resource_mut::<TestLog>()
                .0
                .push(LogEntry::SysHook((n, RevDirection::NOT_LOG)));

            // trigger observer in hook
            world.trigger(SysHookObsv(n));

            let now = world.resource::<RevMeta>().non_log_now().unwrap();

            // trigger command in hook
            world.commands().queue(move |world: &mut World| {
                world
                    .resource_mut::<TestLog>()
                    .0
                    .push(LogEntry::SysHookCmd((n, RevDirection::NOT_LOG)));

                let test = LogEntry::SysHookCmd(n);
                world.buffer_undo_redo(now, test);
            });

            // buffer reversible hook
            let test = LogEntry::SysHook(n);
            world.buffer_undo_redo(now, test);
        });
    world
        .register_component_hooks::<SysCmdHook>()
        .on_add(|mut world, hook| {
            let n = world
                .entity(hook.entity)
                .get::<SysCmdHook>()
                .expect("todo")
                .0;
            world
                .resource_mut::<TestLog>()
                .0
                .push(LogEntry::SysCmdHook((n, RevDirection::NOT_LOG)));

            // buffer reversible hook
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let test = LogEntry::SysCmdHook(n);
            world.buffer_undo_redo(now, test);
        });

    // run tests forward
    for (step, expected) in expected.iter().enumerate() {
        test_step(
            &mut world,
            variant,
            config,
            apply_final_deferred,
            step,
            expected,
            RevDirection::NOT_LOG,
        );
    }

    // run tests backward log
    let mut meta = world.resource_mut::<RevMeta>();
    meta.set_queue(RevQueue::Run(RevDirection::BackwardLog));
    for (step, expected) in expected.iter().enumerate().rev() {
        test_step(
            &mut world,
            variant,
            config,
            apply_final_deferred,
            step,
            expected,
            RevDirection::BackwardLog,
        );
    }

    // run tests forward log
    let mut meta = world.resource_mut::<RevMeta>();
    meta.set_queue(RevQueue::Run(RevDirection::FORWARD_LOG));
    for (step, expected) in expected.iter().enumerate() {
        test_step(
            &mut world,
            variant,
            config,
            apply_final_deferred,
            step,
            expected,
            RevDirection::FORWARD_LOG,
        );
    }
}

fn test_step<C: for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>(
    world: &mut World,
    variant: usize,
    config: &C,
    apply_final_deferred: bool,
    step: usize,
    expected: &Vec<Test>,
    direction: RevDirection,
) {
    world.run_schedule(FixedUpdate);
    let actual_tests = take(&mut world.resource_mut::<TestLog>().0);
    let iter = expected
        .iter()
        .flat_map(|bundle| bundle.into_iter())
        .map(|test| test.map(|n| (n, direction)));
    let expected_tests: Vec<_> = if direction.is_forward() {
        iter.collect()
    } else {
        iter.rev().collect()
    };
    if actual_tests == expected_tests {
        // test step successful
        return;
    }
    let actual = Test::from_log_entries(actual_tests, direction);
    let iter = expected.into_iter().map(|ok| Result::<_, ()>::Ok(ok));
    let expected: Vec<_> = if direction.is_forward() {
        iter.collect()
    } else {
        iter.rev().collect()
    };
    /*
    let mut app = App::new();
    let mut schedule = Schedule::new(RevUpdate);
    config(&mut schedule);
    schedule.set_apply_final_deferred(apply_final_deferred);
    app.add_schedule(schedule);
    //bevy_mod_debugdump::print_schedule_graph(&mut app, RevUpdate);
    */
    panic!(
        "expected: {expected:?}\nactual:   {actual:?}\nconfig: {variant}\napply_final_deferred: {apply_final_deferred}\ndirection: {direction:?}\nstep: {step}"
    )
}

type ConfigsVec = Vec<Box<dyn for<'a> Fn(&'a mut Schedule) -> &'a mut Schedule>>;

pub(super) fn a_then_b(a_exclusive: bool, b_exclusive: bool, ignore_deferred: bool) -> ConfigsVec {
    fn noop<const N: u8>() {}

    let sys_a: fn() -> RevScheduleConfigs<ScheduleSystem>;
    let sys_a_pipe_noop: fn() -> RevScheduleConfigs<ScheduleSystem>;
    let noop_pipe_sys_a: fn() -> RevScheduleConfigs<ScheduleSystem>;

    let sys_b: fn() -> RevScheduleConfigs<ScheduleSystem>;
    let sys_b_pipe_noop: fn() -> RevScheduleConfigs<ScheduleSystem>;
    let noop_pipe_sys_b: fn() -> RevScheduleConfigs<ScheduleSystem>;

    let set_sys_a: InternedSystemSet;
    let set_noop_a = noop::<3>.into_system_set().intern();
    let set_sys_b: InternedSystemSet;
    let set_noop_b = noop::<4>.into_system_set().intern();

    let sys_after: fn(
        RevScheduleConfigs<ScheduleSystem>,
        InternedSystemSet,
    ) -> RevScheduleConfigs<ScheduleSystem>;
    let sys_before: fn(
        RevScheduleConfigs<ScheduleSystem>,
        InternedSystemSet,
    ) -> RevScheduleConfigs<ScheduleSystem>;
    let sys_chain: fn(RevScheduleConfigs<ScheduleSystem>) -> RevScheduleConfigs<ScheduleSystem>;

    let set_after: fn(
        RevScheduleConfigs<InternedSystemSet>,
        InternedSystemSet,
    ) -> RevScheduleConfigs<InternedSystemSet>;
    let set_before: fn(
        RevScheduleConfigs<InternedSystemSet>,
        InternedSystemSet,
    ) -> RevScheduleConfigs<InternedSystemSet>;
    let set_chain: fn(
        RevScheduleConfigs<InternedSystemSet>,
    ) -> RevScheduleConfigs<InternedSystemSet>;

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

    let mut configs: ConfigsVec = vec![
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
        // #16 set after noop-system pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((noop_pipe_sys_a(), sys_b().rev_in_set(TestSet(2))))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_sys_a))
        }),
        // #17 set after noop-system pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b().rev_in_set(TestSet(2)), noop_pipe_sys_a()))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_sys_a))
        }),
        // #18 set after noop-system pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((noop_pipe_sys_a(), sys_b().rev_in_set(TestSet(2))))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_noop_a))
        }),
        // #19 set after noop-system pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b().rev_in_set(TestSet(2)), noop_pipe_sys_a()))
                .rev_configure_sets(set_after(TestSet(2).into_rev_configs(), set_noop_a))
        }),
        // #20 system after set
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_a().rev_in_set(TestSet(1)),
                sys_after(sys_b(), TestSet(1).intern()),
            ))
        }),
        // #21 system after set (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_after(sys_b(), TestSet(1).intern()),
                sys_a().rev_in_set(TestSet(1)),
            ))
        }),
        // #22 set after set
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
        // #23 set after set (flipped)
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
        // #24 system before system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_sys_b), sys_b()))
        }),
        // #25 system before system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_b(), sys_before(sys_a(), set_sys_b)))
        }),
        // #26 system before system-noop pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_sys_b), sys_b_pipe_noop()))
        }),
        // #27 system before system-noop pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_b_pipe_noop(), sys_before(sys_a(), set_sys_b)))
        }),
        // #28 system before system-noop pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_noop_b), sys_b_pipe_noop()))
        }),
        // #29 system before system-noop pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_b_pipe_noop(), sys_before(sys_a(), set_noop_b)))
        }),
        // #30 system before noop-system pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_sys_b), noop_pipe_sys_b()))
        }),
        // #31 system before noop-system pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((noop_pipe_sys_b(), sys_before(sys_a(), set_sys_b)))
        }),
        // #32 system before noop-system pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((sys_before(sys_a(), set_noop_b), noop_pipe_sys_b()))
        }),
        // #33 system before noop-system pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((noop_pipe_sys_b(), sys_before(sys_a(), set_noop_b)))
        }),
        // #34 set before system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), sys_b()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #35 set before system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #36 set before system-noop pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), sys_b_pipe_noop()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #37 set before system-noop pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b_pipe_noop(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #38 set before system-noop pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), sys_b_pipe_noop()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #39 set before system-noop pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_b_pipe_noop(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #40 set before noop-system pipe by system
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), noop_pipe_sys_b()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #41 set before noop-system pipe by system (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((noop_pipe_sys_b(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_sys_b))
        }),
        // #42 set before noop-system pipe by noop
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((sys_a().rev_in_set(TestSet(1)), noop_pipe_sys_b()))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #43 set before noop-system pipe by noop (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((noop_pipe_sys_b(), sys_a().rev_in_set(TestSet(1))))
                .rev_configure_sets(set_before(TestSet(1).into_rev_configs(), set_noop_b))
        }),
        // #44 system before set
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_before(sys_a(), TestSet(2).intern()),
                sys_b().rev_in_set(TestSet(2)),
            ))
        }),
        // #45 system before set (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems((
                sys_b().rev_in_set(TestSet(2)),
                sys_before(sys_a(), TestSet(2).intern()),
            ))
        }),
        // #46 set before set
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
        // #47 set before set (flipped)
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
        // #48 system chain
        Box::new(move |schedule: &mut Schedule| {
            schedule.rev_add_systems(sys_chain((sys_a(), sys_b()).into_rev_configs()))
        }),
        // #49 set chain
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((
                    sys_a().rev_in_set(TestSet(1)),
                    sys_b().rev_in_set(TestSet(2)),
                ))
                .rev_configure_sets(set_chain((TestSet(1), TestSet(2)).into_rev_configs()))
        }),
        // #50 set chain (flipped)
        Box::new(move |schedule: &mut Schedule| {
            schedule
                .rev_add_systems((
                    sys_b().rev_in_set(TestSet(2)),
                    sys_a().rev_in_set(TestSet(1)),
                ))
                .rev_configure_sets(set_chain((TestSet(1), TestSet(2)).into_rev_configs()))
        }),
    ];

    if !ignore_deferred {
        let manual_apply_deferred: ConfigsVec = vec![
            // #51 system after system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a(),
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_sys_a),
                ))
            }),
            // #52 system after system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_sys_a),
                    sys_a(),
                ))
            }),
            // #53 system after system-noop pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a_pipe_noop(),
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_sys_a),
                ))
            }),
            // #54 system after system-noop pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_sys_a),
                    sys_a_pipe_noop(),
                ))
            }),
            // #55 system after system-noop pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a_pipe_noop(),
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_noop_a),
                ))
            }),
            // #56 system after system-noop pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_noop_a),
                    sys_a_pipe_noop(),
                ))
            }),
            // #57 system after noop-system pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    noop_pipe_sys_a(),
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_sys_a),
                ))
            }),
            // #58 system after noop-system pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_sys_a),
                    noop_pipe_sys_a(),
                ))
            }),
            // #59 system after noop-system pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    noop_pipe_sys_a(),
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_noop_a),
                ))
            }),
            // #60 system after noop-system pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(set_noop_a),
                    noop_pipe_sys_a(),
                ))
            }),
            // #61 set after system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a(),
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_sys_a))
            }),
            // #62 set after system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                        sys_a(),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_sys_a))
            }),
            // #63 set after system-noop pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a_pipe_noop(),
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_sys_a))
            }),
            // #64 set after system-noop pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                        sys_a_pipe_noop(),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_sys_a))
            }),
            // #65 set after system-noop pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a_pipe_noop(),
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_noop_a))
            }),
            // #66 set after system-noop pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                        sys_a_pipe_noop(),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_noop_a))
            }),
            // #67 set after noop_system pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        noop_pipe_sys_a(),
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_sys_a))
            }),
            // #68 set after noop_system pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                        noop_pipe_sys_a(),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_sys_a))
            }),
            // #69 set after noop_system pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        noop_pipe_sys_a(),
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_noop_a))
            }),
            // #70 set after noop_system pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                        noop_pipe_sys_a(),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(set_noop_a))
            }),
            // #71 system after set explicit ApplyDeferred in chain on a
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_in_set(TestSet(1)),
                    sys_b().rev_after_ignore_deferred(TestSet(1)),
                ))
            }),
            // #72 system after set explicit ApplyDeferred in chain on a (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b().rev_after_ignore_deferred(TestSet(1)),
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_in_set(TestSet(1)),
                ))
            }),
            // #73 system after set explicit ApplyDeferred in chain on b
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a().rev_in_set(TestSet(1)),
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(TestSet(1)),
                ))
            }),
            // #74 system after set explicit ApplyDeferred in chain on b (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_after_ignore_deferred(TestSet(1)),
                    sys_a().rev_in_set(TestSet(1)),
                ))
            }),
            // #75 set after set explicit ApplyDeferred in chain on a
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        sys_b().rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(TestSet(1)))
            }),
            // #76 set after set explicit ApplyDeferred in chain on a (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b().rev_in_set(TestSet(2)),
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(TestSet(1)))
            }),
            // #77 set after set explicit ApplyDeferred in chain on b
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a().rev_in_set(TestSet(1)),
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(TestSet(1)))
            }),
            // #78 set after set explicit ApplyDeferred in chain on b (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                        sys_a().rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(2).rev_after_ignore_deferred(TestSet(1)))
            }),
            // #79 system before system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_sys_b),
                    sys_b(),
                ))
            }),
            // #80 system after system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b(),
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_sys_b),
                ))
            }),
            // #81 system before system-noop pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_sys_b),
                    sys_b_pipe_noop(),
                ))
            }),
            // #82 system before system-noop pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b_pipe_noop(),
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_sys_b),
                ))
            }),
            // #83 system before system-noop pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_noop_b),
                    sys_b_pipe_noop(),
                ))
            }),
            // #84 system before system-noop pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b_pipe_noop(),
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_noop_b),
                ))
            }),
            // #85 system before noop-system pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_sys_b),
                    noop_pipe_sys_b(),
                ))
            }),
            // #86 system before noop-system pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    noop_pipe_sys_b(),
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_sys_b),
                ))
            }),
            // #87 system before noop-system pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_noop_b),
                    noop_pipe_sys_b(),
                ))
            }),
            // #88 system before noop-system pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    noop_pipe_sys_b(),
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(set_noop_b),
                ))
            }),
            // #89 set before system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        sys_b(),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_sys_b))
            }),
            // #90 set before system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        sys_b(),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_sys_b))
            }),
            // #91 set before system-noop pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        sys_b_pipe_noop(),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_sys_b))
            }),
            // #92 set before system-noop pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b_pipe_noop(),
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_sys_b))
            }),
            // #93 set before system-noop pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        sys_b_pipe_noop(),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_noop_b))
            }),
            // #94 set before system-noop pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b_pipe_noop(),
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_noop_b))
            }),
            // #95 set before noop-system pipe by system explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        noop_pipe_sys_b(),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_sys_b))
            }),
            // #96 set before noop-system pipe by system explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        noop_pipe_sys_b(),
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_sys_b))
            }),
            // #97 set before noop-system pipe by noop explicit ApplyDeferred in chain
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        noop_pipe_sys_b(),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_noop_b))
            }),
            // #98 set before noop-system pipe by noop explicit ApplyDeferred in chain (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        noop_pipe_sys_b(),
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(set_noop_b))
            }),
            // #99 system before set explicit ApplyDeferred in chain on a
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a().rev_before_ignore_deferred(TestSet(2)),
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_in_set(TestSet(2)),
                ))
            }),
            // #100 system before set explicit ApplyDeferred in chain on a (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (ApplyDeferred, sys_b())
                        .rev_chain_ignore_deferred()
                        .rev_in_set(TestSet(2)),
                    sys_a().rev_before_ignore_deferred(TestSet(2)),
                ))
            }),
            // #101 system before set explicit ApplyDeferred in chain on b
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(TestSet(2)),
                    sys_b().rev_in_set(TestSet(2)),
                ))
            }),
            // #102 system before set explicit ApplyDeferred in chain on b (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b().rev_in_set(TestSet(2)),
                    (sys_a(), ApplyDeferred)
                        .rev_chain_ignore_deferred()
                        .rev_before_ignore_deferred(TestSet(2)),
                ))
            }),
            // #103 set before set explicit ApplyDeferred in chain on a
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a().rev_in_set(TestSet(1)),
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(TestSet(2)))
            }),
            // #104 set before set explicit ApplyDeferred in chain on a (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (ApplyDeferred, sys_b())
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(2)),
                        sys_a().rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(TestSet(2)))
            }),
            // #105 set before set explicit ApplyDeferred in chain on b
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                        sys_b().rev_in_set(TestSet(2)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(TestSet(2)))
            }),
            // #106 set before set explicit ApplyDeferred in chain on b (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b().rev_in_set(TestSet(2)),
                        (sys_a(), ApplyDeferred)
                            .rev_chain_ignore_deferred()
                            .rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(TestSet(1).rev_before_ignore_deferred(TestSet(2)))
            }),
            // #107 ApplyDeffered before/after systems
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a(),
                    ApplyDeferred
                        .rev_after_ignore_deferred(set_sys_a)
                        .rev_before_ignore_deferred(set_sys_b),
                    sys_b(),
                ))
            }),
            // #108 ApplyDeffered before/after systems (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b(),
                    ApplyDeferred
                        .rev_after_ignore_deferred(set_sys_a)
                        .rev_before_ignore_deferred(set_sys_b),
                    sys_a(),
                ))
            }),
            // #109 ApplyDeffered before/after system-noop pipes
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a_pipe_noop(),
                    ApplyDeferred
                        .rev_after_ignore_deferred(set_sys_a)
                        .rev_before_ignore_deferred(set_sys_b),
                    sys_b_pipe_noop(),
                ))
            }),
            // #110 ApplyDeffered before/after system-noop pipes (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b_pipe_noop(),
                    ApplyDeferred
                        .rev_after_ignore_deferred(set_sys_a)
                        .rev_before_ignore_deferred(set_sys_b),
                    sys_a_pipe_noop(),
                ))
            }),
            // #111 ApplyDeffered before/after sets
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_a().rev_in_set(TestSet(1)),
                    ApplyDeferred
                        .rev_after_ignore_deferred(TestSet(1))
                        .rev_before_ignore_deferred(TestSet(2)),
                    sys_b().rev_in_set(TestSet(2)),
                ))
            }),
            // #112 ApplyDeffered before/after sets (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule.rev_add_systems((
                    sys_b().rev_in_set(TestSet(2)),
                    ApplyDeferred
                        .rev_after_ignore_deferred(TestSet(1))
                        .rev_before_ignore_deferred(TestSet(2)),
                    sys_a().rev_in_set(TestSet(1)),
                ))
            }),
            // #113 system chain explicit ApplyDeferred
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((sys_a(), ApplyDeferred, sys_b()).rev_chain_ignore_deferred())
            }),
            // #114 set chain explicit ApplyDeferred
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_a().rev_in_set(TestSet(1)),
                        ApplyDeferred.rev_in_set(TestSet(2)),
                        sys_b().rev_in_set(TestSet(3)),
                    ))
                    .rev_configure_sets(
                        (TestSet(1), TestSet(2), TestSet(3)).rev_chain_ignore_deferred(),
                    )
            }),
            // #115 set chain explicit ApplyDeferred (flipped)
            Box::new(move |schedule: &mut Schedule| {
                schedule
                    .rev_add_systems((
                        sys_b().rev_in_set(TestSet(3)),
                        ApplyDeferred.rev_in_set(TestSet(2)),
                        sys_a().rev_in_set(TestSet(1)),
                    ))
                    .rev_configure_sets(
                        (TestSet(1), TestSet(2), TestSet(3)).rev_chain_ignore_deferred(),
                    )
            }),
        ];

        configs.extend(manual_apply_deferred);
    }

    configs
}
