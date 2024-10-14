use std::{collections::VecDeque, marker::PhantomData, ops::Deref};

use bevy::{
    ecs::{
        change_detection::Mut, component::ComponentId, event::Event, observer::{TriggerEvent, TriggerTargets}, system::Resource, world::World
    },
    log::error_once, prelude::{default, Entity},
};

use crate::{
    commands::{InitializedRevCommand, RevCommand},
    error_per_flag,
    log::{LoggedAt, OutOfLog, PackedRevFrame, TransitionLog, TransitionsLog},
    meta::{RevDirection, RevMeta}, RevFrame,
};

#[derive(Event)]
pub struct RevEvent<E: Event + Clone> {
    // Mutations are not logged, therefore nothing is mutably accessible.
    event: E,
    direction: RevDirection,
}

impl<E: Event + Clone> Deref for RevEvent<E> {
    type Target = E;
    fn deref(&self) -> &Self::Target {
        &self.event
    }
}

impl<E: Event + Clone> RevEvent<E> {
    pub fn direction(&self) -> RevDirection {
        self.direction
    }
}

#[derive(Debug, Clone, Copy)]
struct Counts {
    components: usize,
    entities: usize
}

#[derive(Resource)]
struct ObserverLog<E> {
    components_log: VecDeque<ComponentId>,
    entities_log: VecDeque<Entity>,
    counts_log: TransitionLog<(E, Counts, RevFrame)>,
    components_index: usize,
    entities_index: usize,
    rev_meta_err: bool,
    out_of_log_err: bool,
    direction_err: bool,
}

impl<E> Default for ObserverLog<E> {
    fn default() -> Self {
        Self {
            components_log: default(),
            entities_log: default(),
            counts_log: default(),
            components_index: default(),
            entities_index: default(),
            rev_meta_err: default(),
            out_of_log_err: default(),
            direction_err: default(),
        }
    }
}

impl<E: Event + Clone> ObserverLog<E> {
    fn trigger(&mut self, world: &mut World, forward: bool) {
        let transition = match forward {
            true => self.counts_log.forward_log(),
            false => self.counts_log.backward_log()
        };
        let Ok((event, counts, logged_at)) = transition.cloned() else {
            return error_per_flag!(
                &mut self.out_of_log_err,
                "{doing} event {} trigger failed, reached end of internal log, \
                future triggers likely are applied at the wrong frames from now on, \
                this is a crate bug\n{meta:?}",
                std::any::type_name::<E>()
            );
        };
        
    }
}

struct RevEventInitialized<E>(PhantomData<E>);

impl<E: Event + Clone, Targets: TriggerTargets> RevCommand<()> for TriggerEvent<E, Targets> {
    fn rev_apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        let meta = world.get_resource::<RevMeta>().cloned();
        let mut log = world.get_resource_or_insert_with::<ObserverLog<E>>(default);
        let Some(meta) = meta else {
            return error_per_flag!(
                &mut log.rev_meta_err,
                "Initial event {} trigger failed, could not find RevMeta, \
                future triggers likely are applied at the wrong frames from now on",
                std::any::type_name::<E>()
            );
        };
        if meta.get_direction() != Some(RevDirection::NotLog) {
            return error_per_flag!(
                &mut log.direction_err,
                "Initial event {} trigger failed, RevMeta is not in the non-log Direction::Forward, \
                future triggers likely are applied at the wrong frames from now on\n{meta:?}",
                std::any::type_name::<E>()
            );
        }

        let components = self.targets.components();
        let entities = self.targets.entities();
        let components_index = log.components_index;
        let entities_index = log.entities_index;
        log.components_log.truncate(components_index);
        log.entities_log.truncate(entities_index);
        if let Some((_, counts, _)) =  log.counts_log.pop_past_by_logged_at(&meta) {
            log.components_index -= counts.components;
            log.entities_index -= counts.entities;
            log.components_log.drain(..counts.components);
            log.entities_log.drain(..counts.entities);
        }
        let counts = Counts {
            components: components.len(),
            entities: entities.len()
        };
        log.components_index += counts.components;
        log.entities_index += counts.entities;
        log.components_log.extend(components);
        log.entities_log.extend(entities);
        log.counts_log.push_present((self.event.clone(), counts, meta.present_world_state()));

        world.trigger_targets(
            RevEvent {
                event: self.event,
                direction: RevDirection::NotLog,
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
        world.resource_scope(|world, mut log: Mut<ObserverLog<E>>| {
            match meta {
                Some(meta) => match meta.get_direction() {
                    Some(RevDirection::BackwardLog) => {
                        let result = if undo {
                            log.log.backward_log().map(|entry| {
                                let direction = RevDirection::BackwardLog;
                                trigger(world, entry, direction);
                            })
                        } else {
                            log.log.forward_log().map(|entry| {
                                let direction = RevDirection::ForwardLog;
                                trigger(world, entry, direction);
                            })
                        };
                        if result == Err(OutOfLog) {
                            error_per_flag!(
                                &mut log.out_of_log_err,
                                "{doing} event {} trigger failed, reached end of internal log, \
                                future triggers likely are applied at the wrong frames from now on, \
                                this is a crate bug\n{meta:?}",
                                std::any::type_name::<E>()
                            )
                        }
                    },
                    _ => error_per_flag!(
                        &mut log.direction_err,
                        "{doing} event {} trigger failed, RevMeta is not in Direction::Backward, \
                        future triggers likely are applied at the wrong frames from now on, \
                        this is a crate bug\n{meta:?}",
                        std::any::type_name::<E>()
                    )
                },
                None => error_per_flag!(
                    &mut log.rev_meta_err,
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
        log::{LogIter, LoggedAt, ValueEntry},
        meta::RevDirection,
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
            &mut LoggedAt<(TriggerTargetTag, impl Event + Clone)>,
        >,
        direction: RevDirection,
    ) {
        let ValueEntry {
            value: mut iter,
            entry: LoggedAt {
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

#[cfg(not(debug_assertions))] // todo, too hot, make ObserverLog have two logs
mod trigger_target {
    use bevy::ecs::{component::ComponentId, entity::Entity, event::Event, world::World};

    use crate::{
        log::{LogIter, LoggedAt, ValueEntry},
        meta::RevDirection,
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
            &mut LoggedAt<(TriggerTargetTag, impl Event + Clone)>,
        >,
        direction: RevDirection,
    ) {
        let ValueEntry {
            value: iter,
            entry: LoggedAt {
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
