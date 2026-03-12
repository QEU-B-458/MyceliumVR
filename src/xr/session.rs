use std::convert::identity;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use bevy_app::{App, AppExit, MainScheduleOrder, Plugin, PostUpdate, PreUpdate};
use bevy_camera::visibility::Visibility;
use bevy_derive::Deref;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::hierarchy::Children;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::message::{Message, MessageReader, MessageWriter};
use bevy_ecs::query::{Has, With};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::schedule::{
    ExecutorKind, IntoScheduleConfigs as _, Schedule, ScheduleLabel, SystemCondition as _, SystemSet
};
use bevy_ecs::system::{Local, Query, Res, ResMut};
use bevy_ecs::world::DeferredWorld;
use bevy_reflect::Reflect;
use bevy_render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy_render::{Render, RenderApp, RenderSystems};
use bevy_transform::components::{GlobalTransform, Transform};
use bevy_transform::TransformSystems;

/// Message sent to instruct backends to create an XR session.
#[derive(Message, Clone, Copy, Default)]
pub struct XrCreateSessionMessage;

/// A schedule thats ran whenever an [`XrCreateSessionMessage`] is recieved.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
pub struct XrSessionCreated;

/// Message sent after the XrSession was created.
#[derive(Message, Clone, Copy, Default)]
pub struct XrSessionCreatedMessage;

/// Message sent to instruct backends to destroy an XR session.
#[derive(Message, Clone, Copy, Default)]
pub struct XrDestroySessionMessage;

/// Resource flag for informing the render world to drop session resources.
#[derive(Resource, Clone, Default)]
pub struct XrDestroySessionRender(pub Arc<AtomicBool>);

/// Schedule thats ran whenever the XrSession is about to be destroyed
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
pub struct XrPreDestroySession;

/// Message sent to instruct backends to begin an XR session.
#[derive(Message, Clone, Copy, Default)]
pub struct XrBeginSessionMessage;

/// Schedule thats ran when the XrSession has begun.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
pub struct XrPostSessionBegin;

/// Message sent to backends to end an XR session.
#[derive(Message, Clone, Copy, Default)]
pub struct XrEndSessionMessage;

/// Schedule thats ran whenever the XrSession is about to end
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
pub struct XrPreSessionEnd;

/// Message that is emitted when the XrSession is fully destroyed
#[derive(Message, Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
pub struct XrSessionDestroyedMessage;

/// Message sent to backends to request the XrState proceed to Exiting.
#[derive(Message, Clone, Copy, Default)]
pub struct XrRequestExitMessage;

/// Schedule ran before [`First`] to handle XR events.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
pub struct XrFirst;

/// System sets for systems related to handling XR session events.
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, SystemSet)]
pub enum XrHandleEvents {
    Poll,
    ExitEvents,
    SessionStateUpdateEvents,
    Cleanup,
    FrameLoop,
}

/// System sets ran in the render world for XR.
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, SystemSet)]
pub enum XrRenderSystems {
    HandleEvents,
    PreRender,
    PostRender,
}

/// The root transform's global position for late latching in the render world.
#[derive(ExtractResource, Resource, Clone, Copy, Default)]
pub struct XrRootTransform(pub GlobalTransform);

/// Component used to specify the entity we should use as the tracking root.
#[derive(Component)]
#[require(Transform, Visibility)]
pub struct XrTrackingRoot;
#[derive(Resource)]
struct TrackingRootRes(Entity);

/// Makes the entity a child of the XrTrackingRoot if the entity has no parent
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug, Default, Component, Reflect)]
#[component(on_add = on_tracker_add)]
pub struct XrTracker;
fn on_tracker_add(mut world: DeferredWorld, HookContext { entity, .. }: HookContext) {
    if world
        .entity(entity)
        .get_components::<Has<Children>>()
        .is_ok_and(identity)
    {
        return;
    }
    let Some(root) = world.get_resource::<TrackingRootRes>().map(|r| r.0) else {
        return;
    };
    world.commands().entity(root).add_child(entity);
}

pub struct XrSessionPlugin {
    pub auto_handle: bool,
}

