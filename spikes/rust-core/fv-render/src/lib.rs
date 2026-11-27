//! A GPU renderer for the Fractured Valley spike: it turns a
//! [`fv_core::view::View`] into isometric pixel art.
//!
//! How a frame is made:
//! 1. On the CPU, walk the view and pick the cells worth drawing: solid and
//!    porous cells with an open side facing the camera (+x, +y or +z), and air
//!    cells holding smoke or glowing gas. Each becomes one *sprite* instance.
//! 2. Choose an exposure from the light on the visible surfaces, as an eye
//!    adapts: by night it adapts to whatever is lit (firelight), over the
//!    eye's range of about four orders of magnitude, so the ground near a
//!    fire reads and falls off into the dark.
//! 3. On the GPU, draw the sprites into a small native image (one texel per
//!    sprite pixel) with a depth test, so nearer cells cover farther ones
//!    exactly: blocks and tufts first, then smoke blended back to front, then
//!    flames added on top.
//! 4. Scale that image up by a whole number, add a little bloom around
//!    what is far brighter than the exposure, tone-map it with a filmic
//!    curve (the fire's core clips to white while the surroundings keep
//!    their shading) and draw a dark outline where the world meets the sky.
//!
//! Light is physical: a surface shows as its colour × the light the simulation
//! says reached it, plus any light it gives off, divided by the exposure.
//! Everything here is view only; nothing flows back into the simulation.

use bytemuck::{Pod, Zeroable};
use fv_core::view::{View, ViewCell, AIR, POROUS, SOLID};

/// What the camera looks at: the cell at the centre of the image, and how
/// many screen pixels each sprite pixel becomes.
#[derive(Copy, Clone, Debug)]
pub struct Camera {
    pub centre: [i32; 3],
    pub scale: u32,
}

/// Exposure: how much light (W/m², summed over the three bands) shows as
/// "full brightness". Chosen each frame from the scene, or fixed.
#[derive(Copy, Clone, Debug)]
pub struct Exposure {
    /// Which of the *lit* surfaces sets full brightness (0.75 = the
    /// brightest quarter sit at or above the top of the range). Only surfaces
    /// lit by something else count: a burning log's own glow never sets the
    /// exposure, nor do unlit surfaces.
    pub percentile: f32,
    /// The least exposure allowed: the most an eye can adapt to the dark.
    /// Daylit ground is ~100–300 W/m² of visible light; full moonlight is
    /// ~0.002 W/m² (0.1–0.3 lux), which a dark-adapted eye reads well, and
    /// it keeps adapting down to starlight (~0.001 lux). So 0.0003 W/m²
    /// (about 0.1 lux on the ground): a moonlit scene is seen, dimmer
    /// than that goes dark.
    pub min_w_m2: f32,
    /// How quickly the exposure follows the scene, in seconds of simulated
    /// time (an eye takes about a second).
    pub adapt_seconds: f32,
    /// The most the exposure may change per second of simulated time, as a
    /// factor (so a sudden flare can't swing it from one frame to the next).
    pub max_rate: f32,
    /// At night the camera holds this many times more exposure than the lit
    /// surfaces alone would ask for (1 = adapt fully, as an eye near a fire
    /// does; more keeps firelit ground dimmer than the eye would see it).
    pub night_hold: f32,
    /// The sky's own light (sun + sky, W/m²), if the caller knows it. Then
    /// day or night is read from it; if not, it is guessed from the scene.
    pub sky_w_m2: Option<f32>,
    /// Use this exposure instead of choosing one.
    pub fixed: Option<f32>,
}

impl Default for Exposure {
    fn default() -> Self { Exposure { percentile: 0.75, min_w_m2: 0.0003, adapt_seconds: 1.0, max_rate: 4.0, night_hold: 1.0, sky_w_m2: None, fixed: None } }
}

/// Simulated seconds per tick (fv-core ticks are 50 ms).
const TICK_SECONDS: f32 = 0.05;
/// A surface counts as lit above this (W/m²): a tenth of the darkest the
/// eye adapts to (`Exposure::min_w_m2`'s default).
const LIT_MIN: f32 = 0.00003;
/// A surface giving off more than this (W) is its own light source and is
/// left out of the exposure.
const SELF_LIT_W: f32 = 0.005;

/// The colour grade applied to substance colours (in sRGB, before light):
/// a little desaturation and a warm haze, as in the TypeScript sandbox.
#[derive(Copy, Clone, Debug)]
pub struct Grade {
    pub desaturate: f32,
    pub haze: f32,
    pub haze_colour: [f32; 3],
}

impl Default for Grade {
    fn default() -> Self { Grade { desaturate: 0.15, haze: 0.08, haze_colour: [0x9a as f32, 0x94 as f32, 0x80 as f32] } }
}

/// The sky behind the world in daylight (sRGB); it darkens with the light.
const BACKDROP: [u8; 3] = [0x58, 0x7a, 0x82];
const OUTLINE: [u8; 3] = [0x1b, 0x15, 0x20];
/// Charred wood and grass (sRGB).
const CHAR: [u8; 3] = [0x26, 0x1d, 0x1a];
/// Ash (sRGB).
const ASH: [u8; 3] = [0x8a, 0x86, 0x80];

