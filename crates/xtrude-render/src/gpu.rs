// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Executes a [`FrameData`] with wgpu into an offscreen colour texture: the window
//! shows the texture, an image export reads it back.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};

use crate::frame::{Cached, Depth, Draw, FaceVertex, FrameData, LineVertex, Source};

/// The colour target: plain 8-bit RGBA holding gamma-space values, as the
/// GL framebuffer did (egui samples such textures as they are).
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The depth packed into 24 bits of RGB: every adapter renders to it
/// (downlevel ones render to no float format).
const DEPTH_COPY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The packed depth's largest value (1.0, the far plane).
const DEPTH_COPY_MAX: f32 = 16_777_215.0;
/// Bytes between two draws' uniforms (the dynamic-offset alignment).
const UNIFORM_STRIDE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    mvp: [[f32; 4]; 4],
    clip: [f32; 4],
    fade: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
    flags: [u32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    lines: bool,
    depth: Depth,
    bias: bool,
    color: bool,
    /// Single-sample: the depth re-draw of [`ViewportRenderer::read_depth`].
    single: bool,
}

struct Targets {
    size: [u32; 2],
    msaa: Option<wgpu::TextureView>,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// The depth drawn again single-sample for a read-back: wgpu's GL
    /// backend draws nothing into a multisampled depth texture that may
    /// also be bound for reading.
    depth_single: wgpu::TextureView,
    /// `depth_single` packed into colour, which a read-back can copy.
    depth_copy: wgpu::Texture,
}

/// The pass that packs the read-back depth into a colour texture a copy
/// can read on every backend.
struct DepthResolve {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

/// A vertex buffer that grows as needed and remembers what it holds.
#[derive(Default)]
struct GpuArray {
    buffer: Option<wgpu::Buffer>,
    capacity: u64,
    generation: Option<u64>,
}

impl GpuArray {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8], label: &str) {
        let needed = (bytes.len() as u64).max(16).next_power_of_two();
        if self.buffer.is_none() || self.capacity < needed {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: needed,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = needed;
        }
        if !bytes.is_empty() {
            if let Some(b) = &self.buffer {
                queue.write_buffer(b, 0, bytes);
            }
        }
    }

    fn sync<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        c: &Cached<T>,
        label: &str,
    ) {
        if self.generation == Some(c.generation) && self.buffer.is_some() {
            return;
        }
        self.upload(
            device,
            queue,
            bytemuck::cast_slice(c.data.as_slice()),
            label,
        );
        self.generation = Some(c.generation);
    }
}

/// The viewport's GPU state.
pub struct ViewportRenderer {
    samples: u32,
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    bind_layout: wgpu::BindGroupLayout,
    uniforms: Option<(wgpu::Buffer, wgpu::BindGroup, u64)>,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    targets: Option<Targets>,
    scene_faces: GpuArray,
    scene_lines: GpuArray,
    preview_faces: GpuArray,
    preview_lines: GpuArray,
    faces: GpuArray,
    lines: GpuArray,
    depth_resolve: Option<DepthResolve>,
    /// The last frame's draws that write depth, with their uniform slots.
    depth_draws: Vec<(usize, Draw)>,
}

impl ViewportRenderer {
    /// `samples`: 1 or 4 (multisampling).
    pub fn new(device: &wgpu::Device, samples: u32) -> ViewportRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xtrude viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xtrude uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("xtrude viewport"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        ViewportRenderer {
            samples: samples.max(1),
            shader,
            layout,
            bind_layout,
            uniforms: None,
            pipelines: HashMap::new(),
            targets: None,
            scene_faces: GpuArray::default(),
            scene_lines: GpuArray::default(),
            preview_faces: GpuArray::default(),
            preview_lines: GpuArray::default(),
            faces: GpuArray::default(),
            lines: GpuArray::default(),
            depth_resolve: None,
            depth_draws: Vec::new(),
        }
    }

