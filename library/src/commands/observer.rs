use std::{marker::PhantomData, ops::Deref};

use bevy::{
    ecs::{
        change_detection::Mut,
        event::Event,
        observer::{TriggerEvent, TriggerTargets},
        system::Resource,
        world::World,
    },
    log::error_once,
};

use crate::{
    commands::{InitializedRevCommand, RevCommand},
    error_per_flag,
    log::{OutOfLog, TransitionsLog, WithLoggedAt},
    meta::{Direction, RevMeta},
};

#[derive(Event)]
pub struct RevEvent<E: Event> {
    event: E,
    pub direction: Direction,
}

impl<E: Event> Deref for RevEvent<E> {
    type Target = E;
    fn deref(&self) -> &Self::Target {
        &self.event
    }
}

#[derive(Resource)]
struct ObserverLog<E> {
    log: TransitionsLog<TriggerTargetData, WithLoggedAt<(TriggerTargetTag, E)>>,
    rev_meta_err: bool,
    out_of_log_err: bool,
    direction_err: bool,
}

impl<E> Default for ObserverLog<E> {
    fn default() -> Self {
        Self {
            log: Default::default(),
            rev_meta_err: false,
            out_of_log_err: false,
            direction_err: false,
        }
    }
}

struct RevEventInitialized<E>(PhantomData<E>);

impl<E: Event + Clone, Targets: TriggerTargets> RevCommand<()> for TriggerEvent<E, Targets> {
    fn rev_apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        let meta = world.get_resource::<RevMeta>().cloned();
        let mut log = world.get_resource_or_insert_with::<ObserverLog<E>>(Default::default);
        let Some(meta) = meta else {
            return error_per_flag!(
                &mut log.rev_meta_err,
                "Initial event {} trigger failed, could not find RevMeta, \
                future triggers likely are applied at the wrong frames from now on",
                std::any::type_name::<E>()
            );
        };
        if meta.get_direction() != Some(Direction::Forward { log: false }) {
            return error_per_flag!(
                &mut log.direction_err,
                "Initial event {} trigger failed, RevMeta is not in the non-log Direction::Forward, \
                future triggers likely are applied at the wrong frames from now on\n{meta:?}",
                std::any::type_name::<E>()
            );
        }

        let _ = log.log.drain_past_by_timestamp(meta.log_range().start);
        log.log.push_present(|mut log| {
            let components = self.targets.components();
            let entities = self.targets.entities();
            match (components.len(), entities.len()) {
                (0, 0) => meta.with_timestamp((TriggerTargetTag::COMPONENT, self.event.clone())),
                (0, _) => {
                    log.extend(entities.map(TriggerTargetData::from));
                    meta.with_timestamp((TriggerTargetTag::ENTITY, self.event.clone()))
                }
                (_, 0) => {
                    log.extend(components.map(TriggerTargetData::from));
                    meta.with_timestamp((TriggerTargetTag::COMPONENT, self.event.clone()))
                }
                (_, _) => unimplemented!(
                    "consider to support both as users can implement TriggerTargets for both as well"
                ),
            }
        });

        world.trigger_targets(
            RevEvent {
                event: self.event,
                direction: Direction::Forward { log: false },
            },
            self.targets,
        );

        Some(RevEventInitialized(PhantomData::<E>))
    }
}

impl<E: Event + Clone> RevEventInitialized<E> {
    fn undo_redo(&self, world: &mut World, undo: bool) {
        let doing = if undo { "Undoing" } else { "Redoing" };
        let meta = world.get_resource::<RevMeta>().cloned();
        if !world.contains_resource::<ObserverLog<E>>() {
            match meta {
                None => error_once!(
                    "{doing} event {} trigger failed, could not find RevMeta and specific \
                    log resource, this might be or will be the case for other event types as well",
                    std::any::type_name::<E>()
                ),
                Some(meta) => error_once!(
                    "{doing} event {} trigger failed, could not find specific log resource, \
                    this might be or will be the case for other event types as well\n{meta:?}",
                    std::any::type_name::<E>()
                ),
            }
            return;
        }
        world.resource_scope(|world, mut resource: Mut<ObserverLog<E>>| {
            match meta {
                Some(meta) => match meta.get_direction() {
                    Some(Direction::BackwardLog) => {
                        let result = if undo {
                            resource.log.backward_log().map(|entry| {
                                let direction = Direction::BackwardLog;
                                trigger(world, entry, direction);
                            })
                        } else {
                            resource.log.forward_log().map(|entry| {
                                let direction = Direction::Forward { log: true };
                                trigger(world, entry, direction);
                            })
                        };
                        if result == Err(OutOfLog) {
                            error_per_flag!(
                                &mut resource.out_of_log_err,
                                "{doing} event {} trigger failed, reached end of internal log, \
                                future triggers likely are applied at the wrong frames from now on, \
                                this is a crate bug\n{meta:?}",
                                std::any::type_name::<E>()
                            )
                        }
                    },
                    _ => error_per_flag!(
                        &mut resource.direction_err,
                        "{doing} event {} trigger failed, RevMeta is not in Direction::Backward, \
                        future triggers likely are applied at the wrong frames from now on, \
                        this is a crate bug\n{meta:?}",
                        std::any::type_name::<E>()
                    )
                },
                None => error_per_flag!(
                    &mut resource.rev_meta_err,
                    "{doing} event {} trigger failed, could not find RevMeta, \
                    future triggers likely are applied at the wrong frames from now on",
                    std::any::type_name::<E>()
                )
            }
        })
    }
}

