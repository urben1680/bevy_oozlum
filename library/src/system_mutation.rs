use std::{marker::PhantomData, collections::VecDeque, ops::Index, any::{TypeId, type_name}, num::Wrapping, mem::MaybeUninit};

use bevy::{ecs::{system::{SystemParam, Resource, StaticSystemParam, SystemParamFetch, SystemParamItem}, query::{WorldQuery, QueryItem, Fetch}, schedule::IntoSystemDescriptor}, prelude::{Query, Component, Without, Res, ResMut, System, App, Commands, EventWriter, ParallelCommands}};

use crate::{DespawnedEntity, Ticks, commands::{ReversibleCommands, NextCommands}, MAX_LOG_LEN, controller::{Controller, Forget}};


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


pub struct NextTransitionWithState<Transition, Marker>{
    next_state_index: usize,
    transition: Transition,
    commands: NextCommands<Marker>
}

impl<Transition, Marker> NextTransitionWithState<Transition, Marker>{
    pub fn new(next_state_index: usize, transition: Transition) -> Self{
        Self { next_state_index, transition, commands: None }
    }
    pub fn new_with_commands<F: 'static + FnOnce(ReversibleCommands<Marker>)>(next_state_index: usize, transition: Transition, commands: F) -> Self{
        Self { next_state_index, transition, commands: Some(Box::new(commands)) }
    }
}

pub struct NextTransition<Transition, Marker>{
    transition: Transition,
    commands: NextCommands<Marker>
}

impl<Transition, Marker> NextTransition<Transition, Marker>{
    pub fn new(transition: Transition) -> Self{
        Self { transition, commands: None }
    }
    pub fn new_with_commands<F: 'static + FnOnce(ReversibleCommands<Marker>)>(transition: Transition, commands: F) -> Self{
        Self { transition, commands: Some(Box::new(commands)) }
    }
}

trait NextTransitionTrait<Transition, Marker>{
    fn to_inner(self) -> (usize, Transition, NextCommands<Marker>);
}

impl<Transition, Marker> NextTransitionTrait<Transition, Marker> for NextTransition<Transition, Marker>{
    fn to_inner(self) -> (usize, Transition, NextCommands<Marker>){
        (Default::default(), self.transition, self.commands)
    }
}

