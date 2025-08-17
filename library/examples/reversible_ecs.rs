use std::{io::stdout, num::NonZeroU64, time::Duration};

use bevy::{
    app::App,
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    prelude::*,
};

use crossterm::{ExecutableCommand, cursor::*, terminal::*};

use library::{
    log::{DenseTransitionLog, FrameTransitionLog, SparseTransitionLog},
    meta::NonLogNow,
    prelude::*,
};

const MAX_LOG_LEN: u64 = 71;
const FIXED_TIMESTEP: Duration = Duration::from_millis(100);

// todo: mention how the last column cannot be undone

// todo: entirely work with entity disabling instead of removing/readding components

fn main() {
    let _crossterm = GlobalSettings::new();
    let meta = RevMeta::new(NonZeroU64::new(MAX_LOG_LEN), 0, false);
    App::new()
        .add_plugins((
            MinimalPlugins,
            // todo: explain plugin
            RevSystemsPlugin::add_meta_and_runner(meta, FixedUpdate),
            // todo: explain general task of a row
            (row1, row2, row3, row4, row5, row6, row7),
        ))
        .rev_configure_sets(
            RevUpdate,
            (
                Row(1).rev_ambiguous_with_all(),
                Row(2).rev_after(Row(1)),
                (
                    Row(3).rev_after_ignore_deferred(Row(2)),
                    Row(4),
                    Row(5).rev_before(Row(6)),
                    Row(7),
                )
                    .rev_chain(),
            ),
        )
        .add_systems(PreUpdate, map_input)
        .add_systems(
            FixedUpdate,
            (
                control_rev_meta.before(RevMeta::try_run_rev_update),
                render.after(RevMeta::try_run_rev_update),
            ),
        )
        .add_systems(RevUpdate, clear_input.after(RevSystems))
        .init_resource::<KeysPressed>()
        .init_resource::<LostWaste>()
        .insert_resource(Time::<Fixed>::from_duration(FIXED_TIMESTEP))
        .run();
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct Row(u8);

#[derive(Component, Clone, Copy)]
#[component(on_despawn = on_despawn)]
struct Waste {
    tossed_at: u64,
    row: usize,
}

#[derive(Resource, Default)]
struct LostWaste(usize);

// If a Waste component is removed during NOT_LOG, then that only happens because the player missed
// undoing the littering and this adds to the LostWaste penality, bringing the player closer to
// the game-over.
// todo: misleading comment and impl, may only need meta to be running, explain "undone" despawns
fn on_despawn(mut world: DeferredWorld, _: HookContext) {
    if RevDirection::NOT_LOG.is_running(&world) {
        world.resource_mut::<LostWaste>().0 += 1;
    }
}

// The most simple way to cause a Waste entity to be reversibly spawned is just to use Commands::rev_spawn.
// Such spawns are logged in the system state and is undone/redone when this system runs during a log phase.
fn row1(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(1)));

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        // RevMeta::non_log_now returns a tiny type that is used in the API to express that
        // currently the RevDirection::NOT_LOG phase is active. This makes it much easier
        // to prevent incorrectly issued reversible commands, in contrast to some additional
        // runtime checks deep inside the crate's implementation and confusing call stacks.
        if pressed.num1
            && let Some(now) = meta.non_log_now()
        {
            commands.rev_spawn(
                now,
                Waste {
                    tossed_at: meta.now(), // the current frame can be received from RevMeta
                    row: 1,
                },
            );
        }
    }
}

// This row works as the previous one but uses direct World access instead of commands.
fn row2(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(2)));

    fn system(world: &mut World) {
        if world.resource::<KeysPressed>().num2
            && let Some(now) = world.non_log_now()
        {
            world.rev_spawn(
                now,
                Waste {
                    tossed_at: now.get(), // the current frame can be received from NonLogNow
                    row: 2,
                },
            );
        }
    }
}

// Both previous variants use the BuffersUndoRedo trait API under the hood that all reversible
// structural changes are using. This example is a variant of row1 but with a manual implementation.
fn row3(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(3)));

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        // The system is noop during log phases.
        let Some(now) = meta.non_log_now() else {
            return;
        };

        // If the key is pressed we want to spawn and log a waste entity.
        if pressed.num3 {
            let waste = Waste {
                tossed_at: meta.now(),
                row: 3,
            };

            // We spawn the entity with the regular bevy API.
            let entity = commands.spawn(waste).id();

            // We mark the entity as log scoped which means it will be despawned when this frame gets out of log.
            commands.rev_log_scope(now, entity);

            // We pass in a closure here but the underlying UndoRedo trait can be implemented for any
            // of your types.
            commands.buffer_undo_redo(now, move |world: &mut World, variant: UndoRedoDirection| {
                let mut entity = world.entity_mut(entity);
                match variant {
                    UndoRedoDirection::Undo => {
                        entity.remove::<Waste>();
                    }
                    UndoRedoDirection::Redo => {
                        entity.insert(waste);
                    }
                };
            });
        }
    }
}

