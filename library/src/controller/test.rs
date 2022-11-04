use std::{
    collections::{HashSet, VecDeque},
    iter::repeat,
    mem::take,
    num::Wrapping,
    ops::RangeInclusive,
};

use bevy::{
    prelude::{App, Commands, CoreStage, ParallelSystemDescriptorCoercion, Res, ResMut},
    utils::HashMap,
};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks,
};

use super::{
    consts::ControllerConsts,
    debug::DebugLogContainer,
    progress::{Progress, ProgressQuery},
    Controller,
};

mod forward;

/*
mod backward_log_after_backward;
mod backward_log_after_forward;
mod forward_log_after_backward;
mod forward_log_after_forward;
mod forward_log_to_init_after_backward;
mod forward_log_to_init_after_forward;
mod forward_log_to_not_init;
mod forward_to_init;
mod forward_to_not_init;
*/

#[derive(PartialEq, Debug, Clone, Copy, Hash, Eq)]
enum Command {
    Init,
    Undo,
    Redo,
    UndoFinalize,
    RedoFinalize,
}

/// Contains all controller values that are read by `Log` methods.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct CommandTest {
    time_stamp: Wrapping<Ticks>,
    to_time_stamp: Wrapping<Ticks>,
    forget: Wrapping<Ticks>,
    in_this_step: u8,
}

#[derive(Debug, Default)]
struct TestLogCollection {
    added: VecDeque<CommandTest>,
    removed: VecDeque<CommandTest>,
    log: Vec<Command>,
}

#[derive(Clone)]
struct Test {
    /// controller.time_stamp value that reversible systems will see
    time_stamp: Option<Ticks>,
    /// controller.forward_fast_range() value before last
    forward_fast_range: RangeInclusive<Ticks>,
    /// controller.log_range() value before last
    log_range: RangeInclusive<Ticks>,
    /// controller.prrogress_current value before last
    progress_current: Progress,
    /// applied ReversibleCommand methods during the tick, checked after last
    commands: Vec<Command>,
}

#[derive(Default)]
struct SubVariation {
    ranges: Option<(
        RangeInclusive<Wrapping<Ticks>>, //fast_forward
        RangeInclusive<Wrapping<Ticks>>, //log
    )>,
    progress_query: Option<ProgressQuery>,
}

impl SubVariation {
    fn new(
        forward_fast_range: RangeInclusive<Ticks>,
        log_range: RangeInclusive<Ticks>,
        progress_query: Option<ProgressQuery>,
    ) -> Self {
        let forward_fast_range =
            Wrapping(*forward_fast_range.start())..=Wrapping(*forward_fast_range.end());
        let log_range = Wrapping(*log_range.start())..=Wrapping(*log_range.end());
        Self {
            ranges: Some((forward_fast_range, log_range)),
            progress_query,
        }
    }
    fn assert_ranges_try_query(
        self,
        controller: &Controller,
        mut commands: Commands<'_, '_>,
        expected: &mut Result<(), RangeInclusive<Wrapping<Ticks>>>,
        at_first_ran: bool,
        test: usize,
        tests: usize,
    ) {
        if let Some((forward_fast_range, log_range)) = self.ranges {
            assert_eq!(
                controller.forward_fast_range(),
                forward_fast_range,
                "\ntest {test}, at_first_ran: {at_first_ran:?}, debug:\n{:#?}",
                controller.debug
            );
            assert_eq!(
                controller.log_range(),
                log_range,
                "\ntest {test}, at_first_ran: {at_first_ran:?}, debug:\n{:#?}",
                controller.debug
            );
        }
        if let Some(progress_query) = self.progress_query {
            *expected = controller.query_procress_command(&mut commands, progress_query);
            if expected.is_err() {
                assert_eq!(
                    test, tests,
                    "\nerror must occure at the last test, debug:\n{:#?}",
                    controller.debug
                );
            }
        }
    }
}

impl CommandTest {
    fn new_vec(controller: &Controller) -> Vec<Box<dyn ReversibleCommand>> {
        let new_box = |in_this_step: u8, controller: &Controller| {
            Box::new(CommandTest {
                time_stamp: controller.time_stamp(),
                to_time_stamp: controller.to_time_stamp().to_time_stamp,
                forget: controller.forget(),
                in_this_step,
            })
        };
        (0..4)
            .map(|in_this_step| new_box(in_this_step, controller) as _)
            .collect()
    }
}

