use bytemuck::{Pod, Zeroable};

use crate::palette::Palette;
use crate::timeline::{Frame, TRAIL_LEN};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    screen: [f32; 4],
    ring: [f32; 4],
    ring_style: [f32; 4],
    card: [f32; 4],
    card_xf: [f32; 4],
    trail: [[f32; 4]; TRAIL_LEN],
    color_a: [f32; 4],
    color_b: [f32; 4],
    color_c: [f32; 4],
}

/// Draws effect frames into any render target of the format given at construction.
///
/// Owns no surface: the host decides how pixels reach the screen (`DirectComposition` swapchain, layer-shell
/// buffer, an offscreen texture in tests).
#[derive(Debug)]
pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    palette: Palette,
    card_corner: f32,
}

impl Renderer {
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        palette: Palette,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vutric.halqa"),
            source: wgpu::ShaderSource::Wgsl(include_str!("halqa.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vutric.halqa.layout"),
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
            label: Some("vutric.halqa.pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vutric.halqa.pipeline"),
            layout: Some(&pipeline_layout),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vutric.halqa.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vutric.card.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let placeholder = upload_texture(device, queue, &[255, 255, 255, 255], 1, 1);
        let bind_group = make_bind_group(device, &layout, &uniforms, &placeholder, &sampler);
        Self {
            pipeline,
            uniforms,
            layout,
            sampler,
            bind_group,
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
        self.bind_group = make_bind_group(
            device,
            &self.layout,
            &self.uniforms,
            &texture,
            &self.sampler,
        );
    }

    /// Records one frame. `target_size` is the attachment size in pixels; everything outside `frame.bounds`
    /// is cleared to transparent and never shaded.
    pub fn encode(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_size: [u32; 2],
        frame: &Frame,
        time: f32,
    ) {
        let uniforms = Uniforms {
            screen: [target_size[0] as f32, target_size[1] as f32, time, 0.0],
            ring: [
                frame.ring_center[0],
                frame.ring_center[1],
                frame.ring_radius,
                frame.ring_opacity,
            ],
            ring_style: [
                frame.ring_aspect,
                (frame.ring_radius * 0.035).max(2.0),
                frame.ring_phase,
                0.0,
            ],
            card: [
                frame.card_center[0],
                frame.card_center[1],
                frame.card_half[0],
                frame.card_half[1],
            ],
            card_xf: [
                frame.card_rotation,
                frame.card_scale,
                frame.card_opacity,
                self.card_corner,
            ],
            trail: frame.trail,
            color_a: self.palette.rim_a,
            color_b: self.palette.rim_b,
            color_c: self.palette.depth,
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vutric.halqa.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        // The shader composites every layer per pixel and writes without blending, so shading a pixel twice
        // where the regions overlap is idempotent; it is cheaper than shading their bounding box once.
        for region in frame.regions {
            if let Some([x, y, w, h]) = scissor(region, target_size) {
                pass.set_scissor_rect(x, y, w, h);
                pass.draw(0..3, 0..1);
            }
        }
    }
}

/// Converts float bounds to an integer scissor rect inside the target, or `None` when nothing is lit.
#[must_use]
pub fn scissor(bounds: [f32; 4], target: [u32; 2]) -> Option<[u32; 4]> {
    let x0 = bounds[0].floor().max(0.0) as u32;
    let y0 = bounds[1].floor().max(0.0) as u32;
    let x1 = (bounds[2].ceil().max(0.0) as u32).min(target[0]);
    let y1 = (bounds[3].ceil().max(0.0) as u32).min(target[1]);
    (x1 > x0 && y1 > y0).then(|| [x0, y0, x1 - x0, y1 - y0])
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
        label: Some("vutric.card"),
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

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vutric.halqa.bind-group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniforms_match_the_wgsl_layout() {
        // 5 vec4 + 4 trail vec4 + 3 colour vec4 = 12 * 16 bytes.
        assert_eq!(size_of::<Uniforms>(), 12 * 16);
    }

    #[test]
    fn scissor_clamps_and_skips_empty() {
        assert_eq!(
            scissor([10.4, 20.6, 30.1, 40.0], [100, 100]),
            Some([10, 20, 21, 20])
        );
        assert_eq!(
            scissor([-50.0, -50.0, 5000.0, 5000.0], [1920, 1080]),
            Some([0, 0, 1920, 1080])
        );
        assert_eq!(scissor([0.0; 4], [1920, 1080]), None);
    }
}
