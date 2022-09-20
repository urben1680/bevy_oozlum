use bevy::{ecs::{system::{SystemParam, Resource}, query::{WorldQuery, QueryItem}}, prelude::{Res, Query, Without}};
use crate::DespawnedEntity;
use super::{Log, LogStateless, NextTransition, NextTransitionStateless};

pub trait ReversibleComponents: Send + Sync + Sized + 'static{
    type Resources: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    type State: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>) -> Option<NextTransition<Self::Transition>>;
    fn advance(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>);
    fn revert(resources: &Self::Resources, state: &Self::State, query_item: QueryItem<Self::Query>);
    fn advance_by_transition(resources: &Self::Resources, transition: &Self::Transition, query_item: QueryItem<Self::Query>){
        #[allow(clippy::no_effect)]
        (resources, transition, query_item);
    }
    fn revert_by_transition(resources: &Self::Resources, transition: &Self::Transition, query_item: QueryItem<Self::Query>){
        #[allow(clippy::no_effect)]
        (resources, transition, query_item);
    }
}

trait ReversibleComponentsMutation: ReversibleComponents{
    #[allow(clippy::type_complexity)]
    fn mutate<F: Send + Sync + Clone + Fn(
        &Self::Resources,
        &Vec<Self::State>,
        QueryItem<Self::Query>,
        QueryItem<&mut Log<Self::Transition, Self>>
    )>(
        resources: Self::Resources, 
        states: Res<Vec<Self::State>>,
        mut query: Query<(
            Self::Query, 
            &mut Log<Self::Transition, Self>
        ), Without<DespawnedEntity>>,
        f: F
    ){
        if Self::PAR_ITER_BATCH_SIZE == 0{
            query.for_each_mut(|(items, log)|{
                f(&resources, &states, items, log);
            })
        } else {
            query.par_for_each_mut( Self::PAR_ITER_BATCH_SIZE, |(items, log)|{
                f(&resources, &states, items, log);
            })
        }
    }
}

impl<T: ReversibleComponents> ReversibleComponentsMutation for T {}

pub trait ReversibleComponentsStateless: Send + Sync + Sized + 'static{
    type Resources: SystemParam + Send + Sync;
    type Query: WorldQuery;
    type Transition: Resource;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(resources: &Self::Resources, query_item: QueryItem<Self::Query>) -> Option<NextTransitionStateless<Self::Transition>>;
    fn advance(resources: &Self::Resources, query_item: QueryItem<Self::Query>);
    fn revert(resources: &Self::Resources, query_item: QueryItem<Self::Query>);
    fn advance_by_transition(resources: &Self::Resources, transition: &Self::Transition, query_item: QueryItem<Self::Query>){
        #[allow(clippy::no_effect)]
        (resources, transition, query_item);
    }
    fn revert_by_transition(resources: &Self::Resources, transition: &Self::Transition, query_item: QueryItem<Self::Query>){
        #[allow(clippy::no_effect)]
        (resources, transition, query_item);
    }
}

trait ReversibleComponentsMutationStateless: ReversibleComponentsStateless{
    #[allow(clippy::type_complexity)]
    fn mutate<F: Send + Sync + Clone + Fn(
        &Self::Resources,
        QueryItem<Self::Query>,
        QueryItem<&mut LogStateless<Self::Transition, Self>>
    )>(
        resources: Self::Resources, 
        mut query: Query<(
            Self::Query, 
            &mut LogStateless<Self::Transition, Self>
        ), Without<DespawnedEntity>>,
        f: F
    ){
        if Self::PAR_ITER_BATCH_SIZE == 0{
            query.for_each_mut(|(items, log)|{
                f(&resources, items, log);
            })
        } else {
            query.par_for_each_mut( Self::PAR_ITER_BATCH_SIZE, |(items, log)|{
                f(&resources, items, log);
            })
        }
    }
}

impl<T: ReversibleComponentsStateless> ReversibleComponentsMutationStateless for T {}