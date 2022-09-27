use std::{num::Wrapping, marker::PhantomData};
use bevy::{ecs::{system::{SystemParam, Resource}, query::{WorldQuery, QueryItem}}, prelude::{Res, Query, Without, ParallelCommands}};
use crate::{DespawnedEntity, Ticks, controller::Controller};
use super::{Log as LogType, NextTransitionWithState, transition_default_assert, advance_system, LogEntry, SystemId, SystemContainer};

type Log<T> = LogType<LogEntry<<T as ReversibleComponents>::Transition>, <T as ReversibleComponents>::Transition, T>;
type LogSingleState<T> = LogType<LogEntry<<T as ReversibleComponentsSingleState>::Transition>, <T as ReversibleComponentsSingleState>::Transition, T>;
type QueryItems<T> = (<T as ReversibleComponents>::Query, &'static mut Log<T>);
type QueryItemsSingleState<T> = (<T as ReversibleComponentsSingleState>::Query, &'static mut LogSingleState<T>);

pub trait ReversibleComponents: Send + Sync + Sized + 'static{
    type Params: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    type State: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, state: &Self::State) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, state: &Self::State);
    fn revert(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, state: &Self::State);
    fn advance_up_to(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, item, now, state);
        now + Wrapping(1)
    }
    fn revert_down_to(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
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
    fn advance_up_to_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        Self::advance_up_to(params.0, params.1, now, target, state)
    }
    fn revert_down_to_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        Self::revert_down_to(params.0, params.1, now, target, state)
    }
    fn advance_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        Self::advance_transition(params.0, params.1, now, past_state, future_state, transition);
    }
    fn revert_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        Self::revert_transition(params.0, params.1, now, past_state, future_state, transition);
    }
    #[allow(clippy::type_complexity)]
    fn system_tupled<const SYSTEM_ID: u8>(
        params: (
            (Self::Params, Query<'w, 's, QueryItems<Self>, Without<DespawnedEntity>>),
            Res<'w, Vec<Self::State>>
        ),
        controller: Res<Controller>,
        commands: &'w ParallelCommands
    ){
        Self::system::<SYSTEM_ID>(params.0, params.1, controller, commands);
    }
    #[allow(clippy::type_complexity)]
    fn system_in_log_tupled<const SYSTEM_ID: u8>(
        params: (
            (Self::Params, Query<'w, 's, QueryItems<Self>, Without<DespawnedEntity>>),
            Res<'w, Vec<Self::State>>
        ),
        controller: Res<'w, Controller>
    ){
        Self::system_in_log::<SYSTEM_ID>(params.0, params.1, controller);
    }
    fn system_only_log_tupled<const SYSTEM_ID: u8>(
        params: (
            Query<'w, 's, &mut Log<Self>>,
            Res<'w, Vec<Self::State>>
        ),
        controller: Res<'w, Controller>
    ){
        Self::system_only_log::<SYSTEM_ID>(params.0, params.1, controller);
    }
    fn system<const SYSTEM_ID: u8>(
        mut params: (Self::Params, Query<QueryItems<Self>, Without<DespawnedEntity>>),
        states: Res<Vec<Self::State>>,
        controller: Res<Controller>,
        commands: &ParallelCommands
    ){
        let closure = |(mut item, mut log): QueryItem<QueryItems<Self>>|{
            let tupled = &mut (&params.0, &mut item);
            match SYSTEM_ID.try_into(){
                Ok(SystemId::Advance) => advance_system(
                    tupled, &states, &controller, commands, &mut *log, 
                    Self::next_transition_tupled, 
                    Self::advance_tupled, 
                    Self::advance_transition_tupled
                ),
                Ok(SystemId::AdvanceTimestamp) => todo!(),
                _ => unreachable!()
            }
        };
        
        if Self::PAR_ITER_BATCH_SIZE == 0{
            params.1.for_each_mut(closure)
        } else {
            params.1.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, closure)
        }
    }
    fn system_in_log<const ID: u8>(
        mut params: (Self::Params, Query<'w, 's, QueryItems<Self>, Without<DespawnedEntity>>),
        states: Res<'w, Vec<Self::State>>,
        controller: Res<'w, Controller>,
    ){
        let closure = |(mut item, mut log): QueryItem<QueryItems<Self>>|{
            let tupled = &mut (&params.0, &mut item);
            match ID.try_into(){
                Ok(SystemId::AdvanceLog) => todo!(),
                Ok(SystemId::AdvanceLogTimestamp) => todo!(),
                Ok(SystemId::RevertLog) => todo!(),
                Ok(SystemId::RevertLogTimestamp) => todo!(),
                _ => unreachable!()
            }
        };
        
        if Self::PAR_ITER_BATCH_SIZE == 0{
            params.1.for_each_mut(closure)
        } else {
            params.1.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, closure)
        }
    }
    fn system_only_log<const ID: u8>(
        mut params: Query<'w, 's, &mut Log<Self>>,
        states: Res<'w, Vec<Self::State>>,
        controller: Res<'w, Controller>,
    ){
        let closure = |mut log: QueryItem<&mut Log<Self>>|{
            match ID.try_into(){
                Ok(SystemId::LogEnd) => todo!(),
                Ok(SystemId::LogAgeCheck) => todo!(),
                _ => unreachable!()
            }
        };
        
        if Self::PAR_ITER_BATCH_SIZE == 0{
            params.for_each_mut(closure)
        } else {
            params.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, closure)
        }
    }
    #[allow(clippy::type_complexity)]
    fn reversible_systems() -> SystemContainer<'w, 's, 
        (
            (Self::Params, Query<'w, 's, QueryItems<Self>, Without<DespawnedEntity>>),
            Res<'w, Vec<Self::State>>
        ),
        (
            Query<'w, 's, &'static mut Log<Self>>,
            Res<'w, Vec<Self::State>>
        ),
        &'w ParallelCommands<'w, 's>
    >{
        SystemContainer { 
            advance: Self::system_tupled::<{SystemId::ADVANCE}>, 
            advance_timestamp: Self::system_tupled::<{SystemId::ADVANCE_TIMESTAMP}>, 
            advance_log: Self::system_in_log_tupled::<{SystemId::ADVANCE_LOG}>, 
            advance_log_timestamp: Self::system_in_log_tupled::<{SystemId::ADVANCE_LOG_TIMESTAMP}>, 
            revert_log: Self::system_in_log_tupled::<{SystemId::REVERT_LOG}>, 
            revert_log_timestamp: Self::system_in_log_tupled::<{SystemId::REVERT_LOG_TIMESTAMP}>, 
            log_end: Self::system_only_log_tupled::<{SystemId::LOG_END}>,  
            log_age_check: Self::system_only_log_tupled::<{SystemId::LOG_AGE_CHECK}>, 
            p: PhantomData
        }
    }
}

