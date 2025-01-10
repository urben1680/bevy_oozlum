/*
TODO:

- add license
- find new name
-- catchier and more unique to not use reversible-systems/schedules for comparable crates
-- "systems" in reverse: smetsys, can be altered to smet-sys or something
-- "schedules" in reverse: seludehcs (ew)
- should still be descriptive
-- rev..?
-- bevy_smetsys
-- bevy_yveb

Enhancements:
- reduce todo!() and //todo
- impl Error for relevant structs/enums via thiserror
- blanket impl UndoRedo
- #[inline]s
- examples
-- folder next to src
--- how to have cargo check look at it?
--- add CI
-- hooks https://github.com/bevyengine/bevy/blob/main/examples/ecs/component_hooks.rs
-- observers https://github.com/bevyengine/bevy/blob/main/examples/ecs/observers.rs
-- ecs, commands https://github.com/bevyengine/bevy/blob/main/examples/ecs/ecs_guide.rs

Example
- ideally contains scheduling, commands, hooks and observers
- without reversible entity commands this may be difficuilt to do now
- ascii overview of certain things

Stone throw game
shows waves and limited amount of stones
each position logs the splash differently
at log end the stone is returned to inventory via different means

Pass the time waiting for Bevy 1.0 by tossing some stones into the water

        `-._,~´`-._1~´`-.o,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`
        `-._,~´`-._2~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`
        `-._,~´`-._3~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`
        `-._,~´`-._4~´`-._,~´o-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`
        `-._,~´`-._5~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`
        `-._,~´`-._6~´oo._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`

 1-6: toss a stone (x004)
LEFT: jump to future log, then pause    RIGHT: jump to past log end, then pause
  UP: exit log and resume               DOWN: pause

each splash is done in a different reversible way
the phase of the wave is always the same by frame % 8

`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´`-._,~´

the stone in the wave is not the visualized log but the stone itself at a certain position so no log needs to be exposed
the different logic each wave manages how to return the stone

Docs
- documentations
-- point out determinism aspects of methods
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired

ISSUES/DISCUSSIONS:
- reversible change detection (copy over to new repo)
- analyze test schedule::non_exclusive_then_exclusive_ignore_deferred, consider revamping test strategy
- reversible entity commands, link to https://github.com/bevyengine/bevy/issues/15350 as blocker
- manual sync point configuration
-- apply_deferred
-- ScheduleBuildSettings::auto_insert_apply_deferred
- rare_init_none
*/

mod example {
    use std::{num::NonZeroU32, time::Duration};

    use bevy::{
        app::App,
        ecs::{component::ComponentId, system::SystemParam, world::DeferredWorld},
        input::keyboard::KeyboardInput,
        prelude::*,
    };

    use crate::{
        log::{RareTransitionLog, TransitionLog},
        prelude::*,
        undo_redo::UndoRedoDirection,
    };

    #[derive(Component, Clone, Copy)]
    #[component(on_add = on_add_5)]
    struct Stone {
        tossed_at: RevFrame,
        row: usize,
    }

    const MAX_LOG_LEN: u32 = 78;

    fn main() {
        let meta = RevMeta::new(NonZeroU32::new(MAX_LOG_LEN), None, false);
        App::new()
            .add_plugins(
                RevSystemsPlugin::add_meta_and_runner(meta, FixedUpdate),
            )
            .rev_add_systems(
                RevUpdate,
                (
                    system_1,
                    system_2,
                    system_3.rev_run_if(pressed_3),
                    system_4,
                    (system_5, system_6).rev_chain(),
                ),
            )
            .add_systems(
                FixedUpdate,
                (
                    control_rev_meta.before(RevMeta::run_rev_update),
                    render_game.after(RevMeta::run_rev_update)
                )
            )
            .insert_resource(Time::<Fixed>::from_duration(Duration::from_millis(250)))
            .add_observer(observer_6)
            .run();
    }

    fn control_rev_meta(mut meta: ResMut<RevMeta>, mut events: EventReader<KeyboardInput>) {
        for event in events.read() {
            match event.key_code {
                KeyCode::ArrowLeft => {
                    let to = meta.future_end_world_state();
                    let _ok = meta.queue_log(to);
                }
                KeyCode::ArrowRight => {
                    let to = meta.past_end_world_state();
                    let _ok = meta.queue_log(to);
                }
                KeyCode::ArrowUp => meta.queue_forward(),
                KeyCode::ArrowDown => meta.queue_pause(),
                _ => continue,
            }
            events.clear();
            return;
        }
    }

