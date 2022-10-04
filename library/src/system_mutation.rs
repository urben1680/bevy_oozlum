use std::{collections::VecDeque, mem::MaybeUninit, num::Wrapping};

use bevy::{
    ecs::system::SystemParam,
    prelude::{Component, ParallelCommands},
};

use crate::{commands::NextCommands, Ticks, MAX_LOG_LEN};

use self::{log_position::LogPositionTrait, state::UserStateTrait};

mod log_position;
mod state;

pub trait ReversibleSystem: Send + Sync + Sized + 'static {
    type State: UserStateTrait; //() or UserState<T, usize>
    type Params: SystemParam + Send + Sync;
    type LogPosition: LogPositionTrait; //`PerSystem` or `PerEntity<Q, 0>`
    type Transition: Send + Sync + 'static;
    const INITIAL_LOG_CAPACITY: usize = MAX_LOG_LEN;
    const LOG_CAPACITY_GROWTH: usize = 1;
    fn next_transition(
        params: &mut <Self::LogPosition as LogPositionTrait>::In<'_, '_, Self>,
        state: &<Self::State as UserStateTrait>::Output,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<<Self::State as UserStateTrait>::Index, Self::Transition, Self>>;
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
    ) -> Option<Wrapping<u16>> {
        #[allow(clippy::no_effect)]
        (limit,); //calm clippy without adding `_` prefixes to trait function signature
        Self::revert(params, state, now);
        None
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

pub struct NextTransition<Index: Send + Sync + 'static, Transition, Marker: Send + Sync + 'static> {
    pub(super) next_state_index: Index,
    pub(super) transition: Transition,
    pub(super) commands: Option<NextCommands<Marker>>,
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

trait ReversibleComponentsImplemented: ReversibleSystem {}

impl<T: ReversibleSystem> ReversibleComponentsImplemented for T {}
