//! This module manages reversible structural operations on components and resources.
//!
//! It is attempted to be as tolerant towards problems as possible here. For example when at
//! [`NotLog`](super::NotLog) a component is inserted that is new to an entity, and after undoing
//! this and attempting to redo the insertion, an unexpected value at the entity is not overwritten
//! but instead swapped with the to-insert value. These are the `unexpected_swap` methods below that
//! log a warning.

use alloc::vec::Vec;
use bevy_ecs::{
    change_detection::MaybeLocation,
    component::{Component, ComponentId},
    entity::Entity,
    resource::Resource,
    world::{World, error::EntityMutableFetchError},
};
use bevy_log::{error, info, warn};
use core::{any::type_name, error::Error, marker::PhantomData, mem::swap};

use crate::{prelude::UndoRedo, undo_redo::LOCATION_PREFIX};

mod bundle;
#[cfg(test)]
mod test;

pub(super) use bundle::*; // do not make pub to keep RevBundle sealed

struct InnerComponentBuffer<C> {
    entity: Entity,
    buffer: Option<C>,
    caller: MaybeLocation, // todo https://github.com/bevyengine/bevy/issues/20494
}

impl<C> InnerComponentBuffer<C> {
    #[inline(never)]
    fn unexpected_swap(&self, undo_redo: &str, op: &str) {
        warn!(
            "{undo_redo} reversible {op} of {} for {}{LOCATION_PREFIX}{} succeeded but encountered unexpected value in entity that was not present initially which was now swapped with",
            type_name::<C>(),
            self.entity,
            self.caller
        );
    }
    #[inline(never)]
    fn error(&self, undo_redo: &str, op: &str, msg: &str) {
        error!(
            "{undo_redo} reversible {op} of component {} for {}{LOCATION_PREFIX}{} {msg}, this may also have been in an invalid state from earlier error before",
            type_name::<C>(),
            self.entity,
            self.caller
        );
    }
    #[inline(never)]
    fn follow_up_error(&self, undo_redo: &str, op: &str) {
        info!(
            "{undo_redo} reversible {op} of component {} for {}{LOCATION_PREFIX}{} was applied but was in invalid state from an earlier error",
            type_name::<C>(),
            self.entity,
            self.caller
        );
    }
    #[inline(never)]
    fn entity_err(&self, undo_redo: &str, op: &str, err: EntityMutableFetchError) {
        entity_err::<C>(undo_redo, op, self.caller, err);
    }
}

fn entity_err<C>(undo_redo: &str, op: &str, caller: MaybeLocation, err: impl Error) {
    error!(
        "{undo_redo} reversible {op} of component {}{LOCATION_PREFIX}{caller} failed, {err}",
        type_name::<C>()
    );
}

struct InnerResourceBuffer<R> {
    buffer: Option<R>,
    caller: MaybeLocation, // todo https://github.com/bevyengine/bevy/issues/20494
}

impl<R> InnerResourceBuffer<R> {
    #[inline(never)]
    fn unexpected_swap(&self, undo_redo: &str, op: &str) {
        warn!(
            "{undo_redo} reversible {op} of resource {} {LOCATION_PREFIX}{} succeeded but encountered unexpected value in world that was not present initially which was now swapped with",
            type_name::<R>(),
            self.caller
        );
    }
    #[inline(never)]
    fn error(&self, undo_redo: &str, op: &str, msg: &str) {
        error!(
            "{undo_redo} reversible {op} of resource {} {LOCATION_PREFIX}{} {msg}, this may also have been in an invalid state from earlier error before",
            type_name::<R>(),
            self.caller
        );
    }
    #[inline(never)]
    fn follow_up_error(&self, undo_redo: &str, op: &str) {
        info!(
            "{undo_redo} reversible {op} of resource {} {LOCATION_PREFIX}{} was applied but was in invalid state from an earlier error",
            type_name::<R>(),
            self.caller
        );
    }
}

#[derive(PartialEq, Debug)]
enum ToggleResult {
    Noop,
    Taken,
    Inserted,
    Swapped,
}