    fn render_game(meta: Res<RevMeta>, stones: Query<&Stone>, mut last_future_end: Local<Option<RevFrame>>) {
        print!("\x1B[2J"); // this clears the last frame
        println!("   Pass the time waiting for Bevy 1.0 by tossing some stones into the water!");
        println!();

        let iter = || "`-._,~´".chars()
            .cycle()
            .skip(u32::from(meta.future_end_world_state()) as usize % 7);

        let row_future: String = iter()
            .take(meta.future_world_states() as usize)
            .collect();

        let row_past: String = iter()
            .skip(meta.future_world_states() as usize % 7)
            .take(meta.past_world_states() as usize + 1)
            .collect();
        let mut past_rows: [String; 6] = std::array::from_fn(|_| row_past.clone());

        let mut place_stone = |frame: RevFrame, row: usize| {
            let index = (meta.future_end_world_state() - frame) as usize;
            // get_mut would panic if a stone is tossed into the water at a frame that is not the present or that is not within the past log
            // this is ensured by reversible logic and by stones being despawned when they go out of log
            past_rows.get_mut(row).unwrap().replace_range(index..(index+1), "o");
        };

        for stone in stones.iter() {
            place_stone(stone.tossed_at, stone.row - 1);
        }

        let padding = match *last_future_end {
            Some(frame) => {
                let mut padding = frame - meta.future_end_world_state();
                if padding > MAX_LOG_LEN {
                    padding = 0;
                    *last_future_end = Some(meta.future_end_world_state());
                }
                padding
            }
            None => {
                *last_future_end = Some(meta.future_end_world_state());
                0
            }
        };
        let padding: String = std::iter::repeat(' ').take(padding as usize).collect();

        for (i, past_row) in past_rows.into_iter().enumerate() {
            println!("{padding}{row_future}{}{past_row}", i + 1);
        }

        println!();
        println!(" 1-6: toss stone (x{:03})", stones.iter().len());
        println!("LEFT: jump to future log, then pause    RIGHT: jump to past log end, then pause");
        println!("  UP: exit log and resume               DOWN: pause")
    }

    // Helper param to make reading pressed keys simpler, disregards unchecked keys each system run
    #[derive(SystemParam)]
    struct Number<'w, 's, const N: u8> {
        events: EventReader<'w, 's, KeyboardInput>,
    }

    impl<const N: u8> Number<'_, '_, N> {
        const KEYS: [KeyCode; 2] = match N {
            1 => [KeyCode::Numpad1, KeyCode::Digit1],
            2 => [KeyCode::Numpad2, KeyCode::Digit2],
            3 => [KeyCode::Numpad3, KeyCode::Digit3],
            4 => [KeyCode::Numpad4, KeyCode::Digit4],
            5 => [KeyCode::Numpad5, KeyCode::Digit5],
            6 => [KeyCode::Numpad6, KeyCode::Digit6],
            _ => unimplemented!(),
        };
        fn is_pressed(mut self) -> bool {
            self.events
                .read()
                .any(|event| Self::KEYS.contains(&event.key_code) && event.state.is_pressed())
        }
    }

    impl<const N: u8> Drop for Number<'_, '_, N> {
        fn drop(&mut self) {
            self.events.clear();
        }
    }

    fn system_1(meta: Res<RevMeta>, number: Number<1>, mut commands: Commands) {
        if meta.direction() != RevDirection::NOT_LOG || !number.is_pressed() {
            return;
        }
        commands./*rev_*/spawn(Stone {
            tossed_at: meta.present_world_state(),
            row: 1,
        });
    }

    fn system_2(
        meta: Res<RevMeta>,
        number: Number<2>,
        mut log: Local<RareTransitionLog<Entity>>,
        mut commands: Commands,
    ) {
        let stone = Stone {
            tossed_at: meta.present_world_state(),
            row: 2,
        };
        match meta.direction() {
            RevDirection::NOT_LOG => {
                if let Some(entity) = log.pop_past_by_len(meta.past_world_states() as usize) {
                    commands.entity(entity).despawn();
                }
                for entity in log.drain_future() {
                    commands.entity(entity).despawn();
                }
                let entity = number.is_pressed().then(|| commands.spawn(stone).id());
                log.push_present(entity);
            }
            RevDirection::FORWARD_LOG => {
                if let Some(entity) = log.forward_log().unwrap() {
                    commands.entity(*entity).insert(stone);
                }
            }
            RevDirection::BackwardLog => {
                if let Some(entity) = log.backward_log().unwrap() {
                    commands.entity(*entity).remove::<Stone>();
                }
            }
        }
    }

