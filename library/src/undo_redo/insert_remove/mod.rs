use core::{
    any::{TypeId, type_name},
    error::Error,
    marker::PhantomData,
    mem::swap,
};

use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    component::{Component, ComponentId},
    entity::Entity,
    relationship::Relationship,
    resource::Resource,
    spawn::{SpawnOneRelated, SpawnRelatedBundle, SpawnableList},
    world::{EntityWorldMut, World, error::EntityMutableFetchError},
};
use bevy_log::{error, info, warn};
use variadics_please::all_tuples;

use crate::{
    meta::MetaPastLen,
    prelude::UndoRedo,
    undo_redo::{
        AddRemoveRelated, BuffersUndoRedo, LOCATION_PREFIX, RevEntityWorldMut, get_new_related,
        get_new_related_entities, mark_entities,
    },
};

#[cfg(test)]
mod test;

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
    fn entity_err(&self, undo_redo: &str, op: &str, err: impl Error) {
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
    ) -> Result<ToggleResult, impl Error + 'static> {
        // todo: test
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
                        entity.insert(unsafe {
                            // SAFETY: Some branch ensures successful unwrap
                            self.buffer.take().unwrap_unchecked()
                        });
                        ToggleResult::Inserted
                    }),
            }
        })
    }
}

impl<R: Resource> InnerResourceBuffer<R> {
    fn toggle_resource(&mut self, world: &mut World) -> ToggleResult {
        // todo: test
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
                    world.insert_resource(unsafe {
                        // SAFETY: Some branch ensures successful unwrap
                        self.buffer.take().unwrap_unchecked()
                    });
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
        // todo: test
        match world.get_entity_mut(self.entity) {
            Ok(mut entity) => {
                entity.remove_by_ids(&self.new_required);
            }
            Err(EntityMutableFetchError::NotSpawned(err)) => {
                entity_err::<T>(UNDO, INSERT, self.caller, err)
            }
            Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!(),
        }
    }
    fn redo(&mut self, _: &mut World) {
        // required components are reinserted by UndoRedo::redo of inserter type
    }
}

pub trait RevBundle<Marker>: Bundle {
    /// Inserts `self` into `entity` depending on `mode`.
    ///
    /// When undone, the inserted components are removed from `entity`, returning overwritten
    /// components, if there were any and [`InsertMode::Replace`] was picked. When redone, the
    /// components are returned to `entity` again, potentially overwriting existing components
    /// again.
    fn rev_insert(
        self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    );

    /// This is called within [`RevBundle::rev_insert`] and should not be called elsewhere as this
    /// alone will not make the insertion of required components reversible.
    #[doc(hidden)]
    fn rev_insert_nested(
        self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    );

    /// Removes `Self` from `entity`.
    ///
    /// When undone, the removed components are returned to `entity`. When redone, they are removed
    /// from `entity` again.
    fn rev_remove(meta_past_len: MetaPastLen, entity: &mut EntityWorldMut, caller: MaybeLocation);
}

fn required_of_component<C: Component>(
    meta_past_len: MetaPastLen,
    entity: &mut EntityWorldMut,
    caller: MaybeLocation,
) {
    let component_id = entity
        .world()
        .component_id::<C>()
        .unwrap_or_else(|| entity.world_scope(|world| world.register_component::<C>()));
    let new_required: Vec<ComponentId> = entity
        .world()
        .get_required_components_by_id(component_id)
        .unwrap() // component registered above if needed
        .iter_ids()
        .filter(|&component_id| !entity.contains_id(component_id))
        .collect();
    required_inner::<C>(meta_past_len, entity, caller, new_required)
}

fn required_of_bundle<B: Bundle>(
    meta_past_len: MetaPastLen,
    entity: &mut EntityWorldMut,
    caller: MaybeLocation,
) {
    let bundle_id = entity
        .world()
        .bundles()
        .get_id(TypeId::of::<B>())
        .unwrap_or_else(|| entity.world_scope(|world| world.register_bundle::<B>().id()));
    let new_required: Vec<ComponentId> = entity
        .world()
        .bundles()
        .get(bundle_id)
        .unwrap() // bundle registered above if needed
        .required_components()
        .iter()
        .copied()
        .filter(|&component_id| !entity.contains_id(component_id))
        .collect();
    required_inner::<B>(meta_past_len, entity, caller, new_required)
}

fn required_inner<T: Send + 'static>(
    meta_past_len: MetaPastLen,
    entity: &mut EntityWorldMut,
    caller: MaybeLocation,
    new_required: Vec<ComponentId>,
) {
    if !new_required.is_empty() {
        entity.buffer_undo_redo_with_caller(
            meta_past_len,
            RevNewRequired::<T> {
                entity: entity.id(),
                new_required,
                caller,
                _p: PhantomData,
            },
            caller,
        )
    }
}

impl RevBundle<()> for () {
    fn rev_insert(self, _: MetaPastLen, _: &mut EntityWorldMut, _: InsertMode, _: MaybeLocation) {}

    fn rev_insert_nested(
        self,
        _: MetaPastLen,
        _: &mut EntityWorldMut,
        _: InsertMode,
        _: MaybeLocation,
    ) {
    }

    fn rev_remove(_: MetaPastLen, _: &mut EntityWorldMut, _: MaybeLocation) {}
}

