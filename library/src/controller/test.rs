use std::num::Wrapping;

use bevy::prelude::{App, CoreStage, Local, Res, ResMut};

use crate::{controller::Progress, Ticks};

use super::{Controller, ControllerConsts};

mod forward;
mod forward_fast;
mod forward_log;
mod forward_log_end;
mod backward_log;
mod backward_log_end;
mod log_end;
mod pause;
mod pause_log;

#[derive(Clone, Copy)]
pub(super) struct TestAssert {
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

impl TestAssert {
    pub(super) fn assert_eq(&self, controller: &Controller, test_index: usize, at_update: bool) {
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

impl Default for TestAssert {
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

#[derive(Default, Clone, Copy)]
pub(super) struct Test {
    control: TestControl,
    assert_at_update: TestAssert,
    assert_at_end: TestAssert,
}

#[derive(Default, Clone, Copy)]
pub(super) struct TestControl {
    progress_query: Option<Progress>,
    time_step_query: Option<f64>
}

impl Test {
    pub(super) fn tests(
        constants: ControllerConsts,
        setup: impl IntoIterator<Item = TestControl>,
        tests: Vec<Self>,
    ) {
        let mut app = App::new();
        Controller::insert_command(Default::default(), Default::default(), &mut app.world, constants);
        app.world.resource_mut::<Controller>().time_step = 0.0;

        let tests_len = tests.len();
        let (update, last): (Vec<(TestControl, Option<TestAssert>)>, Vec<Option<TestAssert>>) = setup
            .into_iter()
            .map(|control| ((control, None), None))
            .chain(
                tests
                    .into_iter()
                    .map(|test| ((test.control, Some(test.assert_at_update)), Some(test.assert_at_end)))
            )
            .unzip();

        let len = update.len();
        let offset = len - tests_len;
            
        app.add_system_to_stage(CoreStage::PreUpdate, Controller::first_system);
        app.add_system_to_stage(
            CoreStage::Update,
            move |mut controller: ResMut<'_, Controller>, mut index: Local<'_, usize>| {
                let entry = &update[*index];
                if let Some(query) = entry.0.progress_query {
                    controller.query_progress(query);
                }
                if let Some(time_step) = entry.0.time_step_query {
                    controller.query_time_step(time_step);
                }
                if let Some(assert) = entry.1{
                    let test_index = *index - offset;
                    assert.assert_eq(&controller, test_index, true);
                }
                *index += 1;
            },
        );
        app.add_system_to_stage(CoreStage::PostUpdate, Controller::last_system);
        app.add_system_to_stage(
            CoreStage::Last,
            move |controller: Res<'_, Controller>, mut index: Local<'_, usize>| {
                if let Some(assert) = &last[*index]{
                    let test_index = *index - offset;
                    assert.assert_eq(&controller, test_index, false);
                }
                *index += 1;
            },
        );

        (0..len).for_each(|_| app.update());
    }
}