impl ReversibleCommand for CommandTest {
    fn init(
        self: Box<Self>,
        world: &mut bevy::prelude::World,
    ) -> Option<Box<dyn ReversibleCommandInitialized>> {
        let mut collection = world.resource_mut::<TestLogCollection>();
        if let Some(front) = collection.added.front() {
            let mut stamp = front.time_stamp;
            if self.in_this_step == 0 {
                stamp += 1;
            }
            assert_eq!(stamp, self.time_stamp, "{collection:#?}");
        } else if self.in_this_step != 0 {
            panic!()
        }
        collection.added.push_front(*self);
        collection.log.push(Command::Init);
        Some(self)
    }
}

impl ReversibleCommandInitialized for CommandTest {
    fn undo(&mut self, world: &mut bevy::prelude::World) {
        let mut collection = world.resource_mut::<TestLogCollection>();
        let element = collection.added.pop_front().unwrap();
        assert_eq!(*self, element);
        collection.removed.push_front(element);
        collection.log.push(Command::Undo);
    }
    fn redo(&mut self, world: &mut bevy::prelude::World) {
        let mut collection = world.resource_mut::<TestLogCollection>();
        let element = collection.removed.pop_front().unwrap();
        assert_eq!(*self, element);
        collection.added.push_front(element);
        collection.log.push(Command::Redo);
    }
    fn undo_finalize(self: Box<Self>, world: &mut bevy::prelude::World) {
        let mut collection = world.resource_mut::<TestLogCollection>();
        let element = collection.removed.pop_back().unwrap();
        assert_eq!(*self, element);
        collection.log.push(Command::UndoFinalize);
    }
    fn redo_finalize(self: Box<Self>, world: &mut bevy::prelude::World) {
        let mut collection = world.resource_mut::<TestLogCollection>();
        let element = collection.added.pop_back().unwrap();
        assert_eq!(*self, element);
        collection.log.push(Command::RedoFinalize);
    }
}

trait RunTests {
    fn run(self, expected: Result<(), RangeInclusive<Wrapping<Ticks>>>);
    fn sub_run(
        self,
        apply_query_at_first_ran: bool,
    ) -> (
        Result<(), RangeInclusive<Wrapping<Ticks>>>,
        VecDeque<DebugLogContainer>,
    );
}

