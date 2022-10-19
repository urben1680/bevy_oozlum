use std::{any::type_name, num::Wrapping};

use bevy::{
    ecs::{
        query::{QueryItem, WorldQuery},
        system::SystemParam,
    },
    prelude::{ParallelCommands, Query, Res, Without},
};

use crate::{
    controller::{consts::CONTROLLER_CONSTS, Controller},
    DespawnedEntity, LOG_ONLY_PAR_ITER_BATCH_SIZE,
};

use super::{log::Log, NextTransition, StateOption};

pub trait PerEntity: Send + Sync + Sized + 'static {
    type Params<'w, 's>: SystemParam + Sync;
    type Items: WorldQuery;
    type State: StateOption;
    type Transition: Send + Sync;
    const DEFAULT_LOG_CAPACITY: usize = CONTROLLER_CONSTS.log_len;
    const FAST_ADVANCE_SYSTEM: bool = false;
    const FAST_REVERT_SYSTEM: bool = false;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>>;
    fn advance(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn revert(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn advance_up_to_transition_or_limit(
        params: &Self::Params<'_, '_>,
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
        params: &Self::Params<'_, '_>,
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
        params: &Self::Params<'_, '_>,
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
        params: &Self::Params<'_, '_>,
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
    fn debug(params: &Self::Params<'_, '_>, items: &mut QueryItem<'_, Self::Items>) -> String {
        #[allow(clippy::no_effect)]
        (params, items);
        format!(
            "`<{} as PerEntity>::debug` is not implemented.",
            type_name::<Self>()
        )
    }
}

pub(super) trait PerEntitySystems: PerEntity {
    fn advance_system<'w, 's>(
        params: Self::Params<'w, 's>,
        mut query: Query<
            '_,
            '_,
            (
                Self::Items,
                &'static mut Log<Self, Self::Transition, <Self::State as StateOption>::Index>,
            ),
            Without<DespawnedEntity>,
        >,
        states: <Self::State as StateOption>::Param<'_>,
        controller: Res<'_, Controller>,
        commands: ParallelCommands<'_, '_>,
    ) {
        if Self::PAR_ITER_BATCH_SIZE == 0 {
            commands.command_scope(|mut commands| {
                query.for_each_mut(|(mut items, mut log)| {
                    log.advance::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        &mut commands,
                        Self::advance,
                        Self::next_transition,
                        Self::advance_transition,
                    );
                });
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                commands.command_scope(|mut commands| {
                    log.advance::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        &mut commands,
                        Self::advance,
                        Self::next_transition,
                        Self::advance_transition,
                    );
                });
            });
        }
    }
    fn advance_fast_system<'w, 's>(
        params: Self::Params<'w, 's>,
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
                    if Self::FAST_ADVANCE_SYSTEM {
                        log.advance_fast::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::advance_up_to_transition_or_limit,
                            Self::next_transition,
                            Self::advance_transition,
                            Self::debug,
                        );
                    } else {
                        log.advance::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::advance,
                            Self::next_transition,
                            Self::advance_transition,
                        );
                    }
                });
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                commands.command_scope(|mut commands| {
                    if Self::FAST_ADVANCE_SYSTEM {
                        log.advance_fast::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::advance_up_to_transition_or_limit,
                            Self::next_transition,
                            Self::advance_transition,
                            Self::debug,
                        );
                    } else {
                        log.advance::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::advance,
                            Self::next_transition,
                            Self::advance_transition,
                        );
                    }
                });
            });
        }
    }
    fn advance_log_system<'w, 's>(
        params: Self::Params<'w, 's>,
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
                log.advance_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::advance,
                    Self::advance_transition,
                )
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                log.advance_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::advance,
                    Self::advance_transition,
                )
            });
        }
    }
    fn advance_log_fast_system<'w, 's>(
        params: Self::Params<'w, 's>,
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
                if Self::FAST_ADVANCE_SYSTEM {
                    log.advance_log_fast::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::advance_up_to_transition_or_limit,
                        Self::advance_transition,
                        Self::debug,
                    );
                } else {
                    log.advance_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::advance,
                        Self::advance_transition,
                    );
                }
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                if Self::FAST_ADVANCE_SYSTEM {
                    log.advance_log_fast::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::advance_up_to_transition_or_limit,
                        Self::advance_transition,
                        Self::debug,
                    );
                } else {
                    log.advance_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::advance,
                        Self::advance_transition,
                    );
                }
            });
        }
    }
    fn revert_log_system<'w, 's>(
        params: Self::Params<'w, 's>,
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
                log.revert_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::revert,
                    Self::revert_transition,
                )
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                log.revert_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::revert,
                    Self::revert_transition,
                )
            });
        }
    }
    fn revert_log_fast_system<'w, 's>(
        params: Self::Params<'w, 's>,
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
                if Self::FAST_REVERT_SYSTEM {
                    log.revert_log_fast::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::revert_down_to_transition_or_limit,
                        Self::revert_transition,
                    );
                } else {
                    log.revert_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::revert,
                        Self::revert_transition,
                    );
                }
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                if Self::FAST_REVERT_SYSTEM {
                    log.revert_log_fast::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::revert_down_to_transition_or_limit,
                        Self::revert_transition,
                    );
                } else {
                    log.revert_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::revert,
                        Self::revert_transition,
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
