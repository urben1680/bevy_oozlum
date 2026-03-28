use alloc::{
    vec,
    vec::{IntoIter, Vec},
};
use bevy_app::{App, Update};
use bevy_ecs::{
    change_detection::{Res, ResMut},
    component::Component,
    event::Event,
    resource::Resource,
    system::{Commands, IntoSystem, Local},
    world::World,
};

use crate::{
    app::{RevApp, RevPlugin},
    meta::{RevDirection, RevQueue},
    panic_on_error_events,
    schedule::RevUpdate,
    undo_redo::{BuffersUndoRedo, RevCommands, UndoRedo},
};

use super::*;

mod utils;

use utils::*;

#[derive(Clone, Copy, Debug)]
enum Test {
    NonExclusiveSystem(u8),
    NonExclusiveSyncPoint(u8),
}

impl Test {
    fn from_log_entries(
        tests: &[LogEntry<(u8, RevDirection)>],
        direction: RevDirection,
    ) -> Vec<Result<Self, LogEntry<(u8, RevDirection)>>> {
        let mut variants: [(_, Vec<_>); 2] =
            [Test::NonExclusiveSystem(0), Test::NonExclusiveSyncPoint(0)]
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
                    Test::NonExclusiveSystem(_) => Test::NonExclusiveSystem(n.unwrap()),
                    Test::NonExclusiveSyncPoint(_) => Test::NonExclusiveSyncPoint(n.unwrap()),
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

#[derive(Resource, Default)]
struct TestLog(Vec<LogEntry<(u8, RevDirection)>>);

#[derive(Clone, Copy, PartialEq, Debug)]
enum LogEntry<T> {
    NonExclusiveSys(T),
    SysObsvCmd(T),
    SysHookCmd(T),
    SysCmd(T),
}

impl<T> LogEntry<T> {
    fn map<U>(self, map: impl FnOnce(T) -> U) -> LogEntry<U> {
        match self {
            LogEntry::NonExclusiveSys(value) => LogEntry::NonExclusiveSys(map(value)),
            LogEntry::SysObsvCmd(value) => LogEntry::SysObsvCmd(map(value)),
            LogEntry::SysHookCmd(value) => LogEntry::SysHookCmd(map(value)),
            LogEntry::SysCmd(value) => LogEntry::SysCmd(map(value)),
        }
    }
}

impl UndoRedo for LogEntry<u8> {
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
            .push(self.map(|n| (n, RevDirection::ForwardLog)));
    }
}

impl IntoIterator for Test {
    type IntoIter = IntoIter<LogEntry<u8>>;
    type Item = LogEntry<u8>;
    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::NonExclusiveSystem(n) => vec![LogEntry::NonExclusiveSys(n)],
            Self::NonExclusiveSyncPoint(n) => vec![
                LogEntry::SysObsvCmd(n),
                LogEntry::SysHookCmd(n),
                LogEntry::SysCmd(n),
            ],
        }
        .into_iter()
    }
}

