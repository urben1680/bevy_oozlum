use alloc::vec::Vec;
use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    component::{Component, ComponentId},
    relationship::Relationship,
    spawn::{SpawnOneRelated, SpawnRelatedBundle, SpawnableList},
    world::EntityWorldMut,
};
use core::{any::TypeId, marker::PhantomData, mem::swap};
use variadics_please::all_tuples;

use crate::{
    meta::NotLog,
    undo_redo::{
        AddRemoveRelated, RevEntityWorld, SlimRelationship, get_new_related,
        get_new_related_entities,
        insert_remove::{
            InnerComponentBuffer, RevInsertComponentNew, RevInsertComponentOverwrite,
            RevNewRequired, RevRemoveComponent,
        },
        mark_entities, mark_entity,
    },
};

/// Adapter trait for [`Bundle`] implementors to enable reversible insert/remove of the contained
/// components and common bundle effects like [`children!`] returns.
///
/// [`children!`]: bevy_ecs::children
pub trait RevBundle<Marker>: Bundle {
    /// Inserts `self` into `entity` depending on `mode`.
    ///
    /// When undone, the inserted components are removed from `entity`, returning overwritten
    /// components, if there were any and [`InsertMode::Replace`] was picked. When redone, the
    /// components are returned to `entity` again, potentially overwriting existing components
    /// again.
    fn rev_insert(
        self,
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    );

    /// This is called within [`RevBundle::rev_insert`] and should not be called elsewhere as this
    /// alone will not make the insertion of required components reversible.
    #[doc(hidden)]
    fn rev_insert_inner(
        self,
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    );

    /// Removes `Self` from `entity`.
    ///
    /// When undone, the removed components are returned to `entity`. When redone, they are removed
    /// from `entity` again.
    fn rev_remove(not_log: NotLog, entity: &mut EntityWorldMut, caller: MaybeLocation);
}

fn required_of_component<C: Component>(
    not_log: NotLog,
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
    required_inner::<C>(not_log, entity, caller, new_required)
}

fn required_of_bundle<B: Bundle>(
    not_log: NotLog,
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
    required_inner::<B>(not_log, entity, caller, new_required)
}

fn required_inner<T: Send + 'static>(
    not_log: NotLog,
    entity: &mut EntityWorldMut,
    caller: MaybeLocation,
    new_required: Vec<ComponentId>,
) {
    if !new_required.is_empty() {
        entity.buffer_undo_redo(
            not_log,
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
    fn rev_insert(self, _: NotLog, _: &mut EntityWorldMut, _: InsertMode, _: MaybeLocation) {}

    fn rev_insert_inner(self, _: NotLog, _: &mut EntityWorldMut, _: InsertMode, _: MaybeLocation) {}

    fn rev_remove(_: NotLog, _: &mut EntityWorldMut, _: MaybeLocation) {}
}

impl<C: Component> RevBundle<[C; 1]> for C {
    fn rev_insert(
        self,
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        required_of_component::<C>(not_log, entity, caller);
        self.rev_insert_inner(not_log, entity, mode, caller);
    }

    fn rev_insert_inner(
        mut self,
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        match mode {
            InsertMode::Keep => {
                if !entity.contains::<C>() {
                    entity.redo_and_buffer(
                        not_log,
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
                    entity.buffer_undo_redo(not_log, RevInsertComponentOverwrite(inner), caller);
                } else {
                    entity.redo_and_buffer(not_log, RevInsertComponentNew(inner), caller)
                }
            }
        }
    }

    fn rev_remove(not_log: NotLog, entity: &mut EntityWorldMut, caller: MaybeLocation) {
        if let Some(component) = entity.take::<C>() {
            entity.buffer_undo_redo(
                not_log,
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
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        #[allow(clippy::let_unit_value)]
        let _ = <R as SlimRelationship>::ASSERT;
        self.rev_insert_inner(not_log, entity, mode, caller);
    }

    fn rev_insert_inner(
        self,
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        _mode: InsertMode,
        caller: MaybeLocation,
    ) {
        let Some(new_related) = get_new_related::<R>(entity, |entity| entity.insert(self)) else {
            return;
        };
        entity.world_scope(|world| {
            if let Ok(mut new_related) = world.get_entity_mut(new_related) {
                mark_entity::<true>(not_log, &mut new_related, true, caller);
            }
        });
        let id = entity.id();
        entity.buffer_undo_redo(
            not_log,
            AddRemoveRelated::<R, _, true>::new(id, [new_related], caller),
            caller,
        );
    }

    fn rev_remove(_: NotLog, _: &mut EntityWorldMut, _: MaybeLocation) {}
}

impl<R: Relationship, L: SpawnableList<R> + Send + Sync + 'static> RevBundle<[R; 3]>
    for SpawnRelatedBundle<R, L>
{
    fn rev_insert(
        self,
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        mode: InsertMode,
        caller: MaybeLocation,
    ) {
        #[allow(clippy::let_unit_value)]
        let _ = <R as SlimRelationship>::ASSERT;
        self.rev_insert_inner(not_log, entity, mode, caller);
    }

    fn rev_insert_inner(
        self,
        not_log: NotLog,
        entity: &mut EntityWorldMut,
        _mode: InsertMode,
        caller: MaybeLocation,
    ) {
        let new_related = get_new_related_entities::<R>(entity, |entity| entity.insert(self));
        entity.world_scope(|world| {
            mark_entities::<true>(not_log, world, &new_related, true, MaybeLocation::caller())
        });
        let id = entity.id();
        entity.buffer_undo_redo(
            not_log,
            AddRemoveRelated::<R, _, true>::new(id, new_related, caller),
            caller,
        );
    }

    fn rev_remove(_: NotLog, _: &mut EntityWorldMut, _: MaybeLocation) {}
}

macro_rules! impl_buffer_bundle {
    ($(#[$meta:meta])* $(($T: ident, $M: ident, $var: ident)),*) => {
        $(#[$meta])*
        impl<$($T, $M),*> RevBundle<($($M,)*)> for ($($T,)*)
        where
            $($T: RevBundle<$M>,)*
        {
            fn rev_insert(
                self,
                not_log: NotLog,
                entity: &mut EntityWorldMut,
                mode: InsertMode,
                caller: MaybeLocation,
            ) {
                required_of_bundle::<Self>(not_log, entity, caller);
                self.rev_insert_inner(not_log, entity, mode, caller);
            }

            fn rev_insert_inner(
                self,
                not_log: NotLog,
                entity: &mut EntityWorldMut,
                mode: InsertMode,
                caller: MaybeLocation,
            ) {
                let ($($var,)*) = self;
                ($($var.rev_insert_inner(not_log, entity, mode, caller),)*);
            }

            fn rev_remove(
                not_log: NotLog,
                entity: &mut EntityWorldMut,
                caller: MaybeLocation
            ) {
                ($(<$T as RevBundle::<$M>>::rev_remove(not_log, entity, caller),)*);
            }
        }
    };
}

all_tuples!(
    #[doc(fake_variadic)]
    impl_buffer_bundle,
    1,
    15,
    T,
    M,
    var
);

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