impl<C: Component> InnerComponentBuffer<C> {
    fn toggle_component(
        &mut self,
        world: &mut World,
    ) -> Result<ToggleResult, EntityMutableFetchError> {
        world.get_entity_mut(self.entity).map(|mut entity| {
            match self.buffer.as_mut() {
                None => {
                    self.buffer = entity.take::<C>();
                    match self.buffer {
                        Some(_) => ToggleResult::Taken,
                        None => ToggleResult::Noop,
                    }
                }
                Some(c1) => entity
                    .modify_component(|c2| {
                        swap(c1, c2);
                        ToggleResult::Swapped
                    })
                    .unwrap_or_else(|| {
                        // SAFETY: Some branch ensures successful unwrap
                        let c = unsafe { self.buffer.take().unwrap_unchecked() };
                        entity.insert(c);
                        ToggleResult::Inserted
                    }),
            }
        })
    }
}

impl<R: Resource> InnerResourceBuffer<R> {
    fn toggle_resource(&mut self, world: &mut World) -> ToggleResult {
        match self.buffer.as_mut() {
            None => {
                self.buffer = world.remove_resource::<R>();
                match self.buffer {
                    Some(_) => ToggleResult::Taken,
                    None => ToggleResult::Noop,
                }
            }
            Some(r1) => world
                .get_resource_mut::<R>()
                .map(|mut r2| {
                    swap(r1, &mut *r2);
                    ToggleResult::Swapped
                })
                .unwrap_or_else(|| {
                    // SAFETY: Some branch ensures successful unwrap
                    let r = unsafe { self.buffer.take().unwrap_unchecked() };
                    world.insert_resource(r);
                    ToggleResult::Inserted
                }),
        }
    }
}

const UNDO: &str = "undo";
const REDO: &str = "redo";
const INSERT: &str = "insert";
const REMOVE: &str = "remove";

struct RevInsertComponentNew<C>(InnerComponentBuffer<C>);
struct RevInsertComponentOverwrite<C>(InnerComponentBuffer<C>);
struct RevRemoveComponent<C>(InnerComponentBuffer<C>);
pub(super) struct RevInsertResourceNew<R>(InnerResourceBuffer<R>);
pub(super) struct RevInsertResourceOverwrite<R>(InnerResourceBuffer<R>);
pub(super) struct RevRemoveResource<R>(InnerResourceBuffer<R>);

impl<C: Component> UndoRedo for RevInsertComponentNew<C> {
    fn undo(&mut self, world: &mut World) {
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Taken) => {} // expected result
            Ok(ToggleResult::Noop) => self.0.error(UNDO, INSERT, "failed, initially inserted value is now missing in entity and could not be taken back"),
            Ok(ToggleResult::Inserted) | Ok(ToggleResult::Swapped) => self.0.follow_up_error(UNDO, INSERT),
            Err(err) => self.0.entity_err(UNDO, INSERT, err)
        }
    }
    fn redo(&mut self, world: &mut World) {
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Inserted) => {} // expected result
            Ok(ToggleResult::Swapped) => self.0.unexpected_swap(REDO, INSERT),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => {
                self.0.follow_up_error(REDO, INSERT)
            }
            Err(err) => self.0.entity_err(REDO, INSERT, err),
        }
    }
}

impl<C: Component> UndoRedo for RevInsertComponentOverwrite<C> {
    fn undo(&mut self, world: &mut World) {
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Swapped) => {} // expected result
            Ok(ToggleResult::Inserted) => self.0.error(UNDO, INSERT, "failed, initially inserted value is now missing in entity and could not be swapped with, only reinserted initially overwritten value"),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => self.0.follow_up_error(UNDO, INSERT),
            Err(err) => self.0.entity_err(UNDO, INSERT, err),
        }
    }
    fn redo(&mut self, world: &mut World) {
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Swapped) => {} // expected result
            Ok(ToggleResult::Inserted) => self.0.error(REDO, INSERT, "failed, initially overwritten and at undo reinserted value is now missing in entity and could not be swapped with, only reinserted initially inserted value"),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => self.0.follow_up_error(REDO, INSERT),
            Err(err) => self.0.entity_err(REDO, INSERT, err),
        }
    }
}

impl<C: Component> UndoRedo for RevRemoveComponent<C> {
    fn undo(&mut self, world: &mut World) {
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Inserted) => {} // expected result
            Ok(ToggleResult::Swapped) => self.0.unexpected_swap(UNDO, REMOVE),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => {
                self.0.follow_up_error(UNDO, REMOVE)
            }
            Err(err) => self.0.entity_err(UNDO, REMOVE, err),
        }
    }
    fn redo(&mut self, world: &mut World) {
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Taken) => {} // expected result
            Ok(ToggleResult::Noop) => self.0.error(REDO, REMOVE, "failed, initially removed and at undo reinserted value is now missing in entity and could not be taken back"),
            Ok(ToggleResult::Inserted) | Ok(ToggleResult::Swapped) => self.0.follow_up_error(REDO, REMOVE),
            Err(err) => self.0.entity_err(REDO, REMOVE, err),
        }
    }
}