    pub fn color_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.color_view)
    }

    fn pipeline(&mut self, device: &wgpu::Device, key: PipelineKey) -> &wgpu::RenderPipeline {
        let samples = if key.single { 1 } else { self.samples };
        let (shader, layout) = (&self.shader, &self.layout);
        self.pipelines.entry(key).or_insert_with(|| {
            let face_attrs =
                wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32x4];
            let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
            let buffers = [Some(if key.lines {
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &line_attrs,
                }
            } else {
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<FaceVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &face_attrs,
                }
            })];
            let (write, compare) = match key.depth {
                Depth::TestWrite => (true, wgpu::CompareFunction::LessEqual),
                Depth::TestOnly => (false, wgpu::CompareFunction::LessEqual),
                Depth::Off => (false, wgpu::CompareFunction::Always),
                Depth::Behind => (false, wgpu::CompareFunction::Greater),
            };
            let bias = if key.bias {
                // glPolygonOffset(1, 1)
                wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 1.0,
                    clamp: 0.0,
                }
            } else {
                wgpu::DepthBiasState::default()
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("xtrude viewport"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(if key.lines { "vs_line" } else { "vs_face" }),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: if key.lines {
                        wgpu::PrimitiveTopology::LineList
                    } else {
                        wgpu::PrimitiveTopology::TriangleList
                    },
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(if key.lines { "fs_line" } else { "fs_face" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: if key.color {
                            wgpu::ColorWrites::ALL
                        } else {
                            wgpu::ColorWrites::empty()
                        },
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        let size = [size[0].max(1), size[1].max(1)];
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("xtrude viewport colour"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let msaa = (self.samples > 1).then(|| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("xtrude viewport msaa"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: self.samples,
                    dimension: wgpu::TextureDimension::D2,
                    format: COLOR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        });
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("xtrude viewport depth"),
                size: extent,
                mip_level_count: 1,
                sample_count: self.samples,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let depth_single = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("xtrude viewport depth read-back"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let depth_copy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("xtrude viewport depth copy"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_COPY_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color.create_view(&Default::default());
        self.targets = Some(Targets {
            size,
            msaa,
            color,
            color_view,
            depth,
            depth_single,
            depth_copy,
        });
    }

    fn uniforms_for(frame: &FrameData, d: &Draw) -> Uniforms {
        let clip = if d.clip {
            frame.local_clip_for(d.model)
        } else {
            None
        };
        let ident = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        Uniforms {
            mvp: if d.ndc {
                ident
            } else {
                frame.shader_mvp_for(d.model)
            },
            clip: clip.unwrap_or([0.0; 4]),
            fade: [
                frame.fade_color[0],
                frame.fade_color[1],
                frame.fade_color[2],
                d.fade,
            ],
            color: d.color.unwrap_or([0.0; 4]),
            params: [
                d.opacity,
                d.dash_px,
                frame.size[0] as f32,
                frame.size[1] as f32,
            ],
            flags: [
                d.stipple as u32,
                clip.is_some() as u32,
                d.color.is_some() as u32,
                d.ndc as u32,
            ],
        }
    }

    fn sync_uniforms(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &FrameData) {
        let n = frame.draws.len().max(1) as u64;
        let needed = n * UNIFORM_STRIDE;
        if self.uniforms.as_ref().is_none_or(|u| u.2 < needed) {
            let size = needed.next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("xtrude uniforms"),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("xtrude uniforms"),
                layout: &self.bind_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    }),
                }],
            });
            self.uniforms = Some((buffer, bind, size));
        }
        let mut bytes = vec![0u8; (n * UNIFORM_STRIDE) as usize];
        for (i, d) in frame.draws.iter().enumerate() {
            let u = Self::uniforms_for(frame, d);
            let at = i * UNIFORM_STRIDE as usize;
            bytes[at..at + std::mem::size_of::<Uniforms>()].copy_from_slice(bytemuck::bytes_of(&u));
        }
        if let Some((b, _, _)) = &self.uniforms {
            queue.write_buffer(b, 0, &bytes);
        }
    }

    /// Draw `frame` into the offscreen texture; submits its own commands.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &FrameData) {
        self.ensure_targets(device, frame.size);
        self.scene_faces
            .sync(device, queue, &frame.scene_faces, "xtrude scene faces");
        self.scene_lines
            .sync(device, queue, &frame.scene_lines, "xtrude scene lines");
        self.preview_faces
            .sync(device, queue, &frame.preview_faces, "xtrude preview faces");
        self.preview_lines
            .sync(device, queue, &frame.preview_lines, "xtrude preview lines");
        self.faces.upload(
            device,
            queue,
            bytemuck::cast_slice(&frame.faces),
            "xtrude faces",
        );
        self.lines.upload(
            device,
            queue,
            bytemuck::cast_slice(&frame.lines),
            "xtrude lines",
        );
        self.faces.generation = None;
        self.lines.generation = None;
        self.sync_uniforms(device, queue, frame);
        let keys: Vec<PipelineKey> = frame
            .draws
            .iter()
            .map(|d| PipelineKey {
                lines: d.source.is_lines(),
                depth: d.depth,
                bias: d.bias && !d.source.is_lines(),
                color: !d.depth_only,
                single: false,
            })
            .collect();
        for &k in &keys {
            self.pipeline(device, k);
        }
        self.depth_draws = frame
            .draws
            .iter()
            .enumerate()
            .filter(|(_, d)| d.depth == Depth::TestWrite && d.count > 0)
            .map(|(i, d)| (i, d.clone()))
            .collect();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("xtrude viewport"),
        });
        {
            let Some(t) = self.targets.as_ref() else {
                return;
            };
            let bg = frame.background;
            let (view, resolve) = match &t.msaa {
                Some(m) => (m, Some(&t.color_view)),
                None => (&t.color_view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("xtrude viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let Some((_, bind, _)) = &self.uniforms else {
                return;
            };
            for (i, (d, k)) in frame.draws.iter().zip(&keys).enumerate() {
                if d.count == 0 {
                    continue;
                }
                let array = self.array(d.source);
                let Some(buffer) = &array.buffer else {
                    continue;
                };
                let Some(p) = self.pipelines.get(k) else {
                    continue;
                };
                pass.set_pipeline(p);
                pass.set_bind_group(0, bind, &[(i as u64 * UNIFORM_STRIDE) as u32]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(d.first..d.first + d.count, 0..1);
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    fn array(&self, source: Source) -> &GpuArray {
        match source {
            Source::SceneFaces => &self.scene_faces,
            Source::SceneLines => &self.scene_lines,
            Source::Faces => &self.faces,
            Source::Lines => &self.lines,
            Source::PreviewFaces => &self.preview_faces,
            Source::PreviewLines => &self.preview_lines,
        }
    }

    /// The last frame's pixels, RGBA rows top to bottom (exports, tests).
    pub fn read_pixels(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let t = self.targets.as_ref()?;
        let [w, h] = t.size;
        let encoder = device.create_command_encoder(&Default::default());
        let data = read_texture(device, queue, encoder, &t.color, [w, h])?;
        Some((w, h, data))
    }

    /// The last frame's depth (0 near … 1 far, the GL window depth), rows
    /// top to bottom: its depth-writing draws again, single-sample, read
    /// once.
    pub fn read_depth(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<(u32, u32, Vec<f32>)> {
        let keys: Vec<PipelineKey> = self
            .depth_draws
            .iter()
            .map(|(_, d)| PipelineKey {
                lines: d.source.is_lines(),
                depth: Depth::TestWrite,
                bias: d.bias && !d.source.is_lines(),
                color: false,
                single: true,
            })
            .collect();
        for &k in &keys {
            self.pipeline(device, k);
        }
        if self.depth_resolve.is_none() {
            self.depth_resolve = Some(DepthResolve::new(device));
        }
        let resolve = self.depth_resolve.as_ref()?;
        let t = self.targets.as_ref()?;
        let [w, h] = t.size;
        let copy_view = t.depth_copy.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            // the frame's depth again, single-sample
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("xtrude depth read-back"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &copy_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth_single,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let (_, bind, _) = self.uniforms.as_ref()?;
            for ((i, d), k) in self.depth_draws.iter().zip(&keys) {
                let array = self.array(d.source);
                let (Some(buffer), Some(p)) = (&array.buffer, self.pipelines.get(k)) else {
                    continue;
                };
                pass.set_pipeline(p);
                pass.set_bind_group(0, bind, &[(*i as u64 * UNIFORM_STRIDE) as u32]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(d.first..d.first + d.count, 0..1);
            }
        }
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xtrude depth resolve"),
            layout: &resolve.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&t.depth_single),
            }],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("xtrude depth resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &copy_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&resolve.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        let bytes = read_texture(device, queue, encoder, &t.depth_copy, [w, h])?;
        let depth = bytes
            .chunks_exact(4)
            .map(|p| {
                let v = p[0] as u32 | (p[1] as u32) << 8 | (p[2] as u32) << 16;
                v as f32 / DEPTH_COPY_MAX
            })
            .collect();
        Some((w, h, depth))
    }
}

impl DepthResolve {
    fn new(device: &wgpu::Device) -> DepthResolve {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xtrude depth resolve"),
            source: wgpu::ShaderSource::Wgsl(include_str!("depth_resolve.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xtrude depth resolve"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("xtrude depth resolve"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("xtrude depth resolve"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DEPTH_COPY_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        DepthResolve { layout, pipeline }
    }
}

/// Copy a 4-byte-per-texel `texture` out after the commands in `encoder`;
/// rows top to bottom, without the copy's row padding.
fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    [w, h]: [u32; 2],
) -> Option<Vec<u8>> {
    let row = (w * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("xtrude read-back"),
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().ok()?.ok()?;
    let data = slice.get_mapped_range().ok()?;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let at = (y * row) as usize;
        out.extend_from_slice(&data[at..at + (w * 4) as usize]);
    }
    drop(data);
    buffer.unmap();
    Some(out)
}
