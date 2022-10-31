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
    DespawnedEntity, ToTimeStamp, LOG_ONLY_PAR_ITER_BATCH_SIZE,
};

use super::{log::Log, NextTransition, StateOption};

pub trait PerEntity: Send + Sync + Sized + 'static {
    type Params<'w, 's>: SystemParam + Sync;
    type Items: WorldQuery;
    type State: StateOption;
    type Transition: Send + Sync;
    const DEFAULT_LOG_CAPACITY: usize = CONTROLLER_CONSTS.log_capacity;
    const FORWARD_TO_SYSTEM: bool = false;
    const BACKWARD_TO_SYSTEM: bool = false;
    const PAR_ITER_BATCH_SIZE: usize = 0;
    fn next_transition(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    ) -> Option<NextTransition<Self::State, Self::Transition>>;
    fn forward(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn backward(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
    );
    fn forward_to_transition_or_limit(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
        limit: ToTimeStamp,
    ) -> Wrapping<u16> {
        #[allow(clippy::no_effect)]
        (params, items, state, transitioned, now, limit);
        panic!(
            "`<{} as PerEntity>::advance_up_to_transition_or_limit` should be implemented if `FORWARD_TO_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn backward_to_transition_or_limit(
        params: &Self::Params<'_, '_>,
        items: &mut QueryItem<'_, Self::Items>,
        state: &<Self::State as StateOption>::Output,
        transitioned: Wrapping<u16>,
        now: Wrapping<u16>,
        limit: ToTimeStamp,
    ) {
        #[allow(clippy::no_effect)]
        (params, items, state, transitioned, now, limit);
        panic!(
            "`<{} as PerEntity>::revert_down_to_transition_or_limit` should be implemented if `BACKWARD_TO_SYSTEM` is set to `true`.",
            type_name::<Self>()
        );
    }
    fn forward_transition(
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
    fn backward_transition(
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
    fn forward_system<'w, 's>(
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
                    log.forward::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        &mut commands,
                        Self::forward,
                        Self::next_transition,
                        Self::forward_transition,
                    );
                });
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                commands.command_scope(|mut commands| {
                    log.forward::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        &mut commands,
                        Self::forward,
                        Self::next_transition,
                        Self::forward_transition,
                    );
                });
            });
        }
    }
    fn forward_to_system<'w, 's>(
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
                    if Self::FORWARD_TO_SYSTEM {
                        log.forward_to::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::forward_to_transition_or_limit,
                            Self::next_transition,
                            Self::forward_transition,
                            Self::debug,
                        );
                    } else {
                        log.forward::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::forward,
                            Self::next_transition,
                            Self::forward_transition,
                        );
                    }
                });
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                commands.command_scope(|mut commands| {
                    if Self::FORWARD_TO_SYSTEM {
                        log.forward_to::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::forward_to_transition_or_limit,
                            Self::next_transition,
                            Self::forward_transition,
                            Self::debug,
                        );
                    } else {
                        log.forward::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                            &params,
                            &mut items,
                            &states,
                            &controller,
                            &mut commands,
                            Self::forward,
                            Self::next_transition,
                            Self::forward_transition,
                        );
                    }
                });
            });
        }
    }
    fn forward_log_system<'w, 's>(
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
                log.forward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::forward,
                    Self::forward_transition,
                )
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                log.forward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::forward,
                    Self::forward_transition,
                )
            });
        }
    }
    fn forward_log_to_system<'w, 's, const INIT: bool>(
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
                if Self::FORWARD_TO_SYSTEM {
                    log.forward_log_to::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>, INIT>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::forward_to_transition_or_limit,
                        Self::forward_transition,
                        Self::debug,
                    );
                } else {
                    log.forward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::forward,
                        Self::forward_transition,
                    );
                }
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                if Self::FORWARD_TO_SYSTEM {
                    log.forward_log_to::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>, INIT>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::forward_to_transition_or_limit,
                        Self::forward_transition,
                        Self::debug,
                    );
                } else {
                    log.forward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::forward,
                        Self::forward_transition,
                    );
                }
            });
        }
    }
    fn backward_log_system<'w, 's>(
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
                log.backward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::backward,
                    Self::backward_transition,
                )
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                log.backward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                    &params,
                    &mut items,
                    &states,
                    &controller,
                    Self::backward,
                    Self::backward_transition,
                )
            });
        }
    }
    fn backward_to_system<'w, 's, const INIT: bool>(
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
                if Self::BACKWARD_TO_SYSTEM {
                    log.backward_log_to::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>, INIT>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::backward_to_transition_or_limit,
                        Self::backward_transition,
                    );
                } else {
                    log.backward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::backward,
                        Self::backward_transition,
                    );
                }
            });
        } else {
            query.par_for_each_mut(Self::PAR_ITER_BATCH_SIZE, |(mut items, mut log)| {
                if Self::BACKWARD_TO_SYSTEM {
                    log.backward_log_to::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>, INIT>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::backward_to_transition_or_limit,
                        Self::backward_transition,
                    );
                } else {
                    log.backward_log::<Self::State, <Self as PerEntity>::Params<'_, '_>, QueryItem<'_, <Self as PerEntity>::Items>>(
                        &params,
                        &mut items,
                        &states,
                        &controller,
                        Self::backward,
                        Self::backward_transition,
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
                log.log_close();
            });
        } else {
            query.par_for_each_mut(LOG_ONLY_PAR_ITER_BATCH_SIZE, |mut log| {
                log.log_close();
            });
        }
    }
}

impl<T: PerEntity> PerEntitySystems for T {}
