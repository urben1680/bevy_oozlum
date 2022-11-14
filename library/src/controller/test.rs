use std::{
    collections::{VecDeque, BTreeMap},
    iter::repeat,
    mem::take,
    num::Wrapping,
    ops::RangeInclusive,
};

use bevy::prelude::{App, Commands, CoreStage, ParallelSystemDescriptorCoercion, Res, ResMut, DerefMut, Deref, Resource};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks,
};

use super::{
    consts::ControllerConsts,
    progress::{Progress, ProgressQuery},
    Controller,
};

const MAX_LOG_INDEX: Ticks = 2;
const MAX_LOG_LEN: usize = MAX_LOG_INDEX as usize + 2;
const FORWARD_TO_MAX: Ticks = 3;

/*
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
*/

mod forward;

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

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug, PartialOrd, Ord)]
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

#[derive(Debug, Default, Resource)]
struct TestLogCollection {
    added: VecDeque<CommandTest>,
    removed: VecDeque<CommandTest>,
    log: Vec<Command>,
}

#[derive(Resource, DerefMut, Deref)]
struct Tests(Vec<Test>);

#[derive(Resource, DerefMut, Deref)]
struct AtFirstRan(bool);

#[derive(Resource, DerefMut, Deref)]
struct TestQuery(Option<Query>);

