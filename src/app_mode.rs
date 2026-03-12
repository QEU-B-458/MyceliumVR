use bevy::prelude::*;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_log::info;

use crate::oxr::helper_traits::ToTransform;
use crate::oxr::resources::OxrViews;
use crate::xr::camera::XrCamera;
use crate::xr::session::*;

/// Controls the current rendering mode.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AppMode {
    /// Standard flat-screen desktop rendering.
    Flat,
    /// VR rendering via OpenXR.
    #[default]
    Vr,
}

/// Controls what the desktop window shows while in VR mode.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DesktopMirror {
    /// Window is not actively rendered during VR.
    #[default]
    Disabled,
    /// Mirror one of the VR eyes to the desktop window (0 = left, 1 = right).
    Eye(u32),
    /// Use a specific scene camera entity as the desktop window view.
    SceneCamera(Entity),
}

/// Marker for the primary flat-mode camera (the one that renders to the window).
#[derive(Component, Default)]
pub struct FlatModeCamera;

/// Marker for the mirror camera that shows VR content on the desktop window.
#[derive(Component, Default)]
pub struct MirrorCamera;

/// Marker for a camera that can be used as a desktop view source during VR.
/// Spawn any Camera3d with this component, and set `DesktopMirror::SceneCamera(entity)`
/// to route its output to the desktop window.
#[derive(Component, Default)]
pub struct DesktopViewCamera;

/// Flat-mode camera controller state.
#[derive(Resource)]
pub struct FlatModeController {
    pub move_speed: f32,
    pub look_speed: f32,
    pub pitch: f32,
    pub yaw: f32,
}

impl Default for FlatModeController {
    fn default() -> Self {
        Self {
            move_speed: 5.0,
            look_speed: 0.3,
            pitch: -0.4, // slight downward look to match the initial camera angle
            yaw: std::f32::consts::FRAC_PI_2 * -0.3,
        }
    }
}

pub struct AppModePlugin {
    pub start_in_vr: bool,
}

impl Default for AppModePlugin {
    fn default() -> Self {
        Self { start_in_vr: true }
    }
}

impl Plugin for AppModePlugin {
    fn build(&self, app: &mut App) {
        let initial_mode = if self.start_in_vr {
            AppMode::Vr
        } else {
            AppMode::Flat
        };

        app.insert_resource(initial_mode)
            .insert_resource(DesktopMirror::default())
            .init_resource::<FlatModeController>()
            .add_systems(
                PreUpdate,
                (
                    mode_aware_session_handler,
                    sync_cameras_to_mode,
                    update_mirror_camera.run_if(|mode: Res<AppMode>| *mode == AppMode::Vr),
                )
                    .chain(),
            )
            .add_systems(
                Update,
                flat_mode_input
                    .run_if(|mode: Res<AppMode>| *mode == AppMode::Flat),
            );
    }
}

/// Replaces auto_handle_session with mode-aware logic.
/// When the user removes the headset (STOPPING), we switch to flat mode.
/// When the headset comes back (READY while in flat mode from headset removal), we switch to VR.
pub fn mode_aware_session_handler(
    mut state_changed: MessageReader<XrStateChanged>,
    mut mode: ResMut<AppMode>,
    xr_state: Option<Res<XrState>>,
    mut create_session: MessageWriter<XrCreateSessionMessage>,
    mut begin_session: MessageWriter<XrBeginSessionMessage>,
    mut end_session: MessageWriter<XrEndSessionMessage>,
    mut destroy_session: MessageWriter<XrDestroySessionMessage>,
) {
    for XrStateChanged(state) in state_changed.read() {
        match state {
            XrState::Available => {
                if *mode == AppMode::Vr {
                    info!("XR available, creating session (VR mode)");
                    create_session.write_default();
                }
                // In flat mode, we don't auto-create. User must switch to VR explicitly.
            }
            XrState::Ready => {
                if *mode == AppMode::Vr {
                    info!("XR ready, beginning session");
                    begin_session.write_default();
                } else {
                    // Headset was put back on while in flat mode.
                    // Auto-switch back to VR.
                    info!("Headset detected, switching from flat to VR mode");
                    *mode = AppMode::Vr;
                    begin_session.write_default();
                }
            }
            XrState::Stopping => {
                info!("Headset removed, switching to flat mode");
                *mode = AppMode::Flat;
                end_session.write_default();
            }
            XrState::Exiting { should_restart } => {
                destroy_session.write_default();
                if !should_restart {
                    // If not restarting, stay in flat mode
                    *mode = AppMode::Flat;
                }
            }
            _ => (),
        }
    }

    // Handle the case where user programmatically switches to VR while XR is available
    // but no session exists yet.
    if mode.is_changed() && *mode == AppMode::Vr {
        if let Some(state) = &xr_state {
            match **state {
                XrState::Available => {
                    info!("Mode switched to VR, creating session");
                    create_session.write_default();
                }
                XrState::Ready => {
                    info!("Mode switched to VR, beginning session");
                    begin_session.write_default();
                }
                _ => {}
            }
        }
    }
}

