use bevy::{ecs::system::{SystemParam, Resource}, prelude::{Res, ResMut}};

use super::{Log, LogStateless, NextTransition, NextTransitionStateless};

pub trait ReversibleResource: Send + Sync + Sized + 'static{
    type Resources: SystemParam;
    type Transition: Resource;
    type State: Resource;
    fn next_transition(resources: &Self::Resources, state: &Self::State) -> Option<NextTransition<Self::Transition>>;
    fn advance(resources: Self::Resources, state: &Self::State);
    fn revert(resources: Self::Resources, state: &Self::State);
    fn advance_by_transition(resources: Self::Resources, transition: &Self::Transition){
        #[allow(clippy::no_effect)]
        (resources, transition);
    }
    fn revert_by_transition(resources: Self::Resources, transition: &Self::Transition){
        #[allow(clippy::no_effect)]
        (resources, transition);
    }
}

trait ReversibleResourceMutation: ReversibleResource{
    fn mutate<F: for<'a> Fn(
        Self::Resources,
        &Vec<Self::State>,
        &mut Log<Self::Transition, Self>
    )>(
        resources: Self::Resources, 
        states: Res<Vec<Self::State>>,
        mut log: ResMut<Log<Self::Transition, Self>>,
        f: F
    ){
        f(resources, &*states, &mut *log)
    }
}

impl<T: ReversibleResource> ReversibleResourceMutation for T {}

pub trait ReversibleResourceStateless: Send + Sync + Sized + 'static{
    type Resources: SystemParam;
    type Transition: Resource;
    fn next_transition(resources: &Self::Resources) -> Option<NextTransitionStateless<Self::Transition>>;
    fn advance(resources: Self::Resources);
    fn revert(resources: Self::Resources);
    fn advance_by_transition(resources: Self::Resources, transition: &Self::Transition){
        #[allow(clippy::no_effect)]
        (resources, transition);
    }
    fn revert_by_transition(resources: Self::Resources, transition: &Self::Transition){
        #[allow(clippy::no_effect)]
        (resources, transition);
    }
}

trait ReversibleResourceMutationStateless: ReversibleResourceStateless{
    fn mutate<F: for<'a> Fn(
        Self::Resources,
        ResMut<LogStateless<Self::Transition, Self>>
    )>(
        resources: Self::Resources, 
        log: ResMut<LogStateless<Self::Transition, Self>>,
        f: F
    ){
        f(resources, log)
    }
}

impl<T: ReversibleResourceStateless> ReversibleResourceMutationStateless for T {}