/// Smoke optics, per mg/L of gas through one 10 cm cell (optical depth).
/// The simulation's light uses 0.1 for soot (1 m²/g) and 0.015 for drops
/// (see `gas_tau_sim`). Real soot is darker still (~8.7 m²/g → 0.87), so
/// drawn soot sits between the two. Mist is drawn about half as thick as the
/// simulation's light sees it, so steam reads as wisps rather than walls.
const SOOT_TAU: f32 = 0.25;
const MIST_TAU: f32 = 0.008;
/// How much light soot and mist send back (their albedo, linear).
const SOOT_ALBEDO: f32 = 0.06;
const MIST_ALBEDO: f32 = 0.85;
/// Thinner smoke than this (optical depth) isn't drawn.
const SMOKE_MIN_TAU: f32 = 0.03;
/// A glowing gas cell's light as an equivalent exitance: glow (W) spread over
/// the area a 10 cm cube shows the camera (√3 × 0.01 m²), × π/(4π) for an
/// even glow in all directions → W / (4 × 0.0173 m²).
const FLAME_W_TO_W_M2: f32 = 1.0 / (4.0 * 0.0173);
/// How many times the light is averaged with its neighbours (see `smooth`).
const SMOOTH_PASSES: usize = 2;
/// Despeckling: a cell may hold this many times its neighbours' mean light…
const SPECKLE_RATIO: f32 = 3.0;
/// …or this share of the exposure, whichever is more.
const SPECKLE_FLOOR: f32 = 0.05;
/// The colour of the light is averaged wider still.
const COLOUR_SMOOTH_PASSES: usize = 6;
/// Gas catches light more rarely, so its light is averaged wider.
const GAS_SMOOTH_PASSES: usize = 4;
/// Gas glowing less than this (W) isn't drawn as flame.
const FLAME_MIN_W: f32 = 0.005;

/// Surface pattern per substance id (see `shader.wgsl`): 0 plain, 1 stone
/// blocks, 2 wood grain, 3 soil, 4 grass, 5 leaves, 6 metal. Ids follow
/// `fv_core::substance` (none, stone, soil, grass, leaves, wood, brick,
/// mortar, iron, ore).
const PATTERN: [u8; 10] = [0, 1, 3, 4, 5, 2, 0, 0, 6, 1];

// Flag bits shared with the shader.
const OPEN_X: i32 = 1;
const OPEN_Y: i32 = 2;
const OPEN_Z: i32 = 4;
const TOP_AO: i32 = 8;
const LEDGE: i32 = 16;
const SECTION_X: i32 = 32;
const SECTION_Y: i32 = 64;
/// Open water lies on this top face (light.w = its surface reflectance).
const WET: i32 = 128;

/// Water's refractive index for visible light.
const WATER_N: f32 = 1.333;
/// The camera looks down on top faces at atan(½) above the horizon (2:1
/// pixel dimetric), so at this angle from their normal, in radians.
const VIEW_INCIDENCE: f32 = 1.107; // 63.4°
/// A film this thin (mm) or thicker shows fully wet; thinner fades out.
const WET_FILM_MM: f32 = 0.02;

/// One sprite as the GPU sees it.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct Sprite {
    /// x, y, z and flags (bits 0–7 see above; bits 8–15 the surface pattern).
    cell: [i32; 4],
    /// Linear colour; w = how much solid is left (blocks, tufts) or the
    /// optical depth (smoke).
    albedo: [f32; 4],
    /// Light arriving, W/m² per band.
    light: [f32; 4],
    /// Light given off as W/m² of surface per band; w = total glow in W.
    glow: [f32; 4],
    /// Open water on the top face: the light its surface mirrors toward the
    /// camera (W/m² per band, as if it were light arriving); w = the
    /// surface's reflectance.
    mirror: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Globals {
    origin: [i32; 2],
    native_size: [u32; 2],
    out_size: [u32; 2],
    scale: u32,
    tick: u32,
    inv_exposure: f32,
    day: f32,
    depth_div: f32,
    encode_srgb: u32,
    outline: [f32; 4],
    light_curve: f32,
    _pad: [f32; 3],
}

/// The sprites of one frame, sorted into the passes that draw them.
#[derive(Default)]
struct Frame {
    /// Solid blocks, then porous tufts.
    blocks: Vec<Sprite>,
    tufts: Vec<Sprite>,
    /// Smoke, far to near.
    smoke: Vec<Sprite>,
    flames: Vec<Sprite>,
    /// Light on visible surfaces lit from outside (summed bands, cells that
    /// glow themselves left out), for the exposure.
    lit_light: Vec<f32>,
    /// Light on every visible top face that doesn't glow, lit or not, for
    /// telling day from night.
    top_light: Vec<f32>,
}