/// Toggles cameras on/off based on AppMode and DesktopMirror.
pub fn sync_cameras_to_mode(
    mode: Res<AppMode>,
    mirror: Res<DesktopMirror>,
    mut flat_cameras: Query<
        &mut Camera,
        (
            With<FlatModeCamera>,
            Without<XrCamera>,
            Without<MirrorCamera>,
        ),
    >,
    mut mirror_cameras: Query<&mut Camera, (With<MirrorCamera>, Without<XrCamera>, Without<FlatModeCamera>)>,
) {
    if !mode.is_changed() && !mirror.is_changed() {
        return;
    }

    let is_vr = *mode == AppMode::Vr;

    // Flat camera: active only in flat mode
    for mut cam in flat_cameras.iter_mut() {
        cam.is_active = !is_vr;
    }

    // Mirror camera: active only in VR + SceneCamera mode.
    // Eye mode is handled by the GPU blit (MirrorBlitPlugin), not by re-rendering.
    let mirror_active = is_vr && matches!(*mirror, DesktopMirror::SceneCamera(_));
    for mut cam in mirror_cameras.iter_mut() {
        cam.is_active = mirror_active;
    }

    if mode.is_changed() {
        info!("App mode changed to {:?}", *mode);
    }
}

/// Updates the mirror camera transform to follow a VR eye or scene camera.
pub fn update_mirror_camera(
    mirror: Res<DesktopMirror>,
    views: Option<Res<OxrViews>>,
    mut mirror_cameras: Query<&mut Transform, With<MirrorCamera>>,
    scene_cameras: Query<&Transform, (With<DesktopViewCamera>, Without<MirrorCamera>)>,
) {
    match *mirror {
        DesktopMirror::Eye(index) => {
            let Some(views) = views else { return };
            let Some(view) = views.get(index as usize) else {
                return;
            };
            let target_transform = view.pose.to_transform();
            for mut transform in mirror_cameras.iter_mut() {
                *transform = target_transform;
            }
        }
        DesktopMirror::SceneCamera(entity) => {
            if let Ok(source_transform) = scene_cameras.get(entity) {
                let source = *source_transform;
                for mut transform in mirror_cameras.iter_mut() {
                    *transform = source;
                }
            }
        }
        DesktopMirror::Disabled => {}
    }
}

/// Simple WASD + mouse-look controller for flat mode.
pub fn flat_mode_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut controller: ResMut<FlatModeController>,
    mut cameras: Query<&mut Transform, With<FlatModeCamera>>,
    windows: Query<&Window>,
) {
    let Ok(_window) = windows.single() else {
        return;
    };

    // Mouse look: only when right mouse button is held (via cursor lock or similar)
    // For now, use arrow keys for looking to avoid cursor grab complexity
    let look_speed = controller.look_speed * time.delta_secs();
    if keyboard.pressed(KeyCode::ArrowLeft) {
        controller.yaw += look_speed;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        controller.yaw -= look_speed;
    }
    if keyboard.pressed(KeyCode::ArrowUp) {
        controller.pitch += look_speed;
    }
    if keyboard.pressed(KeyCode::ArrowDown) {
        controller.pitch -= look_speed;
    }

    // Clamp pitch
    controller.pitch = controller.pitch.clamp(
        -std::f32::consts::FRAC_PI_2 + 0.01,
        std::f32::consts::FRAC_PI_2 - 0.01,
    );

    let rotation = Quat::from_euler(bevy_math::EulerRot::YXZ, controller.yaw, controller.pitch, 0.0);

    let move_speed = controller.move_speed * time.delta_secs();
    let forward = rotation * Vec3::NEG_Z;
    let right = rotation * Vec3::X;

    let mut movement = Vec3::ZERO;
    if keyboard.pressed(KeyCode::KeyW) {
        movement += forward;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        movement -= forward;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        movement += right;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        movement -= right;
    }
    if keyboard.pressed(KeyCode::Space) {
        movement += Vec3::Y;
    }
    if keyboard.pressed(KeyCode::ShiftLeft) {
        movement -= Vec3::Y;
    }

    if movement.length_squared() > 0.0 {
        movement = movement.normalize() * move_speed;
    }

    for mut transform in cameras.iter_mut() {
        transform.translation += movement;
        transform.rotation = rotation;
    }
}