// One can also use a system-local log to track the spawn of Waste entities to undo/redo that depending on
// the active RevDirection phase.
fn row4(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(4)));

    // We choose a SparseTransitionLog as we understand pressing of a key and the resulting spawn as a
    // transition between a state where a new Waste entity does not exist to the state where it does.
    // It also has to be a sparse log because not every frame the key is pressed.
    fn system(
        meta: Res<RevMeta>,
        pressed: Res<KeysPressed>,
        mut log: Local<SparseTransitionLog<Entity>>,
        mut commands: Commands,
    ) {
        let waste = Waste {
            tossed_at: meta.now(),
            row: 4,
        };

        // Depending on the current RevDirection, the system has different behavior
        match meta.running_direction() {
            // During NOT_LOG we want to react on the pressed keys to spawn Waste entities and to log that.
            RevDirection::NOT_LOG => {
                // Any logs in the future of the log, for example after going backward in time and resuming
                // the NOT_LOG phase, should be despawned as they are no longer part of our reality as we
                // rewrite the future now!
                for entity in log.drain_future() {
                    commands.entity(entity).despawn();
                }

                // A quirk of Transition logs is that they need one less log entry compared to State logs.
                // For example if the global log can be up to 3 states, then...
                // - State logs need to have these three states
                // - Transition logs only need to have the two transitions that are used for transitioning
                //   from the first to the second state and from the second to the third state
                //
                // So, the transition log would just pop earlier transitions in push_and_pop_past as the log
                // itself does not need that transition anymore.
                // In our case however this popped transition is used to despawn a Waste entity. But we dont
                // want that to happen a frame earlier as otherwise the waste would vanish before it went past
                // the right edge of the water.
                //
                // That is why we add a + 1 here to compensate that behavior.
                let past_len = meta.past_len() + 1;

                // If the key is pressed, we spawn another Waste entity.
                // If not, we still need to push a None to advance the log.
                let entity = pressed.num4.then(|| commands.spawn(waste).id());

                // Pushing potential Waste entities may also pop an entity that got out-of-log now.
                // These need to be despawned as they are now past the edge of the screen and cannot come back.
                if let Some(entity) = log.push_and_pop_past(past_len as usize, entity) {
                    commands.entity(entity).despawn();
                }
            }

            // When we go backward in log, we just remove the component from the entity that got spawned
            // this frame. When we go forward in log, we reinsert the component.
            // This way we dont need to despawn and respawn during the log phase.
            RevDirection::BackwardLog => {
                if let Some(entity) = log.backward_log().unwrap() {
                    commands.entity(*entity).remove::<Waste>();
                }
            }
            RevDirection::FORWARD_LOG => {
                if let Some(entity) = log.forward_log().unwrap() {
                    commands.entity(*entity).insert(waste);
                }
            }
        }
    }
}

// This row works as the previous row but we want to demonstrate it with using a running condition.
fn row5(app: &mut App) {
    app.rev_add_systems(
        RevUpdate,
        // The system will now only run when the key is pressed. The state of the condition system will make
        // sure to pass at the correct log phases as well. The actual key press only matters for
        // RevDirection::NOT_LOG.
        spawn_and_log_system
            .rev_run_if(spawn_condition)
            .rev_in_set(Row(5)),
    );

    fn spawn_condition(pressed: Res<KeysPressed>) -> bool {
        pressed.num5
    }

    // Other than the previous row, the used log does not need to support Option updates - every system
    // run will cause a spawn so we pick DenseTransitionLog.
    // However, because the past is "shorter" from the point of view of this system, we need to add a
    // FrameTransitionLog to track the exact past length. This may not be needed if you dont care for the
    // entity log length here, then the RevMeta::past_len is fine. We do it here for demonstration purpose.
    fn spawn_and_log_system(
        meta: Res<RevMeta>,
        mut entity_log: Local<DenseTransitionLog<Entity>>,
        mut frame_log: Local<FrameTransitionLog>,
        mut commands: Commands,
    ) {
        let waste = Waste {
            tossed_at: meta.now(),
            row: 5,
        };

        match meta.running_direction() {
            // Note that no despawns happen in this system as the it does not necessarily run when an entity
            // spawned from here gets out-of-log.
            // Not need to check if the key is pressed before the spawn, it is implied by the system running.
            RevDirection::NOT_LOG => {
                // During RevDirection::NOT_LOG, this is always Some.
                let now = meta.non_log_now().unwrap();

                // We spawn the waste entity and mark is as log scoped to be despawned when out-of-log.
                let entity = commands.spawn(waste).rev_log_scope(now).id();

                // We do not use the push_and_pop_past method because, as this system does not run every frame,
                // multiple log entries may be out of log now.
                let past_len = frame_log.push_and_get_past_len(&meta);
                entity_log.push_and_drain_past(past_len, entity);
            }

            // As before, the log behavior is just removing and readding the Waste component.
            RevDirection::FORWARD_LOG => {
                // We also need to update the frame log. As the frame log is advanced every time this system runs,
                // this method should always return true as well because the run condition makes sure it only runs
                // in these frames, also during the log.
                let expects_forward_log_run = frame_log.forward_log(&meta);
                assert!(expects_forward_log_run);

                let entity = *entity_log.forward_log().unwrap();
                commands.entity(entity).insert(waste);
            }
            RevDirection::BackwardLog => {
                let expects_backward_log_run = frame_log.backward_log(&meta);
                assert!(expects_backward_log_run);
                let entity = *entity_log.backward_log().unwrap();
                commands.entity(entity).remove::<Waste>();
            }
        }
    }
}

