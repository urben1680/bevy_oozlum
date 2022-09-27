use std::{marker::PhantomData, collections::VecDeque, ops::Index, any::{TypeId, type_name}, num::Wrapping, mem::MaybeUninit};

use bevy::{ecs::{system::{SystemParam, Resource, StaticSystemParam, SystemParamFetch, SystemParamItem}, query::{WorldQuery, QueryItem, Fetch}, schedule::IntoSystemDescriptor}, prelude::{Query, Component, Without, Res, ResMut, System, App, Commands, EventWriter, ParallelCommands}};

use crate::{DespawnedEntity, Ticks, commands::{ReversibleCommands, NextCommands, CommandsScope, DelayedCommandWrapper}, MAX_LOG_LEN, controller::{Controller, Forget}};


mod resource_mutation;
mod component_mutation;

pub use resource_mutation::*;
pub use component_mutation::*;


#[repr(u8)]
enum SystemId{
    Advance,
    AdvanceTimestamp,
    AdvanceLog,
    AdvanceLogTimestamp,
    RevertLog,
    RevertLogTimestamp,
    LogEnd,
    LogAgeCheck
}

impl SystemId{
    const ADVANCE: u8 = Self::Advance as u8;
    const ADVANCE_TIMESTAMP: u8 = Self::AdvanceTimestamp as u8;
    const ADVANCE_LOG: u8 = Self::AdvanceLog as u8;
    const ADVANCE_LOG_TIMESTAMP: u8 = Self::AdvanceLogTimestamp as u8;
    const REVERT_LOG: u8 = Self::RevertLog as u8;
    const REVERT_LOG_TIMESTAMP: u8 = Self::RevertLogTimestamp as u8;
    const LOG_END: u8 = Self::LogEnd as u8;
    const LOG_AGE_CHECK: u8 = Self::LogAgeCheck as u8;
}

impl TryFrom<u8> for SystemId{
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error>{
        match value{
            x if x == SystemId::Advance as u8 => Ok(SystemId::Advance),
            x if x == SystemId::AdvanceTimestamp as u8 => Ok(SystemId::AdvanceTimestamp),
            x if x == SystemId::AdvanceLog as u8 => Ok(SystemId::AdvanceLog),
            x if x == SystemId::AdvanceLogTimestamp as u8 => Ok(SystemId::AdvanceLogTimestamp),
            x if x == SystemId::RevertLog as u8 => Ok(SystemId::RevertLog),
            x if x == SystemId::RevertLogTimestamp as u8 => Ok(SystemId::RevertLogTimestamp),
            x if x == SystemId::LogEnd as u8 => Ok(SystemId::LogEnd),
            x if x == SystemId::LogAgeCheck as u8 => Ok(SystemId::LogAgeCheck),
            _ => Err(())
        }
    }
}


pub struct NextTransitionWithState<Transition, Marker: Send + Sync + 'static>{
    next_state_index: usize,
    transition: Transition,
    commands: Option<NextCommands<Marker>>
}

impl<Transition, Marker: Send + Sync + 'static> NextTransitionWithState<Transition, Marker>{
    pub fn new(next_state_index: usize, transition: Transition) -> Self{
        Self { next_state_index, transition, commands: None }
    }
    pub fn new_with_commands<F: FnOnce(ReversibleCommands<Marker>) + Send + Sync + 'static>(next_state_index: usize, transition: Transition, commands: F) -> Self{
        Self { next_state_index, transition, commands: Some(Box::new(commands)) }
    }
}

pub struct NextTransition<Transition, Marker: Send + Sync + 'static>{
    transition: Transition,
    commands: Option<NextCommands<Marker>>
}

impl<Transition, Marker: Send + Sync + 'static> NextTransition<Transition, Marker>{
    pub fn new(transition: Transition) -> Self{
        Self { transition, commands: None }
    }
    pub fn new_with_commands<F: FnOnce(ReversibleCommands<Marker>) + Send + Sync + 'static>(transition: Transition, commands: F) -> Self{
        Self { transition, commands: Some(Box::new(commands)) }
    }
}

