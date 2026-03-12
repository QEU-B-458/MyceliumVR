use bevy_derive::{Deref, DerefMut};
use bevy_ecs::resource::Resource;
use bevy_log::error;
use bevy_math::UVec2;
use bevy_render::extract_resource::ExtractResource;

use crate::oxr::error::OxrError;
use crate::oxr::graphics::*;
use crate::oxr::layer_builder::{CompositionLayer, LayerProvider};
use crate::oxr::session::{OxrSession, OxrSessionCreateNextChain};
use crate::oxr::types::Result as OxrResult;
use crate::oxr::types::*;

/// Wrapper around an [`Entry`](openxr::Entry) with some methods overridden to use bevy types.
#[derive(Deref, Clone)]
pub struct OxrEntry(pub openxr::Entry);

impl OxrEntry {
    pub fn enumerate_extensions(&self) -> OxrResult<OxrExtensions> {
        Ok(self.0.enumerate_extensions().map(Into::into)?)
    }

    pub fn create_instance(
        &self,
        app_info: AppInfo,
        exts: OxrExtensions,
        layers: &[&str],
        backend: GraphicsBackend,
    ) -> OxrResult<OxrInstance> {
        let available_exts = self.enumerate_extensions()?;

        if !backend.is_available(&available_exts) {
            return Err(OxrError::UnavailableBackend(backend));
        }

        let required_exts = exts | backend.required_exts();

        let instance = self.0.create_instance(
            &openxr::ApplicationInfo {
                application_name: &app_info.name,
                application_version: app_info.version.to_u32(),
                engine_name: "Bevy",
                engine_version: Version::BEVY.to_u32(),
                api_version: openxr::Version::new(1, 0, 34),
            },
            &required_exts.into(),
            layers,
        )?;

        Ok(OxrInstance(instance, backend, app_info))
    }

    pub fn available_backends(&self) -> OxrResult<Vec<GraphicsBackend>> {
        Ok(GraphicsBackend::available_backends(
            &self.enumerate_extensions()?,
        ))
    }
}

/// Wrapper around [`openxr::Instance`] with additional data for safety and some methods overriden to use bevy types.
#[derive(Resource, Deref, Clone)]
pub struct OxrInstance(
    #[deref] pub(crate) openxr::Instance,
    pub(crate) GraphicsBackend,
    pub(crate) AppInfo,
);

impl OxrInstance {
    pub unsafe fn from_inner(
        instance: openxr::Instance,
        backend: GraphicsBackend,
        info: AppInfo,
    ) -> Self {
        Self(instance, backend, info)
    }

    pub fn into_inner(self) -> openxr::Instance {
        self.0
    }

    pub fn backend(&self) -> GraphicsBackend {
        self.1
    }

    pub fn app_info(&self) -> &AppInfo {
        &self.2
    }

    pub fn init_graphics(
        &self,
        system_id: openxr::SystemId,
        manual_config: Option<&OxrManualGraphicsConfig>,
    ) -> OxrResult<(WgpuGraphics, SessionGraphicsCreateInfo)> {
        graphics_match!(
            self.1;
            _ => {
                let (graphics, session_info) = Api::init_graphics(&self.2, self, system_id, manual_config)?;

                Ok((graphics, SessionGraphicsCreateInfo(Api::wrap(session_info))))
            }
        )
    }

    pub unsafe fn create_session(
        &self,
        system_id: openxr::SystemId,
        info: SessionGraphicsCreateInfo,
        chain: &mut OxrSessionCreateNextChain,
    ) -> OxrResult<(OxrSession, OxrFrameWaiter, OxrFrameStream)> {
        if !info.0.using_graphics_of_val(&self.1) {
            return OxrResult::Err(OxrError::GraphicsBackendMismatch {
                item: std::any::type_name::<SessionGraphicsCreateInfo>(),
                backend: info.0.graphics_name(),
                expected_backend: self.1.graphics_name(),
            });
        }
        graphics_match!(
            info.0;
            info => {
                let (session, frame_waiter, frame_stream) = unsafe { Api::create_session(self, system_id, &info, chain)? };
                Ok((session.into(), OxrFrameWaiter(frame_waiter), OxrFrameStream(Api::wrap(frame_stream))))
            }
        )
    }
}

/// Graphics agnostic wrapper around [openxr::FrameStream]
#[derive(Resource)]
pub struct OxrFrameStream(pub GraphicsWrap<Self>);

impl GraphicsType for OxrFrameStream {
    type Inner<G: GraphicsExt> = openxr::FrameStream<G>;
}

impl OxrFrameStream {
    pub fn from_inner<G: GraphicsExt>(frame_stream: openxr::FrameStream<G>) -> Self {
        Self(G::wrap(frame_stream))
    }

    pub fn begin(&mut self) -> openxr::Result<()> {
        graphics_match!(
            &mut self.0;
            stream => stream.begin()
        )
    }

