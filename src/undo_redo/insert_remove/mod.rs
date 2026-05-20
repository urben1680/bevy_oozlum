//! This module manages reversible structural operations on components and resources.
//!
//! It is attempted to be as tolerant towards problems as possible here. For example when at
//! [`NotLog`](super::NotLog) a component is inserted that is new to an entity, and after undoing
//! this and attempting to redo the insertion, an unexpected value at the entity is not overwritten
//! but instead swapped with the to-insert value. These are the `unexpected_swap` methods below that
//! log a warning.

use alloc::{format, vec::Vec};
use bevy_ecs::{
    change_detection::MaybeLocation,
    component::{Component, ComponentId},
    entity::Entity,
    error::{BevyError, ErrorContext},
    resource::Resource,
    world::{World, error::EntityMutableFetchError},
};
use bevy_utils::DebugName;
use core::{error::Error, marker::PhantomData, mem::swap};

use crate::undo_redo::{LOCATION_PREFIX, UndoRedo};

mod bundle;
#[cfg(test)]
mod test;

pub(super) use bundle::*; // do not make pub to keep RevBundle sealed

struct InnerComponentBuffer<C> {
    entity: Entity,
    buffer: Option<C>,
    caller: MaybeLocation,
}

impl<C> InnerComponentBuffer<C> {
    fn unexpected_swap(&self, world: &World, undo_redo: &str, name: DebugName) {
        world.fallback_error_handler()(
            BevyError::warning(format!(
                "{undo_redo} reversible {name} for {}{LOCATION_PREFIX}{} succeeded but encountered \
                unexpected value in entity that was not present initially which was now swapped with",
                self.entity, self.caller
            )),
            ErrorContext::Command { name },
        )
    }
    fn error(&self, world: &World, undo_redo: &str, name: DebugName, msg: &str) {
        world.fallback_error_handler()(
            BevyError::error(format!(
                "{undo_redo} reversible {name} for {}{LOCATION_PREFIX}{} {msg}, \
                this may also have been in an invalid state from earlier error before",
                self.entity, self.caller
            )),
            ErrorContext::Command { name },
        )
    }
    fn follow_up_error(&self, world: &World, undo_redo: &str, name: DebugName) {
        world.fallback_error_handler()(
            BevyError::info(format!(
                "{undo_redo} reversible {name} for {}{LOCATION_PREFIX}{} \
                was applied but was in invalid state from an earlier error",
                self.entity, self.caller
            )),
            ErrorContext::Command { name },
        )
    }
    fn entity_err(
        &self,
        world: &World,
        undo_redo: &str,
        name: DebugName,
        err: EntityMutableFetchError,
    ) {
        entity_err(world, undo_redo, name, self.caller, err);
    }
}

fn entity_err(
    world: &World,
    undo_redo: &str,
    name: DebugName,
    caller: MaybeLocation,
    err: impl Error,
) {
    world.fallback_error_handler()(
        BevyError::error(format!(
            "{undo_redo} reversible {name}{LOCATION_PREFIX}{caller} failed, {err}",
        )),
        ErrorContext::Command { name },
    )
}

struct InnerResourceBuffer<R> {
    buffer: Option<R>,
    caller: MaybeLocation,
}

impl<R> InnerResourceBuffer<R> {
    fn unexpected_swap(&self, world: &World, undo_redo: &str, name: DebugName) {
        world.fallback_error_handler()(
            BevyError::warning(format!(
                "{undo_redo} reversible {name}{LOCATION_PREFIX}{} succeeded but encountered \
                unexpected value in world that was not present initially which was now swapped with",
                self.caller
            )),
            ErrorContext::Command { name },
        )
    }
    fn error(&self, world: &World, undo_redo: &str, name: DebugName, msg: &str) {
        world.fallback_error_handler()(
            BevyError::error(format!(
                "{undo_redo} reversible {name}{LOCATION_PREFIX}{} {msg}, this \
                may also have been in an invalid state from earlier error before",
                self.caller
            )),
            ErrorContext::Command { name },
        )
    }
    fn follow_up_error(&self, world: &World, undo_redo: &str, name: DebugName) {
        world.fallback_error_handler()(
            BevyError::info(format!(
                "{undo_redo} reversible {name}{LOCATION_PREFIX}{} was \
                applied but was in invalid state from an earlier error",
                self.caller
            )),
            ErrorContext::Command { name },
        )
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
        toggle_component::<C>(self.entity, &mut self.buffer, world)
    }
}

impl<R: Resource> InnerResourceBuffer<R> {
    fn toggle_resource(&mut self, world: &mut World) -> ToggleResult {
        let comonent_id = world.register_component::<R>();
        world
            .resource_entities()
            .get(comonent_id)
            .and_then(|entity| toggle_component::<R>(entity, &mut self.buffer, world).ok())
            .unwrap_or_else(|| match self.buffer.take() {
                Some(resource) => {
                    world.insert_resource(resource);
                    ToggleResult::Inserted
                }
                None => ToggleResult::Noop,
            })
    }
}

fn toggle_component<C: Component>(
    entity: Entity,
    buffer: &mut Option<C>,
    world: &mut World,
) -> Result<ToggleResult, EntityMutableFetchError> {
    world.get_entity_mut(entity).map(|mut entity| {
        match buffer.as_mut() {
            None => {
                *buffer = entity.take::<C>();
                match buffer {
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
                    let c = unsafe { buffer.take().unwrap_unchecked() };
                    entity.insert(c);
                    ToggleResult::Inserted
                }),
        }
    })
}

const UNDO: &str = "undo";
const REDO: &str = "redo";