    fn pressed_3(number: Number<3>) -> bool {
        number.is_pressed()
    }

    fn system_3(meta: Res<RevMeta>, mut log: Local<TransitionLog<Entity>>, mut commands: Commands) {
        let stone = Stone {
            tossed_at: meta.present_world_state(),
            row: 3,
        };
        match meta.direction() {
            RevDirection::NOT_LOG => {
                if let Some(entity) = log.pop_past_by_len(meta.past_world_states() as usize) {
                    commands.entity(entity).despawn();
                }
                for entity in log.drain_future() {
                    commands.entity(entity).despawn();
                }
                let entity = commands.spawn(stone).id();
                log.push_present(entity);
            }
            RevDirection::FORWARD_LOG => {
                let entity = *log.forward_log().unwrap();
                commands.entity(entity).insert(stone);
            }
            RevDirection::BackwardLog => {
                let entity = *log.backward_log().unwrap();
                commands.entity(entity).remove::<Stone>();
            }
        }
    }

    fn system_4(meta: Res<RevMeta>, number: Number<4>, mut commands: Commands) {
        if meta.direction() != RevDirection::NOT_LOG || !number.is_pressed() {
            return;
        }
        let stone = Stone {
            tossed_at: meta.present_world_state(),
            row: 4,
        };
        let entity = commands.spawn(stone).id();
        commands.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Stone>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(stone);
                }
                UndoRedoDirection::FinalizeUndone | UndoRedoDirection::FinalizeRedone => {
                    entity.despawn();
                }
            };
        });
    }

    fn on_add_5(mut world: DeferredWorld, entity: Entity, _: ComponentId) {
        if world.resource::<RevMeta>().direction().is_log() {
            return;
        }
        let stone = *world.entity_mut(entity).get::<Stone>().unwrap();
        if stone.row != 5 {
            return;
        }
        world.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Stone>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(stone);
                }
                UndoRedoDirection::FinalizeUndone | UndoRedoDirection::FinalizeRedone => {
                    entity.despawn();
                }
            };
        });
    }

    fn system_5(meta: Res<RevMeta>, number: Number<5>, mut commands: Commands) {
        if meta.direction() != RevDirection::NOT_LOG || !number.is_pressed() {
            return;
        }
        commands.spawn(Stone {
            tossed_at: meta.present_world_state(),
            row: 5,
        });
    }

    #[derive(Event)]
    struct Stone6Event(Stone);

    fn observer_6(trigger: Trigger<Stone6Event>, mut world: DeferredWorld) {
        let stone = trigger.0;
        let entity = trigger.entity();
        world.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Stone>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(stone);
                }
                UndoRedoDirection::FinalizeUndone | UndoRedoDirection::FinalizeRedone => {
                    entity.despawn();
                }
            };
        });
    }

    fn system_6(meta: Res<RevMeta>, number: Number<6>, mut commands: Commands) {
        if meta.direction() != RevDirection::NOT_LOG || !number.is_pressed() {
            return;
        }
        let stone = Stone {
            tossed_at: meta.present_world_state(),
            row: 6,
        };
        let entity = commands.spawn(stone).id();
        commands.trigger_targets(Stone6Event(stone), entity);
    }
}

pub mod app;
pub mod frame;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod undo_redo;

/// Contains all extension traits `as _` and common types.
pub mod prelude {
    pub use crate::app::{RevApp as _, RevSystemsPlugin};
    pub use crate::frame::{PackedRevFrame, RevFrame};
    pub use crate::meta::{DrainPastByLoggedAt, RevDirection, RevMeta};
    pub use crate::schedule::{
        IntoRevSystemConfigs as _, IntoRevSystemSetConfigs as _, RevSchedule as _, RevUpdate,
    };
    pub use crate::undo_redo::{BuffersUndoRedo as _, RevCommands as _};
}

macro_rules! error_per_flag {
    ($flag:expr, $($arg:tt)+) => ({
        if !*$flag {
            bevy::utils::tracing::error!($($arg)+);
            *$flag = true;
        }
        core::default::Default::default()
    });
}

use error_per_flag;