impl<C: Component> RevBundle<[C; 1]> for C {
    fn rev_insert(
        self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        required_of_component::<C>(meta_past_len, entity, caller);
        self.rev_insert_nested(meta_past_len, entity, mode, caller);
    }

    fn rev_insert_nested(
        mut self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        match mode {
            InsertMode::Keep => {
                if !entity.contains::<C>() {
                    entity.redo_and_buffer_with_caller(
                        meta_past_len,
                        RevInsertComponentNew::<_>(InnerComponentBuffer {
                            entity: entity.id(),
                            buffer: Some(self),
                            caller,
                        }),
                        caller,
                    )
                }
            }
            InsertMode::Replace => {
                let swapped = entity
                    .modify_component(|existing| swap(existing, &mut self))
                    .is_some();
                let inner = InnerComponentBuffer {
                    entity: entity.id(),
                    buffer: Some(self),
                    caller,
                };
                if swapped {
                    entity.buffer_undo_redo_with_caller(
                        meta_past_len,
                        RevInsertComponentOverwrite(inner),
                        caller,
                    );
                } else {
                    entity.redo_and_buffer_with_caller(
                        meta_past_len,
                        RevInsertComponentNew(inner),
                        caller,
                    )
                }
            }
        }
    }

    fn rev_remove(meta_past_len: MetaPastLen, entity: &mut EntityWorldMut, caller: MaybeLocation) {
        if let Some(component) = entity.take::<C>() {
            entity.buffer_undo_redo_with_caller(
                meta_past_len,
                RevRemoveComponent(InnerComponentBuffer {
                    entity: entity.id(),
                    buffer: Some(component),
                    caller,
                }),
                caller,
            );
        }
    }
}

impl<R: Relationship, B: Bundle> RevBundle<[R; 2]> for SpawnOneRelated<R, B> {
    fn rev_insert(
        self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        self.rev_insert_nested(meta_past_len, entity, mode, caller);
    }

    fn rev_insert_nested(
        self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        _mode: InsertMode,
        caller: MaybeLocation,
    ) {
        let new_related = get_new_related::<R>(entity, |entity| entity.insert(self));
        entity.world_scope(|world| {
            if let Ok(mut new_related) = world.get_entity_mut(new_related) {
                new_related.rev_mark_spawned(meta_past_len, true);
            }
        });
        let id = entity.id();
        entity.buffer_undo_redo_with_caller(
            meta_past_len,
            AddRemoveRelated::<R, _, true>::new(id, [new_related], caller),
            caller,
        );
    }

    fn rev_remove(_: MetaPastLen, _: &mut EntityWorldMut, _: MaybeLocation) {}
}

impl<R: Relationship, L: SpawnableList<R> + Send + Sync + 'static> RevBundle<[R; 3]>
    for SpawnRelatedBundle<R, L>
{
    fn rev_insert(
        self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        self.rev_insert_nested(meta_past_len, entity, mode, caller);
    }

    fn rev_insert_nested(
        self,
        meta_past_len: MetaPastLen,
        entity: &mut EntityWorldMut,
        _mode: InsertMode,
        caller: MaybeLocation,
    ) {
        let new_related = get_new_related_entities::<R>(entity, |entity| entity.insert(self));
        entity.world_scope(|world| {
            mark_entities::<true>(
                meta_past_len,
                world,
                &*new_related,
                true,
                MaybeLocation::caller(),
            )
        });
        let id = entity.id();
        entity.buffer_undo_redo_with_caller(
            meta_past_len,
            AddRemoveRelated::<R, _, true>::new(id, new_related, caller),
            caller,
        );
    }

    fn rev_remove(_: MetaPastLen, _: &mut EntityWorldMut, _: MaybeLocation) {}
}

macro_rules! impl_buffer_bundle {
    ($(($T: ident, $M: ident, $var: ident)),*) => {
        impl<$($T, $M),*> RevBundle<($($M,)*)> for ($($T,)*)
        where
            $($T: RevBundle<$M>,)*
        {
            fn rev_insert(
                self,
                meta_past_len: MetaPastLen,
                entity: &mut EntityWorldMut,
                mode: InsertMode,
                caller: MaybeLocation,
            ) {
                required_of_bundle::<Self>(meta_past_len, entity, caller);
                self.rev_insert_nested(meta_past_len, entity, mode, caller);
            }

            fn rev_insert_nested(
                self,
                meta_past_len: MetaPastLen,
                entity: &mut EntityWorldMut,
                mode: InsertMode,
                caller: MaybeLocation,
            ) {
                let ($($var,)*) = self;
                ($($var.rev_insert_nested(meta_past_len, entity, mode, caller),)*);
            }

            fn rev_remove(
                meta_past_len: MetaPastLen,
                entity: &mut EntityWorldMut,
                caller: MaybeLocation
            ) {
                ($(<$T as RevBundle::<$M>>::rev_remove(meta_past_len, entity, caller),)*);
            }
        }
    };
}

all_tuples!(impl_buffer_bundle, 1, 15, T, M, var);

#[cfg(test)]
const _: () = {
    use bevy_ecs::hierarchy::ChildOf;

    const fn infer_marker_works<T: RevBundle<Marker>, Marker>() {}

    infer_marker_works::<
        (
            ChildOf,
            SpawnOneRelated<ChildOf, ()>,
            SpawnRelatedBundle<ChildOf, ()>,
            (),
            (ChildOf,),
        ),
        _,
    >();
};
