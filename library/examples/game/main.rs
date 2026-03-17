use bevy::prelude::*;
use bevy_ecs::{lifecycle::HookContext, world::DeferredWorld};
use bevy_oozlum::prelude::*;
use std::{num::NonZeroU64, time::Duration};

const ROWS: usize = 7;
const MAX_PAST_LEN: u64 = 70;
const CURRENT_BEVY_VERSION: u64 = 0_17_0;
const WINNING_BEVY_VERSION: u64 = 1_00_0;
const FRAME_DURATION_MAX: Duration = Duration::from_millis(100);
const FRAME_DURATION_MIN: Duration = Duration::from_millis(15);

// This example is a game in which you have to toss a large amount of trash into the ocean.
// Since pollution is bad, you have to undo that to leave the ocean clean but keep your score.
//
// As the log length is limited, your time to undo is limited too.
// Leave behind 10 units of waste and it is game over.
// For every waste tossed, the bevy version number and game speed rises, at 1.0 you win!
//
// To be able to pollute even more, you have 7 rows of water to throw into.

// In this example you can see how to write reversible logic:
mod rows;

// And how to control the world state:
mod control;

// Everything gets rendered to an ASCII interface, which is not that interesting for the purpose of
// this example as showcasing the crate:
mod render;

fn main() {
    App::new()
        .add_plugins((
            // Add the RevPlugin to your application.
            //
            // If you only use bevy_ecs, add the RevMeta resource and the RevMeta::run_rev_update
            // manually to the world and register RevDespawned as a disabling component.
            //
            // The plugin default adds an unpaused RevMeta with a max past length of NonZeroU64::MIN
            // and the mentioned system to FixedUpdate
            //
            // General order:
            // 1. RevMeta::run_rev_update runs in the specified schedule (here FixedUpdate)
            // 2. RevUpdate schedule runs unless paused
            // 3. Reversible systems and sync points, all in the RevSystems set, run in normal or
            //    reversed order, depending on the current RevDirection
            RevPlugin::add_meta_and_runner(
                // Specify how many world states can be at most reversed, always at least 1.
                NonZeroU64::new(MAX_PAST_LEN).unwrap(),
                // Specify if the app starts paused.
                false,
                // Specify in which schedule the RevUpdate schedule should be run, you likely want
                // it to be a schedule with a fixed framerate.
                FixedUpdate,
            ),
            // Add other plugins needed for this example.
            DefaultPlugins.set(render::window_plugin()),
            rows::plugin,
            control::plugin,
            render::plugin,
        ))
        .add_systems(
            // You can add regular, non-reversible systems to RevUpdate using the vanilla
            // add_systems API, though in that case they should be ordered relative to the
            // RevSystems set
            RevUpdate,
            despawn_lost_waste.after(RevSystems),
        )
        .init_state::<GameState>()
        .init_resource::<Stats>()
        .insert_resource(Time::<Fixed>::from_duration(FRAME_DURATION_MAX))
        .run();
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
enum GameState {
    // Did not lose or win yet
    #[default]
    Running,

    // Reached bevy 1.0
    Won,

    // Lost 10 units of waste
    Lost,
}

#[derive(Resource, Default)]
struct Stats {
    // Waste that was lost
    lost: u64,

    // Total waste, including undone
    score: u64,
}

#[derive(Component, Clone, Copy)]
#[component(on_add = increase_score)]
struct Waste {
    // Which row the waste was tossed in
    row: u64,

    // At which RevMeta::now the waste was tossed
    tossed_at: u64,
}

// Increase the score and speed during RevDirection::Forward, check for winning condition
fn increase_score(mut world: DeferredWorld, _: HookContext) {
    // Hooks sensible to RevDirection must be written as such, here we do not want to react on
    // undo-redo logic, only the initial insertion
    if world
        .get_running_direction()
        .is_none_or(RevDirection::is_log)
    {
        return;
    }

    let mut stats = world.resource_mut::<Stats>();
    let score = (stats.score + 1).min(WINNING_BEVY_VERSION);
    stats.score = score;
    const SCORE_MAX: u64 = WINNING_BEVY_VERSION - CURRENT_BEVY_VERSION;

    if score >= SCORE_MAX {
        world
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Won);
        return;
    }

    let mut time = world.resource_mut::<Time<Fixed>>();
    let score_normalized = score as f32 / SCORE_MAX as f32;
    let micros_delta = FRAME_DURATION_MAX.as_micros() - FRAME_DURATION_MIN.as_micros();
    let score_micros = (micros_delta as f32 * score_normalized) as u64;
    let micros = FRAME_DURATION_MAX.as_micros() as u64 - score_micros;
    time.set_timestep(Duration::from_micros(micros));
}

// RevMeta::past_end is increased to prevent the past length to exceed the maximum. Then existing
// Waste could get out-of-log and brings you closer to the losing condition.
fn despawn_lost_waste(
    meta: Res<RevMeta>,
    waste_query: Query<(Entity, &Waste)>,
    this_state: Res<State<GameState>>,
    mut counts: ResMut<Stats>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    // Only during RevDirection::Forward RevMeta::past_end could have increased, so skip otherwise
    if meta
        .get_running_direction()
        .is_none_or(RevDirection::is_log)
    {
        return;
    }

    for (entity, &Waste { tossed_at, .. }) in waste_query {
        if tossed_at < meta.past_end() {
            commands.entity(entity).despawn();

            if *this_state.get() == GameState::Running {
                counts.lost += 1;

                if counts.lost == 10 {
                    next_state.set(GameState::Lost);
                }
            }
        }
    }
}
