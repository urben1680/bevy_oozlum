use std::{collections::VecDeque, num::Wrapping};

use bevy::prelude::{App, CoreStage, Local, Res, ResMut};

use crate::{commands::ReversibleCommandInitialized, controller::Progress, Ticks};

use super::{Controller, ControllerConsts, CONTROLLER_CONSTS};

struct ControllerCmp {
    progress_query: Option<Progress>,
    log_len: usize,
    log_first_len: usize,
    time_stamp: Ticks,
    forward_fast_limit: Option<Ticks>,
    log_index: usize,
    /// determines the picked reversible system
    progress: Progress,
    fast_init: bool,
    log_end: bool,
    delayed_commands_len: usize,
    delayed_commands_first_len: usize,
    commands_overflows: u64,
}

#[allow(dead_code)]
#[derive(Debug)]
struct Failure<'a> {
    test_index: usize,
    at_update: bool,
    controller: &'a Controller,
}

impl ControllerCmp {
    fn assert_eq(&self, controller: &Controller, test_index: usize, at_update: bool) {
        let failure = Failure {
            test_index,
            at_update,
            controller,
        };
        assert_eq!(
            self.progress_query, controller.progress_query,
            "controller.progress_query\n{failure:#?}"
        );
        assert_eq!(
            self.log_len,
            controller.log.len(),
            "controller.log.len()\n{failure:#?}"
        );
        assert_eq!(
            Some(self.log_first_len),
            controller.log.get(0).map(|c| c.len()),
            "controller.log.get(1).map(|c|c.len())\n{failure:#?}"
        );
        assert_eq!(
            self.time_stamp, controller.time_stamp.0,
            "controller.time_stamp\n{failure:#?}"
        );
        assert_eq!(
            controller.time_stamp - Wrapping(controller.consts().max_log_index),
            controller.forget,
            "controller.forget\n{failure:#?}"
        );
        if let Some(forward_fast_limit) = self.forward_fast_limit {
            assert_eq!(
                forward_fast_limit, controller.forward_fast_limit.0,
                "controller.forward_fast_limit\n{failure:#?}"
            );
        }
        assert_eq!(
            self.log_index, controller.log_index,
            "controller.log_index\n{failure:#?}"
        );
        assert_eq!(
            self.progress, controller.progress,
            "controller.progress\n{failure:#?}"
        );
        assert_eq!(
            self.fast_init, controller.fast_init,
            "controller.fast_init\n{failure:#?}"
        );
        assert_eq!(
            self.log_end, controller.log_end,
            "controller.log_end\n{failure:#?}"
        );
        assert_eq!(
            self.delayed_commands_len,
            controller.delayed_commands.len(),
            "controller.delayed_commands.len()\n{failure:#?}"
        );
        assert_eq!(
            Some(self.delayed_commands_first_len),
            controller.delayed_commands.get(0).map(|c| c.len()),
            "controller.delayed_commands.get(1).map(|c|c.len())\n{failure:#?}"
        );
        assert_eq!(
            self.commands_overflows, controller.commands_overflows,
            "controller.commands_overflows\n{failure:#?}"
        );
        assert!(controller.progress.is_pause() || (controller.pre_update_ran == at_update), "controller.progress.is_pause() || (controller.pre_update_ran == at_update)\n{failure:#?}");
    }
}

impl Default for ControllerCmp {
    fn default() -> Self {
        Self {
            progress_query: None,
            log_len: 1,
            log_first_len: 0,
            time_stamp: 1,
            forward_fast_limit: None,
            log_index: 0,
            progress: Progress::Forward,
            fast_init: false,
            log_end: false,
            delayed_commands_len: 1,
            delayed_commands_first_len: 0,
            commands_overflows: 0,
        }
    }
}

#[derive(Default)]
struct Test {
    progress_query: Option<Progress>,
    time_step_query: Option<f64>,
    at_update: ControllerCmp,
    at_end: ControllerCmp,
}

impl Test {
    fn tests<const N: usize>(
        log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
        time_stamp: Wrapping<Ticks>,
        constants: ControllerConsts,
        tests: [Self; N],
    ) {
        let (update, last): (
            Vec<(Option<Progress>, Option<f64>, ControllerCmp)>,
            Vec<ControllerCmp>,
        ) = tests
            .into_iter()
            .map(|test| {
                (
                    (test.progress_query, test.time_step_query, test.at_update),
                    test.at_end,
                )
            })
            .unzip();
        let mut app = App::new();
        Controller::insert_command(log, time_stamp, &mut app.world, constants);
        app.world.resource_mut::<Controller>().time_step = 0.0;
        app.add_system_to_stage(CoreStage::PreUpdate, Controller::first_system);
        app.add_system_to_stage(
            CoreStage::Update,
            move |mut controller: ResMut<'_, Controller>, mut index: Local<'_, usize>| {
                let entry = &update[*index];
                if let Some(query) = entry.0 {
                    controller.query_progress(query);
                }
                if let Some(time_step) = entry.1 {
                    controller.query_time_step(time_step);
                }
                entry.2.assert_eq(&controller, *index, true);
                *index += 1;
            },
        );
        app.add_system_to_stage(CoreStage::PostUpdate, Controller::last_system);
        app.add_system_to_stage(
            CoreStage::Last,
            move |controller: Res<'_, Controller>, mut index: Local<'_, usize>| {
                last[*index].assert_eq(&controller, *index, false);
                *index += 1;
            },
        );
        (0..N).for_each(|_| app.update());
    }
}

