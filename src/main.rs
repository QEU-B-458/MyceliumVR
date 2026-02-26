use bevy::{prelude::*, render::pipelined_rendering::PipelinedRenderingPlugin, DefaultPlugins};
use bevy_app::{App, PluginGroup, ScheduleRunnerPlugin, Startup};
use bevy_asset::AssetPlugin;
use bevy_mod_openxr::{add_xr_plugins, resources::OxrSessionConfig};
use openxr::EnvironmentBlendMode;
use std::time::Duration;
use wasvy::prelude::*;

mod components;
use components::Health;

wasvy::auto_host_components! {
    path = "wit",
    world = "game:components/host",
    module = components_bindings,
}

fn main() {
    let mut app = App::new();
    let mut default_plugins = DefaultPlugins.build();
    let asset_path = format!("{}/assets", env!("CARGO_MANIFEST_DIR"));
    let processed_path = format!("{}/assets/processed", env!("CARGO_MANIFEST_DIR"));
    
    default_plugins = default_plugins.set(AssetPlugin {
        file_path: asset_path,
        processed_file_path: processed_path,
        ..default()
    });
    
    let xr_plugins = add_xr_plugins(default_plugins.disable::<PipelinedRenderingPlugin>());
    
    app.insert_resource(OxrSessionConfig {
            blend_mode_preference: vec![
                EnvironmentBlendMode::ALPHA_BLEND,
                EnvironmentBlendMode::ADDITIVE,
                EnvironmentBlendMode::OPAQUE,
            ],
            ..default()
        })
    .add_plugins(xr_plugins) // This now includes your configured AssetPlugin and TaskPool
    .add_plugins(bevy_mod_xr::hand_debug_gizmos::HandGizmosPlugin)
    .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)))
    .add_plugins(ModloaderPlugin::default().add_functionality(add_components_to_linker))
    .add_plugins(WitGeneratorPlugin::default())
    .add_systems(Startup, (spawn_entities, load_mods))
    .run();
}

fn spawn_entities(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>,) {
    commands.spawn(Health {
        current: 5.0,
        max: 10.0,
    });
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(4.0))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));
    // cube
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(124, 144, 255))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
    // light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn load_mods(mut mods: Mods) {
    mods.load("mods/guest_wit_example.wasm");
}