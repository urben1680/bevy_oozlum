use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use bevy::ecs::{
    change_detection::{Res, ResMut},
    component::Component,
    event::Event,
    resource::Resource,
    system::{Commands, IntoSystem},
    world::World,
};

use crate::{
    meta::RevDirection,
    schedule::RevUpdate,
    undo_redo::{BuffersUndoRedo, UndoRedo},
};

use super::*;

pub mod utils;

pub(crate) use utils::*;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Test<T> {
    NonExclusiveSys(T),
    ExclusiveSys(T),

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
    fn map<U>(self, map: impl FnOnce(T) -> U) -> Test<U> {
        match self {
            Test::NonExclusiveSys(value) => Test::NonExclusiveSys(map(value)),
            Test::ExclusiveSys(value) => Test::ExclusiveSys(map(value)),

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

#[derive(Clone, Copy, Debug)]
enum TestBundle {
    NonExclusiveSystem(u8),
    ExclusiveSystem(u8),
    NonExclusiveSyncPoint(u8),
}

impl TestBundle {
    fn from_tests(
        tests: Vec<Test<(u8, RevDirection)>>,
        direction: RevDirection,
    ) -> Vec<Result<Self, Test<(u8, RevDirection)>>> {
        let mut variants: [(_, Vec<_>); 3] = [
            TestBundle::NonExclusiveSystem(0),
            TestBundle::ExclusiveSystem(0),
            TestBundle::NonExclusiveSyncPoint(0),
        ]
        .map(|bundle| (bundle, bundle.into_iter().collect()));

        if !direction.is_forward() {
            for expected in variants.iter_mut() {
                expected.1.reverse();
            }
        }

        let mut i = 0;

        let mut results = Vec::with_capacity(tests.len());

        'test: while i < tests.len() {
            'variant: for (bundle, expected) in &variants {
                let n = &mut None;
                for (&expected, &test) in expected.iter().zip(&tests[i..]) {
                    test.map(|(actual_n, actual_direction)| {
                        *n = match *n {
                            _ if direction != actual_direction => None,
                            None => Some(actual_n),
                            Some(expected_n) => n.filter(|_| expected_n == actual_n),
                        }
                    });
                    if n.is_none_or(|n| test != expected.map(|_| (n, direction))) {
                        continue 'variant;
                    }
                }
                i += expected.len();
                let ok = match bundle {
                    TestBundle::NonExclusiveSystem(_) => TestBundle::NonExclusiveSystem(n.unwrap()),
                    TestBundle::ExclusiveSystem(_) => TestBundle::ExclusiveSystem(n.unwrap()),
                    TestBundle::NonExclusiveSyncPoint(_) => {
                        TestBundle::NonExclusiveSyncPoint(n.unwrap())
                    }
                };
                results.push(Ok(ok));
                continue 'test;
            }
            let err = tests[i];
            i += 1;
            results.push(Err(err));
        }

        results
    }
}

impl IntoIterator for TestBundle {
    type IntoIter = std::vec::IntoIter<Test<u8>>;
    type Item = Test<u8>;
    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::NonExclusiveSystem(n) => vec![Test::NonExclusiveSys(n)],
            Self::ExclusiveSystem(n) => vec![
                Test::ExclusiveSys(n),
                Test::SysObsv(n),
                Test::SysObsvObsv(n),
                Test::SysObsvCmd(n),
                Test::SysHook(n),
                Test::SysHookObsv(n),
                Test::SysHookCmd(n),
            ],
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
        }
        .into_iter()
    }
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct TestSet(u8);

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

fn non_exclusive_system<const N: u8>(
    direction: RevDirection,
    mut log: ResMut<TestLog>,
    commands: Commands,
) {
    log.0.push(Test::NonExclusiveSys((N, direction)));

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
    let direction = world.resource::<RevMeta>().running_direction();
    world
        .resource_mut::<TestLog>()
        .0
        .push(Test::ExclusiveSys((N, direction)));
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

    let test = Test::SysCmd(N);
    world.buffer_undo_redo(test);
}

