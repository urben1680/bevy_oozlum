use std::{collections::VecDeque, num::Wrapping};

use bevy::{
    ecs::system::Command,
    prelude::{App, Commands, CoreStage, ResMut, World},
};

use crate::Ticks;

use super::{
    consts::{ControllerConsts, CONTROLLER_CONSTS},
    debug::DebugLog,
    progress::{Progress, ProgressQuery},
    Controller,
};

mod forward;
//mod forward_fast;

const TEST_CONTROLLER_CONSTS: ControllerConsts = ControllerConsts {
    default_time_step: 0.0,
    ..CONTROLLER_CONSTS
};

#[derive(Default)]
struct Test {
    before_first_commands: Vec<fn(&mut World)>,
    after_first_check: DebugLog,
    after_first_commands: Vec<fn(&mut World)>,
    after_last_check: DebugLog,
}

fn tests<I: IntoIterator<Item = Option<ProgressQuery>>, const N: usize>(
    constants: ControllerConsts,
    setup: I,
    tests: [Test; N],
) {
    //convert input
    let (before_first_commands, (after_first_commands, (after_first_checks, after_last_checks))): (
        VecDeque<Vec<fn(&mut World)>>,
        (
            VecDeque<Vec<fn(&mut World)>>,
            (VecDeque<Option<DebugLog>>, VecDeque<Option<DebugLog>>),
        ),
    ) = setup
        .into_iter()
        .map(|control| {
            let vec: Vec<fn(&mut World)> = match control {
                Some(control) => vec![control.into()],
                None => vec![],
            };
            (vec, (vec![], (None, None)))
        })
        .chain(tests.into_iter().map(|mut test| {
            //setting these values here makes `tests` less verbose
            let forget_delta = Wrapping(constants.max_log_index);
            test.after_first_check.forget = test.after_first_check.time_stamp - forget_delta;
            test.after_first_check.first_ran = true;
            test.after_last_check.forget = test.after_last_check.time_stamp - forget_delta;
            test.after_last_check.first_ran = false;
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

    let command_system = |mut vd: VecDeque<Vec<fn(&mut World)>>| {
        move |mut commands: Commands<'_, '_>| {
            let cv = vd.pop_front().unwrap();
            if cv.is_empty() {
                return;
            }
            commands.add(move |world: &mut World| {
                cv.into_iter().for_each(|c| c(world));
            });
        }
    };
    let check_system = |mut vd: VecDeque<Option<DebugLog>>, after_first: bool| {
        move |controller: ResMut<'_, Controller>| {
            if let Some(check) = vd.pop_front().unwrap() {
                let i = N - vd.len();
                if after_first {
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
    Controller::into_world(
        Wrapping(0),
        true,
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

impl Into<fn(&mut World)> for ProgressQuery {
    fn into(self) -> fn(&mut World) {
        match self{
            ProgressQuery::Forward => |world| ProgressQuery::Forward.write(world),
            ProgressQuery::ForwardFast { to_time_stamp } => {
                match to_time_stamp.0{
                    0 => forward_fast::<0>,
                    1 => forward_fast::<1>,
                    2 => forward_fast::<2>,
                    3 => forward_fast::<3>,
                    4 => forward_fast::<4>,
                    5 => forward_fast::<5>,
                    6 => forward_fast::<6>,
                    7 => forward_fast::<7>,
                    8 => forward_fast::<8>,
                    9 => forward_fast::<9>,
                    10 => forward_fast::<10>,
                    n => unimplemented!("`Into<fn(&mut World)>` not implemented for `ProgressQuery::ForwardFast {{ to_time_stamp: Wrapping({n}) }}`")
                }
            },
            ProgressQuery::ForwardLog => |world| ProgressQuery::ForwardLog.write(world),
            ProgressQuery::ForwardLogEnd => |world| ProgressQuery::ForwardLogEnd.write(world),
            ProgressQuery::BackwardLog => |world| ProgressQuery::BackwardLog.write(world),
            ProgressQuery::BackwardLogEnd => |world| ProgressQuery::BackwardLogEnd.write(world),
            ProgressQuery::Pause => |world| ProgressQuery::Pause.write(world)
        }
    }
}

fn forward_fast<const N: Ticks>(world: &mut World) {
    ProgressQuery::ForwardFast {
        to_time_stamp: Wrapping(N),
    }
    .write(world);
}
