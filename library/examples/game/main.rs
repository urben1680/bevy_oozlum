use bevy::prelude::*;
use bevy_ecs::{lifecycle::HookContext, world::DeferredWorld};
use bevy_oozlum::prelude::*;
use std::{num::NonZeroU64, time::Duration};

// todo: mention how the last column cannot be undone

mod control;
mod render;
mod rows;

const ROWS: usize = 7;
const MAX_PAST_LEN: u64 = 70;
const CURRENT_BEVY_VERSION: u64 = 0_17_0;
const WINNING_BEVY_VERSION: u64 = 1_00_0;
const FRAME_DURATION_MAX: Duration = Duration::from_micros(100000);
const FRAME_DURATION_MIN: Duration = Duration::from_micros(15000);

fn main() {
    App::new()
        .add_plugins((
            RevPlugin::add_meta_and_runner(
                NonZeroU64::new(MAX_PAST_LEN).unwrap(),
                false,
                FixedUpdate,
            ),
            DefaultPlugins.set(render::window_plugin::<ROWS>()),
            rows::plugin,
            control::plugin::<ROWS>,
            render::plugin::<ROWS>,
        ))
        .add_systems(RevUpdate, despawn_not_undoable_waste.after(RevSystems))
        .init_state::<GameState>()
        .init_resource::<Stats>()
        .insert_resource(Time::<Fixed>::from_duration(FRAME_DURATION_MAX))
        .run();
}

#[derive(Resource, Default)]
struct Stats {
    lost: u64,
    score: u64,
}

#[derive(Component, Clone, Copy)]
struct Waste {
    row: u64,
}

#[derive(Component, Clone, Copy)]
#[component(on_add = increase_score)]
struct TossedAt(u64);

fn increase_score(mut world: DeferredWorld, _: HookContext) {
    if world
        .get_running_direction()
        .is_none_or(RevDirection::is_log)
    {
        return;
    }

    let mut score = world.resource_mut::<Stats>();
    score.score += 1;
    let score = score.score;
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

fn despawn_not_undoable_waste(
    meta: Res<RevMeta>,
    waste_query: Query<(Entity, &TossedAt, Has<Waste>)>,
    this_state: Res<State<GameState>>,
    mut counts: ResMut<Stats>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    if meta
        .get_running_direction()
        .is_none_or(RevDirection::is_log)
    {
        return;
    }

    for (entity, tossed_at, has_waste) in waste_query {
        if tossed_at.0 < meta.past_end() {
            commands.entity(entity).despawn();

            if has_waste && *this_state.get() == GameState::Running {
                counts.lost += 1;

                if counts.lost == 10 {
                    next_state.set(GameState::Lost);
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
enum GameState {
    #[default]
    Running,
    Won,
    Lost,
}