trait NextTransitionTrait<Transition, Marker: Send + Sync + 'static>{
    fn explode(self) -> (usize, Transition, Option<NextCommands<Marker>>);
}

impl<Transition, Marker: Send + Sync + 'static> NextTransitionTrait<Transition, Marker> for NextTransition<Transition, Marker>{
    fn explode(self) -> (usize, Transition, Option<NextCommands<Marker>>){
        (Default::default(), self.transition, self.commands)
    }
}

impl<Transition, Marker: Send + Sync + 'static> NextTransitionTrait<Transition, Marker> for NextTransitionWithState<Transition, Marker>{
    fn explode(self) -> (usize, Transition, Option<NextCommands<Marker>>){
        (self.next_state_index, self.transition, self.commands)
    }
}

#[derive(Component)]
struct Log<Entry: LogEntryTrait<Transition>, Transition: Resource, Marker: Resource>{
    entry_index: usize,
    entries: VecDeque<Entry>,
    p: PhantomData<(Transition, Marker)>
}

struct LogEntryWithState<Transition: Resource>{
    transition: MaybeUninit<Transition>,
    time_stamp: Wrapping<Ticks>,
    state_index: usize,
}

struct LogEntry<Transition: Resource>{
    transition: MaybeUninit<Transition>,
    time_stamp: Wrapping<Ticks>
}

trait LogEntryTrait<Transition: Resource>{
    fn new(time_stamp: Wrapping<Ticks>, state_index: usize) -> Self;
    fn state_index(&self) -> usize;
    unsafe fn transition(&self) -> &Transition;
    fn time_stamp(&self) -> Wrapping<Ticks>;
    fn set_transition(&mut self, transition: Transition);
    //fn set_time_stamp(&mut self, time_stamp: Wrapping<Ticks>);
}

impl<Transition: Resource> LogEntryTrait<Transition> for LogEntry<Transition>{
    fn new(time_stamp: Wrapping<Ticks>, _state_index: usize) -> Self {
        Self { transition: MaybeUninit::uninit(), time_stamp }
    }
    fn state_index(&self) -> usize{
        Default::default()
    }
    unsafe fn transition(&self) -> &Transition{
        &self.transition.assume_init_ref()
    }
    fn time_stamp(&self) -> Wrapping<Ticks>{
        self.time_stamp
    }
    fn set_transition(&mut self, transition: Transition){
        self.transition.write(transition);
    }
}

impl<Transition: Resource> LogEntryTrait<Transition> for LogEntryWithState<Transition>{
    fn new(time_stamp: Wrapping<Ticks>, state_index: usize) -> Self {
        Self { transition: MaybeUninit::uninit(), time_stamp, state_index }
    }
    fn state_index(&self) -> usize{
        self.state_index
    }
    unsafe fn transition(&self) -> &Transition{
        &self.transition.assume_init_ref()
    }
    fn time_stamp(&self) -> Wrapping<Ticks>{
        self.time_stamp
    }
    fn set_transition(&mut self, transition: Transition){
        self.transition.write(transition);
    }
}

trait GetState: SystemParam{
    type Output;
    fn get_state(&self, index: usize) -> &Self::Output;
}

impl<'w, T: Resource> GetState for Res<'w, Vec<T>>{
    type Output = T;
    fn get_state(&self, index: usize) -> &Self::Output{
        self.get(index).unwrap()
    }
}

impl GetState for (){
    type Output = ();
    fn get_state(&self, _index: usize) -> &Self::Output{
        &()
    }
}

trait Mutation{
    type In: SystemParam;
    type Out;
}

pub struct SystemContainer<
    'w, 's,
    Params: SystemParam,
    ParamsOnlyLog: SystemParam,
    Commands: CommandsScope<'w, 's>
