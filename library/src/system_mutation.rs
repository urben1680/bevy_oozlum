use std::{collections::VecDeque, mem::MaybeUninit, num::Wrapping};

use bevy::{
    ecs::system::SystemParam,
    prelude::{Component, ParallelCommands, Res},
};

use crate::{
    commands::{NextCommands, ReversibleCommands},
    controller::Controller,
    Ticks, LOG_LEN,
};

use self::{log_position::LogPositionTrait, state::UserStateTrait};

mod log_position;
mod state;

pub trait ReversibleSystem: Send + Sync + Sized + 'static {
    type State: UserStateTrait; //() or UserState<T, usize>
    type Params: SystemParam + Send + Sync;
    type LogPosition: LogPositionTrait; //`PerSystem` or `PerEntity<Q, 0>`
    type Transition: Send + Sync + 'static;
    const DEFAULT_LOG_CAPACITY: usize = LOG_LEN;
    fn next_transition(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        state: &<Self::State as UserStateTrait>::Output,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self>>;
    fn advance(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        state: &<Self::State as UserStateTrait>::Output,
        now: Wrapping<u16>,
    );
    fn revert(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        state: &<Self::State as UserStateTrait>::Output,
        now: Wrapping<u16>,
    );
    /// Return `None` to call `next_transition` with current timestamp.
    /// Return `Some` with given timestamp which expects `Some` to be returned as well if the timestamp is not equal to `limit`.
    fn advance_up_to_transition_or_limit(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        state: &<Self::State as UserStateTrait>::Output,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> Option<Wrapping<u16>> {
        #[allow(clippy::no_effect)]
        (limit,); //calm clippy without adding `_` prefixes to trait function signature
        Self::advance(params, state, now);
        None
    }
    /// Return `None` if transition might not be reached and to have reverted by one tick.
    /// Return `Some` if transition is happening at given timestamp which is assured in the log if it is not equal to `limit`.
    fn revert_down_to_transition_or_limit(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        state: &<Self::State as UserStateTrait>::Output,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> bool {
        #[allow(clippy::no_effect)]
        (limit,); //calm clippy without adding `_` prefixes to trait function signature
        Self::revert(params, state, now);
        false
    }
    fn advance_transition(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        past_state: &<Self::State as UserStateTrait>::Output,
        future_state: &<Self::State as UserStateTrait>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    fn revert_transition(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        past_state: &<Self::State as UserStateTrait>::Output,
        future_state: &<Self::State as UserStateTrait>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
}

pub struct NextTransition<T: ReversibleSystem> {
    pub(super) next_state_index: <T::State as UserStateTrait>::Index,
    pub(super) transition: T::Transition,
    pub(super) commands: Option<NextCommands<T>>,
}

impl<T: ReversibleSystem> NextTransition<T> {
    fn new(
        next_state_index: <T::State as UserStateTrait>::Index,
        transition: T::Transition,
    ) -> Self {
        Self {
            next_state_index,
            transition,
            commands: None,
        }
    }
    fn with_commands<FN: FnOnce(ReversibleCommands<T>) + Send + Sync + 'static>(
        next_state_index: <T::State as UserStateTrait>::Index,
        transition: T::Transition,
        commands: FN,
    ) -> Self {
        Self {
            next_state_index,
            transition,
            commands: Some(Box::new(commands)),
        }
    }
}

#[derive(Component)]
pub struct Log<T: ReversibleSystem> {
    pub(super) entry_index: usize,
    pub(super) entries: VecDeque<LogEntry<T>>,
}

pub(super) struct LogEntry<T: ReversibleSystem> {
    pub(super) transition: MaybeUninit<T::Transition>,
    pub(super) time_stamp: Wrapping<Ticks>,
    pub(super) state_index: <T::State as UserStateTrait>::Index,
}

pub(super) trait ReversibleComponentsImplemented: ReversibleSystem {
    /*
    fn advance_system<'w, 's>(
        controller: Res<'w, Controller>,
        commands: ParallelCommands,
        states: <Self::State as UserStateTrait>::Param<'w>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
        <Self::LogPosition as LogPositionTrait>::mutate(
            controller,
            commands,
            states,
            params,
            Self::advance_inner,
        )
    }
    fn advance_fast_system<'w, 's>(
        controller: Res<'w, Controller>,
        commands: ParallelCommands,
        states: <Self::State as UserStateTrait>::Param<'w>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
        <Self::LogPosition as LogPositionTrait>::mutate(
            controller,
            commands,
            states,
            params,
            Self::advance_fast_inner,
        )
    }
    fn advance_log_system<'w, 's>(
        controller: Res<'w, Controller>,
        states: <Self::State as UserStateTrait>::Param<'w>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
        <Self::LogPosition as LogPositionTrait>::mutate_log(
            controller,
            states,
            params,
            Self::advance_log_inner,
        )
    }
    fn advance_log_fast_system<'w, 's>(
        controller: Res<'w, Controller>,
        states: <Self::State as UserStateTrait>::Param<'w>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
        <Self::LogPosition as LogPositionTrait>::mutate_log(
            controller,
            states,
            params,
            Self::advance_log_inner,
        )
    }
    fn revert_log_system<'w, 's>(
        controller: Res<'w, Controller>,
        states: <Self::State as UserStateTrait>::Param<'w>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
        <Self::LogPosition as LogPositionTrait>::mutate_log(
            controller,
            states,
            params,
            Self::revert_log_inner,
        )
    }
    fn revert_log_fast_system<'w, 's>(
        controller: Res<'w, Controller>,
        states: <Self::State as UserStateTrait>::Param<'w>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
        <Self::LogPosition as LogPositionTrait>::mutate_log(
            controller,
            states,
            params,
            Self::revert_log_fast_inner,
        )
    }
    fn log_age_check_system<'w, 's>(
        controller: Res<'w, Controller>,
        log: <Self::LogPosition as LogPositionTrait>::InLogOnly<'w, 's, Self>,
    ) {
        <Self::LogPosition as LogPositionTrait>::mutate_log_only(
            controller,
            log,
            Self::log_age_check_inner,
        )
    }
    fn advance_inner<'w, 's>(
        controller: &Controller,
        commands: &ParallelCommands,
        states: &<Self::State as UserStateTrait>::Param<'w>,
        log: &mut Log<Self>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
    }
    fn advance_fast_inner<'w, 's>(
        controller: &Controller,
        commands: &ParallelCommands,
        states: &<Self::State as UserStateTrait>::Param<'w>,
        log: &mut Log<Self>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
    }
    fn advance_log_inner<'w, 's>(
        controller: &Controller,
        states: &<Self::State as UserStateTrait>::Param<'w>,
        log: &mut Log<Self>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
    }
    fn advance_log_fast_inner<'w, 's>(
        controller: &Controller,
        states: &<Self::State as UserStateTrait>::Param<'w>,
        log: &mut Log<Self>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
    }
    fn revert_log_inner<'w, 's>(
        controller: &Controller,
        states: &<Self::State as UserStateTrait>::Param<'w>,
        log: &mut Log<Self>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
    }
    fn revert_log_fast_inner<'w, 's>(
        controller: &Controller,
        states: &<Self::State as UserStateTrait>::Param<'w>,
        log: &mut Log<Self>,
        params: <Self::LogPosition as LogPositionTrait>::In<'w, 's, Self>,
    ) {
    }
    fn log_age_check_inner(controller: &Controller, log: &mut Log<Self>) {}
    */
}

impl<T: ReversibleSystem> ReversibleComponentsImplemented for T {}
