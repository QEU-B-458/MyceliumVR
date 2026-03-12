use bevy_app::{PluginGroup, PluginGroupBuilder};
use bevy_ecs::system::Res;
use bevy_render::RenderPlugin;
use bevy_window::{PresentMode, Window, WindowPlugin};
use crate::xr::session::{XrSessionPlugin, XrState};
use crate::xr::camera::XrCameraPlugin;
use self::{features::handtracking::HandTrackingPlugin, reference_space::OxrReferenceSpacePlugin};
use init::OxrInitPlugin;
use poll_events::OxrEventsPlugin;
use render::OxrRenderPlugin;
use resources::OxrInstance;
use session::OxrSession;

pub mod action_binding;
pub mod action_set_attaching;
pub mod action_set_syncing;
pub mod environment_blend_mode;
pub mod error;
pub mod exts;
pub mod features;
pub mod graphics;
pub mod helper_traits;
pub mod init;
pub mod layer_builder;
pub mod mirror_blit;
pub mod next_chain;
pub mod poll_events;
pub mod reference_space;
pub mod render;
pub mod resources;
pub mod session;
pub mod spaces;
pub mod types;

/// A [`Condition`](bevy::ecs::schedule::Condition) system that says if the OpenXR session is available.
pub fn openxr_session_available(
    status: Option<Res<XrState>>,
    instance: Option<Res<OxrInstance>>,
) -> bool {
    status.is_some_and(|s| *s != XrState::Unavailable) && instance.is_some()
}

/// A [`Condition`](bevy::ecs::schedule::Condition) system that says if the OpenXR is running.
/// use this when working with OpenXR specific things
pub fn openxr_session_running(
    status: Option<Res<XrState>>,
    session: Option<Res<OxrSession>>,
) -> bool {
    matches!(status.as_deref(), Some(XrState::Running)) & session.is_some()
}

pub fn add_xr_plugins<G: PluginGroup>(plugins: G) -> PluginGroupBuilder {
    let plugins = plugins
        .build()
        .disable::<RenderPlugin>()
        .add_before::<RenderPlugin>(XrSessionPlugin { auto_handle: false })
        .add_before::<RenderPlugin>(OxrInitPlugin::default())
        .add(OxrEventsPlugin)
        .add(OxrReferenceSpacePlugin::default())
        .add(OxrRenderPlugin::default())
        .add(HandTrackingPlugin::default())
        .add(XrCameraPlugin)
        .add(action_set_attaching::OxrActionAttachingPlugin)
        .add(action_binding::OxrActionBindingPlugin)
        .add(action_set_syncing::OxrActionSyncingPlugin)
        .add(features::overlay::OxrOverlayPlugin)
        .add(spaces::OxrSpatialPlugin)
        .add(spaces::OxrSpacePatchingPlugin)
        .set(WindowPlugin {
            primary_window: Some(Window {
                transparent: true,
                present_mode: PresentMode::AutoNoVsync,
                ..Default::default()
            }),
            ..Default::default()
        });
    plugins
}
