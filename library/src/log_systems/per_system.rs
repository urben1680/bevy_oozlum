use std::{any::type_name, num::Wrapping};

use bevy::{
    ecs::system::SystemParam,
    prelude::{Commands, Res, ResMut},
};

use crate::controller::{consts::CONTROLLER_CONSTS, Controller};

use super::{log::Log, NextTransition, StateOption};

pub trait PerSystem: Send + Sync + Sized + 'static {
    type Params<'w, 's>: SystemParam;
    type State: StateOption;
    type Transition: Send + Sync;
    const DEFAULT_LOG_CAPACITY: usize = CONTROLLER_CONSTS.log_len;
    const FAST_ADVANCE_SYSTEM: bool = false;
    const FAST_REVERT_SYSTEM: bool = false;
    fn next_transition(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>>;
    fn advance(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn revert(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn advance_up_to_transition_or_limit(
        params: &mut Self::Params<'_, '_>,
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
        params: &mut Self::Params<'_, '_>,
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
        params: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    fn revert_transition(
        params: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    /// only used in fast-forward
    fn debug(params: &mut Self::Params<'_, '_>) -> String {
        #[allow(clippy::no_effect)]
        (params,);
        format!(
            "`<{} as PerEntity>::debug` is not implemented.",
            type_name::<Self>()
        )
    }
}

pub(super) trait PerSystemSystems: PerSystem {
    fn next_transition_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>> {
        Self::next_transition(mut_param, state, transitioned, now)
    }
    fn advance_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        Self::advance(mut_param, state, transitioned, now);
    }
    fn revert_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        Self::revert(mut_param, state, transitioned, now);
    }
    fn advance_up_to_transition_or_limit_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> Wrapping<u16> {
        Self::advance_up_to_transition_or_limit(mut_param, state, transitioned, now, limit)
    }
    fn revert_down_to_transition_or_limit_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        Self::revert_down_to_transition_or_limit(mut_param, state, transitioned, now);
    }
    fn advance_transition_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        Self::advance_transition(mut_param, past_state, future_state, transition, now);
    }
    fn revert_transition_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        Self::revert_transition(mut_param, past_state, future_state, transition, now);
    }
    fn debug_dud_ref(_ref_param: &(), mut_param: &mut Self::Params<'_, '_>) -> String {
        Self::debug(mut_param)
    }
    fn advance_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        mut commands: Commands<'w, 's>,
    ) {
        log.advance::<Self::State, (), Self::Params<'w, 's>>(
            &(),
            &mut params,
            &states,
            &controller,
            &mut commands,
            Self::advance_dud_ref,
            Self::next_transition_dud_ref,
            Self::advance_transition_dud_ref,
        );
    }
    fn advance_fast_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        mut commands: Commands<'w, 's>,
    ) {
        if Self::FAST_ADVANCE_SYSTEM {
            log.advance_fast::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                &mut commands,
                Self::advance_up_to_transition_or_limit_dud_ref,
                Self::next_transition_dud_ref,
                Self::advance_transition_dud_ref,
                Self::debug_dud_ref,
            );
        } else {
            log.advance::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                &mut commands,
                Self::advance_dud_ref,
                Self::next_transition_dud_ref,
                Self::advance_transition_dud_ref,
            );
        }
    }
    fn advance_log_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        log.advance_log::<Self::State, (), Self::Params<'w, 's>>(
            &(),
            &mut params,
            &states,
            &controller,
            Self::advance_dud_ref,
            Self::advance_transition_dud_ref,
        )
    }
    fn advance_log_fast_system<'w, 's, const INIT: bool>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::FAST_ADVANCE_SYSTEM {
            log.advance_log_fast::<Self::State, (), Self::Params<'w, 's>, INIT>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::advance_up_to_transition_or_limit_dud_ref,
                Self::advance_transition_dud_ref,
                Self::debug_dud_ref,
            );
        } else {
            log.advance_log::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::advance_dud_ref,
                Self::advance_transition_dud_ref,
            );
        }
    }
    fn revert_log_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        log.revert_log::<Self::State, (), Self::Params<'w, 's>>(
            &(),
            &mut params,
            &states,
            &controller,
            Self::revert_dud_ref,
            Self::revert_transition_dud_ref,
        )
    }
    fn revert_log_fast_system<'w, 's, const INIT: bool>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::FAST_REVERT_SYSTEM {
            log.revert_log_fast::<Self::State, (), Self::Params<'w, 's>, INIT>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::revert_down_to_transition_or_limit_dud_ref,
                Self::revert_transition_dud_ref,
            );
        } else {
            log.revert_log::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::revert_dud_ref,
                Self::revert_transition_dud_ref,
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
