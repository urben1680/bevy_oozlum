use std::mem::MaybeUninit;

use bevy::ecs::system::SystemParam;

use super::{log_position::LogPositionTrait, state::UserStateTrait, ReversibleSystem};

pub struct ReversibleParams<
    'w,
    'a,
    UserState: UserStateTrait,
    UserParams: SystemParam,
    UserQuery: LogPositionTrait,
> {
    state: MaybeUninit<&'a UserState>,
    params_mut: MaybeUninit<&'a mut UserParams>,
    params: MaybeUninit<&'a UserParams>,
    query_items: MaybeUninit<&'a mut UserQuery::QueryItem<'w>>,
}

pub type Params<'w, 'a, T> = ReversibleParams<
    'w,
    'a,
    <T as ReversibleSystem>::State,
    <T as ReversibleSystem>::Params,
    <T as ReversibleSystem>::LogPosition,
>;

pub struct ReversibleParamsTransition<
    'w,
    'a,
    UserState: UserStateTrait,
    UserParams: SystemParam,
    UserQuery: LogPositionTrait,
    UserTransition,
> {
    past_state: MaybeUninit<&'a UserState>,
    future_state: MaybeUninit<&'a UserState>,
    params_mut: MaybeUninit<&'a mut UserParams>,
    params: MaybeUninit<&'a UserParams>,
    query_items: MaybeUninit<&'a mut UserQuery::QueryItem<'w>>,
    transition: MaybeUninit<&'a UserTransition>,
}

pub type ParamsTransition<'w, 'a, T> = ReversibleParamsTransition<
    'w,
    'a,
    <T as ReversibleSystem>::State,
    <T as ReversibleSystem>::Params,
    <T as ReversibleSystem>::LogPosition,
    <T as ReversibleSystem>::Transition,
>;
