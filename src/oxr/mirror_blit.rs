use bevy_app::{App, Plugin};
use bevy_camera::ManualTextureViewHandle;
use bevy_ecs::change_detection::Mut;
use bevy_ecs::prelude::*;
use bevy_render::renderer::{RenderDevice, RenderQueue};
use bevy_render::texture::ManualTextureViews;
use bevy_render::view::window::ExtractedWindows;
use bevy_render::{Extract, ExtractSchedule, Render, RenderApp};

use crate::app_mode::DesktopMirror;
use crate::oxr::init::should_run_frame_loop;
use crate::oxr::render::XR_TEXTURE_INDEX;
use crate::oxr::resources::OxrFrameState;
use crate::xr::session::XrRenderSystems;

/// Extracted mirror config for the render world — just the eye index if enabled.
#[derive(Resource, Clone, Copy, Default)]
struct MirrorBlitConfig {
    eye_index: Option<u32>,
}

/// GPU resources for the fullscreen blit pipeline.
#[derive(Resource)]
struct MirrorBlitPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

/// Copies a rendered VR eye texture directly to the desktop window surface.
/// Replaces the old MirrorCamera approach (which re-rendered the entire scene).
pub struct MirrorBlitPlugin;

impl Plugin for MirrorBlitPlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .init_resource::<MirrorBlitConfig>()
            .add_systems(ExtractSchedule, extract_mirror_config)
            .add_systems(
                Render,
                mirror_blit
                    .run_if(should_run_frame_loop)
                    .in_set(XrRenderSystems::PostRender)
                    .before(super::render::release_image),
            );
    }
}

fn extract_mirror_config(mut commands: Commands, mirror: Extract<Res<DesktopMirror>>) {
    let config = match **mirror {
        DesktopMirror::Eye(idx) => MirrorBlitConfig {
            eye_index: Some(idx),
        },
        _ => MirrorBlitConfig { eye_index: None },
    };
    commands.insert_resource(config);
}

fn create_blit_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> MirrorBlitPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mirror_blit_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("mirror_blit.wgsl").into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mirror_blit_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mirror_blit_layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mirror_blit_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("mirror_blit_sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    MirrorBlitPipeline {
        pipeline,
        bind_group_layout,
        sampler,
    }
}

/// Exclusive-world system: reads the XR eye texture and blits it to the desktop window.
fn mirror_blit(world: &mut World) {
    // 1. Check if blit is enabled
    let eye_index = match world.get_resource::<MirrorBlitConfig>() {
        Some(config) => match config.eye_index {
            Some(idx) => idx,
            None => return,
        },
        None => return,
    };

    // 2. Check frame state
    let should_render = world
        .get_resource::<OxrFrameState>()
        .is_some_and(|fs| fs.should_render);
    if !should_render {
        return;
    }

    // 3. Clone the XR eye texture view (Arc-based, cheap)
    let xr_view = {
        let views = world.resource::<ManualTextureViews>();
        let handle = ManualTextureViewHandle(XR_TEXTURE_INDEX + eye_index);
        match views.get(&handle) {
            Some(v) => v.texture_view.clone(),
            None => return,
        }
    };

    // 4. Clone the window surface texture view and get format
    let (target_view, target_format, primary_entity) = {
        let windows = world.resource::<ExtractedWindows>();
        let primary = match windows.primary {
            Some(e) => e,
            None => return,
        };
        let window = match windows.get(&primary) {
            Some(w) => w,
            None => return,
        };
        let view = match &window.swap_chain_texture_view {
            Some(v) => v.clone(),
            None => return,
        };
        let format = window
            .swap_chain_texture_view_format
            .or(window.swap_chain_texture_format)
            .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
        (view, format, primary)
    };

    // 5. Lazily create the pipeline on first use
    if !world.contains_resource::<MirrorBlitPipeline>() {
        let device = world.resource::<RenderDevice>();
        let pipeline = create_blit_pipeline(device.wgpu_device(), target_format);
        world.insert_resource(pipeline);
    }

    // 6. Render the blit
    world.resource_scope(|world, pipeline: Mut<MirrorBlitPipeline>| {
        let device = world.resource::<RenderDevice>();
        let raw_device = device.wgpu_device();

        let bind_group = raw_device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mirror_blit_bg"),
            layout: &pipeline.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&*xr_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                },
            ],
        });

        let mut encoder = raw_device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mirror_blit_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mirror_blit_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &*target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        let queue = world.resource::<RenderQueue>();
        queue.submit(std::iter::once(encoder.finish()));
    });

    // 7. Present the window
    let mut windows = world.resource_mut::<ExtractedWindows>();
    if let Some(window) = windows.windows.get_mut(&primary_entity) {
        window.present();
    }
}
