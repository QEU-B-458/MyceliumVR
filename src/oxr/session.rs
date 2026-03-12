use std::ffi::c_void;

use crate::oxr::next_chain::{OxrNextChain, OxrNextChainStructBase, OxrNextChainStructProvider};
use crate::oxr::resources::{OxrPassthrough, OxrPassthroughLayerFB, OxrSwapchain};
use crate::oxr::types::{Result, SwapchainCreateInfo};
use bevy_derive::Deref;
use bevy_ecs::resource::Resource;
use openxr::AnyGraphics;

use crate::oxr::graphics::{graphics_match, GraphicsExt, GraphicsType, GraphicsWrap};

/// Graphics agnostic wrapper around [openxr::Session].
///
/// See [`openxr::Session`] for other available methods.
#[derive(Resource, Deref, Clone)]
pub struct OxrSession(
    #[deref]
    pub(crate) openxr::Session<AnyGraphics>,
    pub(crate) GraphicsWrap<Self>,
);

impl GraphicsType for OxrSession {
    type Inner<G: GraphicsExt> = openxr::Session<G>;
}

impl<G: GraphicsExt> From<openxr::Session<G>> for OxrSession {
    fn from(session: openxr::Session<G>) -> Self {
        Self::from_inner(session)
    }
}

impl OxrSession {
    pub fn from_inner<G: GraphicsExt>(session: openxr::Session<G>) -> Self {
        Self(session.clone().into_any_graphics(), G::wrap(session))
    }

    pub fn typed_session(&self) -> &GraphicsWrap<Self> {
        &self.1
    }

    pub fn enumerate_swapchain_formats(&self) -> Result<Vec<wgpu::TextureFormat>> {
        graphics_match!(
            &self.1;
            session => Ok(session.enumerate_swapchain_formats()?.into_iter().filter_map(Api::into_wgpu_format).collect())
        )
    }

    pub fn create_swapchain(&self, info: SwapchainCreateInfo) -> Result<OxrSwapchain> {
        Ok(OxrSwapchain(graphics_match!(
            &self.1;
            session => session.create_swapchain(&info.try_into()?)? => OxrSwapchain
        )))
    }

    pub fn create_passthrough(&self, flags: openxr::PassthroughFlagsFB) -> Result<OxrPassthrough> {
        Ok(OxrPassthrough(
            graphics_match! {
                &self.1;
                session => session.create_passthrough(flags)?
            },
            flags,
        ))
    }

    pub fn create_passthrough_layer(
        &self,
        passthrough: &OxrPassthrough,
        purpose: openxr::PassthroughLayerPurposeFB,
    ) -> Result<OxrPassthroughLayerFB> {
        Ok(OxrPassthroughLayerFB(graphics_match! {
            &self.1;
            session => session.create_passthrough_layer(&passthrough.0, passthrough.1, purpose)?
        }))
    }
}

pub trait OxrSessionCreateNextProvider: OxrNextChainStructProvider {}

/// NonSend Resource
#[derive(Default)]
pub struct OxrSessionCreateNextChain(OxrNextChain);

impl OxrSessionCreateNextChain {
    pub fn push<T: OxrSessionCreateNextProvider>(&mut self, info_struct: T) {
        self.0.push(info_struct)
    }
    pub fn chain(&self) -> Option<&OxrNextChainStructBase> {
        self.0.chain()
    }
    pub fn chain_pointer(&self) -> *const c_void {
        self.0.chain_pointer()
    }
}
