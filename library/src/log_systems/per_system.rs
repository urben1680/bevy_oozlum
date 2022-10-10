use std::{any::type_name, num::Wrapping};

use bevy::{
    ecs::system::SystemParam,
    prelude::{Commands, Res, ResMut},
};

use crate::{controller::Controller, LOG_LEN};

use super::{log::Log, NextTransition, StateOption};

pub trait PerSystem: Send + Sync + Sized + 'static {
    type Params: SystemParam;
    type State: StateOption;
    type Transition: Send + Sync;
    const DEFAULT_LOG_CAPACITY: usize = LOG_LEN;
    const FAST_ADVANCE_SYSTEM: bool = false;
    const FAST_REVERT_SYSTEM: bool = false;
    fn next_transition(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>>;
    fn advance(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn revert(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn advance_up_to_transition_or_limit(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> Wrapping<u16> {
        #[allow(clippy::no_effect)]
        (params, state, transitioned, now, limit);
        panic!(
            "`<{} as PerSystem>::advance_up_to_transition_or_limit` should be implemented if `FAST_ADVANCE_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn revert_down_to_transition_or_limit(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, state, transitioned, now);
        panic!(
            "`<{} as PerSystem>::revert_down_to_transition_or_limit` should be implemented if `FAST_REVERT_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn advance_transition(
        params: &mut Self::Params,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    fn revert_transition(
        params: &mut Self::Params,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    /// only used in fast-forward
    fn debug(params: &mut Self::Params) -> String {
        #[allow(clippy::no_effect)]
        (params,);
        format!(
            "`<{} as PerEntity>::debug` is not implemented.",
            type_name::<Self>()
        )
    }
}

pub(super) trait PerSystemSystems: PerSystem {
    fn advance_system<'w, 's>(
        mut params: Self::Params,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        mut commands: Commands<'w, 's>,
    ) {
        log.advance::<Self::State, Self::Params>(
            &mut params,
            &states,
            &controller,
            &mut commands,
            Self::advance,
            Self::next_transition,
            Self::advance_transition,
        );
    }
    fn advance_fast_system<'w, 's>(
        mut params: Self::Params,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        mut commands: Commands<'w, 's>,
    ) {
        if Self::FAST_ADVANCE_SYSTEM {
            log.advance_fast::<Self::State, Self::Params>(
                &mut params,
                &states,
                &controller,
                &mut commands,
                Self::advance_up_to_transition_or_limit,
                Self::next_transition,
                Self::advance_transition,
                Self::debug,
            );
        } else {
            log.advance::<Self::State, Self::Params>(
                &mut params,
                &states,
                &controller,
                &mut commands,
                Self::advance,
                Self::next_transition,
                Self::advance_transition,
            );
        }
    }
    fn advance_log_system<'w, 's>(
        mut params: Self::Params,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        log.advance_log::<Self::State, Self::Params>(
            &mut params,
            &states,
            &controller,
            Self::advance,
            Self::advance_transition,
        )
    }
    fn advance_log_fast_system<'w, 's>(
        mut params: Self::Params,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::FAST_ADVANCE_SYSTEM {
            log.advance_log_fast::<Self::State, Self::Params>(
                &mut params,
                &states,
                &controller,
                Self::advance_up_to_transition_or_limit,
                Self::advance_transition,
                Self::debug,
            );
        } else {
            log.advance_log::<Self::State, Self::Params>(
                &mut params,
                &states,
                &controller,
                Self::advance,
                Self::advance_transition,
            );
        }
    }
    fn revert_log_system<'w, 's>(
        mut params: Self::Params,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        log.revert_log::<Self::State, Self::Params>(
            &mut params,
            &states,
            &controller,
            Self::revert,
            Self::revert_transition,
        )
    }
    fn revert_log_fast_system<'w, 's>(
        mut params: Self::Params,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::FAST_REVERT_SYSTEM {
            log.revert_log_fast::<Self::State, Self::Params>(
                &mut params,
                &states,
                &controller,
                Self::revert_down_to_transition_or_limit,
                Self::revert_transition,
            );
        } else {
            log.revert_log::<Self::State, Self::Params>(
                &mut params,
                &states,
                &controller,
                Self::revert,
                Self::revert_transition,
            );
        }
    }
    fn age_check_system<'w, 's>(
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        controller: Res<'w, Controller>,
    ) {
        log.age_check(&controller);
    }
    fn log_end_system<'w, 's>(
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
    ) {
        log.log_end();
    }
}

impl<T: PerSystem> PerSystemSystems for T {}