/*
TODO
List of tests:
- forward
-- keeps running at this progress
-- changes on query
- forward fast
-- keeps running at this progress
-- sets fast_init to true for the first step
-- at the end switches to forward with no query
-- at the end switches to query
-- queries overwrite each other
-- immediately reacts on extending limit
- forward log / backward log
-- keeps running at this progress
-- calls commands as expected
-- stops at the end end and changes to log pause
-- triggers log end when reacting on non-log progress query (test all)
-- reacts other log progresses immediately without triggering log end (test all)
- forward fast log / backward fast log
-- keeps running at this progress
-- sets fast_init to true for the first step
-- calls commands as expected during correct time stamps
-- stops at the end end and changes to log pause without set query
-- stops at the end end and changes to query
-- queries overwrite each other while ignored by fast log variant
- pause
-- keeps running at this progress
-- has zero time step
-- reacts other log progresses immediately
- log pause
-- keeps running at this progress
-- has zero time step
-- refuses to change into other log progress that would overstep log ends
-- triggers log end when reacting on non-log progress query (test all)
-- reacts other log progresses immediately without triggering log end (test all)
- log end
-- keeps running at this progress
-- has zero time step
-- switches to query next step (test all)

check controller source to come up with more tests


*/

#[test]
fn forward() {
    Test::tests(
        Default::default(),
        Default::default(),
        CONTROLLER_CONSTS,
        [
            Test {
                //#0
                progress_query: None,
                time_step_query: None,
                at_update: ControllerCmp {
                    log_len: 1,
                    time_stamp: 1,
                    ..Default::default()
                },
                at_end: ControllerCmp {
                    log_len: 1,
                    time_stamp: 1,
                    ..Default::default()
                },
            },
            Test {
                //#1
                progress_query: None,
                time_step_query: None,
                at_update: ControllerCmp {
                    log_len: 2,
                    time_stamp: 2,
                    ..Default::default()
                },
                at_end: ControllerCmp {
                    log_len: 2,
                    time_stamp: 2,
                    ..Default::default()
                },
            },
        ],
    );
}

#[test]
/// Test the init, the course and the end of FastForward including the behavior to then update progress to a previous query
fn forward_fast() {
    Test::tests(
        Default::default(),
        Default::default(),
        CONTROLLER_CONSTS,
        [
            Test {
                //#0
                progress_query: Some(Progress::ForwardFast {
                    to_time_stamp: Wrapping(3),
                }),
                time_step_query: None,
                at_update: ControllerCmp {
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    progress: Progress::Forward,
                    forward_fast_limit: None,
                    delayed_commands_len: 1,
                    fast_init: false,
                    log_len: 1,
                    time_stamp: 1,
                    ..Default::default()
                },
                at_end: ControllerCmp {
                    progress_query: None,
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    forward_fast_limit: Some(3),
                    delayed_commands_len: 3,
                    fast_init: true,
                    log_len: 1,
                    time_stamp: 1,
                    ..Default::default()
                },
            },
            Test {
                //#1
                progress_query: Some(Progress::Pause),
                time_step_query: None,
                at_update: ControllerCmp {
                    progress_query: Some(Progress::Pause),
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    forward_fast_limit: Some(3),
                    delayed_commands_len: 3,
                    fast_init: true,
                    log_len: 2,
                    time_stamp: 2,
                    ..Default::default()
                },
                at_end: ControllerCmp {
                    progress_query: Some(Progress::Pause),
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    forward_fast_limit: Some(3),
                    delayed_commands_len: 2,
                    fast_init: false,
                    log_len: 2,
                    time_stamp: 2,
                    ..Default::default()
                },
            },
            Test {
                //#2
                progress_query: None,
                time_step_query: None,
                at_update: ControllerCmp {
                    progress_query: Some(Progress::Pause),
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    forward_fast_limit: Some(3),
                    delayed_commands_len: 2,
                    fast_init: false,
                    log_len: 3,
                    time_stamp: 3,
                    ..Default::default()
                },
                at_end: ControllerCmp {
                    progress_query: None,
                    progress: Progress::Pause,
                    forward_fast_limit: None,
                    delayed_commands_len: 1,
                    fast_init: false,
                    log_len: 3,
                    time_stamp: 3,
                    ..Default::default()
                },
            },
        ],
    );
}