/// Counts from the last frame, for logging.
#[derive(Copy, Clone, Debug, Default)]
pub struct Stats {
    pub blocks: usize,
    pub tufts: usize,
    pub smoke: usize,
    pub flames: usize,
    pub exposure: f32,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    out_format: wgpu::TextureFormat,
    globals: wgpu::Buffer,
    sprite_group: wgpu::BindGroup,
    composite_layout: wgpu::BindGroupLayout,
    block_pipe: wgpu::RenderPipeline,
    tuft_pipe: wgpu::RenderPipeline,
    smoke_pipe: wgpu::RenderPipeline,
    flame_pipe: wgpu::RenderPipeline,
    composite_pipe: wgpu::RenderPipeline,
    /// The native image and its depth, rebuilt when the scale changes.
    native: Option<Native>,
    instances: Option<(wgpu::Buffer, u64)>,
    /// Headless only: the image we draw into and read back.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    target: Option<wgpu::Texture>,
    /// The eased exposure and day-ness, and the tick they were set at.
    current_exposure: Option<f32>,
    current_day: f32,
    last_tick: u64,
    pub exposure: Exposure,
    pub grade: Grade,
    /// Tone curve on light: a surface shows as (light / exposure) ^ this.
    /// 1 is linear; lower lifts shadows and tames highlights, by day and by
    /// night (where the eye has adapted to the firelight).
    pub light_curve: f32,
    /// Diffuse skylight (W/m², all bands), for what water surfaces mirror.
    /// Unset: the scene's average light stands in.
    pub sky_light_w_m2: Option<f32>,
    pub stats: Stats,
}

struct Native {
    size: [u32; 2],
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
    group: wgpu::BindGroup,
}

const NATIVE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