    pub fn end(
        &mut self,
        display_time: openxr::Time,
        environment_blend_mode: openxr::EnvironmentBlendMode,
        layers: &[&dyn CompositionLayer],
    ) -> OxrResult<()> {
        graphics_match!(
            &mut self.0;
            stream => {
                let mut new_layers = vec![];

                for (i, layer) in layers.iter().enumerate() {
                    if let Some(swapchain) = layer.swapchain()
                        && !swapchain.0.using_graphics::<Api>() {
                        error!(
                            "Composition layer {i} is using graphics api '{}', expected graphics api '{}'. Excluding layer from frame submission.",
                            swapchain.0.graphics_name(),
                            std::any::type_name::<Api>(),
                        );
                        continue;
                    }
                    new_layers.push(unsafe {
                        #[allow(clippy::missing_transmute_annotations)]
                        std::mem::transmute(layer.header())
                    });
                }

                Ok(stream.end(display_time, environment_blend_mode, new_layers.as_slice())?)
            }
        )
    }
}

/// Handle for waiting to render a frame.
#[derive(Resource, Deref, DerefMut)]
pub struct OxrFrameWaiter(pub openxr::FrameWaiter);

/// Graphics agnostic wrapper around [openxr::Swapchain]
#[derive(Resource)]
pub struct OxrSwapchain(pub GraphicsWrap<Self>);

impl GraphicsType for OxrSwapchain {
    type Inner<G: GraphicsExt> = openxr::Swapchain<G>;
}

impl OxrSwapchain {
    pub fn from_inner<G: GraphicsExt>(swapchain: openxr::Swapchain<G>) -> Self {
        Self(G::wrap(swapchain))
    }

    pub fn acquire_image(&mut self) -> OxrResult<u32> {
        graphics_match!(
            &mut self.0;
            swap => Ok(swap.acquire_image()?)
        )
    }

    pub fn wait_image(&mut self, timeout: openxr::Duration) -> OxrResult<()> {
        graphics_match!(
            &mut self.0;
            swap => Ok(swap.wait_image(timeout)?)
        )
    }

    pub fn release_image(&mut self) -> OxrResult<()> {
        graphics_match!(
            &mut self.0;
            swap => Ok(swap.release_image()?)
        )
    }

    pub fn enumerate_images(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        resolution: UVec2,
    ) -> OxrResult<OxrSwapchainImages> {
        graphics_match!(
            &self.0;
            swap => {
                let mut images = vec![];
                for image in swap.enumerate_images()? {
                    unsafe {
                        images.push(Api::to_wgpu_img(image, device, format, resolution)?);
                    }
                }
                Ok(OxrSwapchainImages(images.leak()))
            }
        )
    }
}

/// Stores the generated swapchain images.
#[derive(Debug, Deref, Resource, Clone, Copy, ExtractResource)]
pub struct OxrSwapchainImages(pub &'static [wgpu::Texture]);

/// Stores the latest generated [OxrViews]
#[derive(Clone, Resource, ExtractResource, Deref, DerefMut, Default)]
pub struct OxrViews(pub Vec<openxr::View>);

/// Wrapper around [openxr::SystemId] to allow it to be stored as a resource.
#[derive(Debug, Copy, Clone, Deref, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Resource)]
pub struct OxrSystemId(pub openxr::SystemId);

/// Wrapper around [`openxr::Passthrough`].
#[derive(Resource, Deref, DerefMut)]
pub struct OxrPassthrough(
    #[deref] pub openxr::Passthrough,
    pub openxr::PassthroughFlagsFB,
);

impl OxrPassthrough {
    pub fn from_inner(passthrough: openxr::Passthrough, flags: openxr::PassthroughFlagsFB) -> Self {
        Self(passthrough, flags)
    }
}

/// Wrapper around [`openxr::PassthroughLayerFB`].
#[derive(Resource, Deref, DerefMut)]
pub struct OxrPassthroughLayerFB(pub openxr::PassthroughLayerFB);

#[derive(Resource, Deref, DerefMut, Default)]
pub struct OxrRenderLayers(pub Vec<Box<dyn LayerProvider + Send + Sync>>);

/// Resource storing graphics info for the currently running session.
#[derive(Clone, Copy, Resource, ExtractResource)]
pub struct OxrCurrentSessionConfig {
    pub resolution: UVec2,
    pub format: wgpu::TextureFormat,
}

#[derive(Clone, Resource, Debug)]
pub struct OxrSessionConfig {
    pub blend_mode_preference: Vec<EnvironmentBlendMode>,
    pub formats: Option<Vec<wgpu::TextureFormat>>,
    pub resolutions: Option<Vec<UVec2>>,
}
impl Default for OxrSessionConfig {
    fn default() -> Self {
        Self {
            blend_mode_preference: vec![openxr::EnvironmentBlendMode::OPAQUE],
            formats: Some(vec![wgpu::TextureFormat::Rgba8UnormSrgb]),
            resolutions: None,
        }
    }
}

/// Info needed to create a session.
#[derive(Clone)]
pub struct SessionGraphicsCreateInfo(pub GraphicsWrap<Self>);

impl GraphicsType for SessionGraphicsCreateInfo {
    type Inner<G: GraphicsExt> = G::SessionCreateInfo;
}

#[derive(ExtractResource, Resource, Clone, Default)]
pub struct OxrSessionStarted(pub bool);

/// The frame state returned from [FrameWaiter::wait_frame](openxr::FrameWaiter::wait)
#[derive(Clone, Deref, DerefMut, Resource, ExtractResource)]
pub struct OxrFrameState(pub openxr::FrameState);

/// Instructs systems to add display period
#[derive(Clone, Copy, Default, Resource)]
pub struct Pipelined;