fn normalize_direction(direction: RevDirection) -> RevDirection {
    match direction {
        RevDirection::NotLog(_) => RevDirection::NOT_LOG_MIN,
        RevDirection::ForwardLog => RevDirection::ForwardLog,
        RevDirection::BackwardLog => RevDirection::BackwardLog,
    }
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct TestSet(u8);

#[derive(Component)]
struct SysHook(u8);

#[derive(Event, Clone)]
struct SysObsv(u8);

fn non_exclusive_system<const N: u8>(
    meta: Res<RevMeta>,
    mut log: ResMut<TestLog>,
    commands: Commands,
) {
    // makes assertion on enum variant easier
    let direction = normalize_direction(meta.running_direction());

    log.0.push(LogEntry::NonExclusiveSys((N, direction)));

    non_exclusive_system_commands_only::<N>(meta, commands);
}

fn non_exclusive_system_commands_only<const N: u8>(meta: Res<RevMeta>, mut commands: Commands) {
    let Some(not_log) = meta.get_not_log() else {
        return;
    };

    // trigger observer in system
    commands.trigger(SysObsv(N));

    // trigger hook in system
    commands.spawn(SysHook(N));

    // trigger command in system
    commands.queue(|world: &mut World| {
        world
            .resource_mut::<TestLog>()
            .0
            .push(LogEntry::SysCmd((N, RevDirection::NOT_LOG_MIN)))
    });
    commands.buffer_undo_redo(not_log, LogEntry::SysCmd(N));
}

#[test]
fn single_non_exclusive_system() {
    fn configs(schedule: &mut Schedule) -> &mut Schedule {
        schedule.rev_add_systems(non_exclusive_system::<1>)
    }
    test_run(
        vec![configs],
        vec![vec![
            Test::NonExclusiveSystem(1),
            Test::NonExclusiveSyncPoint(1),
        ]],
    );
}

#[test]
fn non_exclusive_then_non_exclusive() {
    test_run(
        a_then_b(false),
        vec![vec![
            Test::NonExclusiveSystem(1),
            Test::NonExclusiveSyncPoint(1),
            Test::NonExclusiveSystem(2),
            Test::NonExclusiveSyncPoint(2),
        ]],
    )
}

#[test]
fn non_exclusive_then_non_exclusive_ignore_deferred() {
    test_run(
        a_then_b(true),
        vec![vec![
            Test::NonExclusiveSystem(1),
            Test::NonExclusiveSystem(2),
            Test::NonExclusiveSyncPoint(1),
            Test::NonExclusiveSyncPoint(2),
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
            Test::NonExclusiveSyncPoint(1),
            Test::NonExclusiveSyncPoint(2),
        ]],
    )
}

#[test]
fn run_if() {
    fn at_2(meta: Res<RevMeta>) -> bool {
        meta.now() == 2
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
    test_run(
        vec![config0, config1, config2],
        vec![
            vec![], // does not run at 1
            vec![Test::NonExclusiveSystem(1), Test::NonExclusiveSyncPoint(1)],
            vec![], // does not run at 3
        ],
    );
}

#[test]
fn duplicate_system_chain_builds() {
    let mut schedule = Schedule::new(RevUpdate);
    schedule.rev_add_systems((non_exclusive_system::<1>, non_exclusive_system::<1>).rev_chain());
    schedule.initialize(&mut World::new()).unwrap();
}

#[test]
fn truncates_future_command_log() {
    fn system(meta: Res<RevMeta>, mut commands: Commands, mut command_queued: Local<bool>) {
        if !*command_queued
            && let Some(RevDirection::NotLog(not_log)) = meta.get_running_direction()
        {
            if meta.now() == 2 {
                commands.rev_spawn_empty(not_log);
                *command_queued = true;
            }
        }
    }

    panic_on_error_events();

    let mut app = App::new();
    app.add_plugins(
        RevPlugin
            .set_max_past_len(u64::MAX)
            .set_runner_in_schedule(Update),
    )
    .rev_add_systems(RevUpdate, system);

    app.update(); // do 1
    app.update(); // do 2, command queued
    app.world_mut()
        .resource_mut::<RevMeta>()
        .set_queue(RevQueue::RunBackwardLog);
    app.update(); // undo 2
    app.update(); // undo 1
    app.world_mut()
        .resource_mut::<RevMeta>()
        .set_queue(RevQueue::RunForwardLog);
    app.update(); // do 1, should truncate logs
    app.update(); // do 2, no command queued
    app.world_mut()
        .resource_mut::<RevMeta>()
        .set_queue(RevQueue::RunBackwardLog);
    app.update(); // undo 2
    app.update(); // undo 1
    app.world_mut()
        .resource_mut::<RevMeta>()
        .set_queue(RevQueue::RunForwardLog);
    app.update(); // redo 1
    app.update(); // redo 2, should not panic
}