impl Renderer {
    /// A renderer on a device the caller already has (e.g. a web page's
    /// canvas device). It draws `width`×`height` images in `out_format`.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, width: u32, height: u32, out_format: wgpu::TextureFormat) -> Renderer {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fv-render"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let sprite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sprites"), entries: &[uniform_entry] });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[uniform_entry, wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        });
        let sprite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprites"),
            layout: &sprite_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let sprite_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&sprite_layout)], immediate_size: 0 });
        let composite_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&composite_layout)], immediate_size: 0 });

        let attrs = wgpu::vertex_attr_array![0 => Sint32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
        let instance_layout = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Sprite>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &attrs };
        // The three ways a sprite pass treats colour and depth.
        let opaque = (None, true);
        let over = (Some(wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            // Keep the coverage mark of what's underneath (it drives the outline).
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        }), false);
        let add = (Some(wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        }), false);
        let sprite_pipe = |entry: &str, (blend, write): (Option<wgpu::BlendState>, bool)| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&sprite_pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_sprite"), compilation_options: Default::default(), buffers: &[Some(instance_layout.clone())] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: NATIVE_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let block_pipe = sprite_pipe("fs_block", opaque);
        let tuft_pipe = sprite_pipe("fs_tuft", opaque);
        let smoke_pipe = sprite_pipe("fs_smoke", over);
        let flame_pipe = sprite_pipe("fs_flame", add);
        let composite_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&composite_pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_composite"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: out_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Renderer {
            device, queue, width, height, out_format, globals, sprite_group, composite_layout,
            block_pipe, tuft_pipe, smoke_pipe, flame_pipe, composite_pipe,
            native: None, instances: None, target: None, current_exposure: None, current_day: 1.0, last_tick: 0,
            exposure: Exposure::default(), grade: Grade::default(), light_curve: 0.6, sky_light_w_m2: None, stats: Stats::default(),
        }
    }

    /// A renderer with its own GPU device and an offscreen image, for
    /// recording and tests (native only). Prefers a real GPU, falls back to
    /// a software one (e.g. Mesa lavapipe).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_headless(width: u32, height: u32) -> Result<Renderer, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        })).map_err(|e| format!("no GPU adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fv-render"),
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..Default::default()
        })).map_err(|e| format!("no GPU device: {e}"))?;
        let mut r = Renderer::new(device, queue, width, height, wgpu::TextureFormat::Rgba8Unorm);
        r.target = Some(r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }));
        Ok(r)
    }

    /// Which GPU this renderer runs on.
    pub fn device(&self) -> &wgpu::Device { &self.device }

    /// Render a view and read the image back: RGBA8, `width`×`height`, rows
    /// top to bottom. Needs a renderer from [`Renderer::new_headless`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render(&mut self, view: &View, camera: &Camera) -> Vec<u8> {
        let target = self.target.clone().expect("render() needs a headless renderer");
        let out = target.create_view(&Default::default());
        self.draw(view, camera, &out);
        // Copy out, with rows padded to the GPU's 256-byte alignment.
        let row = self.width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * self.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(self.height) } },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback buffer"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        let data = slice.get_mapped_range().expect("read mapped buffer");
        let mut pixels = Vec::with_capacity((row * self.height) as usize);
        for y in 0..self.height as usize {
            let start = y * padded as usize;
            pixels.extend_from_slice(&data[start..start + row as usize]);
        }
        drop(data);
        buffer.unmap();
        pixels
    }

    /// Draw a view into `target` (an image of this renderer's size and
    /// format, e.g. a canvas's current texture). Works on the web too.
    pub fn draw(&mut self, view: &View, camera: &Camera, target: &wgpu::TextureView) {
        let scale = camera.scale.max(1);
        let frame = self.build(view);
        let (exposure, day) = self.choose_exposure(&frame, view.tick);
        self.stats = Stats { blocks: frame.blocks.len(), tufts: frame.tufts.len(), smoke: frame.smoke.len(), flames: frame.flames.len(), exposure };

        let native_size = [self.width.div_ceil(scale), self.height.div_ceil(scale)];
        self.ensure_native(native_size);
        // Put the centre cell's sprite in the middle of the image.
        let [cx, cy, cz] = camera.centre;
        let origin = [native_size[0] as i32 / 2 - 2 * (cx - cy) - 2, native_size[1] as i32 / 2 - (cx + cy - 2 * cz) - 2];
        let encode_srgb = !self.out_format.is_srgb();
        let outline = srgb_to_linear(OUTLINE);
        let globals = Globals {
            origin,
            native_size,
            out_size: [self.width, self.height],
            scale,
            tick: view.tick as u32,
            inv_exposure: 1.0 / exposure,
            day,
            depth_div: (view.nx + view.ny + view.nz + 4) as f32,
            encode_srgb: encode_srgb as u32,
            // The outline darkens with the backdrop, so it never shows as a
            // pale line against a night sky.
            outline: [outline[0] * day, outline[1] * day, outline[2] * day, 1.0],
            light_curve: self.light_curve,
            _pad: [0.0; 3],
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        // All sprites in one buffer: blocks, tufts, smoke, flames.
        let all: Vec<Sprite> = [&frame.blocks, &frame.tufts, &frame.smoke, &frame.flames].into_iter().flatten().copied().collect();
        let ranges = {
            let mut start = 0u32;
            [&frame.blocks, &frame.tufts, &frame.smoke, &frame.flames].map(|v| { let r = start..start + v.len() as u32; start = r.end; r })
        };
        let bytes: &[u8] = bytemuck::cast_slice(&all);
        let need = (bytes.len() as u64).max(64);
        if self.instances.as_ref().is_none_or(|(_, cap)| *cap < need) {
            let cap = need.next_power_of_two();
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("sprites"), size: cap, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.instances = Some((buf, cap));
        }
        let (buf, _) = self.instances.as_ref().unwrap();
        if !bytes.is_empty() { self.queue.write_buffer(buf, 0, bytes) }

        let native = self.native.as_ref().unwrap();
        let bg = srgb_to_linear(BACKDROP).map(|c| (c * day) as f64);
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sprites"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &native.colour,
                    depth_slice: None,
                    resolve_target: None,
                    // Alpha 0 marks "nothing drawn here" for the outline.
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0], g: bg[1], b: bg[2], a: 0.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &native.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.sprite_group, &[]);
            pass.set_vertex_buffer(0, buf.slice(..));
            for (pipe, range) in [&self.block_pipe, &self.tuft_pipe, &self.smoke_pipe, &self.flame_pipe].into_iter().zip(ranges) {
                if range.is_empty() { continue }
                pass.set_pipeline(pipe);
                pass.draw(0..6, range);
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.composite_pipe);
            pass.set_bind_group(0, &native.group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
    }

    /// (Re)make the native image when its size changes.
    fn ensure_native(&mut self, size: [u32; 2]) {
        if self.native.as_ref().is_some_and(|n| n.size == size) { return }
        let texture = |format, usage| self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        }).create_view(&Default::default());
        let colour = texture(NATIVE_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let depth = texture(DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&colour) },
            ],
        });
        self.native = Some(Native { size, colour, depth, group });
    }

    /// Pick this frame's exposure and how much like day the scene is.
    ///
    /// Day or night: by day nearly every top face gets skylight, so the
    /// middle top face is lit about as well as the well-lit ones; at night
    /// only those near a fire are lit and the middle one is dark. So
    /// day-ness = (median top light) ÷ (well-lit light), eased into 0…1.
    /// (A big fire in a small box can fool that; when the caller knows the
    /// sky, `Exposure::sky_w_m2` settles it.)
    ///
    /// Exposure: a high percentile of the lit surfaces, never below the gain
    /// cap, held higher at night (`night_hold`), then eased toward over about
    /// a second of simulated time and limited in speed, like an eye.
    fn choose_exposure(&mut self, frame: &Frame, tick: u64) -> (f32, f32) {
        let e = self.exposure;
        let key = percentile(&frame.lit_light, e.percentile);
        let middle = percentile(&frame.top_light, 0.5);
        let guess = if key > 0.0 { ((middle / key - 0.05) / 0.25).clamp(0.0, 1.0) } else { 0.0 };
        // A sky of 200 W/m² or more is full day.
        let day_now = e.sky_w_m2.map_or(guess, |sky| (sky / 200.0).clamp(0.0, 1.0));
        let hold = e.night_hold + (1.0 - e.night_hold) * day_now;
        let target = (key * hold).max(e.min_w_m2);
        let (exposure, day) = match self.current_exposure {
            None => (target, day_now),
            Some(last) => {
                let dt = tick.saturating_sub(self.last_tick).max(1) as f32 * TICK_SECONDS;
                let k = 1.0 - (-dt / e.adapt_seconds.max(1e-3)).exp();
                // Ease in log space, and never faster than `max_rate`.
                let limit = e.max_rate.powf(dt);
                let step = (target / last).powf(k).clamp(1.0 / limit, limit);
                (last * step, self.current_day + (day_now - self.current_day) * k)
            }
        };
        self.current_exposure = Some(exposure);
        self.current_day = day;
        self.last_tick = tick;
        (e.fixed.unwrap_or(exposure), day)
    }

    /// Walk the view and make this frame's sprites.
    fn build(&self, view: &View) -> Frame {
        let (nx, ny, nz) = (view.nx, view.ny, view.nz);
        let idx = |x: usize, y: usize, z: usize| (z * ny + y) * nx + x;
        // Does this neighbour block the view? Outside the box never does.
        let solid = |x: usize, y: usize, z: usize| x < nx && y < ny && z < nz && view.cells[idx(x, y, z)].kind == SOLID;
        let colours: Vec<[f32; 3]> = view.colours.iter().map(|&c| srgb_to_linear(self.grade.apply(c))).collect();
        let (char_lin, ash_lin) = (srgb_to_linear(CHAR), srgb_to_linear(ASH));

        // First, which cells are seen: surfaces (with an open side toward the
        // camera) and gas thick or hot enough to show.
        let mut surfaces: Vec<(usize, [bool; 3])> = Vec::new();
        let mut gases: Vec<usize> = Vec::new();
        for z in 0..nz { for y in 0..ny { for x in 0..nx {
            let i = idx(x, y, z);
            let c = &view.cells[i];
            if c.kind != AIR {
                let open = [!solid(x + 1, y, z), !solid(x, y + 1, z), !solid(x, y, z + 1)];
                if open != [false; 3] { surfaces.push((i, open)) }
            }
            if c.kind != SOLID && (gas_tau(c) > SMOKE_MIN_TAU || c.glow.iter().sum::<f32>() > FLAME_MIN_W) { gases.push(i) }
        }}}

        // The light field comes from a few random light packets per cell each
        // tick, so on its own it speckles. Average each seen cell's light with
        // its seen neighbours (3×3×3, a few times over), a view-only smoothing.
        let surface_cells: Vec<usize> = surfaces.iter().map(|s| s.0).collect();
        let raw: Vec<[f32; 3]> = surface_cells.iter().map(|&i| view.cells[i].light).collect();
        // A lone packet landing far from any other light (firelight on a
        // distant wall at night) would smooth into a coloured blob: clamp
        // each cell to a few times what its neighbours got.
        let floor = self.current_exposure.unwrap_or(self.exposure.min_w_m2) * SPECKLE_FLOOR;
        let raw = despeckle(view, &surface_cells, raw, floor);
        let surface_light = smooth_colour(view, &surface_cells, raw);
        // Gas records only the light it caught, and thin gas catches a packet
        // rarely (so most smoke cells caught nothing this tick, a few caught a
        // lot). The light passing through is about (light caught nearby) ÷
        // (share of light the gas nearby catches): smooth both, then divide.
        // Only the brightness is taken per cell: each packet is one colour, so
        // per-band gas light would show as red, green and blue blobs. The
        // colour is the scene's light colour (the average over all surfaces).
        let mean_light = {
            let n = surface_light.len().max(1) as f32;
            let s = surface_light.iter().fold([0.0f32; 3], |a, l| [a[0] + l[0], a[1] + l[1], a[2] + l[2]]);
            [s[0] / n, s[1] / n, s[2] / n]
        };
        let scene_tint = {
            let sum: f32 = mean_light.iter().sum();
            if sum > 0.0 { mean_light.map(|v| v / sum) } else { [1.0 / 3.0; 3] }
        };
        let caught = smooth(view, &gases, gases.iter().map(|&i| [view.cells[i].light.iter().sum(); 3]).collect(), GAS_SMOOTH_PASSES);
        let share = smooth(view, &gases, gases.iter().map(|&i| [1.0 - (-gas_tau_sim(&view.cells[i])).exp(); 3]).collect(), GAS_SMOOTH_PASSES);
        let gas_light: Vec<[f32; 3]> = caught.iter().zip(&share).map(|(c, s)| scene_tint.map(|t| t * c[0] / s[0].max(0.02))).collect();

        // The sky a water surface mirrors: the skylight the caller gave, split
        // into visible bands as fv-core's skylight is (red 15 %, green 17 %,
        // blue 23 % of it), else the scene's average light as a stand-in.
        let sky = match self.sky_light_w_m2 { Some(w) => [0.15 * w, 0.17 * w, 0.23 * w], None => mean_light };
        let mut f = Frame::default();
        for (&(i, open), light) in surfaces.iter().zip(&surface_light) {
            let c = &view.cells[i];
            let (x, y, z) = (i % nx, i / nx % ny, i / (nx * ny));
            // Shading hints for the top face.
            let behind_above = (x > 0 && solid(x - 1, y, z + 1)) || (y > 0 && solid(x, y - 1, z + 1));
            let front_open = open[0] || open[1];
            let flags = [
                (open[0], OPEN_X), (open[1], OPEN_Y), (open[2], OPEN_Z),
                (open[2] && behind_above, TOP_AO), (open[2] && front_open, LEDGE),
                (x + 1 == nx, SECTION_X), (y + 1 == ny, SECTION_Y),
            ].iter().filter(|(on, _)| *on).fold(0, |acc, (_, bit)| acc | bit);
            let pattern = PATTERN.get(c.substance as usize).copied().unwrap_or(0) as i32;
            let mut albedo = surface_colour(colours[c.substance as usize], c, char_lin, ash_lin);
            // Open water lying on this top face (all of it in the column above).
            let water_mm = if open[2] && c.kind == SOLID { (z + 1..nz).map(|k| view.cells[idx(x, y, k)].water_mm).take_while(|&d| d > 0.0).sum::<f32>() } else { 0.0 };
            let (mut flags, mut reflect, mut mirror) = (flags, 0.0, [0.0f32; 3]);
            if water_mm > 0.0 {
                let (wet, r) = wet_surface(albedo, water_mm);
                albedo = wet;
                reflect = r;
                mirror = mirrored(view, &colours, [x, y, z], sky);
                flags |= WET;
            }
            // A glowing solid shines from its open faces (0.01 m² each).
            let faces = open.iter().filter(|o| **o).count() as f32;
            let glow = glow_colour(c).map(|w| w / (0.01 * faces));
            let sprite = Sprite {
                cell: [x as i32, y as i32, z as i32, flags | pattern << 8],
                albedo: [albedo[0], albedo[1], albedo[2], c.left as f32 / 255.0],
                light: [light[0], light[1], light[2], 0.0],
                glow: [glow[0], glow[1], glow[2], c.glow.iter().sum()],
                mirror: [mirror[0], mirror[1], mirror[2], reflect],
            };
            let lsum: f32 = light.iter().sum();
            let self_lit = c.glow.iter().sum::<f32>() > SELF_LIT_W;
            if !self_lit && lsum > LIT_MIN { f.lit_light.push(lsum) }
            if !self_lit && open[2] { f.top_light.push(lsum) }
            let list = if c.kind == POROUS { &mut f.tufts } else { &mut f.blocks };
            list.push(sprite);
        }
        // Falling pieces: each cell drawn at the nearest whole cell (a piece
        // turned partway shows as quarter-turn pixel art), lit like the
        // scene's surfaces on average.
        for (pos, c) in &view.bodies {
            let xyz = [pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32];
            let pattern = PATTERN.get(c.substance as usize).copied().unwrap_or(0) as i32;
            let albedo = surface_colour(colours[c.substance as usize], c, char_lin, ash_lin);
            let glow = glow_colour(c).map(|w| w / 0.03);
            let sprite = Sprite {
                cell: [xyz[0], xyz[1], xyz[2], OPEN_X | OPEN_Y | OPEN_Z | pattern << 8],
                albedo: [albedo[0], albedo[1], albedo[2], c.left as f32 / 255.0],
                light: [mean_light[0], mean_light[1], mean_light[2], 0.0],
                glow: [glow[0], glow[1], glow[2], c.glow.iter().sum()],
                mirror: [0.0; 4],
            };
            let list = if c.kind == POROUS { &mut f.tufts } else { &mut f.blocks };
            list.push(sprite);
        }
        for (&i, light) in gases.iter().zip(&gas_light) {
            let c = &view.cells[i];
            let xyz = [(i % nx) as i32, (i / nx % ny) as i32, (i / (nx * ny)) as i32];
            gas_sprites(c, xyz, *light, &mut f);
        }
        // Smoke is blended, so draw it far to near (nearer = larger x + y + z).
        f.smoke.sort_by_key(|s| s.cell[0] + s.cell[1] + s.cell[2]);
        f
    }
}

