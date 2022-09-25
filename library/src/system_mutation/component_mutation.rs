use std::{num::Wrapping, alloc::System};
use bevy::{ecs::{system::{SystemParam, Resource}, query::{WorldQuery, QueryItem}}, prelude::{Res, Query, Without, ParallelCommands, EventWriter, Mut}};
use crate::{DespawnedEntity, Ticks, controller::{Controller, Forget}};
use super::{Log, NextTransitionWithState, NextTransition, transition_default_assert, LogEntryWithState, mutate, advance_system, translation_components, LogEntry, SystemId};

pub trait ReversibleComponents: Send + Sync + Sized + 'static{
    type Params: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    type State: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, state: &Self::State) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, state: &Self::State);
    fn revert(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, state: &Self::State);
    fn advance_timestamp(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, item, now, state);
        now + Wrapping(1)
    }
    fn revert_timestamp(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(params, item, now, state);
        now - Wrapping(1)
    }
    fn advance_transition(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, item, now, past_state, future_state, transition);
    }
    fn revert_transition(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, item, now, past_state, future_state, transition);
    }
}

trait ReversibleComponentsMutation<'w, 's>: ReversibleComponents{
    fn next_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, state: &Self::State) -> Option<NextTransitionWithState<Self::Transition, Self>>{
        Self::next_transition(params.0, params.1, now, state)
    }
    fn advance_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, state: &Self::State){
        Self::advance(params.0, params.1, now, state);
    }
    fn revert_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, state: &Self::State){
        Self::revert(params.0, params.1, now, state);
    }
    fn advance_timestamp_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        Self::advance_timestamp(params.0, params.1, now, target, state)
    }
    fn revert_timestamp_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        Self::revert_timestamp(params.0, params.1, now, target, state)
    }
    fn advance_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        Self::advance_transition(params.0, params.1, now, past_state, future_state, transition);
    }
    fn revert_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        Self::revert_transition(params.0, params.1, now, past_state, future_state, transition);
    }
    fn system<const SYSTEM_ID: u8>(
        mut params: (
            Self::Params, 
            Query<
                (Self::Query, &mut Log<LogEntry<Self::Transition>, Self::Transition, Self>), 
                Without<DespawnedEntity>
            >
        ),
        states: Res<Vec<Self::State>>,
        controller: Controller,
        mut forget: EventWriter<Forget>, 
        commands: ParallelCommands
    ){
        let mut inner = |(mut item, log)|{
            let params = &mut (&params.0, &mut item);
            let mut log: Mut<Log<LogEntry<Self::Transition>, Self::Transition, Self>> = log;
            match SYSTEM_ID.try_into(){
                Ok(SystemId::Advance) => advance_system(
                    params, &states, &mut forget, &controller, &commands, &mut *log, 
                    Self::next_transition_tupled, 
                    Self::advance_tupled, 
                    Self::advance_transition_tupled
                ),
                Ok(SystemId::AdvanceTimestamp) => todo!(),
                _ => unreachable!()
            }
        };
        if Self::PAR_ITER_BATCH_SIZE == 0{
            params.1.for_each_mut(|tupled|{
                inner(tupled);
            })
        } else {
            todo!("how to write parallel EventWriter? also: inner should be Copy by not using context");
            /* 
            params.1.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |tupled|{
                inner(tupled);
            })
            */
        }
    }
    fn system_in_log<const ID: u8>(
        params: (
            Self::Params, 
            Query<
                (Self::Query, &mut Log<LogEntry<Self::Transition>, Self::Transition, Self>),
                Without<DespawnedEntity>
            >
        )
    ){
        match ID.try_into(){
            Ok(SystemId::AdvanceLog) => {},
            Ok(SystemId::AdvanceLogTimestamp) => {},
            Ok(SystemId::RevertLog) => {},
            Ok(SystemId::RevertLogTimestamp) => {},
            _ => unreachable!()
        }
    }
    fn system_only_log<const ID: u8>(
        params: Query<&mut Log<LogEntry<Self::Transition>, Self::Transition, Self>>
    ){
        match ID.try_into(){
            Ok(SystemId::LogEnd) => {},
            Ok(SystemId::LogAgeCheck) => {},
            _ => unreachable!()
        }
    }
}

impl<'w, 's, T: ReversibleComponents> ReversibleComponentsMutation<'w, 's> for T {}

pub trait ReversibleComponentsSingleState: Send + Sync + Sized + 'static{
    type Params: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(params: &Self::Params, item: QueryItem<Self::Query>, now: Wrapping<Ticks>) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(params: &Self::Params, item: QueryItem<Self::Query>, now: Wrapping<Ticks>);
    fn revert(params: &Self::Params, item: QueryItem<Self::Query>, now: Wrapping<Ticks>);
    fn advance_timestamp(params: &Self::Params, item: QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, item, now);
        now + Wrapping(1)
    }
    fn revert_timestamp(params: &Self::Params, item: QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(params, item, now);
        now - Wrapping(1)
    }
    fn advance_transition(params: &Self::Params, item: QueryItem<Self::Query>, now: Wrapping<Ticks>, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, item, now, transition);
    }
    fn revert_transition(params: &Self::Params, item: QueryItem<Self::Query>, now: Wrapping<Ticks>, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, item, now, transition);
    }
}

trait ReversibleComponentsMutationSingleState: ReversibleComponentsSingleState{
    
}

impl<T: ReversibleComponentsSingleState> ReversibleComponentsMutationSingleState for T {}