use std::{collections::VecDeque, num::Wrapping};

use bevy::{
    ecs::system::Command,
    prelude::{App, Commands, CoreStage, Res, ResMut, World},
};

use super::{
    consts::{ControllerConsts, CONTROLLER_CONSTS},
    debug::DebugLog,
    progress::{Progress, ProgressQuery},
    Controller,
};

mod forward;
mod forward_fast;

const TEST_CONTROLLER_CONSTS: ControllerConsts = ControllerConsts {
    default_time_step: 0.0,
    ..CONTROLLER_CONSTS
};

#[derive(Copy, Clone, Default)]
struct Control {
    progress_query: Option<ProgressQuery>,
    time_step_query: Option<f64>,
}

struct Test {
    before_first: Control,
    after_first: DebugLog,
    after_first_command: Option<fn(&mut World)>,
    after_last: DebugLog,
    after_last_command: Option<fn(&mut World)>,
}

fn tests<I: IntoIterator<Item = Control>, const N: usize>(
    constants: ControllerConsts,
    setup: I,
    tests: [Test; N],
) {
    //convert input
    let (mut controls, (mut after_firsts, mut after_lasts)): (
        VecDeque<Control>,
        (
            VecDeque<(Option<DebugLog>, Option<fn(&mut World)>)>,
            VecDeque<(Option<DebugLog>, Option<fn(&mut World)>)>,
        ),
    ) = setup
        .into_iter()
        .map(|control| (control, ((None, None), (None, None))))
        .chain(tests.into_iter().map(|mut test| {
            //setting these values here makes `tests` less verbose
            let forget_delta = Wrapping(constants.max_log_index);
            test.after_first.forget = test.after_first.time_stamp - forget_delta;
            test.after_last.forget = test.after_last.time_stamp - forget_delta;
            test.after_first.first_ran = true;
            test.after_last.first_ran = false;
            (
                test.before_first,
                (
                    (Some(test.after_first), test.after_first_command),
                    (Some(test.after_last), test.after_last_command),
                ),
            )
        }))
        .unzip();
    let ticks = controls.len();

    //set up controller systems and tests
    let mut app = App::new();
    Controller::into_world(
        Wrapping(0),
        true,
        VecDeque::with_capacity(constants.log_len),
        constants,
        &mut app.world,
    );
    app.add_system_to_stage(
        CoreStage::First,
        move |mut controller: ResMut<'_, Controller>| {
            let control = controls.pop_front().unwrap();
            if let Some(progress) = control.progress_query {
                controller.query_progress(progress);
            }
            if let Some(time_step) = control.time_step_query {
                controller.query_time_step(time_step);
            }
        },
    );
    app.add_system_to_stage(CoreStage::PreUpdate, Controller::system_first);
    app.add_system_to_stage(
        CoreStage::Update,
        move |controller: Res<'_, Controller>, mut commands: Commands<'_, '_>| {
            let next = after_firsts.pop_front().unwrap();
            if let Some(test) = next.0 {
                let i = N - after_firsts.len();
                let log = &controller.debug.front().unwrap().after_first;
                assert_eq!(
                    log, &test,
                    "\nTest #{i} (after_first)\n{:#?}",
                    controller.debug
                );
            }
            if let Some(command) = next.1 {
                commands.add(command);
            }
        },
    );
    app.add_system_to_stage(CoreStage::PostUpdate, Controller::system_last);
    app.add_system_to_stage(
        CoreStage::Last,
        move |controller: Res<'_, Controller>, mut commands: Commands<'_, '_>| {
            let next = after_lasts.pop_front().unwrap();
            if let Some(test) = next.0 {
                let i = N - after_lasts.len();
                let log = controller
                    .debug
                    .front()
                    .unwrap()
                    .after_last
                    .as_ref()
                    .unwrap();
                assert_eq!(
                    log, &test,
                    "\nTest #{i} (after_last)\n{:#?}",
                    controller.debug
                );
            }
            if let Some(command) = next.1 {
                commands.add(command);
            }
        },
    );

    //run
    (0..ticks).for_each(|_| app.update());
}

impl Default for Test {
    fn default() -> Self {
        Self {
            before_first: Default::default(),
            after_first: Default::default(),
            after_first_command: None,
            after_last: Default::default(),
            after_last_command: None,
        }
    }
}

impl Default for DebugLog {
    fn default() -> Self {
        Self {
            time_step_query: None,
            time_step: 0.0,
            first_ran: false,
            current: Progress::Forward {
                after_forward: true,
            },
            progress_query: None,
            time_stamp: Wrapping(0),
            forget: -Wrapping(CONTROLLER_CONSTS.max_log_index),
            forward_fast_limit: Default::default(),
            log_len: 1,
            log_index: 0,
            delayed_commands_len: 0,
            commands_overflows: 0,
        }
    }
}
