use std::num::Wrapping;
use bevy::{ecs::{system::{SystemParam, Resource}, query::{WorldQuery, QueryItem}}, prelude::{Res, Query, Without, ParallelCommands, EventWriter}};
use crate::{DespawnedEntity, Ticks, controller::{Controller, Forget}};
use super::{Log, NextTransitionWithState, NextTransition, transition_default_assert, LogEntryWithState, mutate, advance_system, translation_components};

pub trait ReversibleComponents: Send + Sync + Sized + 'static{
    type Params: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    type State: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(params: &Self::Params, now: Wrapping<Ticks>, state: &Self::State, item: QueryItem<Self::Query>) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(params: &Self::Params, now: Wrapping<Ticks>, state: &Self::State, item: QueryItem<Self::Query>);
    fn revert(params: &Self::Params, now: Wrapping<Ticks>, state: &Self::State, item: QueryItem<Self::Query>);
    fn advance_timestamp(params: &Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State, item: QueryItem<Self::Query>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, now, state, item);
        now + Wrapping(1)
    }
    fn revert_timestamp(params: &Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State, item: QueryItem<Self::Query>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(params, now, state, item);
        now - Wrapping(1)
    }
    fn advance_by_transition(params: &Self::Params, now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition, item: QueryItem<Self::Query>){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, past_state, future_state, transition, item);
    }
    fn revert_by_transition(params: &Self::Params, now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition, item: QueryItem<Self::Query>){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, past_state, future_state, transition, item);
    }
}

type In<'w, 's, T> = (
    <T as ReversibleComponents>::Params, 
    Res<'w, Vec<<T as ReversibleComponents>::State>>, 
    Res<'w, Controller>, 
    Query<'w, 's, (
        <T as ReversibleComponents>::Query, 
        &'static mut Log<LogEntryWithState<<T as ReversibleComponents>::Transition>, <T as ReversibleComponents>::Transition, T>
    ), Without<DespawnedEntity>>,
    EventWriter<'w, 's, Forget>
);

type Out<'w, 's, T> = (
    &'w <T as ReversibleComponents>::Params,
    QueryItem<'w, <T as ReversibleComponents>::Query>
);

type Log1<T> = Log<LogEntryWithState<<T as ReversibleComponents>::Transition>, <T as ReversibleComponents>::Transition, T>;

trait ReversibleComponentsMutation<'w, 's>: ReversibleComponents{
    fn advance_system(params: In<Self>, commands: ParallelCommands){
        let inner = |out, states, controller, log|{
            Self::advance_system_inner(out, states, params.4, controller, commands, log);
        };
        mutate(params.0, params.1, params.2, Self::PAR_ITER_BATCH_SIZE, translation_components, inner);
    }
    fn advance_system_inner(params: Out<Self>, states: &Res<Vec<Self::State>>, forget: EventWriter<'w, 's, Forget>, controller: &Controller, commands: ParallelCommands, log: &mut Log1<Self>){
        advance_system(&mut params, states, forget, controller, commands, log, Self::next_transition, Self::advance, Self::advance_by_transition)
    }
}

impl<'w, 's, T: ReversibleComponents> ReversibleComponentsMutation<'w, 's> for T {}

pub trait ReversibleComponentsSingleState: Send + Sync + Sized + 'static{
    type Params: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(params: &Self::Params, now: Wrapping<Ticks>, item: QueryItem<Self::Query>) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(params: &Self::Params, now: Wrapping<Ticks>, item: QueryItem<Self::Query>);
    fn revert(params: &Self::Params, now: Wrapping<Ticks>, item: QueryItem<Self::Query>);
    fn advance_timestamp(params: &Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>, item: QueryItem<Self::Query>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, now, item);
        now + Wrapping(1)
    }
    fn revert_timestamp(params: &Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>, item: QueryItem<Self::Query>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(params, now, item);
        now - Wrapping(1)
    }
    fn advance_by_transition(params: &Self::Params, now: Wrapping<Ticks>, transition: &Self::Transition, item: QueryItem<Self::Query>){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, transition, item);
    }
    fn revert_by_transition(params: &Self::Params, now: Wrapping<Ticks>, transition: &Self::Transition, item: QueryItem<Self::Query>){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, transition, item);
    }
}

trait ReversibleComponentsMutationSingleState: ReversibleComponentsSingleState{
    
}

impl<T: ReversibleComponentsSingleState> ReversibleComponentsMutationSingleState for T {}