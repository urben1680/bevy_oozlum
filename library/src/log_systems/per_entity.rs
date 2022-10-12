use std::{any::type_name, num::Wrapping};

use bevy::{
    ecs::{
        query::{QueryItem, WorldQuery},
        system::SystemParam,
    },
    prelude::{ParallelCommands, Query, Res, Without},
};

use crate::{
    controller::{Controller, CONTROLLER_CONSTS},
    DespawnedEntity, LOG_ONLY_PAR_ITER_BATCH_SIZE,
};

use super::{log::Log, NextTransition, StateOption};

pub trait PerEntity: Send + Sync + Sized + 'static {
    type Params: SystemParam + Sync;
    type Items: WorldQuery;
    type State: StateOption;
    type Transition: Send + Sync;
    const DEFAULT_LOG_CAPACITY: usize = CONTROLLER_CONSTS.log_len;
    const FAST_ADVANCE_SYSTEM: bool = false;
    const FAST_REVERT_SYSTEM: bool = false;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(
        params: &Self::Params,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>>;
    fn advance(
        params: &Self::Params,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn revert(
        params: &Self::Params,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn advance_up_to_transition_or_limit(
        params: &Self::Params,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> Wrapping<u16> {
        #[allow(clippy::no_effect)]
        (params, items, state, transitioned, now, limit);
        panic!(
            "`<{} as PerEntity>::advance_up_to_transition_or_limit` should be implemented if `FAST_ADVANCE_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn revert_down_to_transition_or_limit(
        params: &Self::Params,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, items, state, transitioned, now);
        panic!(
            "`<{} as PerEntity>::revert_down_to_transition_or_limit` should be implemented if `FAST_REVERT_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn advance_transition(
        params: &Self::Params,
        items: &mut QueryItem<'_, Self::Items>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, items, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    fn revert_transition(
        params: &Self::Params,
        items: &mut QueryItem<'_, Self::Items>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        #[allow(clippy::no_effect)]
        (params, items, past_state, future_state, transition, now); //calm clippy without adding `_` prefixes to trait function signature
    }
    /// only used in fast-forward
    fn debug(params: &Self::Params, items: &mut QueryItem<'_, Self::Items>) -> String {
        #[allow(clippy::no_effect)]
        (params, items);
        format!(
            "`<{} as PerEntity>::debug` is not implemented.",
            type_name::<Self>()
        )
    }
}

type Params<'a, 'b, T> = (
    &'a <T as PerEntity>::Params,
    &'a mut QueryItem<'b, <T as PerEntity>::Items>,
);

pub(super) trait PerEntitySystems: PerEntity {
    fn intern_next_transition(
        params: &mut Params<'_, '_, Self>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>> {
        Self::next_transition(params.0, params.1, state, transitioned, now)
    }
    fn intern_advance(
        params: &mut Params<'_, '_, Self>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        Self::advance(params.0, params.1, state, transitioned, now);
    }
    fn intern_advance_up_to_transition_or_limit(
        params: &mut Params<'_, '_, Self>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
        limit: Wrapping<u16>,
    ) -> Wrapping<u16> {
        Self::advance_up_to_transition_or_limit(params.0, params.1, state, transitioned, now, limit)
    }
    fn intern_revert(
        params: &mut Params<'_, '_, Self>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        Self::revert(params.0, params.1, state, transitioned, now);
    }
    fn intern_revert_down_to_transition_or_limit(
        params: &mut Params<'_, '_, Self>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) {
        Self::revert_down_to_transition_or_limit(params.0, params.1, state, transitioned, now);
    }
    fn intern_advance_transition(
        params: &mut Params<'_, '_, Self>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        Self::advance_transition(
            params.0,
            params.1,
            past_state,
            future_state,
            transition,
            now,
        );
    }
    fn intern_revert_transition(
        params: &mut Params<'_, '_, Self>,
        past_state: &<Self::State as StateOption>::Output,
        future_state: &<Self::State as StateOption>::Output,
        transition: &Self::Transition,
        now: Wrapping<u16>,
    ) {
        Self::revert_transition(
            params.0,
            params.1,
            past_state,
            future_state,
            transition,
            now,
        );
    }
    fn intern_debug(params: &mut Params<'_, '_, Self>) -> String {
        Self::debug(params.0, params.1)
    }
    fn advance_system<'w, 's>(
        params: Self::Params,
        mut query: Query<
            'w,
            's,
            (
                Self::Items,
                &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
            ),
            Without<DespawnedEntity>,
        >,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        commands: ParallelCommands<'w, 's>,
    ) {
        if Self::PAR_ITER_BATCH_SIZE == 0 {
            commands.command_scope(|mut commands| {
                query.for_each_mut(|(mut items, mut log)| {
                    let mut params = (&params, &mut items);
                    log.advance::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        &mut commands,
                        Self::intern_advance,
                        Self::intern_next_transition,
                        Self::intern_advance_transition,
                    );
                });
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                commands.command_scope(|mut commands| {
                    let mut params = (&params, &mut items);
                    log.advance::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        &mut commands,
                        Self::intern_advance,
                        Self::intern_next_transition,
                        Self::intern_advance_transition,
                    );
                });
            });
        }
    }
    fn advance_fast_system<'w, 's>(
        params: Self::Params,
        mut query: Query<
            'w,
            's,
            (
                Self::Items,
                &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
            ),
            Without<DespawnedEntity>,
        >,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
        commands: ParallelCommands<'w, 's>,
    ) {
        if Self::PAR_ITER_BATCH_SIZE == 0 {
            commands.command_scope(|mut commands| {
                query.for_each_mut(|(mut items, mut log)| {
                    let mut params = (&params, &mut items);
                    if Self::FAST_ADVANCE_SYSTEM {
                        log.advance_fast::<Self::State, Params<'_, '_, Self>>(
                            &mut params,
                            &states,
                            &controller,
                            &mut commands,
                            Self::intern_advance_up_to_transition_or_limit,
                            Self::intern_next_transition,
                            Self::intern_advance_transition,
                            Self::intern_debug,
                        );
                    } else {
                        log.advance::<Self::State, Params<'_, '_, Self>>(
                            &mut params,
                            &states,
                            &controller,
                            &mut commands,
                            Self::intern_advance,
                            Self::intern_next_transition,
                            Self::intern_advance_transition,
                        );
                    }
                });
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                commands.command_scope(|mut commands| {
                    let mut params = (&params, &mut items);
                    if Self::FAST_ADVANCE_SYSTEM {
                        log.advance_fast::<Self::State, Params<'_, '_, Self>>(
                            &mut params,
                            &states,
                            &controller,
                            &mut commands,
                            Self::intern_advance_up_to_transition_or_limit,
                            Self::intern_next_transition,
                            Self::intern_advance_transition,
                            Self::intern_debug,
                        );
                    } else {
                        log.advance::<Self::State, Params<'_, '_, Self>>(
                            &mut params,
                            &states,
                            &controller,
                            &mut commands,
                            Self::intern_advance,
                            Self::intern_next_transition,
                            Self::intern_advance_transition,
                        );
                    }
                });
            });
        }
    }
    fn advance_log_system<'w, 's>(
        params: Self::Params,
        mut query: Query<
            'w,
            's,
            (
                Self::Items,
                &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
            ),
            Without<DespawnedEntity>,
        >,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(|(mut items, mut log)| {
                let mut params = (&params, &mut items);
                log.advance_log::<Self::State, Params<'_, '_, Self>>(
                    &mut params,
                    &states,
                    &controller,
                    Self::intern_advance,
                    Self::intern_advance_transition,
                )
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                let mut params = (&params, &mut items);
                log.advance_log::<Self::State, Params<'_, '_, Self>>(
                    &mut params,
                    &states,
                    &controller,
                    Self::intern_advance,
                    Self::intern_advance_transition,
                )
            });
        }
    }
    fn advance_log_fast_system<'w, 's>(
        params: Self::Params,
        mut query: Query<
            'w,
            's,
            (
                Self::Items,
                &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
            ),
            Without<DespawnedEntity>,
        >,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(|(mut items, mut log)| {
                let mut params = (&params, &mut items);
                if Self::FAST_ADVANCE_SYSTEM {
                    log.advance_log_fast::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_advance_up_to_transition_or_limit,
                        Self::intern_advance_transition,
                        Self::intern_debug,
                    );
                } else {
                    log.advance_log::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_advance,
                        Self::intern_advance_transition,
                    );
                }
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                let mut params = (&params, &mut items);
                if Self::FAST_ADVANCE_SYSTEM {
                    log.advance_log_fast::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_advance_up_to_transition_or_limit,
                        Self::intern_advance_transition,
                        Self::intern_debug,
                    );
                } else {
                    log.advance_log::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_advance,
                        Self::intern_advance_transition,
                    );
                }
            });
        }
    }
    fn revert_log_system<'w, 's>(
        params: Self::Params,
        mut query: Query<
            'w,
            's,
            (
                Self::Items,
                &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
            ),
            Without<DespawnedEntity>,
        >,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(|(mut items, mut log)| {
                let mut params = (&params, &mut items);
                log.revert_log::<Self::State, Params<'_, '_, Self>>(
                    &mut params,
                    &states,
                    &controller,
                    Self::intern_revert,
                    Self::intern_revert_transition,
                )
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                let mut params = (&params, &mut items);
                log.revert_log::<Self::State, Params<'_, '_, Self>>(
                    &mut params,
                    &states,
                    &controller,
                    Self::intern_revert,
                    Self::intern_revert_transition,
                )
            });
        }
    }
    fn revert_log_fast_system<'w, 's>(
        params: Self::Params,
        mut query: Query<
            'w,
            's,
            (
                Self::Items,
                &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
            ),
            Without<DespawnedEntity>,
        >,
        states: <Self::State as StateOption>::Param<'w>,
        controller: Res<'w, Controller>,
    ) {
        if Self::PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(|(mut items, mut log)| {
                let mut params = (&params, &mut items);
                if Self::FAST_REVERT_SYSTEM {
                    log.revert_log_fast::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_revert_down_to_transition_or_limit,
                        Self::intern_revert_transition,
                    );
                } else {
                    log.revert_log::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_revert,
                        Self::intern_revert_transition,
                    );
                }
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                let mut params = (&params, &mut items);
                if Self::FAST_REVERT_SYSTEM {
                    log.revert_log_fast::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_revert_down_to_transition_or_limit,
                        Self::intern_revert_transition,
                    );
                } else {
                    log.revert_log::<Self::State, Params<'_, '_, Self>>(
                        &mut params,
                        &states,
                        &controller,
                        Self::intern_revert,
                        Self::intern_revert_transition,
                    );
                }
            });
        }
    }
    fn age_check_system<'w, 's>(
        mut query: Query<
            'w,
            's,
            &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
        >,
        controller: Res<'w, Controller>,
    ) {
        if LOG_ONLY_PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(|mut log| {
                log.age_check(&controller);
            });
        } else {
            query.par_for_each_mut(LOG_ONLY_PAR_ITER_BATCH_SIZE, |mut log| {
                log.age_check(&controller);
            });
        }
    }
    fn log_end_system<'w, 's>(
        mut query: Query<
            'w,
            's,
            &mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
        >,
    ) {
        if LOG_ONLY_PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(|mut log| {
                log.log_end();
            });
        } else {
            query.par_for_each_mut(LOG_ONLY_PAR_ITER_BATCH_SIZE, |mut log| {
                log.log_end();
            });
        }
    }
}

impl<T: PerEntity> PerEntitySystems for T {}
