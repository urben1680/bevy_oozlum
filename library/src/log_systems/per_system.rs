use std::{any::type_name, num::Wrapping};

use bevy::{
    ecs::system::SystemParam,
    prelude::{Commands, Res, ResMut},
};

use crate::{
    controller::{consts::CONTROLLER_CONSTS, Controller},
    Ticks, ToTimeStamp,
};

use super::{log::Log, NextTransition, StateOption};

pub trait PerSystem: Send + Sync + Sized + 'static {
    type Params<'w, 's>: SystemParam;
    type State: StateOption;
    type Transition: Send + Sync;
    const DEFAULT_LOG_CAPACITY: usize = CONTROLLER_CONSTS.log_capacity;
    const FAST_ADVANCE_SYSTEM: bool = false;
    const FAST_REVERT_SYSTEM: bool = false;
    fn next_transition(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
    ) -> Option<NextTransition<Self::State, Self::Transition>>;
    fn forward(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
    );
    fn backward(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
    );
    fn forward_to_transition_or_limit(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
        limit: ToTimeStamp,
    ) -> Wrapping<Ticks> {
        #[allow(clippy::no_effect)]
        (params, state, transitioned, now, limit);
        panic!(
            "`<{} as PerSystem>::advance_up_to_transition_or_limit` should be implemented if `FAST_ADVANCE_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn backward_to_transition_or_limit(
        params: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
        limit: ToTimeStamp,
    ) {
        #[allow(clippy::no_effect)]
        (params, state, transitioned, now, limit);
        panic!(
            "`<{} as PerSystem>::revert_down_to_transition_or_limit` should be implemented if `FAST_REVERT_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn forward_transition(
        params: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<Ticks>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    fn backward_transition(
        params: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<Ticks>,
    ) {
        #[allow(clippy::no_effect)]
        (params, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
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
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
    ) -> Option<NextTransition<Self::State, Self::Transition>> {
        Self::next_transition(mut_param, state, transitioned, now)
    }
    fn forward_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
    ) {
        Self::forward(mut_param, state, transitioned, now);
    }
    fn backward_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
    ) {
        Self::backward(mut_param, state, transitioned, now);
    }
    fn forward_to_transition_or_limit_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
        limit: ToTimeStamp,
    ) -> Wrapping<Ticks> {
        Self::forward_to_transition_or_limit(mut_param, state, transitioned, now, limit)
    }
    fn backward_to_transition_or_limit_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<Ticks>,
        now: Wrapping<Ticks>,
        limit: ToTimeStamp,
    ) {
        Self::backward_to_transition_or_limit(mut_param, state, transitioned, now, limit);
    }
    fn forward_transition_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<Ticks>,
    ) {
        Self::forward_transition(mut_param, past_state, future_state, transition, now);
    }
    fn bacward_transition_dud_ref(
        _ref_param: &(),
        mut_param: &mut Self::Params<'_, '_>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<Ticks>,
    ) {
        Self::backward_transition(mut_param, past_state, future_state, transition, now);
    }
    fn debug_dud_ref(_ref_param: &(), mut_param: &mut Self::Params<'_, '_>) -> String {
        Self::debug(mut_param)
    }
    fn forward_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        mut commands: Commands<'w, 's>,
    ) {
        log.forward::<Self::State, (), Self::Params<'w, 's>>(
            &(),
            &mut params,
            &states,
            &controller,
            &mut commands,
            Self::forward_dud_ref,
            Self::next_transition_dud_ref,
            Self::forward_transition_dud_ref,
        );
    }
    fn forward_to_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        mut commands: Commands<'w, 's>,
    ) {
        if Self::FAST_ADVANCE_SYSTEM {
            log.forward_to::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                &mut commands,
                Self::forward_to_transition_or_limit_dud_ref,
                Self::next_transition_dud_ref,
                Self::forward_transition_dud_ref,
                Self::debug_dud_ref,
            );
        } else {
            log.forward::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                &mut commands,
                Self::forward_dud_ref,
                Self::next_transition_dud_ref,
                Self::forward_transition_dud_ref,
            );
        }
    }
    fn forward_log_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        log.forward_log::<Self::State, (), Self::Params<'w, 's>>(
            &(),
            &mut params,
            &states,
            &controller,
            Self::forward_dud_ref,
            Self::forward_transition_dud_ref,
        )
    }
    fn forward_log_to_system<'w, 's, const INIT: bool>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::FAST_ADVANCE_SYSTEM {
            log.forward_log_to::<Self::State, (), Self::Params<'w, 's>, INIT>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::forward_to_transition_or_limit_dud_ref,
                Self::forward_transition_dud_ref,
                Self::debug_dud_ref,
            );
        } else {
            log.forward_log::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::forward_dud_ref,
                Self::forward_transition_dud_ref,
            );
        }
    }
    fn backward_log_system<'w, 's>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        log.backward_log::<Self::State, (), Self::Params<'w, 's>>(
            &(),
            &mut params,
            &states,
            &controller,
            Self::backward_dud_ref,
            Self::bacward_transition_dud_ref,
        )
    }
    fn backward_log_to_system<'w, 's, const INIT: bool>(
        mut params: Self::Params<'w, 's>,
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::FAST_REVERT_SYSTEM {
            log.backward_log_to::<Self::State, (), Self::Params<'w, 's>, INIT>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::backward_to_transition_or_limit_dud_ref,
                Self::bacward_transition_dud_ref,
            );
        } else {
            log.backward_log::<Self::State, (), Self::Params<'w, 's>>(
                &(),
                &mut params,
                &states,
                &controller,
                Self::backward_dud_ref,
                Self::bacward_transition_dud_ref,
            );
        }
    }
    fn age_check_system<'w, 's>(
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
        controller: Res<'w, Controller>,
    ) {
        log.age_check(&controller);
    }
    fn log_close_system<'w, 's>(
        mut log: ResMut<'w, Log<Self, Self::Transition, <Self::State as StateOption>::Index>>,
    ) {
        log.log_close();
    }
}

impl<T: PerSystem> PerSystemSystems for T {}
