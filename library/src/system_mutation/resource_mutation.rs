use std::num::Wrapping;
use bevy::{ecs::system::{SystemParam, Resource}, prelude::{Res, ResMut}};
use crate::Ticks;
use super::{LogWithStates, Log, NextTransitionWithState, NextTransition, transition_default_assert};

pub trait ReversibleResource: Send + Sync + Sized + 'static{
    type Resources: SystemParam;
    type Transition: Resource;
    type State: Resource;
    fn next_transition(resources: &Self::Resources, state: &Self::State) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(resources: Self::Resources, state: &Self::State);
    fn revert(resources: Self::Resources, state: &Self::State);
    fn advance_timestamp(resources: Self::Resources, state: &Self::State, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(resources, state);
        current_timestamp + Wrapping(1)
    }
    fn revert_timestamp(resources: Self::Resources, state: &Self::State, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(resources, state);
        current_timestamp - Wrapping(1)
    }
    fn advance_by_transition(resources: Self::Resources, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, past_state, future_state, transition);
    }
    fn revert_by_transition(resources: Self::Resources, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, past_state, future_state, transition);
    }
}

trait ReversibleResourceMutation: ReversibleResource{
    fn mutate<F: for<'a> Fn(
        Self::Resources,
        &Res<Vec<Self::State>>,
        &mut LogWithStates<Self::Transition, Self>
    )>(
        resources: Self::Resources, 
        states: Res<Vec<Self::State>>,
        mut log: ResMut<LogWithStates<Self::Transition, Self>>,
        f: F
    ){
        f(resources, &states, &mut *log)
    }
}

impl<T: ReversibleResource> ReversibleResourceMutation for T {}

pub trait ReversibleResourceStateless: Send + Sync + Sized + 'static{
    type Resources: SystemParam;
    type Transition: Resource;
    fn next_transition(resources: &Self::Resources) -> Option<NextTransition<Self::Transition, Self>>;
    fn advance(resources: Self::Resources);
    fn revert(resources: Self::Resources);
    fn advance_timestamp(resources: Self::Resources, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(resources);
        current_timestamp + Wrapping(1)
    }
    fn revert_timestamp(resources: Self::Resources, current_timestamp: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(resources);
        current_timestamp - Wrapping(1)
    }
    fn advance_by_transition(resources: Self::Resources, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, transition);
    }
    fn revert_by_transition(resources: Self::Resources, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (resources, transition);
    }
}

trait ReversibleResourceMutationStateless: ReversibleResourceStateless{
    fn mutate<F: for<'a> Fn(
        Self::Resources,
        &mut Log<Self::Transition, Self>
    )>(
        resources: Self::Resources, 
        mut log: ResMut<Log<Self::Transition, Self>>,
        f: F
    ){
        f(resources, &mut *log)
    }
}

impl<T: ReversibleResourceStateless> ReversibleResourceMutationStateless for T {}