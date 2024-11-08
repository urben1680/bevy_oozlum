use std::{collections::VecDeque, marker::PhantomData, ops::Deref};

use bevy::{
    ecs::{
        component::ComponentId,
        entity::Entity,
        event::Event,
        observer::{TriggerEvent, TriggerTargets},
        system::Resource,
        world::World,
    },
    log::error_once,
    utils::default,
};

use crate::{
    commands::{RevCommand, RevCommandLog},
    error_per_flag,
    log::TransitionLog,
    meta::{RevDirection, RevMeta},
    RevFrame,
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

// currently (bevy 0.15) only either one or the other can be triggered
#[derive(Debug, Clone, Copy)]
enum TriggerTargetsCount {
    Components(usize),
    Entities(usize),
}

struct TargetLog<T> {
    log: VecDeque<T>,
    index: usize,
}

impl<T> Default for TargetLog<T> {
    fn default() -> Self {
        Self {
            log: default(),
            index: default(),
        }
    }
}

impl<T: Copy> TargetLog<T> {
    fn drain(&mut self, count: usize) {
        self.index -= count;
        self.log.drain(..count);
    }
    fn extend(
        &mut self,
        iter: impl ExactSizeIterator<Item = T>,
        map: impl FnOnce(usize) -> TriggerTargetsCount,
    ) -> TriggerTargetsCount {
        let len = iter.len();
        self.log.truncate(self.index);
        self.log.extend(iter);
        self.index += len;
        map(len)
    }
    fn forward(&mut self, count: usize) -> Vec<T> {
        let from = self.index;
        self.index += count;
        self.log.range(from..self.index).copied().collect()
    }
    fn backward(&mut self, count: usize) -> Vec<T> {
        let to = self.index;
        self.index -= count;
        self.log.range(self.index..to).copied().collect()
    }
}

#[derive(Resource)]
pub(crate) struct ObserverLog<E> {
    components_log: TargetLog<ComponentId>,
    entities_log: TargetLog<Entity>,
    counts_log: TransitionLog<(E, TriggerTargetsCount, RevFrame)>,
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
            rev_meta_err: default(),
            out_of_log_err: default(),
            direction_err: default(),
        }
    }
}

impl<E: Event + Clone> ObserverLog<E> {
    fn undo_redo(&mut self, world: &mut World, undo: bool) {
        let doing = if undo { "Undoing" } else { "Redoing" };
        let Some(meta) = world.get_resource::<RevMeta>().cloned() else {
            return error_per_flag!(
                &mut self.rev_meta_err,
                "{doing} event {} trigger failed, could not find RevMeta, \
                future triggers likely are applied at the wrong frames from now on",
                std::any::type_name::<E>()
            );
        };
        let transition = if undo {
            self.counts_log.backward_log()
        } else {
            self.counts_log.forward_log()
        };
        let Ok((event, count, logged_at)) = transition.cloned() else {
            return error_per_flag!(
                &mut self.out_of_log_err,
                "{doing} event {} trigger failed, reached end of internal log, \
                future triggers likely are applied at the wrong frames from now on, \
                this is a crate bug\n{meta:?}",
                std::any::type_name::<E>()
            );
        };
        if undo {
            debug_assert_eq!(
                logged_at.wrapping_sub(1),
                meta.present_world_state(),
                "todo"
            );
            let event = RevEvent {
                event,
                direction: RevDirection::BackwardLog,
            };
            match count {
                TriggerTargetsCount::Components(count) => {
                    let targets = self.components_log.backward(count);
                    world.trigger_targets(event, targets);
                }
                TriggerTargetsCount::Entities(count) => {
                    let targets = self.entities_log.backward(count);
                    world.trigger_targets(event, targets);
                }
            }
        } else {
            debug_assert_eq!(logged_at, meta.present_world_state(), "todo");
            let event = RevEvent {
                event,
                direction: RevDirection::ForwardLog,
            };
            match count {
                TriggerTargetsCount::Components(count) => {
                    let targets = self.components_log.forward(count);
                    world.trigger_targets(event, targets);
                }
                TriggerTargetsCount::Entities(count) => {
                    let targets = self.entities_log.forward(count);
                    world.trigger_targets(event, targets);
                }
            }
        }
    }
}

struct RevEventInitialized<E>(PhantomData<E>);

impl<E: Event + Clone, Targets: TriggerTargets> RevCommand<()> for TriggerEvent<E, Targets> {
    fn rev_apply(self, world: &mut World) -> Option<impl RevCommandLog> {
        apply_trigger_event(self, world)
    }
}

pub(crate) fn apply_trigger_event<E: Event + Clone, Targets: TriggerTargets>(
    event: TriggerEvent<E, Targets>,
    world: &mut World, // triggering observers on DeferedWorld also works through commands to get &mut World
) -> Option<impl RevCommandLog> {
    let meta = world.get_resource::<RevMeta>().cloned();
    let mut log = world
        .get_resource_mut::<ObserverLog<E>>()
        .unwrap_or_else(|| {
            panic!(
                "Could not find internal observer log for event `{}`, use `world.rev_observe` \
                to make use of reversible observers instead of `world.observe`.",
                std::any::type_name::<E>()
            )
        });
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
    // todo muss über observer gemacht werden, an dieser stelle könnte zu viel Zeit vergangen sein
    match log.counts_log.pop_past_by_logged_at(&meta) {
        Some((_, TriggerTargetsCount::Components(count), _)) => {
            log.components_log.drain(count);
        }
        Some((_, TriggerTargetsCount::Entities(count), _)) => {
            log.entities_log.drain(count);
        }
        None => {}
    }
    let count = if event.targets.entities().len() == 0 {
        log.components_log
            .extend(event.targets.components(), TriggerTargetsCount::Components)
    } else {
        log.entities_log
            .extend(event.targets.entities(), TriggerTargetsCount::Entities)
    };
    log.counts_log
        .push_present((event.event.clone(), count, meta.present_world_state()));

    world.trigger_targets(
        RevEvent {
            event: event.event,
            direction: RevDirection::NotLog,
        },
        event.targets,
    );

    Some(RevEventInitialized(PhantomData::<E>))
}

impl<E: Event + Clone> RevEventInitialized<E> {
    fn undo_redo(world: &mut World, undo: bool) {
        let Some(mut log) = world.remove_resource::<ObserverLog<E>>() else {
            let meta = world.get_resource::<RevMeta>();
            let doing = if undo { "Undoing" } else { "Redoing" };
            return error_once!(
                "{doing} event {} trigger failed, could not find specific log resource, \
                this might be or will be the case for other event types as well\n{meta:?}",
                std::any::type_name::<E>()
            );
        };
        log.undo_redo(world, undo);
        world.insert_resource(log);
    }
}

impl<E: Event + Clone> RevCommandLog for RevEventInitialized<E> {
    fn undo(&mut self, world: &mut World) {
        Self::undo_redo(world, true)
    }
    fn redo(&mut self, world: &mut World) {
        Self::undo_redo(world, false)
    }
}
