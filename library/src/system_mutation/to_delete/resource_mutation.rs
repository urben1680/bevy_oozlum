use std::num::Wrapping;
use bevy::{ecs::system::{SystemParam, Resource}, prelude::{Res, ResMut}};
use crate::Ticks;

use super::{next_transition::{NextTransitionWithState, NextTransition}, transition_default_assert};

pub trait ReversibleResource: Send + Sync + Sized + 'static{
    type Params: SystemParam;
    type Transition: Resource;
    type State: Resource;
    fn next_transition(params: Self::Params, now: Wrapping<Ticks>, state: &Self::State) -> Option<NextTransitionWithState<Self::Transition, Self>>;
    fn advance(params: Self::Params, now: Wrapping<Ticks>, state: &Self::State);
    fn revert(params: Self::Params, now: Wrapping<Ticks>, state: &Self::State);
    fn advance_up_to(params: Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, now, state);
        now + Wrapping(1)
    }
    fn revert_down_to(params: Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>, state: &Self::State) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(params, now, state);
        now - Wrapping(1)
    }
    fn advance_transition(params: Self::Params, now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, past_state, future_state, transition);
    }
    fn revert_transition(params: Self::Params, now: Wrapping<Ticks>, past_state: &Self::State, future_state: &Self::State, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, past_state, future_state, transition);
    }
}

trait ReversibleResourceMutation: ReversibleResource{
    
}

impl<T: ReversibleResource> ReversibleResourceMutation for T {}

pub trait ReversibleResourceStateless: Send + Sync + Sized + 'static{
    type Params: SystemParam;
    type Transition: Resource;
    fn next_transition(params: Self::Params, now: Wrapping<Ticks>) -> Option<NextTransition<Self::Transition, Self>>;
    fn advance(params: Self::Params, now: Wrapping<Ticks>);
    fn revert(params: Self::Params, now: Wrapping<Ticks>);
    fn advance_up_to(params: Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::advance(params, now);
        now + Wrapping(1)
    }
    fn revert_down_to(params: Self::Params, now: Wrapping<Ticks>, target: Wrapping<Ticks>) -> Wrapping<Ticks>{
        #[allow(clippy::no_effect)]
        {target};
        Self::revert(params, now);
        now - Wrapping(1)
    }
    fn advance_transition(params: Self::Params, now: Wrapping<Ticks>, transition: &Self::Transition){
        transition_default_assert::<true, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, transition);
    }
    fn revert_transition(params: Self::Params, now: Wrapping<Ticks>, transition: &Self::Transition){
        transition_default_assert::<false, Self::Transition, Self>();
        #[allow(clippy::no_effect)]
        (params, now, transition);
    }
}

trait ReversibleResourceMutationStateless: ReversibleResourceStateless{
    
}

impl<T: ReversibleResourceStateless> ReversibleResourceMutationStateless for T {}