#[test]
fn single_non_exclusive_system() {
    fn configs(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(non_exclusive_system::<1>)
    }
    test_run(
        vec![configs],
        vec![vec![
            TestBundle::NonExclusiveSystem(1),
            TestBundle::NonExclusiveSyncPoint(1),
        ]],
    );
}

#[test]
fn single_exclusive_system() {
    fn configs(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(exclusive_system::<1>)
    }
    test_run(vec![configs], vec![vec![TestBundle::ExclusiveSystem(1)]]);
}

#[test]
fn non_exclusive_then_non_exclusive() {
    test_run(
        a_then_b(false, false, false),
        vec![vec![
            TestBundle::NonExclusiveSystem(1),
            TestBundle::NonExclusiveSyncPoint(1),
            TestBundle::NonExclusiveSystem(2),
            TestBundle::NonExclusiveSyncPoint(2),
        ]],
    )
}

#[test]
fn exclusive_then_non_exclusive() {
    test_run(
        a_then_b(true, false, false),
        vec![vec![
            TestBundle::ExclusiveSystem(1),
            TestBundle::NonExclusiveSystem(2),
            TestBundle::NonExclusiveSyncPoint(2),
        ]],
    )
}

#[test]
fn non_exclusive_then_exclusive() {
    test_run(
        a_then_b(false, true, false),
        vec![vec![
            TestBundle::NonExclusiveSystem(1),
            TestBundle::NonExclusiveSyncPoint(1),
            TestBundle::ExclusiveSystem(2),
        ]],
    )
}

#[test]
fn exclusive_then_exclusive() {
    test_run(
        a_then_b(true, true, false),
        vec![vec![
            TestBundle::ExclusiveSystem(1),
            TestBundle::ExclusiveSystem(2),
        ]],
    )
}

#[test]
fn non_exclusive_then_non_exclusive_ignore_deferred() {
    test_run(
        a_then_b(false, false, true),
        vec![vec![
            TestBundle::NonExclusiveSystem(1),
            TestBundle::NonExclusiveSystem(2),
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
            TestBundle::ExclusiveSystem(1),
            TestBundle::NonExclusiveSystem(2),
            TestBundle::NonExclusiveSyncPoint(2),
        ]],
    )
}

#[test]
fn non_exclusive_then_exclusive_ignore_deferred() {
    test_run(
        a_then_b(false, true, true),
        vec![vec![
            TestBundle::NonExclusiveSystem(1),
            TestBundle::NonExclusiveSyncPoint(1),
            TestBundle::ExclusiveSystem(2),
        ]],
    )
}

#[test]
fn exclusive_then_exclusive_ignore_deferred() {
    test_run(
        a_then_b(true, true, true),
        vec![vec![
            TestBundle::ExclusiveSystem(1),
            TestBundle::ExclusiveSystem(2),
        ]],
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
        meta.now() == 2
    }
    /// do not make the exclusive system in the latter configs run to test rev_distributive_run
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
    fn config5(schedule: &mut Schedule) -> &mut Schedule {
        schedule
            .rev_add_systems(
                (
                    non_exclusive_system::<1>.rev_in_set(TestSet(1)),
                    exclusive_system::<2>.rev_in_set(TestSet(2)),
                )
                    .rev_chain(),
            )
            .rev_configure_sets(
                (TestSet(1), TestSet(2))
                    .rev_chain()
                    .rev_distributive_run_if(at_2_once()),
            )
    }
    test_run(
        vec![config0, config1, config2, config3, config4, config5],
        vec![
            vec![], // does not run at 1
            vec![
                TestBundle::NonExclusiveSystem(1),
                TestBundle::NonExclusiveSyncPoint(1),
            ],
            vec![], // does not run at 3
        ],
    );
}

#[test]
fn duplicate_system_chain_builds() {
    // todo: assert these don't get mixed up by asserting on system states
    let mut schedule = Schedule::new(RevUpdate);
    schedule.rev_add_systems((non_exclusive_system::<1>, non_exclusive_system::<1>).rev_chain());
    schedule.initialize(&mut World::new()).unwrap();
}