// Hooks can be reversible as well.
fn row6(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(5)))
        .world_mut()
        .register_component_hooks::<Waste>()
        .on_add(on_add);

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if pressed.num6
            && let Some(now) = meta.non_log_now()
        {
            commands.spawn(Waste {
                tossed_at: now.get(),
                row: 6,
            });
        }
    }

    fn on_add(mut world: DeferredWorld, context: HookContext) {
        let Some(now) = world.non_log_now() else {
            return;
        };

        let entity = context.entity;
        let waste = *world.entity(entity).get::<Waste>().unwrap();

        // We only want this hook to be in effect for this specific row.
        if waste.row != 6 {
            return;
        }

        world.rev_log_scope(now, entity);

        world.buffer_undo_redo(now, move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Waste>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(waste);
                }
            };
        });
    }
}

// And observer can also be reversible.
fn row7(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(7)))
        .add_observer(observer);

    #[derive(Event)]
    struct WasteObserverEvent(NonLogNow);

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if pressed.num7
            && let Some(now) = meta.non_log_now()
        {
            commands.trigger(WasteObserverEvent(now));
        }
    }

    fn observer(trigger: On<WasteObserverEvent>, mut world: DeferredWorld) {
        let now = trigger.0;

        let waste = Waste {
            tossed_at: now.get(),
            row: 7,
        };

        let entity = world.commands().spawn(waste).id();

        world.rev_log_scope(now, entity);

        world.buffer_undo_redo(now, move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Waste>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(waste);
                }
            };
        });
    }
}

#[derive(Resource, Default)]
struct KeysPressed {
    direction: Option<Direction>,
    num1: bool,
    num2: bool,
    num3: bool,
    num4: bool,
    num5: bool,
    num6: bool,
    num7: bool,
}

enum Direction {
    Forward,
    Pause,
    FutureEnd,
    PastEnd,
}

fn map_input(mut keys: ResMut<KeysPressed>, lost: Res<LostWaste>, mut exit: EventWriter<AppExit>) {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, poll, read};

    let mut f = || -> std::io::Result<()> {
        // check if event exists to read, do not block thread to wait for one if not
        if !poll(Duration::from_secs(0))? {
            return Ok(());
        }
        // check if the event is a pressed key
        let Event::Key(KeyEvent {
            kind: KeyEventKind::Press,
            code,
            ..
        }) = read()?
        else {
            return Ok(());
        };
        match code {
            KeyCode::Esc => {
                exit.write(AppExit::Success);
            }
            // ignore any other inputs at game over
            _ if lost.0 >= 10 => {}
            KeyCode::Left => keys.direction = Some(Direction::FutureEnd),
            KeyCode::Right => keys.direction = Some(Direction::PastEnd),
            KeyCode::Up => keys.direction = Some(Direction::Forward),
            KeyCode::Down => keys.direction = Some(Direction::Pause),
            KeyCode::Char('1') => keys.num1 = true,
            KeyCode::Char('2') => keys.num2 = true,
            KeyCode::Char('3') => keys.num3 = true,
            KeyCode::Char('4') => keys.num4 = true,
            KeyCode::Char('5') => keys.num5 = true,
            KeyCode::Char('6') => keys.num6 = true,
            KeyCode::Char('7') => keys.num7 = true,
            _ => {}
        }
        Ok(())
    };

    if enable_raw_mode().is_ok() {
        let _ = f();
        let _ = disable_raw_mode();
    }
}

fn clear_input(mut keys: ResMut<KeysPressed>) {
    *keys = default();
}