impl<R: Resource> UndoRedo for RevInsertResourceNew<R> {
    fn undo(&mut self, world: &mut World) {
        match self.0.toggle_resource(world) {
            ToggleResult::Taken => {} // expected result
            ToggleResult::Noop => self.0.error(UNDO, INSERT, "failed, initially inserted value is now missing in world and could not be taken back"),
            ToggleResult::Inserted | ToggleResult::Swapped => self.0.follow_up_error(UNDO, INSERT),
        }
    }
    fn redo(&mut self, world: &mut World) {
        match self.0.toggle_resource(world) {
            ToggleResult::Inserted => {} // expected result
            ToggleResult::Swapped => self.0.unexpected_swap(REDO, INSERT),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(REDO, INSERT),
        }
    }
}

impl<R: Resource> UndoRedo for RevInsertResourceOverwrite<R> {
    fn undo(&mut self, world: &mut World) {
        match self.0.toggle_resource(world) {
            ToggleResult::Swapped => {} // expected result
            ToggleResult::Inserted => self.0.error(UNDO, INSERT, "failed, initially inserted value is now missing in world and could not be swapped with, only reinserted initially overwritten value"),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(UNDO, INSERT),
        }
    }
    fn redo(&mut self, world: &mut World) {
        match self.0.toggle_resource(world) {
            ToggleResult::Swapped => {} // expected result
            ToggleResult::Inserted => self.0.error(REDO, INSERT, "failed, initially overwritten and at undo reinserted value is now missing in world and could not be swapped with, only reinserted initially inserted value"),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(REDO, INSERT),
        }
    }
}

impl<R: Resource> UndoRedo for RevRemoveResource<R> {
    fn undo(&mut self, world: &mut World) {
        match self.0.toggle_resource(world) {
            ToggleResult::Inserted => {} // expected result
            ToggleResult::Swapped => self.0.unexpected_swap(UNDO, REMOVE),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(UNDO, REMOVE),
        }
    }
    fn redo(&mut self, world: &mut World) {
        match self.0.toggle_resource(world) {
            ToggleResult::Taken => {} // expected result
            ToggleResult::Noop => self.0.error(REDO, REMOVE, "failed, initially removed and at undo reinserted value is now missing in world and could not be taken back"),
            ToggleResult::Inserted | ToggleResult::Swapped => self.0.follow_up_error(REDO, REMOVE),
        }
    }
}

impl<R: Resource> RevInsertResourceNew<R> {
    #[track_caller]
    pub(super) fn new(caller: MaybeLocation) -> Self {
        Self(InnerResourceBuffer {
            buffer: None,
            caller,
        })
    }
}

impl<R: Resource> RevInsertResourceOverwrite<R> {
    #[track_caller]
    pub(super) fn new(resource: R, caller: MaybeLocation) -> Self {
        Self(InnerResourceBuffer {
            buffer: Some(resource),
            caller,
        })
    }
}

impl<R: Resource> RevRemoveResource<R> {
    #[track_caller]
    pub(super) fn new(resource: R, caller: MaybeLocation) -> Self {
        Self(InnerResourceBuffer {
            buffer: Some(resource),
            caller,
        })
    }
}

struct RevNewRequired<T> {
    entity: Entity,
    new_required: Vec<ComponentId>,
    caller: MaybeLocation,
    _p: PhantomData<T>,
}

impl<T: Send + 'static> UndoRedo for RevNewRequired<T> {
    fn undo(&mut self, world: &mut World) {
        match world.get_entity_mut(self.entity) {
            Ok(mut entity) => {
                entity.remove_by_ids(&self.new_required);
            }
            Err(EntityMutableFetchError::NotSpawned(err)) => {
                entity_err::<T>(UNDO, INSERT, self.caller, err)
            }
            // only one entity is fetched
            Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!(),
        }
    }
    fn redo(&mut self, _: &mut World) {
        // required components are reinserted by UndoRedo::redo of the requiring type
    }
}