/// Clamp each listed cell's light to `SPECKLE_RATIO` × the mean of its listed
/// neighbours, but never below `floor` (W/m²).
fn despeckle(view: &View, cells: &[usize], values: Vec<[f32; 3]>, floor: f32) -> Vec<[f32; 3]> {
    let (nx, ny, nz) = (view.nx as isize, view.ny as isize, view.nz as isize);
    let mut slot = vec![u32::MAX; view.cells.len()];
    for (k, &i) in cells.iter().enumerate() { slot[i] = k as u32 }
    cells.iter().enumerate().map(|(k, &i)| {
        let (x, y, z) = (i as isize % nx, i as isize / nx % ny, i as isize / (nx * ny));
        let (mut sum, mut n) = (0.0f32, 0.0f32);
        for dz in -1..=1 { for dy in -1..=1 { for dx in -1..=1 {
            let (a, b, c) = (x + dx, y + dy, z + dz);
            if (dx, dy, dz) == (0, 0, 0) || a < 0 || b < 0 || c < 0 || a >= nx || b >= ny || c >= nz { continue }
            let s = slot[((c * ny + b) * nx + a) as usize];
            if s == u32::MAX { continue }
            sum += values[s as usize].iter().sum::<f32>();
            n += 1.0;
        }}}
        let own: f32 = values[k].iter().sum();
        let limit = (SPECKLE_RATIO * sum / n.max(1.0)).max(floor);
        if own <= limit { return values[k] }
        values[k].map(|v| v * limit / own)
    }).collect()
}