impl<E: Event + Clone> InitializedRevCommand for RevEventInitialized<E> {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo(world, true)
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo(world, false)
    }
}

use trigger_target::*;

#[cfg(debug_assertions)]
mod trigger_target {
    use bevy::ecs::{component::ComponentId, entity::Entity, event::Event, world::World};

    use crate::{
        log::{LogIter, ValueEntry, WithLoggedAt},
        meta::Direction,
    };

    use super::RevEvent;

    pub(super) struct TriggerTargetTag;

    impl TriggerTargetTag {
        pub(super) const COMPONENT: Self = Self;
        pub(super) const ENTITY: Self = Self;
    }

    #[derive(Clone, Copy)]
    pub(super) enum TriggerTargetData {
        Component(ComponentId),
        Entity(Entity),
    }

    impl From<ComponentId> for TriggerTargetData {
        fn from(component: ComponentId) -> Self {
            Self::Component(component)
        }
    }

    impl From<Entity> for TriggerTargetData {
        fn from(entity: Entity) -> Self {
            Self::Entity(entity)
        }
    }

    pub(super) fn trigger<'a>(
        world: &mut World,
        entry: ValueEntry<
            impl LogIter<'a, &'a mut TriggerTargetData>,
            &mut WithLoggedAt<(TriggerTargetTag, impl Event + Clone)>,
        >,
        direction: Direction,
    ) {
        let ValueEntry {
            value: mut iter,
            entry: WithLoggedAt {
                value: (_, event), ..
            },
        } = entry;

        let event = RevEvent {
            event: event.clone(),
            direction,
        };

        let len = iter.len();
        if len == 0 {
            return world.trigger(event);
        }

        match iter.next().unwrap() {
            TriggerTargetData::Component(id) => {
                let iter = iter.map(|data| match data {
                    TriggerTargetData::Component(id) => *id,
                    _ => panic!(
                        "Overlapping triggers of both components and entities, this is a crate bug"
                    ),
                });
                let mut targets = Vec::with_capacity(len);
                targets.push(*id);
                targets.extend(iter);
                world.trigger_targets(event, targets);
            }
            TriggerTargetData::Entity(entity) => {
                let iter = iter.map(|data| match data {
                    TriggerTargetData::Entity(entity) => *entity,
                    _ => panic!(
                        "Overlapping triggers of both components and entities, this is a crate bug"
                    ),
                });
                let mut targets = Vec::with_capacity(len);
                targets.push(*entity);
                targets.extend(iter);
                world.trigger_targets(event, targets);
            }
        }
    }
}

#[cfg(not(debug_assertions))]
mod trigger_target {
    use bevy::ecs::{component::ComponentId, entity::Entity, event::Event, world::World};

    use crate::{
        log::{LogIter, ValueEntry, WithLoggedAt},
        meta::Direction,
    };

    use super::RevEvent;

    pub(super) enum TriggerTargetTag {
        ComponentId,
        Entity,
    }

    impl TriggerTargetTag {
        pub(super) const COMPONENT: Self = Self::ComponentId;
        pub(super) const ENTITY: Self = Self::Entity;
    }

    pub(super) union TriggerTargetData {
        component: ComponentId,
        entity: Entity,
    }

    impl From<ComponentId> for TriggerTargetData {
        fn from(component: ComponentId) -> Self {
            Self { component }
        }
    }

    impl From<Entity> for TriggerTargetData {
        fn from(entity: Entity) -> Self {
            Self { entity }
        }
    }

    pub(super) fn trigger<'a>(
        world: &mut World,
        entry: ValueEntry<
            impl LogIter<'a, &'a mut TriggerTargetData>,
            &mut WithLoggedAt<(TriggerTargetTag, impl Event + Clone)>,
        >,
        direction: Direction,
    ) {
        let ValueEntry {
            value: iter,
            entry:
                WithLoggedAt {
                    value: (tag, event),
                    ..
                },
        } = entry;

        let event = RevEvent {
            event: event.clone(),
            direction,
        };

        if iter.len() == 0 {
            return world.trigger(event);
        }

        match tag {
            TriggerTargetTag::ComponentId => {
                let targets: Vec<ComponentId> = iter
                    .map(|data| unsafe {
                        // SAFETY:
                        // data pushed in this entry consists purely of components
                        data.component
                    })
                    .collect();
                world.trigger_targets(event, targets);
            }
            TriggerTargetTag::Entity => {
                let targets: Vec<Entity> = iter
                    .map(|data| unsafe {
                        // SAFETY:
                        // data pushed in this entry consists purely of entities
                        data.entity
                    })
                    .collect();
                world.trigger_targets(event, targets);
            }
        }
    }
}
