use std::num::Wrapping;
use bevy::{ecs::{system::{SystemParam, Resource}, query::{WorldQuery, QueryItem}}, prelude::{Res, Query, Without}};
use crate::{DespawnedEntity, Ticks};
use super::{LogWithStates, Log, NextTransitionWithState, NextTransition, transition_default_assert};

pub trait ReversibleComponents: Send + Sync + Sized + 'static{
    type Resources: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    type State: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>);
    fn revert(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>);
    fn advance_timestamp(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(resources, state, query_item);
        current_timestamp + Wrapping(1)
    }
    fn revert_timestamp(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(resources, state, query_item);
        current_timestamp - Wrapping(1)
    }
    fn advance_by_transition(resources: &Self::Resources, query_item: QueryItem<Self::Query>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, query_item, past_state, future_state, transition);
    }
    fn revert_by_transition(resources: &Self::Resources, query_item: QueryItem<Self::Query>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, query_item, past_state, future_state, transition);
    }
}

trait ReversibleComponentsMutation: ReversibleComponents{
    #[allow(clippy::type_complexity)]
    fn mutate(
        mut input: (
            Self::Resources,
            Res<Vec<Self::State>>,
            Query<(
                Self::Query, 
                &mut LogWithStates<Self::Transition, Self>
            ), Without<DespawnedEntity>>,
        ),
        f: fn((
            &Self::Resources,
            &Res<Vec<Self::State>>,
            QueryItem<Self::Query>,
            &mut LogWithStates<Self::Transition, Self>
        ))
    ){
        if Self::PAR_ITER_BATCH_SIZE == 0{
            input.2.for_each_mut(|(items, mut log)|{
                f((&input.0, &input.1, items, &mut log));
            })
        } else {
            input.2.par_for_each_mut( Self::PAR_ITER_BATCH_SIZE, |(items, mut log)|{
                f((&input.0, &input.1, items, &mut log));
            })
        }
    }
}

impl<T: ReversibleComponents> ReversibleComponentsMutation for T {}

pub trait ReversibleComponentsSingleState: Send + Sync + Sized + 'static{
    type Resources: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(resources: &Self::Resources, query_item: QueryItem<Self::Query>) -> Option<NextTransition<Self::Transition, Self>>;
    fn advance(resources: &Self::Resources, query_item: QueryItem<Self::Query>);
    fn revert(resources: &Self::Resources, query_item: QueryItem<Self::Query>);
    fn advance_timestamp(resources: &Self::Resources, query_item: QueryItem<Self::Query>, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(resources, query_item);
        current_timestamp + Wrapping(1)
    }
    fn revert_timestamp(resources: &Self::Resources, query_item: QueryItem<Self::Query>, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(resources, query_item);
        current_timestamp - Wrapping(1)
    }
    fn advance_by_transition(resources: &Self::Resources, query_item: QueryItem<Self::Query>, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, transition, query_item);
    }
    fn revert_by_transition(resources: &Self::Resources, query_item: QueryItem<Self::Query>, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, transition, query_item);
    }
}

trait ReversibleComponentsMutationSingleState: ReversibleComponentsSingleState{
    #[allow(clippy::type_complexity)]
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
    ){
        if Self::PAR_ITER_BATCH_SIZE == 0{
            query.for_each_mut(|(items, mut log)|{
                f(&resources, items, &mut log);
            })
        } else {
            query.par_for_each_mut( Self::PAR_ITER_BATCH_SIZE, |(items, mut log)|{
                f(&resources, items, &mut log);
            })
        }
    }
}

impl<T: ReversibleComponentsSingleState> ReversibleComponentsMutationSingleState for T {}