use std::{any::type_name, num::Wrapping};

use bevy::ecs::system::SystemParam;

use crate::LOG_LEN;

use super::{NextTransition, StateOption};

pub trait PerSystem: Send + Sync + Sized + 'static {
    type Params: SystemParam;
    type State: StateOption;
    type Transition;
    const DEFAULT_LOG_CAPACITY: usize = LOG_LEN;
    const FAST_ADVANCE_SYSTEM: bool = false;
    const FAST_REVERT_SYSTEM: bool = false;
    fn next_transition(
        params: &mut <Self as PerSystemParams>::UserParams<'_>,
        state: &<Self::State as StateOption>::Output,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>>;
    fn advance(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        now: Wrapping<u16>,
    );
    fn revert(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        now: Wrapping<u16>,
    );
    fn advance_up_to_transition_or_limit(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> Option<Wrapping<u16>> {
        #[allow(clippy::no_effect)]
        (limit,); //calm clippy without adding `_` prefixes to trait function signature
        Self::advance(params, state, now);
        None
    }
    fn revert_down_to_transition_or_limit(
        params: &mut Self::Params,
        state: &<Self::State as StateOption>::Output,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> bool {
        #[allow(clippy::no_effect)]
        (limit,); //calm clippy without adding `_` prefixes to trait function signature
        Self::revert(params, state, now);
        false
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
            "`{}` does not provide debug information.",
            type_name::<Self>()
        )
    }
}

pub trait PerSystemParams: PerSystem {
    type SystemParams;
    type UserParams<'a>;
}

impl<T: PerSystem> PerSystemParams for T {
    type SystemParams = <Self as PerSystem>::Params;
    type UserParams<'a> = &'a mut <Self as PerSystem>::Params;
}