#[derive(Resource, DerefMut, Deref, Default)]
struct TestIndex(usize);

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
    fn panic_prefix(query: Option<Query>, index: usize, at_first_ran: bool) -> String{
        match query{
            None => format!("test index: {index}, at_first_ran: {at_first_ran:?}"),
            Some(query) => format!("test branch {query:?}, test index: {index}, at_first_ran: {at_first_ran:?}\n")
        }
    }
    fn forward_fast_range(&self) -> RangeInclusive<Wrapping<Ticks>>{
        Wrapping(*self.forward_fast_range.start())..=Wrapping(*self.forward_fast_range.end())
    }
    fn log_range(&self) -> RangeInclusive<Wrapping<Ticks>>{
        Wrapping(*self.log_range.start())..=Wrapping(*self.log_range.end())
    }
    fn query_forward_fast_to(
        &self,
        start_if_in_range: Option<bool>,
    ) -> (ProgressQuery, Result<(), RangeInclusive<Wrapping<Ticks>>>) {
        let range = self.forward_fast_range();
        match start_if_in_range {
            None => (
                ProgressQuery::ForwardFastTo(*range.end() + Wrapping(1)),
                Err(range),
            ),
            Some(false) => (ProgressQuery::ForwardFastTo(*range.end()), Ok(())),
            Some(true) => (ProgressQuery::ForwardFastTo(*range.start()), Ok(())),
        }
    }
    fn query_log_to(
        &self,
        start_if_in_range: Option<bool>,
    ) -> (ProgressQuery, Result<(), RangeInclusive<Wrapping<Ticks>>>) {
        let range = self.log_range();
        match start_if_in_range {
            None => (ProgressQuery::LogTo(*range.end() + Wrapping(1)), Err(range)),
            Some(false) => (ProgressQuery::LogTo(*range.end()), Ok(())),
            Some(true) => (ProgressQuery::LogTo(*range.start()), Ok(())),
        }
    }
    fn query_log_fast_to(
        &self,
        start_if_in_range: Option<bool>,
    ) -> (ProgressQuery, Result<(), RangeInclusive<Wrapping<Ticks>>>) {
        let range = self.log_range();
        match start_if_in_range {
            None => (
                ProgressQuery::LogFastTo(*range.end() + Wrapping(1)),
                Err(range),
            ),
            Some(false) => (ProgressQuery::LogFastTo(*range.end()), Ok(())),
            Some(true) => (ProgressQuery::LogFastTo(*range.start()), Ok(())),
        }
    }
    fn query(&self, controller: &Controller, commands: &mut Commands<'_, '_>, panic_prefix: &String) {
        let (query, expected) = match self.query {
            Query::None => return,
            Query::Forward => (ProgressQuery::Forward, Ok(())),
            Query::ForwardFastToRangeStart => self.query_forward_fast_to(Some(true)),
            Query::ForwardFastToRangeEnd => self.query_forward_fast_to(Some(false)),
            Query::ForwardFastToOutOfRange => self.query_forward_fast_to(None),
            Query::LogToRangeStart => self.query_log_to(Some(true)),
            Query::LogToRangeEnd => self.query_log_to(Some(false)),
            Query::LogToOutOfRange => self.query_log_to(None),
            Query::LogFastToRangeStart => self.query_log_fast_to(Some(true)),
            Query::LogFastToRangeEnd => self.query_log_fast_to(Some(false)),
            Query::LogFastToOutOfRange => self.query_log_fast_to(None),
            Query::Pause => (ProgressQuery::Pause, Ok(())),
        };
        match (controller.query_progress_command(commands, query), expected) {
            (Ok(()), Ok(())) => (),
            (Err(r1), Err(r2)) => assert_eq!(
                r1, r2, "{panic_prefix}{query:?}: errors should be equal, debug:\n{:#?}", controller.debug),
            (a, b) => panic!(
                "{panic_prefix}{query:?}: test should result in {b:?}, not {a:?}, debug:\n{:#?}",
                controller.debug
            )
        }
    }
    fn before_first_ran(
        tests: Res<'_, Tests>,
        test_query: Res<'_, TestQuery>,
        index: Res<'_, TestIndex>,
        at_first_ran: Res<'_, AtFirstRan>,
        controller: Res<'_, Controller>,
        mut commands: Commands<'_, '_>,
    ) {
        let index = *index;
        let test = &tests[index];
        let panic_prefix = Self::panic_prefix(*test_query, index, *at_first_ran);
        let progress_current = test.progress_current;
        assert_eq!(
            progress_current, controller.progress_current,
            "{panic_prefix}progress_current should be {progress_current:?}, debug:\n{:#?}",
            controller.debug
        );
        if !*at_first_ran {
            test.query(&controller, &mut commands, &panic_prefix);
        }
    }
    fn at_first_ran(
        tests: Res<'_, Tests>,
        test_query: Res<'_, TestQuery>,
        index: Res<'_, TestIndex>,
        at_first_ran: Res<'_, AtFirstRan>,
        controller: Res<'_, Controller>,
        mut commands: Commands<'_, '_>
    ) {
        let index = *index;
        let test = &tests[index];
        let panic_prefix = Self::panic_prefix(*test_query, index, *at_first_ran);

        assert!(test.log_range.len() <= MAX_LOG_LEN, "{panic_prefix}, test log_len should have a max len of {MAX_LOG_LEN}");
        assert_eq!(test.forward_fast_range.len(), FORWARD_TO_MAX as usize, "{panic_prefix}, test forward_fast_range should have a len of {FORWARD_TO_MAX}");
        assert!(controller.first_ran, "{panic_prefix}, first_ran should be true, debug:\n{:#?}",
        controller.debug);
        assert_eq!(controller.forward_fast_range(), test.forward_fast_range(), "{panic_prefix}forward fast range should match, debug:\n{:#?}",
        controller.debug);
        assert_eq!(controller.log_range(), test.log_range(), "{panic_prefix}log range should match, debug:\n{:#?}",
        controller.debug);

        if *at_first_ran {
            test.query(&controller, &mut commands, &panic_prefix);
        }
        match controller.progress_current {
            Progress::Forward => {
                controller.send_commands(CommandTest::new_vec(&controller), &mut commands, 0);
            }
            Progress::ForwardFast { .. } => controller.send_commands(
                CommandTest::new_vec(&controller),
                &mut commands,
                controller.to_time_stamp.delta_abs.checked_sub(1).unwrap_or_else(||panic!(
                    "{panic_prefix}to_time_stamp.delta_abs should not be zero, debug:\n{:#?}", controller.debug
                )),
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
                "{panic_prefix}time_stamp should be {time_stamp}, debug:\n{:#?}",
                controller.debug
            ),
            (true, None) => panic!(
                "{panic_prefix}time_stamp should be Some at this test, debug:\n{:#?}",
                controller.debug
            ),
            (false, Some(..)) => panic!(
                "{panic_prefix}time_stamp should be None at this test, debug:\n{:#?}",
                controller.debug
            ),
            (false, None) => {}
        }
    }
    fn after_first_ran(
        tests: Res<'_, Tests>,
        test_query: Res<'_, TestQuery>,
        mut index_res: ResMut<'_, TestIndex>,
        at_first_ran: Res<'_, AtFirstRan>,
        controller: Res<'_, Controller>,
        mut log: ResMut<'_, TestLogCollection>,
    ) {
        let index = *index_res;
        let test = &tests[index];
        let panic_prefix = Self::panic_prefix(*test_query, index, *at_first_ran);
        let log = take(&mut log.log);
        let commands_check: Vec<Command> = test
            .commands
            .iter()
            .flat_map(|c| repeat(c.clone()).take(4))
            .collect();
        assert_eq!(
            commands_check, log,
            "{panic_prefix}commands should be {commands_check:#?}, debug:\n{:#?}",
            controller.debug
        );
        *index_res.0 += 1;
    }
    fn test_all_queries(setup: Vec<Self>, mut branches: BTreeMap<Query, Vec<Self>>) {
        assert!(
            branches.insert(Query::ForwardFastToOutOfRange, vec![]).is_none() &&
            branches.insert(Query::LogToOutOfRange, vec![]).is_none() &&
            branches.insert(Query::LogFastToOutOfRange, vec![]).is_none(),
            "out of range queries should not be present as they are added automatically"
        );
        assert_eq!(branches.len(), 12, "all queries should be tested");
        assert!(!setup.is_empty(), "setup should not be empty");
        for (query, mut branch) in branches.into_iter() {
            let mut tests = setup.clone();
            tests.last_mut().unwrap().query = query;
            tests.append(&mut branch);
            Self::test(tests, Some(query));
        }
    }
    fn test(tests: Vec<Self>, query: Option<Query>) {
        Self::test_with_at_first_ran(tests.clone(), query, false);
        Self::test_with_at_first_ran(tests, query, true);
    }
    fn test_with_at_first_ran(tests: Vec<Self>, query: Option<Query>, at_first_ran: bool) {
        let len = tests.len();
        let constants = ControllerConsts::new(
            MAX_LOG_INDEX,   //forget after 4 steps
            FORWARD_TO_MAX,   //panic at jumping further than 3 steps
            1,   //third and fourth `CommandTest` should be added without sync_channel
            0.0, //next step at each app update
            len,
        );
        let controller = Controller::new(
            Wrapping(0),
            VecDeque::with_capacity(constants.log_capacity),
            constants,
        );

        let mut app = App::new();
        app.init_resource::<TestIndex>()
            .init_resource::<TestLogCollection>()
            .insert_resource(AtFirstRan(at_first_ran))
            .insert_resource(Tests(tests))
            .insert_resource(controller)
            .insert_resource(TestQuery(query))
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
        while app.world.resource::<TestIndex>().0 < len {
            app.update();
            assert_ne!(count, len, "every app update should progress controller");
            count += 1;
        }
    }
}
