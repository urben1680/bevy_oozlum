use std::{collections::VecDeque, iter::repeat, mem::take, num::Wrapping};

use bevy::prelude::{App, Commands, CoreStage, Res, ResMut};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks,
};

use super::{
    consts::ControllerConsts,
    progress::{Progress, ProgressQuery, ProgressQueryError},
    Controller,
};

mod forward;
mod forward_log;
mod forward_log_to;
mod forward_to;

#[derive(PartialEq, Debug, Clone, Copy)]
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
    /// `TestLog` is created in pairs at each `Forward`/`ForwardTo` tick with different values for `first`.
    /// This way the correct order of calls to `ReversibleCommandsInitialized` methods can be checked.
    first: bool,
}

#[derive(Debug, Default)]
struct TestLogCollection {
    added: VecDeque<CommandTest>,
    removed: VecDeque<CommandTest>,
    log: Vec<Command>,
}

#[derive(Clone)]
struct Test {
    time_stamp: Ticks,
    progress_current: Progress,
    progress_query: Option<ProgressQuery>,
    commands: Vec<Command>,
}

impl CommandTest {
    fn new_pair(controller: &Controller) -> Vec<Box<dyn ReversibleCommand>> {
        let new_box = |first: bool, controller: &Controller| {
            Box::new(CommandTest {
                time_stamp: controller.time_stamp(),
                to_time_stamp: controller.to_time_stamp().to_time_stamp,
                forget: controller.forget(),
                first,
            })
        };
        vec![new_box(true, controller), new_box(false, controller)]
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
            if self.first {
                stamp += 1;
            }
            assert_eq!(stamp, self.time_stamp, "{collection:#?}");
        } else if !self.first {
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
    fn run(self, result: Result<(), ProgressQueryError>);
    fn sub_run(self, apply_query_at_first_ran: bool) -> Result<(), ProgressQueryError>;
}

impl<const N: usize> RunTests for [Test; N] {
    fn run(self, result: Result<(), ProgressQueryError>) {
        assert_eq!(
            result,
            self.clone().sub_run(false),
            "unexpected result when querying when first did not ran"
        );
        assert_eq!(
            result,
            self.sub_run(true),
            "unexpected result when querying when first did ran"
        );
    }
    fn sub_run(self, apply_query_at_first_ran: bool) -> Result<(), ProgressQueryError> {
        //todo: run test twice, one where the query is applied before and one after the firts controller system

        let constants = ControllerConsts::new(
            2,   //forget after 4 steps
            2,   //panic at jumping further than 2 steps
            0,   //second `CommandTest` should be added without sync_channel
            0.0, //next step at each app update
            10,
        );

        let (mut before_first, (mut after_first, mut after_last)): (Vec<_>, (Vec<_>, Vec<_>)) =
            self.into_iter()
                .map(|step| {
                    if apply_query_at_first_ran {
                        (
                            (step.progress_current, None),
                            (step.progress_query, (step.time_stamp, step.commands)),
                        )
                    } else {
                        (
                            (step.progress_current, step.progress_query),
                            (None, (step.time_stamp, step.commands)),
                        )
                    }
                })
                .rev()
                .unzip();

        let mut app = App::new();
        app.init_resource::<TestLogCollection>();
        app.init_resource::<usize>();
        app.insert_resource::<Result<(), ProgressQueryError>>(Ok(()));
        Controller::into_world(
            Wrapping(0),
            VecDeque::with_capacity(constants.log_capacity),
            constants,
            &mut app.world,
        );

        app.add_system_to_stage(
            CoreStage::First,
            move |controller: Res<'_, Controller>,
                  test_count: Res<'_, usize>,
                  mut error: ResMut<'_, Result<(), ProgressQueryError>>,
                  mut commands: Commands<'_, '_>| {
                let (progress_current, progress_query) = before_first.pop().unwrap();
                assert_eq!(
                    progress_current, controller.progress_current,
                    "time_stamp : {}",
                    controller.time_stamp
                );
                if let Some(progress_query) = progress_query {
                    *error = controller.query_procress_command(&mut commands, progress_query);
                    if error.is_err() {
                        assert_eq!(*test_count + 1, N, "error must occure at the last test")
                    }
                }
            },
        );
        app.add_system_to_stage(CoreStage::PreUpdate, Controller::system_first);
        app.add_system_to_stage(
            CoreStage::Update,
            move |controller: Res<'_, Controller>,
                  test_count: Res<'_, usize>,
                  mut commands: Commands<'_, '_>,
                  mut error: ResMut<'_, Result<(), ProgressQueryError>>| {
                match controller.progress_current {
                    Progress::Forward => {
                        controller.send_commands(
                            CommandTest::new_pair(&controller),
                            &mut commands,
                            0,
                        );
                    }
                    Progress::ForwardTo { .. } => controller.send_commands(
                        CommandTest::new_pair(&controller),
                        &mut commands,
                        controller.to_time_stamp.delta_abs - 1,
                    ),
                    _ => {}
                }
                let progress_query = after_first.pop().unwrap();
                if let Some(progress_query) = progress_query {
                    *error = controller.query_procress_command(&mut commands, progress_query);
                    if error.is_err() {
                        assert_eq!(*test_count + 1, N, "error must occure at the last test")
                    }
                }
            },
        );
        app.add_system_to_stage(CoreStage::PostUpdate, Controller::system_last);
        app.add_system_to_stage(
            CoreStage::Last,
            move |mut log: ResMut<'_, TestLogCollection>,
                  controller: Res<'_, Controller>,
                  mut test_count: ResMut<'_, usize>| {
                *test_count += 1;
                let (stamp, mut commands_check) = after_last.pop().unwrap();
                if !matches!(
                    controller.progress_current,
                    Progress::LogClose { .. } | Progress::Pause { .. }
                ) {
                    assert_eq!(Wrapping(stamp), controller.time_stamp);
                }
                let log = take(&mut log.log);
                commands_check = commands_check
                    .into_iter()
                    .flat_map(|c| repeat(c).take(2))
                    .collect();
                assert_eq!(commands_check, log, "time_stamp: {}", controller.time_stamp);
            },
        );

        (0..N).for_each(|_| app.update());
        assert_eq!(N, *app.world.resource::<usize>(), "not all tests ran");
        app.world
            .remove_resource::<Result<(), ProgressQueryError>>()
            .unwrap()
    }
}
