//! The renderer for the effects next to Halqa: one pipeline per [`Kind`], all sharing one uniform layout
//! (`common.wgsl`), one card texture and one sampler.
//!
//! A main layer clears the target and writes without blending, like W2's Halqa renderer, so shading a pixel twice
//! where two regions overlap is harmless. An overlay layer ([`Kind::is_overlay`], the many-files stack) loads the
//! target, blends premultiplied alpha over it, and is drawn once over the union of its regions.

use bytemuck::{Pod, Zeroable};

use super::Pose;
use super::math;
use crate::palette::Palette;
use crate::renderer::scissor;

/// Effect parameter slots in [`Scene::fx`] (`array<vec4<f32>, 8>` in WGSL).
pub const FX_LEN: usize = 8;

/// The shared WGSL prelude; each effect's source is appended to it.
pub const COMMON_WGSL: &str = include_str!("common.wgsl");

/// Which shader draws a [`Scene`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Oddiy,
    Kometa,
    Pufak,
    Stack,
}

impl Kind {
    /// Every kind, in pipeline order (`kind as usize` indexes it).
    pub const ALL: [Self; 4] = [Self::Oddiy, Self::Kometa, Self::Pufak, Self::Stack];

    /// The effect's own WGSL (without the prelude).
    #[must_use]
    pub const fn wgsl(self) -> &'static str {
        match self {
            Self::Oddiy => include_str!("oddiy.wgsl"),
            Self::Kometa => include_str!("kometa.wgsl"),
            Self::Pufak => include_str!("pufak.wgsl"),
            Self::Stack => include_str!("stack.wgsl"),
        }
    }

    /// Drawn over what is already in the target instead of clearing it.
    #[must_use]
    pub const fn is_overlay(self) -> bool {
        matches!(self, Self::Stack)
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Oddiy => "vutric.oddiy",
            Self::Kometa => "vutric.kometa",
            Self::Pufak => "vutric.pufak",
            Self::Stack => "vutric.stack",
        }
    }
}

/// The complete WGSL of one kind: the prelude followed by the effect.
#[must_use]
pub fn shader_source(kind: Kind) -> String {
    format!("{COMMON_WGSL}\n{}", kind.wgsl())
}

/// One frame of one effect, laid out like the WGSL `Scene` uniform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scene {
    pub kind: Kind,
    pub card: Pose,
    /// Effect parameters; each effect's module documents its slots.
    pub fx: [[f32; 4]; FX_LEN],
    /// Rectangles whose union covers every lit pixel; all-zero ones are skipped.
    pub regions: [[f32; 4]; 3],
}

impl Scene {
    /// One rectangle containing every lit pixel (all zero when nothing is lit).
    #[must_use]
    pub fn bounds(&self) -> [f32; 4] {
        self.regions
            .iter()
            .fold(math::EMPTY, |acc, r| math::union(acc, *r))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    screen: [f32; 4],
    card: [f32; 4],
    card_xf: [f32; 4],
    fx: [[f32; 4]; FX_LEN],
    color_a: [f32; 4],
    color_b: [f32; 4],
    color_c: [f32; 4],
}

/// Draws [`Scene`]s into any render target of the format given at construction. Like W2's
/// [`crate::Renderer`], it owns no surface.
#[derive(Debug)]
pub struct SceneRenderer {
    /// Indexed by `Kind as usize`.
    pipelines: Vec<wgpu::RenderPipeline>,
    /// Slot 0 for the main layer, slot 1 for an overlay, so both can be written before one submit.
    uniforms: [wgpu::Buffer; 2],
    bind_groups: [wgpu::BindGroup; 2],
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    palette: Palette,
    card_corner: f32,
}

impl SceneRenderer {
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        palette: Palette,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vutric.scene.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vutric.scene.pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipelines = Kind::ALL
            .iter()
            .map(|&kind| make_pipeline(device, &pipeline_layout, format, kind))
            .collect();
        let uniforms = [0, 1].map(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(if i == 0 {
                    "vutric.scene.uniforms.main"
                } else {
                    "vutric.scene.uniforms.overlay"
                }),
                size: size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vutric.scene.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let placeholder = upload_texture(device, queue, &[255, 255, 255, 255], 1, 1);
        let bind_groups = make_bind_groups(device, &layout, &uniforms, &placeholder, &sampler);
        Self {
            pipelines,
            uniforms,
            bind_groups,
            layout,
            sampler,
            palette,
            card_corner: 22.0,
        }
    }