impl<'w, 's, T: ReversibleComponents> ReversibleComponentsMutation<'w, 's> for T {}

pub trait ReversibleComponentsSingleState: Send + Sync + Sized + 'static{
    type Params: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>);
    fn revert(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>);
    fn advance_up_to(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, item, now);
        now + Wrapping(1)
    }
    fn revert_down_to(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(params, item, now);
        now - Wrapping(1)
    }
    fn advance_transition(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, item, now, transition);
    }
    fn revert_transition(params: &Self::Params, item: &mut QueryItem<Self::Query>, now: Wrapping<Ticks>, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, item, now, transition);
    }
}

trait ReversibleComponentsMutationSingleState: ReversibleComponentsSingleState{
    fn next_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>) -> Option<NextTransitionWithState<Self::Transition, Self>>{
        Self::next_transition(params.0, params.1, now)
    }
    fn advance_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>){
        Self::advance(params.0, params.1, now);
    }
    fn revert_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>){
        Self::revert(params.0, params.1, now);
    }
    fn advance_up_to_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        Self::advance_up_to(params.0, params.1, now, target)
    }
    fn revert_down_to_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        Self::revert_down_to(params.0, params.1, now, target)
    }
    fn advance_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, transition: &Self::Transition){
        Self::advance_transition(params.0, params.1, now, transition);
    }
    fn revert_transition_tupled(params: &mut (&Self::Params, &mut QueryItem<Self::Query>), now: Wrapping<Ticks>, transition: &Self::Transition){
        Self::revert_transition(params.0, params.1, now, transition);
    }
}

impl<T: ReversibleComponentsSingleState> ReversibleComponentsMutationSingleState for T {}