impl RunTests for Vec<(Test, Option<ProgressQuery>)> {
    fn run(self, expected: Result<(), RangeInclusive<Wrapping<Ticks>>>) {
        let [(r0, mut d0), (r1, d1)] = [false, true]
            .map(|apply_query_at_first_ran| self.clone().sub_run(apply_query_at_first_ran));

        for log in &mut d0 {
            log.after_first.progress_query = None; //make sure d0 == d1 check does not return false because of this value
        }

        match (r0, d0, r1, d1) {
            (r0, _, r1, _) if r0 == expected && r1 == expected => {}
            (r0, d0, r1, d1) if r0 == r1 && d0 == d1 => {
                panic!("\ntest failed for both apply_query_at_first_ran in the same way, expected: {expected:?}, result: {r0:?}, debug:\n{d0:#?}");
            }
            (r0, _, r1, d1) if r0 == expected => {
                panic!("\ntest failed only for apply_query_at_first_ran: true, expected: {expected:?}, result: {r1:?}, debug:\n{d1:#?}")
            }
            (r0, d0, r1, _) if r1 == expected => {
                panic!("\ntest failed only for apply_query_at_first_ran: false, expected: {expected:?}, result: {r0:?}, debug:\n{d0:#?}")
            }
            (r0, d0, r1, d1) => {
                panic!("\ntest failed differently, expected: {expected:?}\nresult: {r0:?} (apply_query_at_first_ran: false)\nresult: {r1:?} (apply_query_at_first_ran: true)\ndebugs: {:#?}", [d0.front(), d1.front()])
            }
        }
    }
    fn sub_run(
        self,
        apply_query_at_first_ran: bool,
    ) -> (
        Result<(), RangeInclusive<Wrapping<Ticks>>>,
        VecDeque<DebugLogContainer>,
    ) {
        let len = self.len();
        let constants = ControllerConsts::new(
            2,   //forget after 4 steps
            2,   //panic at jumping further than 2 steps
            1,   //third and fourth `CommandTest` should be added without sync_channel
            0.0, //next step at each app update
            len,
        );

        // Split up tests to move parts into their systems.
        let (mut before_first, (mut after_first, mut after_last)): (Vec<_>, (Vec<_>, Vec<_>)) =
            self.into_iter()
                .map(|step| {
                    let sub_variation =
                        SubVariation::new(step.0.forward_fast_range, step.0.log_range, step.1);
                    if apply_query_at_first_ran {
                        (
                            (Default::default(), step.0.progress_current),
                            ((sub_variation, step.0.time_stamp), step.0.commands),
                        )
                    } else {
                        (
                            (sub_variation, step.0.progress_current),
                            ((Default::default(), step.0.time_stamp), step.0.commands),
                        )
                    }
                })
                .rev()
                .unzip();

        // Set up app and resources.
        let mut app = App::new();
        app.init_resource::<TestLogCollection>();
        app.init_resource::<usize>();
        app.insert_resource::<Result<(), RangeInclusive<Wrapping<Ticks>>>>(Ok(()));
        Controller::into_world(
            Wrapping(0),
            VecDeque::with_capacity(constants.log_capacity),
            constants,
            &mut app.world,
        );

        // In the first system the current progress is asserted and, if apply_query_at_first_ran, the next is queried and its result is saved.
        app.add_system_to_stage(
            CoreStage::First,
            move |controller: Res<'_, Controller>,
                  test_count: Res<'_, usize>,
                  mut expected: ResMut<'_, Result<(), RangeInclusive<Wrapping<Ticks>>>>,
                  commands: Commands<'_, '_>| {
                let (sub_variation, progress_current) = before_first.pop().unwrap();
                assert_eq!(
                    progress_current, controller.progress_current,
                    "\ntime_stamp : {}",
                    controller.time_stamp
                );
                sub_variation.assert_ranges_try_query(
                    &controller,
                    commands,
                    &mut expected,
                    false,
                    *test_count + 1,
                    len,
                );
            },
        );

        // In the second system the controller updates the log_index and time_stamp and on the progres...
        // - Forward, ForwardTo: calls `redo_finalize` on the oldest commands if the log is full.
        // - BackwardLog, BackwardLogTo: calls `undo` on the current commands in the log.
        app.add_system_to_stage(CoreStage::PreUpdate, Controller::system_first);

        // In the third system reversible commands in packs of four are sent like reversible systems would do at this point.
        // The commands will themselves check if they are called in the correct order using the TestLogCollection resource.
        // If !apply_query_at_first_ran, the next progress is queried and its result is saved.
        app.add_system_to_stage(
            CoreStage::Update,
            move |controller: Res<'_, Controller>,
                  test_count: Res<'_, usize>,
                  mut commands: Commands<'_, '_>,
                  mut expected: ResMut<'_, Result<(), RangeInclusive<Wrapping<Ticks>>>>| {
                match controller.progress_current {
                    Progress::Forward => {
                        controller.send_commands(
                            CommandTest::new_vec(&controller),
                            &mut commands,
                            0,
                        );
                    }
                    Progress::ForwardFast { .. } => controller.send_commands(
                        CommandTest::new_vec(&controller),
                        &mut commands,
                        controller.to_time_stamp.delta_abs - 1,
                    ),
                    _ => {}
                }
                let next_test_count = *test_count + 1;
                let (sub_variation, stamp) = after_first.pop().unwrap();
                sub_variation.assert_ranges_try_query(&controller, commands, &mut expected, true, next_test_count, len);
                if !matches!(
                    controller.progress_current,
                    Progress::LogClose { .. } | Progress::Pause { .. }
                ) {
                    assert_eq!(Wrapping(stamp.expect("\ntime_stamp should be asserted outside LogClose or Pause")), controller.time_stamp);
                }
                else if stamp.is_some(){
                    panic!("\ntime_stamp should not be asserted during LogClose or Pause at test {next_test_count}");
                }
                else if next_test_count == len && expected.is_ok(){
                    panic!("\nthe last test should not be LogClose or Pause to assert time_stamp in a following progress");
                }
            },
        );