impl<Transition, Marker> NextTransitionTrait<Transition, Marker> for NextTransitionWithState<Transition, Marker>{
    fn to_inner(self) -> (usize, Transition, NextCommands<Marker>){
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

pub struct SystemContainer<ParamsFull: SystemParam, ParamsLog: SystemParam>{
    pub advance: fn(ParamsFull, ParallelCommands),
    pub advance_timestamp: fn(ParamsFull, ParallelCommands),
    pub advance_log: fn(ParamsFull),
    pub advance_log_timestamp: fn(ParamsFull),
    pub revert_log: fn(ParamsFull),
    pub revert_log_timestamp: fn(ParamsFull),
    pub log_end: fn(ParamsLog),
    pub log_age_check: fn(ParamsLog) //check if timestamp of oldest entry is equal to current time stamp to drop just when time stamp was raised in controller

    /*
    ////// COMPONENTS

    fn mutate<F: Send + Sync + Clone + Fn(
        &Self::Resources,
        &Res<Vec<Self::State>>,
        QueryItem<Self::Query>,
        &mut LogWithStates<Self::Transition, Self>
    )>(
        resources: Self::Resources, 
        states: Res<Vec<Self::State>>,
        mut query: Query<(
            Self::Query, 
            &mut LogWithStates<Self::Transition, Self>
        ), Without<DespawnedEntity>>,
        f: F
    )

    fn mutate<F: Send + Sync + Clone + Fn(
        &Self::Resources,
        QueryItem<Self::Query>,
        &mut Log<Self::Transition, Self>
    )>(
        resources: Self::Resources, 
        mut query: Query<(
            Self::Query, 
            &mut Log<Self::Transition, Self>
        ), Without<DespawnedEntity>>,
        f: F
    )

    ////// RESOURCES

    fn mutate<F: for<'a> Fn(
        Self::Resources,
        &Res<Vec<Self::State>>,
        &mut LogWithStates<Self::Transition, Self>
    )>(
        resources: Self::Resources, 
        states: Res<Vec<Self::State>>,
        mut log: ResMut<LogWithStates<Self::Transition, Self>>,
        f: F
    )

    fn mutate<F: for<'a> Fn(
        Self::Resources,
        &mut Log<Self::Transition, Self>
    )>(
        resources: Self::Resources, 
        mut log: ResMut<Log<Self::Transition, Self>>,
        f: F
    )

    advance_system:
    - advance
    - next
    - (advance_transition)

    advance_timestamp_system:
    - advance_timestamp
    - next
    - (advance_transition)

    advance_log_system:
    - advance
    - (advance_translation)

    revert_log_system:
    - (revert_transition)
    - revert

    revert_log_timestamp_system:
    - (revert_transition)
    - revert_timestamp

    log_end:
    - log_end

    Idee:

    Mutate traits erzeugen die funktionen als fn und greifen dabei auf die allgemeinen funktionen zu und auf die user definierten funktionen
    diese werden gebündelt als struct zurückgegeben in fn(SystemParams) format


    Unterschied advance_timestamp und advance_log_timestamp?
    - keiner, controller.target_time_stamp() nutzen
    - doch, einer ruft next auf und schreibt commands, der andere nicht
    */
}

#[allow(clippy::type_complexity)]
fn mutate<
    UserParamsIn: SystemParam + Send + Sync,
    UserParamsOut,
    States: SystemParam + Send + Sync,
    Log: Component
>(
    params: UserParamsIn,
    states: States,
    controller: Res<Controller>,
    batch_size: usize,
    translation: fn(
        UserParamsIn,
        States,
        Res<Controller>,
        usize,
        fn(
            UserParamsOut,
            &States,
            &Controller,
            &mut Log
        )
    ),
    inner: fn(
        UserParamsOut,
        &States,
        &Controller,
        &mut Log
    )
){
    translation(params, states, controller, batch_size, inner);
    todo!("make this more general to serve all mutations (like now) and only log");
}

fn translation_components<
    Resources: SystemParam + Send + Sync,
    UserQuery: WorldQuery,
    States: SystemParam + Send + Sync,
    Log: Component
>(
    mut params: (
        Resources, 
        Query<(UserQuery, &mut Log), Without<DespawnedEntity>>
    ),
    states: States,
    controller: Res<Controller>,
    batch_size: usize,
    inner: fn(
        (&Resources, QueryItem<UserQuery>),
        &States,
        &Controller,
        &mut Log
    )
){
    if batch_size == 0{
        params.1.for_each_mut(|(items, mut log)|{
            inner((&params.0, items), &states, &controller, &mut log);
        })
    } else {
        params.1.par_for_each_mut( batch_size, |(items, mut log)|{
            inner((&params.0, items), &states, &controller, &mut log);
        })
    }
}

fn translation_resources<
    Resources: SystemParam + Send + Sync,
    States: SystemParam + Send + Sync,
    Log: Component
>(
    mut params: (
        Resources,
        ResMut<Log>
    ),
    states: States,
    controller: Res<Controller>,
    _: usize,
    inner: fn(
        Resources,
        &States,
        &Controller,
        &mut Log
    )
){
    inner(params.0, &states, &controller, &mut params.1)
}

#[allow(clippy::too_many_arguments)]
fn advance_system<
    Params,
    States: GetState,
    Entry: LogEntryTrait<Transition>,
    NextTransition: NextTransitionTrait<Transition, Marker>,
    Transition: Resource,
    Marker: Resource
>(
    params: &mut Params, 
    states: &States,
    controller: &Controller,
    commands: &ParallelCommands,
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
        advance_result(next, params, states, controller, commands, log, current_state, advance_transition);
    }
}

#[allow(clippy::too_many_arguments)]
fn advance_result<
    Params,
    States: GetState,
    Entry: LogEntryTrait<Transition>,
    NextTransition: NextTransitionTrait<Transition, Marker>,
    Transition: Resource,
    Marker: Resource
>(
    next: NextTransition,
    params: &mut Params,
    states: &States,
    controller: &Controller,
    mut commands: &ParallelCommands,
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
    let (next_state_index, transition, command) = next.to_inner();
    if let Some(command) = command{
        command(ReversibleCommands::new(commands));
    }
    let next_state = states.get_state(next_state_index);
    advance_transition(params, controller.time_stamp, current_state, next_state, &transition);
    log.entries.back_mut().unwrap().set_transition(transition);
    if log.entries.len() == MAX_LOG_LEN {
        log.entries.pop_front();
        let age = log.entries.front().unwrap().time_stamp();
        let forget = Forget(Some(age - Wrapping(1)));
        forget.send(commands);
    } else {
        log.entry_index += 1;
    }
    log.entries.push_back(Entry::new(controller.time_stamp, next_state_index));
}

fn transition_default_assert<const FORWARD: bool, Transition: 'static, S>(){
    let fn_name = if FORWARD{
        "advance_by_transition"
    } else {
        "revert_by_transition"
    };
    debug_assert!(
        TypeId::of::<Transition>() == TypeId::of::<()>(), 
        "Default impl for `{}` should be replaced if `Transition` is not `()` for trait implementator {}", 
        fn_name, type_name::<S>()
    );
    /* desirable solution if Rust allowed usage of "generic parameters from outer function"
    const ASSERT: () = {
        let fn_name = if FORWARD{
            "advance_by_transition"
        } else {
            "revert_by_transition"
        };
        assert!(
            TypeId::of::<Transition>() == TypeId::of::<()>(), 
            "Default impl for `{}` should be replaced if `Transition` is not `()` for trait implementator {}", 
            fn_name, type_name::<S>()
        )
    };
    */
}