/// Smooth light in two parts: its brightness over a small neighbourhood (so
/// shadow edges stay sharp), its colour over a wider one. Dim corners get only
/// the odd packet, each of one colour, so their colour is the noisiest part.
fn smooth_colour(view: &View, cells: &[usize], raw: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    let near = smooth(view, cells, raw.clone(), SMOOTH_PASSES);
    let wide = smooth(view, cells, raw, COLOUR_SMOOTH_PASSES);
    near.iter().zip(&wide).map(|(n, w)| {
        let (bright, wsum) = (n.iter().sum::<f32>(), w.iter().sum::<f32>());
        if wsum <= 0.0 { return *n }
        w.map(|v| v / wsum * bright)
    }).collect()
}

/// Average a per-cell light over each listed cell and those of its 26
/// neighbours that are also listed (the cell itself counts twice), `passes`
/// times over. `values[k]` belongs to `cells[k]`.
fn smooth(view: &View, cells: &[usize], mut values: Vec<[f32; 3]>, passes: usize) -> Vec<[f32; 3]> {
    let (nx, ny, nz) = (view.nx as isize, view.ny as isize, view.nz as isize);
    // Where each listed cell's value sits in `values`.
    let mut slot = vec![u32::MAX; view.cells.len()];
    for (k, &i) in cells.iter().enumerate() { slot[i] = k as u32 }
    for _ in 0..passes {
        values = cells.iter().enumerate().map(|(k, &i)| {
            let (x, y, z) = (i as isize % nx, i as isize / nx % ny, i as isize / (nx * ny));
            let mut sum = values[k].map(|v| v * 2.0);
            let mut n = 2.0f32;
            for dz in -1..=1 { for dy in -1..=1 { for dx in -1..=1 {
                let (a, b, c) = (x + dx, y + dy, z + dz);
                if (dx, dy, dz) == (0, 0, 0) || a < 0 || b < 0 || c < 0 || a >= nx || b >= ny || c >= nz { continue }
                let s = slot[((c * ny + b) * nx + a) as usize];
                if s == u32::MAX { continue }
                for band in 0..3 { sum[band] += values[s as usize][band] }
                n += 1.0;
            }}}
            sum.map(|v| v / n)
        }).collect();
    }
    values
}

