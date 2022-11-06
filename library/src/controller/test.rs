use std::{
    collections::{HashMap, VecDeque},
    iter::repeat,
    mem::take,
    num::Wrapping,
    ops::RangeInclusive,
};

use bevy::prelude::{App, Commands, CoreStage, ParallelSystemDescriptorCoercion, Res, ResMut};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks,
};

use super::{
    consts::ControllerConsts,
    progress::{Progress, ProgressQuery},
    Controller,
};

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
    /// applied query before last
    query: Query,
}

#[derive(Copy, Clone)]
enum Query {
    None,
    Forward,
    ForwardFastToRangeStart,
    ForwardFastToRangeEnd,
    ForwardFastToOutOfRange,
    LogToRangeStart,
    LogToRangeEnd,
    LogToOutOfRange,
    LogFastToRangeStart,
    LogFastToRangeEnd,
    LogFastToOutOfRange,
    Pause,
}

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

impl Test {
    fn forward_fast_to(
        &self,
        start_if_in_range: Option<bool>,
    ) -> (ProgressQuery, Result<(), RangeInclusive<Wrapping<Ticks>>>) {
        let range =
            Wrapping(*self.forward_fast_range.start())..=Wrapping(*self.forward_fast_range.end());
        match start_if_in_range {
            None => (
                ProgressQuery::ForwardFastTo(*range.end() + Wrapping(1)),
                Err(range),
            ),
            Some(false) => (ProgressQuery::ForwardFastTo(*range.end()), Ok(())),
            Some(true) => (ProgressQuery::ForwardFastTo(*range.start()), Ok(())),
        }
    }
    fn log_to(
        &self,
        start_if_in_range: Option<bool>,
    ) -> (ProgressQuery, Result<(), RangeInclusive<Wrapping<Ticks>>>) {
        let range =
            Wrapping(*self.forward_fast_range.start())..=Wrapping(*self.forward_fast_range.end());
        match start_if_in_range {
            None => (ProgressQuery::LogTo(*range.end() + Wrapping(1)), Err(range)),
            Some(false) => (ProgressQuery::LogTo(*range.end()), Ok(())),
            Some(true) => (ProgressQuery::LogTo(*range.start()), Ok(())),
        }
    }
    fn log_fast_to(
        &self,
        start_if_in_range: Option<bool>,
    ) -> (ProgressQuery, Result<(), RangeInclusive<Wrapping<Ticks>>>) {
        let range =
            Wrapping(*self.forward_fast_range.start())..=Wrapping(*self.forward_fast_range.end());
        match start_if_in_range {
            None => (
                ProgressQuery::LogFastTo(*range.end() + Wrapping(1)),
                Err(range),
            ),
            Some(false) => (ProgressQuery::LogFastTo(*range.end()), Ok(())),
            Some(true) => (ProgressQuery::LogFastTo(*range.start()), Ok(())),
        }
    }
    fn query(&self, controller: &Controller, commands: &mut Commands<'_, '_>, index: usize) {
        let (query, expected) = match self.query {
            Query::None => return,
            Query::Forward => (ProgressQuery::Forward, Ok(())),
            Query::ForwardFastToRangeStart => self.forward_fast_to(Some(true)),
            Query::ForwardFastToRangeEnd => self.forward_fast_to(Some(false)),
            Query::ForwardFastToOutOfRange => self.forward_fast_to(None),
            Query::LogToRangeStart => self.log_to(Some(true)),
            Query::LogToRangeEnd => self.log_to(Some(false)),
            Query::LogToOutOfRange => self.log_to(None),
            Query::LogFastToRangeStart => self.log_fast_to(Some(true)),
            Query::LogFastToRangeEnd => self.log_fast_to(Some(false)),
            Query::LogFastToOutOfRange => self.log_fast_to(None),
            Query::Pause => (ProgressQuery::Pause, Ok(())),
        };
        match (controller.query_progress_command(commands, query), expected) {
            (Ok(()), Ok(())) => (),
            (Err(r1), Err(r2)) => assert_eq!(
                r1, r2, "test index {index}, errors should be equal, debug:\n{:#?}", controller.debug),
            (Err(r), Ok(..)) => panic!(
                "test index {index}, test should not result in Err({r:?}), debug:\n{:#?}",
                controller.debug
            ),
            (Ok(()), Err(r)) => panic!(
                "test index {index}, test should result in Err({r:?}), debug:\n{:#?}",
                controller.debug
            ),
        }
    }
    fn before_first_ran(
        tests: Res<'_, Vec<Self>>,
        index: Res<'_, usize>,
        at_first_ran: Res<'_, bool>,
        controller: Res<'_, Controller>,
        mut commands: Commands<'_, '_>,
    ) {
        let index = *index;
        let test = &tests[index];
        let progress_current = test.progress_current;
        assert_eq!(
            progress_current, controller.progress_current,
            "test index {index}, progress_current should be {progress_current:?}, debug:\n{:#?}",
            controller.debug
        );
        if !*at_first_ran {
            test.query(&controller, &mut commands, index);
        }
    }
    fn at_first_ran(
        tests: Res<'_, Vec<Self>>,
        index: Res<'_, usize>,
        at_first_ran: Res<'_, bool>,
        controller: Res<'_, Controller>,
        mut commands: Commands<'_, '_>,
    ) {
        let index = *index;
        let test = &tests[index];
        if *at_first_ran {
            test.query(&controller, &mut commands, index);
        }
        match controller.progress_current {
            Progress::Forward => {
                controller.send_commands(CommandTest::new_vec(&controller), &mut commands, 0);
            }
            Progress::ForwardFast { .. } => controller.send_commands(
                CommandTest::new_vec(&controller),
                &mut commands,
                controller.to_time_stamp.delta_abs - 1,
            ),
            _ => {}
        }
        let check_time_stamp = !matches!(
            controller.progress_current,
            Progress::LogClose { .. } | Progress::Pause { .. }
        );
        match (check_time_stamp, test.time_stamp) {
            (true, Some(time_stamp)) => assert_eq!(
                controller.time_stamp,
                Wrapping(time_stamp),
                "test index {index}, time_stamp should be {time_stamp}, debug:\n{:#?}",
                controller.debug
            ),
            (true, None) => panic!(
                "test index {index}, time_stamp should be Some at this test, debug:\n{:#?}",
                controller.debug
            ),
            (false, Some(..)) => panic!(
                "test index {index}, time_stamp should be None at this test, debug:\n{:#?}",
                controller.debug
            ),
            (false, None) => {}
        }
    }
    fn after_first_ran(
        tests: Res<'_, Vec<Self>>,
        mut index_res: ResMut<'_, usize>,
        controller: Res<'_, Controller>,
        mut log: ResMut<'_, TestLogCollection>,
    ) {
        let index = *index_res;
        let test = &tests[index];
        let log = take(&mut log.log);
        let commands_check: Vec<Command> = test
            .commands
            .iter()
            .flat_map(|c| repeat(c.clone()).take(4))
            .collect();
        assert_eq!(
            commands_check, log,
            "test index {index}, commands should be {commands_check:#?}, debug:\n{:#?}",
            controller.debug
        );
        *index_res += 1;
    }
    fn test_all_queries(setup: Vec<Self>, branches: HashMap<Query, Vec<Self>>) {
        assert_eq!(branches.len(), 12, "all queries should be tested");
        assert!(!setup.is_empty(), "setup should not be empty");
        for (query, mut branch) in branches.into_iter() {
            let mut tests = setup.clone();
            tests.last_mut().unwrap().query = query;
            tests.append(&mut branch);
            Self::test(tests);
        }
    }
    fn test(tests: Vec<Self>) {
        Self::test_with_at_first_ran(tests.clone(), false);
        Self::test_with_at_first_ran(tests, true);
    }
    fn test_with_at_first_ran(tests: Vec<Self>, at_first_ran: bool) {
        let len = tests.len();
        let constants = ControllerConsts::new(
            2,   //forget after 4 steps
            2,   //panic at jumping further than 2 steps
            1,   //third and fourth `CommandTest` should be added without sync_channel
            0.0, //next step at each app update
            len,
        );
        let (controller, receiver) = Controller::new(
            Wrapping(0),
            VecDeque::with_capacity(constants.log_capacity),
            constants,
        );

        let mut app = App::new();
        app.init_resource::<usize>()
            .insert_resource(at_first_ran)
            .insert_resource(tests)
            .insert_resource(controller)
            .insert_non_send_resource(receiver)
            .add_system_to_stage(CoreStage::First, Self::before_first_ran)
            .add_system_to_stage(CoreStage::PreUpdate, Controller::system_first)
            .add_system_to_stage(CoreStage::Update, Self::at_first_ran)
            .add_system_to_stage(
                CoreStage::PostUpdate,
                Controller::system_last.after(|controller: Res<'_, Controller>| {
                    assert_eq!(controller.commands_overflows, 2)
                }),
            )
            .add_system_to_stage(CoreStage::Last, Self::after_first_ran);

        let mut count = 0;
        while *app.world.resource::<usize>() < len {
            app.update();
            assert_ne!(count, len, "every app update should progress controller");
            count += 1;
        }
    }
}