        // In the fourth system it is asserted two of the four commands sent in this iteration were not sent through the sync_channel.
        // After that the controller processes the query and on the progress...
        // - Forward, ForwardTo: calls `init` of the commands for this step and puts the results into the log
        // - ForwardLog, ForwardLogTo: calls `redo` on the current commands in the log.
        // - LogClose: calls `undo_finalize` on all commands in the future.
        app.add_system_to_stage(
            CoreStage::PostUpdate,
            Controller::system_last.after(|controller: Res<'_, Controller>| {
                assert_eq!(controller.commands_overflows, 2)
            }),
        );

        // In the fifth system the current time_stamp and the logged commands of this iteration is asserted.
        app.add_system_to_stage(
            CoreStage::Last,
            move |mut log: ResMut<'_, TestLogCollection>,
                  controller: Res<'_, Controller>,
                  mut test_count: ResMut<'_, usize>| {
                *test_count += 1;
                let mut commands_check = after_last.pop().unwrap();
                let log = take(&mut log.log);
                commands_check = commands_check
                    .into_iter()
                    .flat_map(|c| repeat(c).take(4))
                    .collect();
                assert_eq!(commands_check, log, "\ndebug:\n{:#?}", controller.debug);
            },
        );

        // Update the app for each test and assert all tests ran.
        (0..len).for_each(|_| app.update());
        assert_eq!(len, *app.world.resource::<usize>(), "\nnot all tests ran");

        // Return the result.
        let result = app
            .world
            .remove_resource::<Result<(), RangeInclusive<Wrapping<Ticks>>>>()
            .unwrap();
        let debug = app.world.remove_resource::<Controller>().unwrap().debug;
        (result, debug)
    }
}

struct TestTree {
    nodes: Vec<TestNode>,
    check_list: HashSet<(Progress, Vec<Command>, Query)>,
    node: usize,
    query: usize,
}

enum TestNode {
    Branch {
        previous_node: usize,
        previous_query: usize,
        test: Test,
        queries: [Option<(usize, ProgressQuery)>; Query::QueryPause as _],
    },
    Leaf {
        previous_node: usize,
        previous_query: usize,
        expected: Result<(), RangeInclusive<Wrapping<Ticks>>>,
    },
}

enum TestData {
    Test {
        test: Test,
        next: HashMap<ProgressQuery, Self>,
    },
    Expect(Result<(), RangeInclusive<Wrapping<Ticks>>>),
}

impl From<TestData> for Vec<TestNode> {
    fn from(value: TestData) -> Self {
        todo!(); //recursive closure
    }
}

impl Default for TestData {
    fn default() -> Self {
        todo!()
    }
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Query {
    QueryNone,
    QueryForward,
    QueryForwardFastToOutOfRange,
    QueryForwardFastToRangeStart,
    QueryForwardFastToRangeEnd,
    QueryLogToOutOfRange,
    QueryLogToRangeStart,
    QueryLogToRangeEnd,
    QueryLogFastToOutOfRange,
    QueryLogFastToRangeStart,
    QueryLogFastToRangeEnd,
    QueryPause,
}

impl TryFrom<usize> for Query {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            value if value == Self::QueryNone as usize => Ok(Self::QueryNone),
            value if value == Self::QueryNone as usize => Ok(Self::QueryForward),
            value if value == Self::QueryNone as usize => Ok(Self::QueryForwardFastToOutOfRange),
            value if value == Self::QueryNone as usize => Ok(Self::QueryForwardFastToRangeStart),
            value if value == Self::QueryNone as usize => Ok(Self::QueryForwardFastToRangeEnd),
            value if value == Self::QueryNone as usize => Ok(Self::QueryLogToOutOfRange),
            value if value == Self::QueryNone as usize => Ok(Self::QueryLogToRangeStart),
            value if value == Self::QueryNone as usize => Ok(Self::QueryLogToRangeEnd),
            value if value == Self::QueryNone as usize => Ok(Self::QueryLogFastToOutOfRange),
            value if value == Self::QueryNone as usize => Ok(Self::QueryLogFastToRangeStart),
            value if value == Self::QueryNone as usize => Ok(Self::QueryLogFastToRangeEnd),
            value if value == Self::QueryNone as usize => Ok(Self::QueryPause),
            _ => Err(()),
        }
    }
}

impl Progress {
    fn possible_commands(&self) -> impl Iterator<Item = (Self, Vec<Command>)> {
        let progress = *self;
        (match self {
            Progress::Forward => vec![
                vec![Command::Init],
                vec![Command::RedoFinalize, Command::Init],
                vec![Command::RedoFinalize, Command::RedoFinalize, Command::Init],
            ],
            Progress::ForwardFast(..) => vec![vec![], vec![Command::Init, Command::Init]],
            Progress::ForwardLog(..) | Progress::ForwardLogFast(..) => {
                vec![vec![Command::Redo], vec![Command::Redo, Command::Redo]]
            }
            Progress::BackwardLog(..) | Progress::BackwardLogFast(..) => {
                vec![vec![Command::Undo], vec![Command::Undo, Command::Undo]]
            }
            Progress::LogClose(false) => vec![
                vec![],
                vec![Command::UndoFinalize],
                vec![Command::UndoFinalize, Command::UndoFinalize],
            ],
            Progress::LogClose(true) => vec![
                vec![],
                vec![Command::UndoFinalize],
                vec![Command::UndoFinalize, Command::UndoFinalize],
                vec![
                    Command::UndoFinalize,
                    Command::UndoFinalize,
                    Command::UndoFinalize,
                ],
            ],
            Progress::Pause(..) => vec![vec![]],
        })
        .into_iter()
        .map(move |commands| (progress, commands))
    }
}