/// The colour of hot matter's glow by its temperature (°C), as shares of the
/// red, green and blue bands (Planck's law through the three bands, roughly):
/// dull red at 600 °C, orange at 1000 °C, yellow-white past 1500 °C.
const GLOW_RAMP: [(f32, [f32; 3]); 6] = [
    (600.0, [1.0, 0.06, 0.0]),
    (800.0, [1.0, 0.20, 0.02]),
    (1000.0, [1.0, 0.36, 0.07]),
    (1200.0, [1.0, 0.52, 0.18]),
    (1500.0, [1.0, 0.70, 0.40]),
    (2000.0, [1.0, 0.84, 0.68]),
];

/// A cell's glow (W per band) with its total kept but its colour taken from
/// its temperature. The per-band glow the simulation reports comes from a few
/// random packets, so its colour flickers between bands; the temperature
/// gives the same physics (the Planck split) without the noise.
fn glow_colour(c: &ViewCell) -> [f32; 3] {
    let total: f32 = c.glow.iter().sum();
    if total <= 0.0 { return [0.0; 3] }
    let t = c.temp_c;
    let hi = GLOW_RAMP.iter().position(|(at, _)| *at >= t).unwrap_or(GLOW_RAMP.len() - 1).max(1);
    let ((t0, c0), (t1, c1)) = (GLOW_RAMP[hi - 1], GLOW_RAMP[hi]);
    let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    let rgb: [f32; 3] = std::array::from_fn(|k| c0[k] + (c1[k] - c0[k]) * f);
    let sum: f32 = rgb.iter().sum();
    rgb.map(|v| v / sum * total)
}

/// Optical depth of the gas in one cell, as drawn.
fn gas_tau(c: &ViewCell) -> f32 { c.soot * SOOT_TAU + c.mist * MIST_TAU }
/// The same, as the simulation's light sees it (see fv-core `rad.rs`):
/// soot 1 m⁻¹ and drops 0.15 m⁻¹ per mg/L, over a 0.1 m cell.
fn gas_tau_sim(c: &ViewCell) -> f32 { c.soot * 0.1 + c.mist * 0.015 }

/// Smoke and flame sprites for the gas in one cell, if there's enough to see.
fn gas_sprites(c: &ViewCell, xyz: [i32; 3], light: [f32; 3], f: &mut Frame) {
    let (soot, mist) = (c.soot * SOOT_TAU, c.mist * MIST_TAU);
    let tau = soot + mist;
    if tau > SMOKE_MIN_TAU {
        // Mixed smoke: its colour is the soot and mist albedos weighted by how
        // much each blocks the light.
        let a = (soot * SOOT_ALBEDO + mist * MIST_ALBEDO) / tau;
        f.smoke.push(Sprite {
            cell: [xyz[0], xyz[1], xyz[2], 0],
            albedo: [a, a, a, tau],
            light: [light[0], light[1], light[2], 0.0],
            glow: [0.0; 4],
            mirror: [0.0; 4],
        });
    }
    let total: f32 = c.glow.iter().sum();
    if total > FLAME_MIN_W {
        let g = glow_colour(c).map(|w| w * FLAME_W_TO_W_M2);
        f.flames.push(Sprite { cell: [xyz[0], xyz[1], xyz[2], 0], albedo: [0.0; 4], light: [0.0; 4], glow: [g[0], g[1], g[2], total], mirror: [0.0; 4] });
    }
}