>{
    pub advance: fn(Params, Res<'w, Controller>, Commands),
    pub advance_timestamp: fn(Params, Res<'w, Controller>, Commands), //commands must be applied later when returning timestamp is reached
    pub advance_log: fn(Params, Res<'w, Controller>),
    pub advance_log_timestamp: fn(Params, Res<'w, Controller>),
    pub revert_log: fn(Params, Res<'w, Controller>),
    pub revert_log_timestamp: fn(Params, Res<'w, Controller>),
    pub log_end: fn(ParamsOnlyLog, Res<'w, Controller>),
    pub log_age_check: fn(ParamsOnlyLog, Res<'w, Controller>), //check if timestamp of oldest entry is equal to current time stamp to drop just when time stamp was raised in controller
    p: PhantomData<&'s()>
    /*
    Unterschied advance_timestamp und advance_log_timestamp?
    - keiner, controller.target_time_stamp() nutzen
    - doch, einer ruft next auf und schreibt commands, der andere nicht
    */
}

#[allow(clippy::too_many_arguments)]
fn advance_system<
    'w, 's,
    Params,
    States: GetState,
    CommandsType: CommandsScope<'w, 's>,
    Entry: LogEntryTrait<Transition>,
    NextTransition: NextTransitionTrait<Transition, Marker>,
    Transition: Resource,
    Marker: Resource
>(
    params: &mut Params, 
    states: &States,
    controller: &Controller,
    commands: CommandsType,
    log: &mut Log<Entry, Transition, Marker>,
    next_transition: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output
    ) -> Option<NextTransition>,
    advance: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output
    ),
    advance_transition: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output,
        &<States as GetState>::Output,
        &Transition
    )
){
    let entry = log.entries.back().unwrap();
    let current_state = states.get_state(entry.state_index());
    advance(params, controller.time_stamp, current_state);
    if let Some(next) = next_transition(params, controller.time_stamp, current_state){
        advance_result(None, next, params, states, controller, commands, log, current_state, advance_transition);
    }
}

#[allow(clippy::too_many_arguments)]
fn advance_result<
    'w, 's,
    Params,
    States: GetState,
    CommandsType: CommandsScope<'w, 's>,
    Entry: LogEntryTrait<Transition>,
    NextTransition: NextTransitionTrait<Transition, Marker>,
    Transition: Resource,
    Marker: Resource
>(
    delay_commands_to: Option<Wrapping<Ticks>>,
    next: NextTransition,
    params: &mut Params,
    states: &States,
    controller: &Controller,
    commands: CommandsType,
    log: &mut Log<Entry, Transition, Marker>,
    current_state: &<States as GetState>::Output,
    advance_transition: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output,
        &<States as GetState>::Output,
        &Transition
    )
){
    let forget = if log.entries.len() < MAX_LOG_LEN.min(log.entries.capacity()){
        log.entry_index += 1;
        None
    } else {
        log.entries.pop_front();
        let age = log.entries.front().unwrap().time_stamp();
        Some(Forget(Some(age - Wrapping(1))))
    };
    let (next_state_index, transition, command) = next.explode();
    let next_state = states.get_state(next_state_index);
    advance_transition(params, controller.time_stamp, current_state, next_state, &transition);
    log.entries.back_mut().unwrap().set_transition(transition);
    log.entries.push_back(Entry::new(controller.time_stamp, next_state_index));
    match (forget, command){
        (None, None) => {},
        (Some(forget), None) => {
            commands.get_command_scope(|mut commands|{
                forget.send(&mut commands);
            });
        },
        (forget, Some(command)) => {
            commands.get_command_scope(|mut commands|{
                if let Some(forget) = forget{
                    forget.send(&mut commands);
                }
                if let Some(target) = delay_commands_to{
                    ReversibleCommands::delayed(commands, command, target);
                } else {
                    command(ReversibleCommands::new(&mut commands));
                }
            })
        }
    }
}

fn transition_default_assert<const FORWARD: bool, Transition: 'static, S>(){
    let fn_name = if FORWARD{
        "advance_transition"
    } else {
        "revert_transition"
    };
    debug_assert!(
        TypeId::of::<Transition>() == TypeId::of::<()>(), 
        "Default impl for `{}` should be replaced if `Transition` is not `()` for trait implementator {}", 
        fn_name, type_name::<S>()
    );
    /* desirable solution if Rust allowed usage of "generic parameters from outer function"
    const ASSERT: () = {
        //above code
    };
    */
}