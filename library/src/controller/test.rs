use std::{num::Wrapping, fmt::Debug};

use bevy::{prelude::{App, CoreStage, Local, Res, ResMut}, ecs::system::Resource};

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

/*
todo:
- test commands wenn system mockups und command mockups existieren
*/

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
struct TestMeta<'a, T: Debug> {
    test_index: isize,
    at_update: bool,
    controller: &'a Controller,
    param: &'a mut T,
    log: Option<&'a Vec<PrettyPrintedString>>
}

struct PrettyPrintedString(String);

impl Debug for PrettyPrintedString{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

trait TestSystem<T: Resource + Default + Debug>: Resource{
    fn test(&self, meta: TestMeta<'_, T>);
}

struct NoTest;

impl<T: Resource + Default + Debug> TestSystem<T> for NoTest{
    fn test(&self, _meta: TestMeta<'_, T>) {}
}

impl TestSystem<()> for TestAssert{
    fn test(&self, meta: TestMeta<'_, ()>){
        assert_eq!(
            self.progress_query, meta.controller.progress_query,
            "controller.progress_query\n{meta:#?}"
        );
        assert_eq!(
            self.log_len,
            meta.controller.log.len(),
            "controller.log.len()\n{meta:#?}"
        );
        assert_eq!(
            Some(self.log_first_len),
            meta.controller.log.get(0).map(|c| c.len()),
            "controller.log.get(1).map(|c|c.len())\n{meta:#?}"
        );
        assert_eq!(
            self.time_stamp, meta.controller.time_stamp.0,
            "controller.time_stamp\n{meta:#?}"
        );
        assert_eq!(
            meta.controller.time_stamp - Wrapping(meta.controller.consts().max_log_index),
            meta.controller.forget,
            "controller.forget\n{meta:#?}"
        );
        if let Some(forward_fast_limit) = self.forward_fast_limit {
            assert_eq!(
                forward_fast_limit, meta.controller.forward_fast_limit.0,
                "controller.forward_fast_limit\n{meta:#?}"
            );
        }
        assert_eq!(
            self.log_index, meta.controller.log_index,
            "controller.log_index\n{meta:#?}"
        );
        assert_eq!(
            self.progress, meta.controller.progress,
            "controller.progress\n{meta:#?}"
        );
        assert_eq!(
            self.fast_init, meta.controller.fast_init,
            "controller.fast_init\n{meta:#?}"
        );
        assert_eq!(
            self.log_end, meta.controller.log_end,
            "controller.log_end\n{meta:#?}"
        );
        assert_eq!(
            self.delayed_commands_len,
            meta.controller.delayed_commands.len(),
            "controller.delayed_commands.len()\n{meta:#?}"
        );
        assert_eq!(
            Some(self.delayed_commands_first_len),
            meta.controller.delayed_commands.get(0).map(|c| c.len()),
            "controller.delayed_commands.get(1).map(|c|c.len())\n{meta:#?}"
        );
        assert_eq!(
            self.commands_overflows, meta.controller.commands_overflows,
            "controller.commands_overflows\n{meta:#?}"
        );
        assert!(meta.controller.progress.is_pause() || (meta.controller.pre_update_ran == meta.at_update), "controller.progress.is_pause() || (controller.pre_update_ran == at_update)\n{meta:#?}");
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

#[derive(Default)]
pub(super) struct Test<T: Resource + Default + Debug> {
    control: TestControl,
    assert_at_update: Box<dyn TestSystem<T>>,
    assert_at_end: Box<dyn TestSystem<T>>,
}

impl<T: Resource + Default + Debug> Default for Box<dyn TestSystem<T>>{
    fn default() -> Self {
        Box::new(NoTest)
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct TestControl {
    progress_query: Option<Progress>,
    time_step_query: Option<f64>
}

impl TestControl{
    fn to_tuple<T: Resource + Default + Debug>(self) -> ((TestControl, Box<dyn TestSystem<T>>), Box<dyn TestSystem<T>>){
        ((self, Box::new(NoTest)), Box::new(NoTest))
    }
}

impl<T: Resource + Default + Debug> Test<T> {
    pub(super) fn tests(
        constants: ControllerConsts,
        setup: impl IntoIterator<Item = TestControl>,
        tests: Vec<Test<T>>,
    ) {
        let mut app = App::new();
        Controller::insert_command(Default::default(), Default::default(), &mut app.world, constants);
        app.world.resource_mut::<Controller>().time_step = 0.0;
        app.init_resource::<Vec<PrettyPrintedString>>();
        app.init_resource::<T>();

        let tests_len = tests.len();

        let (update, last): (Vec<(TestControl, Box<dyn TestSystem<T>>)>, Vec<Box<dyn TestSystem<T>>>) = setup
            .into_iter()
            .map(|control| control.to_tuple())
            .chain(
                tests
                    .into_iter()
                    .map(|test| ((test.control, test.assert_at_update), test.assert_at_end))
            )
            .unzip();

        let len = update.len();
        let offset = (len - tests_len) as isize;
            
        app.add_system_to_stage(CoreStage::PreUpdate, Controller::first_system);
        app.add_system_to_stage(
            CoreStage::Update,
            move |mut controller: ResMut<'_, Controller>, mut param: ResMut<'_, T>, mut index: Local<'_, usize>, mut log: ResMut<'_, Vec<PrettyPrintedString>>| {
                let entry = &update[*index];
                if let Some(query) = entry.0.progress_query {
                    controller.query_progress(query);
                }
                if let Some(time_step) = entry.0.time_step_query {
                    controller.query_time_step(time_step);
                }
                let test_index = *index as isize - offset;
                let meta = TestMeta { test_index, at_update: true, controller: &controller, param: &mut *param, log: Some(& *log) };
                entry.1.test(meta);
                let meta = TestMeta { test_index, at_update: true, controller: &controller, param: &mut *param, log: None };
                log.push(PrettyPrintedString(format!("{meta:#?}")));
                *index += 1;
            },
        );
        app.add_system_to_stage(CoreStage::PostUpdate, Controller::last_system);
        app.add_system_to_stage(
            CoreStage::Last,
            move |controller: Res<'_, Controller>, mut param: ResMut<'_, T>, mut index: Local<'_, usize>, mut log: ResMut<'_, Vec<PrettyPrintedString>>| {
                let test_index = *index as isize - offset;
                let meta = TestMeta { test_index, at_update: false, controller: &controller, param: &mut *param, log: Some(& *log) };
                last[*index].test(meta);
                let meta = TestMeta { test_index, at_update: false, controller: &controller, param: &mut *param, log: None };
                log.push(PrettyPrintedString(format!("{meta:#?}")));
                *index += 1;
            },
        );

        (0..len).for_each(|_| app.update());
    }
}