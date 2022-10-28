use std::{collections::VecDeque, num::Wrapping};

use bevy::{
    ecs::system::Command,
    prelude::{App, Commands, CoreStage, Res, ResMut, World},
};

use crate::{commands::{ReversibleCommand, ReversibleCommandInitialized}, log_systems::{per_system::PerSystem, NextTransition}, Ticks};

use super::{
    consts::{ControllerConsts, CONTROLLER_CONSTS},
    debug::{DebugLog, DebugLogContainer},
    progress::{Progress, ProgressQuery},
    Controller,
};

mod forward;
mod forward_to;
//mod forward_log;

const CONTROLLER_CONSTS_TIME_STEP_ZERO: ControllerConsts = ControllerConsts {
    default_time_step: 0.0,
    ..CONTROLLER_CONSTS
};

#[derive(Default)]
struct Test {
    before_first_commands: Vec<Box<dyn TestCommandTrait + Send + Sync + 'static>>,
    after_first_check: DebugLog,
    after_first_commands: Vec<Box<dyn TestCommandTrait + Send + Sync + 'static>>,
    after_last_check: DebugLog,
}

fn tests<I: IntoIterator<Item = Option<ProgressQuery>>, const N: usize>(
    constants: ControllerConsts,
    setup: I,
    tests: [Test; N],
) -> VecDeque<DebugLogContainer> {
    //convert input
    let (before_first_commands, (after_first_commands, (after_first_checks, after_last_checks))): (
        VecDeque<Vec<Box<dyn TestCommandTrait + Send + Sync + 'static>>>,
        (
            VecDeque<Vec<Box<dyn TestCommandTrait + Send + Sync + 'static>>>,
            (VecDeque<Option<DebugLog>>, VecDeque<Option<DebugLog>>),
        ),
    ) = setup
        .into_iter()
        .map(|control| {
            let vec: Vec<Box<dyn TestCommandTrait + Send + Sync + 'static>> = match control {
                Some(control) => vec![control.into()],
                None => vec![],
            };
            (vec, (vec![LogAtIndexLenCommand.into()], (None, None)))
        })
        .chain(tests.into_iter().map(|mut test| {
            //setting these values here makes `tests` less verbose
            let forget_delta = Wrapping(constants.max_log_index);
            test.after_first_check.forget = test.after_first_check.time_stamp - forget_delta;
            test.after_first_check.first_ran = true;
            test.after_last_check.forget = test.after_last_check.time_stamp - forget_delta;
            test.after_last_check.first_ran = false;
            test.after_first_commands.push(LogAtIndexLenCommand.into());
            (
                test.before_first_commands,
                (
                    test.after_first_commands,
                    (Some(test.after_first_check), Some(test.after_last_check)),
                ),
            )
        }))
        .unzip();
    let ticks = before_first_commands.len();

    let command_system =
        |mut vd: VecDeque<Vec<Box<dyn TestCommandTrait + Send + Sync + 'static>>>| {
            move |mut commands: Commands<'_, '_>| {
                let cv = vd.pop_front().unwrap();
                commands.add(move |world: &mut World| {
                    cv.into_iter().for_each(|mut c| c.write(world));
                });
            }
        };
    let check_system = |mut vd: VecDeque<Option<DebugLog>>, after_first: bool| {
        move |controller: Res<'_, Controller>, mut count: ResMut<'_, usize>| {
            if let Some(check) = vd.pop_front().unwrap() {
                let i = N - vd.len();
                *count += 1;
                if after_first {
                    if matches!(controller.progress_current, Progress::Forward | Progress::ForwardTo{ .. }){
                        assert_eq!(controller.log_index, 0);
                        assert_eq!(controller.log.get(0).unwrap().len(), 0);
                    }
                    let log = &controller.debug.front().unwrap().after_first;
                    assert_eq!(
                        log, &check,
                        "\nTest #{i} (after_first)\n{:#?}",
                        controller.debug
                    );
                } else {
                    let log = controller
                        .debug
                        .front()
                        .unwrap()
                        .after_last
                        .as_ref()
                        .unwrap();
                    assert_eq!(
                        log, &check,
                        "\nTest #{i} (after_last)\n{:#?}",
                        controller.debug
                    );
                }
            }
        }
    };

    //set up controller systems and tests
    let mut app = App::new();
    app.init_resource::<usize>();
    Controller::into_world(
        Wrapping(0),
        VecDeque::with_capacity(constants.log_len),
        constants,
        &mut app.world,
    );
    app.add_system_to_stage(CoreStage::First, command_system(before_first_commands));
    app.add_system_to_stage(CoreStage::PreUpdate, Controller::system_first);
    app.add_system_to_stage(CoreStage::Update, command_system(after_first_commands));
    app.add_system_to_stage(CoreStage::Update, check_system(after_first_checks, true));
    app.add_system_to_stage(CoreStage::PostUpdate, Controller::system_last);
    app.add_system_to_stage(CoreStage::Last, check_system(after_last_checks, false));

    //run
    (0..ticks).for_each(|_| app.update());
    assert_eq!(*app.world.resource::<usize>(), N * 2, "not all tests ran");

    app.world.remove_resource::<Controller>().unwrap().debug
}

impl Default for DebugLog {
    fn default() -> Self {
        Self {
            time_step_query: None,
            time_step: 0.0,
            first_ran: false,
            progress_current: Progress::Forward,
            progress_query: None,
            time_stamp: Wrapping(0),
            forget: -Wrapping(CONTROLLER_CONSTS.max_log_index),
            to_time_stamp: Default::default(),
            log_len: 1,
            log_at_index_len: 0,
            log_index: 0,
            delayed_commands_len: 1,
            commands_overflows: 0,
        }
    }
}

struct TestCommand<C: Command>(Option<C>);

trait TestCommandTrait {
    fn write(&mut self, world: &mut World);
}

impl<C: Command> TestCommandTrait for TestCommand<C> {
    fn write(&mut self, world: &mut World) {
        self.0.take().unwrap().write(world);
    }
}

impl<C: Command> From<C> for Box<dyn TestCommandTrait + Send + Sync + 'static> {
    fn from(command: C) -> Self {
        Box::new(TestCommand(Some(command)))
    }
}

struct LogAtIndexLenCommand;

impl ReversibleCommand for LogAtIndexLenCommand{
    fn init(self: Box<Self>, _: &mut World) -> Option<Box<dyn crate::commands::ReversibleCommandInitialized>> {
        Some(self)
    }
}

impl ReversibleCommandInitialized for LogAtIndexLenCommand{
    fn redo(&mut self, _: &mut World) {}
    fn undo(&mut self, _: &mut World) {}
    fn redo_finalize(self: Box<Self>, _: &mut World) {}
    fn undo_finalize(self: Box<Self>, _: &mut World) {}
}

impl Command for LogAtIndexLenCommand{
    fn write(self, world: &mut World) {
        let mut controller = world.resource_mut::<Controller>();
        assert!(controller.first_ran, "{}", controller.time_stamp);
        if !matches!(controller.progress_current, Progress::Forward | Progress::ForwardTo { .. }){
            return;
        }
        let v = (0..controller.time_stamp.0).map(|_| Box::new(LogAtIndexLenCommand) as _).collect();
        controller.add_delayed_commands(0, v);
    }
}

trait ShowTestResult{
    fn show_result(self);
}

impl ShowTestResult for VecDeque<DebugLogContainer>{
    fn show_result(self) {
        println!("{self:#?}");
    }
}