    /// Replaces the card image. `rgba` is straight (non-premultiplied) sRGB, row-major, `width * height * 4` bytes.
    pub fn set_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        assert_eq!(
            rgba.len(),
            (width * height * 4) as usize,
            "rgba buffer size mismatch"
        );
        let texture = upload_texture(device, queue, rgba, width, height);
        self.bind_groups = make_bind_groups(
            device,
            &self.layout,
            &self.uniforms,
            &texture,
            &self.sampler,
        );
    }

    /// Records one layer. `target_size` is the attachment size in pixels.
    pub fn encode(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_size: [u32; 2],
        scene: &Scene,
        time: f32,
    ) {
        let overlay = scene.kind.is_overlay();
        let slot = usize::from(overlay);
        let c = scene.card;
        let uniforms = Uniforms {
            screen: [target_size[0] as f32, target_size[1] as f32, time, 0.0],
            card: [c.center[0], c.center[1], c.half[0], c.half[1]],
            card_xf: [c.rotation, c.scale, c.opacity, self.card_corner],
            fx: scene.fx,
            color_a: self.palette.rim_a,
            color_b: self.palette.rim_b,
            color_c: self.palette.depth,
        };
        queue.write_buffer(&self.uniforms[slot], 0, bytemuck::bytes_of(&uniforms));

        let load = if overlay {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(scene.kind.label()),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipelines[scene.kind as usize]);
        pass.set_bind_group(0, &self.bind_groups[slot], &[]);
        if overlay {
            // Blending is not idempotent: shade each pixel once.
            if let Some([x, y, w, h]) = scissor(scene.bounds(), target_size) {
                pass.set_scissor_rect(x, y, w, h);
                pass.draw(0..3, 0..1);
            }
        } else {
            for region in scene.regions {
                if let Some([x, y, w, h]) = scissor(region, target_size) {
                    pass.set_scissor_rect(x, y, w, h);
                    pass.draw(0..3, 0..1);
                }
            }
        }
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    kind: Kind,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(kind.label()),
        source: wgpu::ShaderSource::Wgsl(shader_source(kind).into()),
    });
    let blend = kind
        .is_overlay()
        .then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(kind.label()),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vutric.scene.card"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    texture
}

fn make_bind_groups(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &[wgpu::Buffer; 2],
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> [wgpu::BindGroup; 2] {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    [0, 1].map(|i| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vutric.scene.bind-group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms[i].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniforms_match_the_wgsl_layout() {
        // screen, card, card_xf + 8 fx + 3 colours = 14 vec4.
        assert_eq!(size_of::<Uniforms>(), 14 * 16);
    }

    #[test]
    fn every_kind_has_a_pipeline_slot_in_order() {
        for (i, kind) in Kind::ALL.iter().enumerate() {
            assert_eq!(*kind as usize, i);
        }
    }

    #[test]
    fn every_shader_parses_and_validates() {
        for kind in Kind::ALL {
            let source = shader_source(kind);
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{kind:?}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{kind:?}: {e:?}"));
        }
    }

    #[test]
    fn bounds_are_the_union_of_regions() {
        let scene = Scene {
            kind: Kind::Oddiy,
            card: Pose::default(),
            fx: [[0.0; 4]; FX_LEN],
            regions: [[10.0, 20.0, 30.0, 40.0], [0.0; 4], [25.0, 5.0, 50.0, 30.0]],
        };
        assert_eq!(scene.bounds(), [10.0, 5.0, 50.0, 40.0]);
    }
}