impl Plugin for XrSessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<XrDestroySessionRender>();
        let mut xr_first = Schedule::new(XrFirst);
        xr_first.set_executor_kind(ExecutorKind::SingleThreaded);
        app.add_message::<XrCreateSessionMessage>()
            .add_message::<XrDestroySessionMessage>()
            .add_message::<XrBeginSessionMessage>()
            .add_message::<XrEndSessionMessage>()
            .add_message::<XrRequestExitMessage>()
            .add_message::<XrStateChanged>()
            .add_message::<XrSessionCreatedMessage>()
            .add_message::<XrSessionDestroyedMessage>()
            .init_schedule(XrSessionCreated)
            .init_schedule(XrPreDestroySession)
            .init_schedule(XrPostSessionBegin)
            .init_schedule(XrPreSessionEnd)
            .add_schedule(xr_first)
            .configure_sets(
                XrFirst,
                (
                    XrHandleEvents::Poll,
                    XrHandleEvents::ExitEvents,
                    XrHandleEvents::SessionStateUpdateEvents,
                    XrHandleEvents::Cleanup,
                    XrHandleEvents::FrameLoop,
                )
                    .chain(),
            )
            .add_systems(
                XrFirst,
                exits_session_on_app_exit
                    .run_if(on_message::<AppExit>)
                    .run_if(session_created)
                    .in_set(XrHandleEvents::ExitEvents),
            );
        let root = app.world_mut().spawn(XrTrackingRoot).id();
        app.world_mut().insert_resource(TrackingRootRes(root));
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .labels
            .insert(0, XrFirst.intern());

        if self.auto_handle {
            app.add_systems(PreUpdate, auto_handle_session);
        }
    }

    fn finish(&self, app: &mut App) {
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }

        app.add_plugins((
            ExtractResourcePlugin::<XrState>::default(),
            ExtractResourcePlugin::<XrRootTransform>::default(),
        ))
        .init_resource::<XrRootTransform>()
        .add_systems(
            PostUpdate,
            update_root_transform.after(TransformSystems::Propagate),
        )
        .add_systems(
            XrFirst,
            exits_session_on_app_exit
                .before(XrHandleEvents::ExitEvents)
                .run_if(on_message::<AppExit>.and(session_running)),
        );

        let render_app = app.sub_app_mut(RenderApp);

        render_app
            .init_schedule(XrPreDestroySession)
            .configure_sets(
                Render,
                (XrRenderSystems::HandleEvents, XrRenderSystems::PreRender).chain(),
            )
            .configure_sets(
                Render,
                XrRenderSystems::HandleEvents.after(RenderSystems::ExtractCommands),
            )
            .configure_sets(
                Render,
                XrRenderSystems::PreRender
                    .before(RenderSystems::ManageViews)
                    .before(RenderSystems::PrepareAssets),
            )
            .configure_sets(
                Render,
                XrRenderSystems::PostRender
                    .after(RenderSystems::Render)
                    .before(RenderSystems::Cleanup),
            );
    }
}

fn exits_session_on_app_exit(mut request_exit: MessageWriter<XrRequestExitMessage>) {
    request_exit.write_default();
}

/// Message sent by backends whenever [`XrState`] is changed.
#[derive(Message, Clone, Copy, Deref)]
pub struct XrStateChanged(pub XrState);

/// A resource representing the current session state.
#[derive(Clone, Copy, Debug, ExtractResource, Resource, PartialEq, Eq)]
#[repr(u8)]
pub enum XrState {
    Unavailable,
    Available,
    Idle,
    Ready,
    Running,
    Stopping,
    Exiting {
        should_restart: bool,
    },
}

pub fn auto_handle_session(
    mut state_changed: MessageReader<XrStateChanged>,
    mut create_session: MessageWriter<XrCreateSessionMessage>,
    mut begin_session: MessageWriter<XrBeginSessionMessage>,
    mut end_session: MessageWriter<XrEndSessionMessage>,
    mut destroy_session: MessageWriter<XrDestroySessionMessage>,
    mut no_auto_restart: Local<bool>,
) {
    for XrStateChanged(state) in state_changed.read() {
        match state {
            XrState::Available => {
                if !*no_auto_restart {
                    create_session.write_default();
                }
            }
            XrState::Ready => {
                begin_session.write_default();
            }
            XrState::Stopping => {
                end_session.write_default();
            }
            XrState::Exiting { should_restart } => {
                *no_auto_restart = !should_restart;
                destroy_session.write_default();
            }
            _ => (),
        }
    }
}

pub fn update_root_transform(
    mut root_transform: ResMut<XrRootTransform>,
    root: Query<&GlobalTransform, With<XrTrackingRoot>>,
) {
    let Ok(transform) = root.single() else {
        return;
    };

    root_transform.0 = *transform;
}

pub fn status_changed_to(
    status: XrState,
) -> impl FnMut(MessageReader<XrStateChanged>) -> bool + Clone {
    move |mut reader: MessageReader<XrStateChanged>| {
        reader.read().any(|new_status| new_status.0 == status)
    }
}

pub fn session_available(status: Option<Res<XrState>>) -> bool {
    status.is_some_and(|s| *s != XrState::Unavailable)
}

pub fn session_created(status: Option<Res<XrState>>) -> bool {
    !matches!(
        status.as_deref(),
        Some(XrState::Unavailable | XrState::Available) | None
    )
}

pub fn session_ready_or_running(status: Option<Res<XrState>>) -> bool {
    matches!(status.as_deref(), Some(XrState::Ready | XrState::Running))
}

pub fn session_running(status: Option<Res<XrState>>) -> bool {
    matches!(status.as_deref(), Some(XrState::Running))
}

pub fn state_equals(status: XrState) -> impl FnMut(Option<Res<XrState>>) -> bool {
    move |state: Option<Res<XrState>>| state.is_some_and(|s| *s == status)
}

#[macro_export]
macro_rules! state_matches {
    ($match:pat) => {
        |state: Option<bevy_ecs::system::Res<crate::xr::session::XrState>>| core::matches!(state.as_deref(), Some($match))
    };
}


