use std::{marker::PhantomData, collections::VecDeque, ops::Index, any::{TypeId, type_name}, num::Wrapping};

use bevy::{ecs::{system::{SystemParam, Resource, StaticSystemParam, SystemParamFetch, SystemParamItem}, query::{WorldQuery, QueryItem, Fetch}, schedule::IntoSystemDescriptor}, prelude::{Query, Component, Without, Res, ResMut, System, App}};

use crate::{DespawnedEntity, Ticks, commands::ReversibleCommands};


mod resource_mutation;
mod component_mutation;

pub use resource_mutation::*;
pub use component_mutation::*;

pub struct NextTransitionWithState<T, M>{
    next_state_index: usize,
    transition: T,
    commands: Option<Box<dyn FnOnce(&mut ReversibleCommands<M>)>>
}

impl<T, M> NextTransitionWithState<T, M>{
    pub fn new(next_state_index: usize, transition: T) -> Self{
        Self { next_state_index, transition, commands: None }
    }
    pub fn new_with_commands<F: 'static + FnOnce(&mut ReversibleCommands<M>)>(next_state_index: usize, transition: T, commands: F) -> Self{
        Self { next_state_index, transition, commands: Some(Box::new(commands)) }
    }
}

pub struct NextTransition<T, M>{
    transition: T,
    commands: Option<Box<dyn FnOnce(&mut ReversibleCommands<M>)>>
}

impl<T, M> NextTransition<T, M>{
    pub fn new(transition: T) -> Self{
        Self { transition, commands: None }
    }
    pub fn new_with_commands<F: 'static + FnOnce(&mut ReversibleCommands<M>)>(transition: T, commands: F) -> Self{
        Self { transition, commands: Some(Box::new(commands)) }
    }
}

#[derive(Component)]
pub struct LogWithStates<T: Resource, M: Resource>{
    entry_index: usize,
    entries: VecDeque<LogEntryWithState<T>>,
    marker: PhantomData<M>
}

#[derive(Component)]
pub struct Log<T: Resource, M: Resource>{
    entry_index: usize,
    entries: VecDeque<LogEntry<T>>,
    marker: PhantomData<M>
}

struct LogEntryWithState<T: Resource>{
    state_index: usize,
    transition: T,
    time_stamp: Wrapping<Ticks>
}

struct LogEntry<T: Resource>{
    transition: T,
    time_stamp: Wrapping<Ticks>
}

trait LogMutation{
    //systems go here, entry functions are generic with both component and resource variants

    /*
    mutations:

    - resources (as reference or as value/&mut(?))
    - state collection
    - state
    - log / next transition
    -- transition
    -- marker
    - query
    - queryitems

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
    
    */
    
    fn mutate<S: GetState, U, F: Send + Sync + Clone + Fn(
        &S, U, &mut Self
    )>(f: F){}
}

impl<T: Resource, M: Resource> LogMutation for LogWithStates<T, M> {}
impl<T: Resource, M: Resource> LogMutation for Log<T, M> {}

trait GetState: SystemParam{
    type Output;
    type Idx;
    fn get_state(&self, index: Self::Idx) -> &Self::Output;
}

impl<'w, T: Resource> GetState for Res<'w, Vec<T>>{
    type Output = T;
    type Idx = usize;
    fn get_state(&self, index: Self::Idx) -> &Self::Output{
        self.get(index).unwrap()
    }
}

impl GetState for (){
    type Output = ();
    type Idx = ();
    fn get_state(&self, _index: Self::Idx) -> &Self::Output{
        &()
    }
}


trait Mutation{
    type In: SystemParam;
    type Out;
}

/*
jedes trait hat eine load_in_app methode die die übergabe von feinheiten in allgemeine muster übernimmt
jene funktion gibt ein container object zurück anstatt app zu verändern damit die app weiter im builder pattern gebaut werden kann
die felder des containers sind alle funktion pointer
benötigt GAT für die jeweiligen typen
*/

pub struct SystemContainer<ParamsFull: SystemParam, ParamsLog: SystemParam>{
    pub advance: fn(ParamsFull),
    pub advance_timestamp: fn(ParamsFull), //both log and not-log
    pub advance_log: fn(ParamsFull),
    pub revert_log: fn(ParamsFull),
    pub revert_timestamp: fn(ParamsFull),
    pub log_end: fn(ParamsLog)

    /*
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
    */
}




pub struct IntoApp<I: SystemParam + 'static, O: 'static>{
    pub(super) translation: fn(SystemParamItem<I>, fn(O)),
    pub(super) system: fn(O),
    pub(super) system_1: fn(StaticSystemParam<I>)
}
impl<I: SystemParam + 'static, O: 'static> IntoApp<I, O>{
    fn add_system(&self, app: &mut App){
        app.add_system(self.system_1);
    }
    fn system(&self) -> impl Fn(StaticSystemParam<I>){
        let translation = self.translation;
        let system = self.system;
        move |params: StaticSystemParam<I>|{
            (translation)(params.into_inner(), system);
        }
    }
    fn proxy(app: &mut App, f: fn(StaticSystemParam<I>)){}
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
        debug_assert!(
            TypeId::of::<Transition>() == TypeId::of::<()>(), 
            "Default impl for `{}` should be replaced if `Transition` is not `()` for trait implementator {}", 
            fn_name, type_name::<S>()
        )
    };
    */
}