fn control_rev_meta(mut meta: ResMut<RevMeta>, keys: Res<KeysPressed>) {
    match keys.direction {
        Some(Direction::Forward) => meta.queue_not_log_forward(),
        Some(Direction::Pause) => meta.queue_pause(),
        Some(Direction::FutureEnd) => {
            let to = meta.future_end();
            let _ok = meta.queue_log(to);
        }
        Some(Direction::PastEnd) => {
            let to = meta.past_end();
            let _ok = meta.queue_log(to);
        }
        None => {}
    }
}

struct GlobalSettings;

impl GlobalSettings {
    fn new() -> Self {
        use bevy::log::{
            Level,
            tracing::{Event, Subscriber, dispatcher::get_default},
            tracing_subscriber::{
                Layer,
                layer::{Context, SubscriberExt},
                registry,
                util::SubscriberInitExt,
            },
        };

        struct PanicOnError;
        impl<S: Subscriber> Layer<S> for PanicOnError {
            fn on_event(&self, event: &Event, _ctx: Context<S>) {
                if *event.metadata().level() == Level::ERROR {
                    panic!("{event:#?}")
                }
            }
        }
        if registry().with(PanicOnError).try_init().is_err() {
            get_default(|subscriber| {
                assert!(subscriber.downcast_ref::<PanicOnError>().is_some());
            })
        }
        let _ = stdout().execute(SetSize(MAX_LOG_LEN as u16 + 2, 13));
        let _ = stdout().execute(Hide);
        Self
    }
}

impl Drop for GlobalSettings {
    fn drop(&mut self) {
        let _ = stdout().execute(MoveToRow(14));
        let _ = stdout().execute(Show);
    }
}

fn render(
    meta: Res<RevMeta>,
    waste: Query<&Waste>,
    lost: Res<LostWaste>,
    mut last_future_end: Local<Option<u64>>,
) {
    let _ = stdout().execute(BeginSynchronizedUpdate);
    let _ = stdout().execute(Clear(ClearType::All));

    println!();
    println!("Let's waste the time 'til Bevy 1.0 by tossing said waste into the ocean!");
    println!("No worry, it's okay as long you undo it. Just don't wait for too long...");
    println!();

    let wave = |phase: u64| "`-._,~'".chars().cycle().skip(7 - phase as usize % 7);

    let row_future: String = wave(meta.future_end())
        .take(meta.future_len() as usize)
        .collect();

    let row_past: String = wave(meta.future_end())
        .skip(meta.future_len() as usize % 7)
        .take(meta.past_len() as usize + 1)
        .collect();
    let mut past_rows: [String; 7] = std::array::from_fn(|_| row_past.clone());

    let padding_cols = match *last_future_end {
        Some(frame) => {
            let mut padding = frame.wrapping_sub(meta.future_end());
            if padding > MAX_LOG_LEN as u64 {
                padding = 0;
                *last_future_end = Some(meta.future_end());
            }
            padding as usize
        }
        None => {
            *last_future_end = Some(meta.future_end());
            0
        }
    };
    let padding = " ".repeat(padding_cols);

    for Waste { row, tossed_at } in waste.iter().cloned() {
        let index = (meta.now() - tossed_at) as usize;
        // replace_range would panic if a waste is tossed into the water at a frame that is not the present or that is not within the past log
        // this is ensured by reversible logic and by wastes being despawned when they go out of log
        let str = past_rows.get_mut(row - 1).unwrap();
        if str.len() <= index {
            panic!("row {row} out of log");
        }
        str.replace_range(index..=index, "#");
    }

    for (i, past_row) in past_rows.into_iter().enumerate() {
        println!("{padding}{row_future}{}{past_row}", i + 1);
    }

    let lost = lost.0.min(10);
    let lost_bar: String = "#"
        .repeat(lost)
        .chars()
        .chain(wave(meta.now()).take(10 - lost))
        .collect();

    println!(
        "{}^",
        " ".repeat(padding_cols + meta.future_len() as usize + 1)
    );
    if meta.get_ran_direction() != Some(RevDirection::NOT_LOG) || lost == 10 {
        println!("           (waste lost: {lost_bar})    ESC: close");
    } else {
        println!(" 1-6: toss waste (lost: {lost_bar})    ESC: close");
    }
    if lost < 10 {
        println!("LEFT: forward log, pause at end        RIGHT: backward log, pause at end");
        println!("  UP: exit log and resume              DOWN: pause");
    } else {
        println!();
        println!("You left too much waste behind that you can no longer recover. GAME OVER");
    }
    let _ = stdout().execute(MoveTo(0, 0));
    let _ = stdout().execute(EndSynchronizedUpdate);
}