impl Default for TestTree {
    fn default() -> Self {
        let progresses = [
            Progress::Forward,
            Progress::ForwardFast(false),
            Progress::ForwardFast(true),
            Progress::ForwardLog(false),
            Progress::ForwardLog(true),
            Progress::ForwardLogFast(None),
            Progress::ForwardLogFast(Some(false)),
            Progress::ForwardLogFast(Some(true)),
            Progress::BackwardLog(false),
            Progress::BackwardLog(true),
            Progress::BackwardLogFast(None),
            Progress::BackwardLogFast(Some(false)),
            Progress::BackwardLogFast(Some(true)),
            Progress::LogClose(false),
            Progress::LogClose(true),
            Progress::Pause(None),
            Progress::Pause(Some(false)),
            Progress::Pause(Some(true)),
        ];
        let queries = [
            Query::QueryNone,
            Query::QueryForward,
            Query::QueryForwardFastToOutOfRange,
            Query::QueryForwardFastToRangeStart,
            Query::QueryForwardFastToRangeEnd,
            Query::QueryLogToOutOfRange,
            Query::QueryLogToRangeStart,
            Query::QueryLogToRangeEnd,
            Query::QueryLogFastToOutOfRange,
            Query::QueryLogFastToRangeStart,
            Query::QueryLogFastToRangeEnd,
            Query::QueryPause,
        ];

        let check_list = progresses
            .iter()
            .flat_map(Progress::possible_commands)
            .flat_map(|(progress, commands)| {
                queries
                    .iter()
                    .map(move |query| (progress, commands.clone(), *query))
            })
            .collect();

        Self {
            nodes: TestData::default().into(),
            check_list,
            node: 0,
            query: 0,
        }
    }
}

impl TestTree {
    fn next_test(
        &mut self,
    ) -> Option<(
        Vec<(Test, Option<ProgressQuery>)>,
        Result<(), RangeInclusive<Wrapping<Ticks>>>,
    )> {
        if let TestNode::Leaf {
            previous_node,
            previous_query,
            ..
        } = &self.nodes[self.node]
        {
            self.node = *previous_node;
            self.query = *previous_query + 1;
        }
        while let TestNode::Branch {
            previous_node,
            previous_query,
            queries,
            ..
        } = &self.nodes[self.node]
        {
            if let Some((offset, node)) = &queries[self.query..]
                .iter()
                .enumerate()
                .find_map(|(offset, node)| node.map(move |node| (offset, node)))
            {
                self.query += offset;
                self.node = node.0;
            } else {
                if self.node == 0 {
                    return None;
                }
                self.node = *previous_node;
                self.query = *previous_query + 1;
            }
        }
        let (mut node, mut query, expected) = match &self.nodes[self.node] {
            TestNode::Leaf {
                previous_node,
                previous_query,
                expected,
            } => (*previous_node, *previous_query, expected.clone()),
            _ => unreachable!(),
        };
        let mut tests = vec![];
        let mut end = false;
        while !end {
            end = node == 0;
            match &self.nodes[self.node] {
                TestNode::Branch {
                    previous_node,
                    previous_query,
                    test,
                    queries,
                } => {
                    self.check_list.remove(&(
                        test.progress_current,
                        test.commands.clone(),
                        query.try_into().unwrap(),
                    ));
                    let query_option = queries[query].as_ref().map(|x| x.1);
                    node = *previous_node;
                    query = *previous_query;
                    tests.push((test.clone(), query_option));
                }
                _ => unreachable!(),
            }
        }
        tests.reverse();
        Some((tests, expected))
    }
}

#[test]
fn test() {
    let mut tree = TestTree::default();
    while let Some((tests, expected)) = tree.next_test() {
        tests.run(expected);
    }
    match tree.check_list.len() {
        0 => {}
        len if len < 10 => panic!(
            "test was successful but not comprehensive, missing tests:\n{:#?}",
            tree.check_list
        ),
        len => panic!(
            "test was successful but not comprehensive, {len} missing tests, such as:\n{:#?}",
            tree.check_list.into_iter().take(10).collect::<Vec<_>>()
        ),
    }
}