/// What a water surface on top of cell `at` mirrors toward the camera: the
/// camera looks down at atan(½) from +x, +y, so the mirrored ray leaves the
/// surface rising at atan(½) toward −x, −y. Followed through the grid, it
/// either meets a solid (whose lit colour it shows) or leaves the box (the
/// sky's).
fn mirrored(view: &View, colours: &[[f32; 3]], at: [usize; 3], sky: [f32; 3]) -> [f32; 3] {
    let (nx, ny, nz) = (view.nx as f32, view.ny as f32, view.nz as f32);
    let d = [-0.632_f32, -0.632, 0.447]; // (−1, −1, ½·√2) normalised
    let mut p = [at[0] as f32 + 0.5, at[1] as f32 + 0.5, at[2] as f32 + 1.01];
    for _ in 0..400 {
        for k in 0..3 { p[k] += d[k] * 0.25 }
        if p[0] < 0.0 || p[1] < 0.0 || p[0] >= nx || p[1] >= ny || p[2] >= nz { return sky }
        let c = &view.cells[((p[2] as usize) * view.ny + p[1] as usize) * view.nx + p[0] as usize];
        if c.kind == SOLID {
            let col = colours.get(c.substance as usize).copied().unwrap_or([0.0; 3]);
            return std::array::from_fn(|k| col[k] * c.light[k]);
        }
    }
    sky
}

/// Fresnel reflectance of a water surface for unpolarised light arriving
/// `theta` from its normal (radians).
fn fresnel(theta: f32) -> f32 {
    let (ci, si) = (theta.cos(), theta.sin());
    let st = si / WATER_N;
    let ct = (1.0 - st * st).max(0.0).sqrt();
    let rs = ((ci - WATER_N * ct) / (ci + WATER_N * ct)).powi(2);
    let rp = ((ct - WATER_N * ci) / (ct + WATER_N * ci)).powi(2);
    (rs + rp) / 2.0
}

/// How a surface looks under open water `depth_mm` deep: its colour (linear,
/// what's seen through the water) and the water's own reflectance toward
/// the camera.
///
/// - Wet things look darker: light that has come up from the bed is partly
///   reflected back down at the water's underside (about half, by total
///   internal reflection) and the bed absorbs some of it again (Lekner &
///   Dorf 1988, Appl. Opt. 27, 1278): a_wet = (1 − R)(1 − r̄)·a / (1 − r̄·a),
///   r̄ = 1 − (1 − r̄_out)/n² the water's internal reflectance, from its
///   hemispherical external reflectance r̄_out ≈ 0.066.
/// - The water tints what's under it by its own absorption per band
///   (fv-core's `WATER_KAPPA_MILLI_PER_M`, Pope & Fry 1997) over the path
///   down (refracted) and back up (diffuse, about twice the depth): blue-green
///   in deep water, nothing to see in a puddle.
/// - The surface mirrors the sky by Fresnel's law at the camera's angle.
fn wet_surface(dry: [f32; 3], depth_mm: f32) -> ([f32; 3], f32) {
    let r = fresnel(VIEW_INCIDENCE);
    let r_internal = 1.0 - (1.0 - 0.066) / (WATER_N * WATER_N);
    let cos_t = (1.0 - (VIEW_INCIDENCE.sin() / WATER_N).powi(2)).sqrt();
    let path_m = depth_mm / 1_000.0 * (1.0 / cos_t + 2.0);
    // Visible bands [red, green, blue] of fv-core's table (per metre).
    let kappa = [1, 2, 3].map(|b| fv_core::substance::WATER_KAPPA_MILLI_PER_M[b] as f32 / 1_000.0);
    let film = (depth_mm / WET_FILM_MM).clamp(0.0, 1.0);
    let colour = std::array::from_fn(|k| {
        let a = dry[k] * (-kappa[k] * path_m).exp();
        let wet = (1.0 - r) * (1.0 - r_internal) * a / (1.0 - r_internal * a);
        dry[k] + (wet - dry[k]) * film
    });
    (colour, r * film)
}

/// A solid's colour as it is now: darkened as it chars, greying to ash as it
/// burns away (linear).
fn surface_colour(base: [f32; 3], c: &ViewCell, char_lin: [f32; 3], ash_lin: [f32; 3]) -> [f32; 3] {
    let charred = (c.char as f32 / 255.0).powf(0.7) * 0.92;
    let gone = (1.0 - c.left as f32 / 255.0).clamp(0.0, 1.0);
    // Ash shows only once most of the solid has gone.
    let ashen = ((gone - 0.6) / 0.4).clamp(0.0, 1.0);
    std::array::from_fn(|k| {
        let v = base[k] + (char_lin[k] - base[k]) * charred;
        v + (ash_lin[k] - v) * ashen
    })
}

impl Grade {
    /// Desaturate and haze an sRGB colour.
    fn apply(&self, c: [u8; 3]) -> [u8; 3] {
        let c = c.map(|v| v as f32);
        let l = c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
        std::array::from_fn(|k| {
            let v = c[k] + (l - c[k]) * self.desaturate;
            (v + (self.haze_colour[k] - v) * self.haze).round().clamp(0.0, 255.0) as u8
        })
    }
}

fn srgb_to_linear(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    })
}

/// The value below which `q` of the samples fall (0 for none).
fn percentile(samples: &[f32], q: f32) -> f32 {
    if samples.is_empty() { return 0.0 }
    let mut v = samples.to_vec();
    let k = ((v.len() - 1) as f32 * q.clamp(0.0, 1.0)) as usize;
    let (_, at, _) = v.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
    *at
}
