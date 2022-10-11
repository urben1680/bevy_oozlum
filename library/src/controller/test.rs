use std::{num::Wrapping, collections::VecDeque};

use bevy::prelude::{App, CoreStage, Res, Local};

use crate::{controller::Progress, MAX_LOG_INDEX, Ticks, commands::ReversibleCommandInitialized};

use super::Controller;

struct ControllerCmp{
    progress_query: Option<Progress>,
    log_len: usize,
    time_stamp: Ticks,
    forget: Ticks,
    forward_fast_limit: Option<Ticks>,
    log_index: usize,
    progress: Progress,
    pre_update_ran: bool,
    fast_init: bool,
    log_end: bool,
    delayed_commands_len: usize,
    delayed_commands_first_len: usize,
    commands_overflows: u64,
}

impl ControllerCmp{
    fn assert_eq(&self, controller: &Controller, test_id: usize, at_update: bool){
        assert_eq!(self.progress_query, controller.progress_query, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.log_len,  controller.log.len(), "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.time_stamp, controller.time_stamp.0, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.forget, controller.forget.0, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        if let Some(forward_fast_limit) = self.forward_fast_limit{
            assert_eq!(forward_fast_limit, controller.forward_fast_limit.0, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        }
        assert_eq!(self.log_index, controller.log_index, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.progress, controller.progress, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.pre_update_ran, controller.pre_update_ran, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.fast_init, controller.fast_init, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.log_end, controller.log_end, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.delayed_commands_len, controller.delayed_commands.len(), "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.delayed_commands_first_len, controller.delayed_commands[0].len(), "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert_eq!(self.commands_overflows, controller.commands_overflows, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert!(controller.progress.is_pause() || controller.pre_update_ran, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
        assert!(controller.delayed_commands.len() > 0, "test_index: {test_id}, at_update: {at_update:?}, controller: {controller:?}");
    }
}

impl Default for ControllerCmp{
    fn default() -> Self {
        Self { 
            progress_query: None, 
            log_len: 0, 
            time_stamp: 0, 
            forget: (Wrapping(0) - Wrapping(MAX_LOG_INDEX)).0, 
            forward_fast_limit: None, 
            log_index: 0, 
            progress: Progress::Forward, 
            pre_update_ran: false, 
            fast_init: false, 
            log_end: false, 
            delayed_commands_len: 1, 
            delayed_commands_first_len: 0, 
            commands_overflows: 0 
        }   
    }
}

struct Test{
    progress_query: Option<Progress>,
    at_update: ControllerCmp,
    at_end: Option<ControllerCmp>
}

impl Test{
    fn tests<const N: usize>(log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>, time_stamp: Wrapping<Ticks>, tests: &'static [Self; N]){
        let mut app = App::new();
        Controller::insert_command(log, time_stamp, &mut app.world);
        app.world.resource_mut::<Controller>().time_step = 0.0;
        app.add_system_to_stage(CoreStage::PreUpdate, Controller::first_system);
        app.add_system_to_stage(CoreStage::Update, |controller: Res<'_, Controller>, mut index: Local<'_, usize>| {
            tests[*index].at_update.assert_eq(&controller, *index, true);
            *index += 1;
        });
        app.add_system_to_stage(CoreStage::PostUpdate, Controller::last_system);
        app.add_system_to_stage(CoreStage::Last, |controller: Res<'_, Controller>, mut index: Local<'_, usize>| {
            if let Some(at_end) = &tests[*index].at_end{
                at_end.assert_eq(&controller, *index, false)
            }
            *index += 1;
        });
        (0..N).for_each(|_| app.update());
    }
}

#[test]
fn forward(){
    Test::tests(Default::default(), Default::default(), &[]);
    /*
    let progress = Progress::Forward;
    let mut app = app(progress);

    app.update_controller(1, progress, false, false);
    app.update_controller(2, progress, false, false);
    */
}

#[test]
fn forward_fast(){
    /*
    let to_time_stamp = Wrapping(3);
    let progress = Progress::ForwardFast{ to_time_stamp };
    let mut app = app(progress);

    let c = app.update_controller(1, progress, true, false);
    assert_eq!(c.forward_fast_limit(), to_time_stamp);

    println!("--");

    let c = app.update_controller();
    assert_eq!(c.time_stamp().0, 2);
    assert_eq!(c.forget() + Wrapping(MAX_LOG_INDEX), c.time_stamp());
    assert_eq!(c.fast_init(), false);
    
    println!("--");

    let c = app.update_controller();
    assert_eq!(c.time_stamp().0, 3);
    assert_eq!(c.forget() + Wrapping(MAX_LOG_INDEX), c.time_stamp());
    assert_eq!(c.fast_init(), false);
    */
}