struct RevInsertComponentNew<C>(InnerComponentBuffer<C>);
struct RevInsertComponentOverwrite<C>(InnerComponentBuffer<C>);
struct RevRemoveComponent<C>(InnerComponentBuffer<C>);
pub(super) struct RevInsertResourceNew<R>(InnerResourceBuffer<R>);
pub(super) struct RevInsertResourceOverwrite<R>(InnerResourceBuffer<R>);
pub(super) struct RevRemoveResource<R>(InnerResourceBuffer<R>);

impl<C: Component> UndoRedo for RevInsertComponentNew<C> {
    fn undo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Taken) => {} // expected result
            Ok(ToggleResult::Noop) => self.0.error(
                world,
                UNDO,
                name,
                "failed, initially inserted value is now \
                missing in entity and could not be taken back",
            ),
            Ok(ToggleResult::Inserted) | Ok(ToggleResult::Swapped) => {
                self.0.follow_up_error(world, UNDO, name)
            }
            Err(err) => self.0.entity_err(world, UNDO, name, err),
        }
    }
    fn redo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Inserted) => {} // expected result
            Ok(ToggleResult::Swapped) => self.0.unexpected_swap(world, REDO, name),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => {
                self.0.follow_up_error(world, REDO, name)
            }
            Err(err) => self.0.entity_err(world, REDO, name, err),
        }
    }
}

impl<C: Component> UndoRedo for RevInsertComponentOverwrite<C> {
    fn undo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Swapped) => {} // expected result
            Ok(ToggleResult::Inserted) => self.0.error(
                world,
                UNDO,
                name,
                "failed, initially inserted value is now missing in entity and could \
                not be swapped with, only reinserted initially overwritten value",
            ),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => {
                self.0.follow_up_error(world, UNDO, name)
            }
            Err(err) => self.0.entity_err(world, UNDO, name, err),
        }
    }
    fn redo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Swapped) => {} // expected result
            Ok(ToggleResult::Inserted) => self.0.error(
                world,
                REDO,
                name,
                "failed, initially overwritten and at undo reinserted value is now missing \
                in entity and could not be swapped with, only reinserted initially inserted value",
            ),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => {
                self.0.follow_up_error(world, REDO, name)
            }
            Err(err) => self.0.entity_err(world, REDO, name, err),
        }
    }
}

impl<C: Component> UndoRedo for RevRemoveComponent<C> {
    fn undo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Inserted) => {} // expected result
            Ok(ToggleResult::Swapped) => self.0.unexpected_swap(world, UNDO, name),
            Ok(ToggleResult::Taken) | Ok(ToggleResult::Noop) => {
                self.0.follow_up_error(world, UNDO, name)
            }
            Err(err) => self.0.entity_err(world, UNDO, name, err),
        }
    }
    fn redo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_component(world) {
            Ok(ToggleResult::Taken) => {} // expected result
            Ok(ToggleResult::Noop) => self.0.error(
                world,
                REDO,
                name,
                "failed, initially removed and at undo reinserted value \
                is now missing in entity and could not be taken back",
            ),
            Ok(ToggleResult::Inserted) | Ok(ToggleResult::Swapped) => {
                self.0.follow_up_error(world, REDO, name)
            }
            Err(err) => self.0.entity_err(world, REDO, name, err),
        }
    }
}

impl<R: Resource> UndoRedo for RevInsertResourceNew<R> {
    fn undo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_resource(world) {
            ToggleResult::Taken => {} // expected result
            ToggleResult::Noop => self.0.error(
                world,
                UNDO,
                name,
                "failed, initially inserted value is now \
                missing in world and could not be taken back",
            ),
            ToggleResult::Inserted | ToggleResult::Swapped => {
                self.0.follow_up_error(world, UNDO, name)
            }
        }
    }
    fn redo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_resource(world) {
            ToggleResult::Inserted => {} // expected result
            ToggleResult::Swapped => self.0.unexpected_swap(world, REDO, name),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(world, REDO, name),
        }
    }
}

impl<R: Resource> UndoRedo for RevInsertResourceOverwrite<R> {
    fn undo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_resource(world) {
            ToggleResult::Swapped => {} // expected result
            ToggleResult::Inserted => self.0.error(
                world,
                UNDO,
                name,
                "failed, initially inserted value is now missing in world and could \
                not be swapped with, only reinserted initially overwritten value",
            ),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(world, UNDO, name),
        }
    }
    fn redo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_resource(world) {
            ToggleResult::Swapped => {} // expected result
            ToggleResult::Inserted => self.0.error(
                world,
                REDO,
                name,
                "failed, initially overwritten and at undo reinserted value is now missing in \
                world and could not be swapped with, only reinserted initially inserted value",
            ),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(world, REDO, name),
        }
    }
}

impl<R: Resource> UndoRedo for RevRemoveResource<R> {
    fn undo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_resource(world) {
            ToggleResult::Inserted => {} // expected result
            ToggleResult::Swapped => self.0.unexpected_swap(world, UNDO, name),
            ToggleResult::Taken | ToggleResult::Noop => self.0.follow_up_error(world, UNDO, name),
        }
    }
    fn redo(&mut self, world: &mut World) {
        let name = DebugName::type_name::<Self>();
        match self.0.toggle_resource(world) {
            ToggleResult::Taken => {} // expected result
            ToggleResult::Noop => self.0.error(
                world,
                REDO,
                name,
                "failed, initially removed and at undo reinserted value \
                is now missing in world and could not be taken back",
            ),
            ToggleResult::Inserted | ToggleResult::Swapped => {
                self.0.follow_up_error(world, REDO, name)
            }
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
                let name = DebugName::type_name::<Self>();
                entity_err(world, UNDO, name, self.caller, err)
            }
            // only one entity is fetched
            Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!(),
        }
    }
    fn redo(&mut self, _: &mut World) {
        // required components are reinserted by UndoRedo::redo of the requiring type
    }
}
