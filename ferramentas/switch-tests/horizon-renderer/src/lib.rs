//! Etapa 4 (`test4_horizon_renderer`): Real Horizon Graphics Engine (`wgpu-hal` + `naga`)
//! and Real Minecraft 26.3 Asset Pipeline (`horizon-assets` + `png`) on Nintendo Switch.
//!
//! This crate integrates:
//! 1. Compile-time ABI verification (163 assertions against devkitPro/libnx C headers).
//! 2. Core Horizon crates: `horizon-core`, `horizon-world`, `horizon-client-core`, `horizon-items`.
//! 3. Real Minecraft 26.3 asset decoding & baking via `horizon-assets`:
//!    - Syncs prepared 26.3 assets from `romfs:/` to `sdmc:/switch/horizon/assets/` if needed.
//!    - Parses 23 block models (`CuboidModel::from_json`) and resolves parent chains (`ModelLibrary`).
//!    - Parses 10 blockstate definitions (`BlockStateDefinition::from_json`).
//!    - Bakes block geometry into `ModelQuad`s (`bake_block_model` & `bake_flat_face`), including
//!      multi-element models (`grass_block` base + biome-tinted `grass_block_side_overlay`),
//!      rotations, UV locks, culling directions, and `tint_index`.
//!    - Decodes 14 real 26.3 PNG block textures (`png::Decoder`) and 2 `.png.mcmeta` animation
//!      clips (`AnimationMetadata::from_json` -> `AnimationClip` for `water_still` & `sea_lantern`).
//! 4. Real `wgpu-hal` (v30.0.1) GLES backend over Nintendo Switch EGL + OpenGL ES 3.2 (`NV120`):
//!    - `wgpu_hal::gles::Adapter::new_external` & `wgpu_hal::gles::Surface::new_external`.
//!    - `wgpu_hal::Adapter::open`, `wgpu_hal::Surface::configure`, `acquire_texture`, `present`.
//!    - `wgpu_hal::Device` creation of `ShaderModule` (Naga WGSL), `BindGroupLayout`,
//!      `PipelineLayout`, `RenderPipeline`, `Texture` (`D2Array` 16x16x14 + `Depth24Plus`),
//!      `TextureView`, `Sampler`, `Buffer` (Uniform, Vertex, Staging), `BindGroup`,
//!      `CommandEncoder`, and `Fence`.
//!    - `wgpu_hal::CommandEncoder` recording of `copy_buffer_to_texture`, `copy_buffer_to_buffer`,
//!      `begin_render_pass`, `set_render_pipeline`, `set_bind_group`, `set_vertex_buffer`, `draw`.
//! 5. Background 20 TPS `"Server thread"` + live Joy-Con / Pro Controller interaction + clean `+` exit.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::fs;
use std::num::NonZeroU64;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use horizon_assets::animation::{AnimationClip, AnimationMetadata};
use horizon_assets::bake::{BlockModelTransform, ModelQuad, bake_block_model, bake_flat_face};
use horizon_assets::blockstate::{BlockStateDefinition, Variant, VariantSet};
use horizon_assets::model::{CuboidModel, Direction as AssetDirection, Material};
use horizon_assets::resolve::ModelLibrary;
use horizon_client_core::text::Text;
use horizon_core::{
    geometry::Direction,
    noise::PerlinNoise,
    position::{BlockPos, ChunkPos},
    random::{LegacyRandom, RandomSource},
    version::{DATA_VERSION, PROTOCOL_VERSION, VANILLA_VERSION},
};
use horizon_items::items::ItemStack;
use horizon_world::chunk::{BlockProperties, BlockState, PalettedContainer};

use wgpu_hal::{
    Adapter as HalAdapter, Api as HalApi, Attachment, AttachmentOps, BindGroupDescriptor,
    BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutFlags, BufferBinding, BufferCopy,
    BufferDescriptor, BufferTextureCopy, ColorAttachment, CommandEncoder as HalCommandEncoder,
    CommandEncoderDescriptor, CopyExtent, DepthStencilAttachment, Device as HalDevice,
    FormatAspects, MemoryFlags, NagaShader, PipelineLayoutDescriptor, PipelineLayoutFlags,
    ProgrammableStage, Queue as HalQueue, RenderPassDescriptor, RenderPipelineDescriptor,
    SamplerDescriptor, ShaderInput, ShaderModuleDescriptor, Surface as HalSurface,
    SurfaceConfiguration, TextureBinding, TextureCopyBase, TextureDescriptor,
    TextureViewDescriptor, VertexBufferLayout, VertexProcessor,
};

type GlesApi = wgpu_hal::gles::Api;

include!("../../runtime-audit/abi_assertions.rs");

unsafe extern "C" {
    fn probe_log(s: *const c_char);
    fn horizon_switch_default_window() -> *mut c_void;
    fn horizon_switch_romfs_mounted() -> c_int;
    fn horizon_switch_poll(
        buttons_down_out: *mut u64,
        buttons_held_out: *mut u64,
        lx_out: *mut i32,
        ly_out: *mut i32,
        rx_out: *mut i32,
        ry_out: *mut i32,
    ) -> c_int;
}

fn log_line(msg: &str) {
    if let Ok(c) = CString::new(msg) {
        unsafe { probe_log(c.as_ptr()) };
    }
}

// ============================================================================
// Raw EGL 1.5 FFI (linked statically from devkitPro portlibs libEGL.a)
// ============================================================================
type EGLDisplay = *mut c_void;
type EGLConfig = *mut c_void;
type EGLContext = *mut c_void;
type EGLSurface = *mut c_void;
type EGLNativeWindowType = *mut c_void;
type EGLBoolean = c_uint;
type EGLint = i32;

const EGL_FALSE: EGLBoolean = 0;
const EGL_DEFAULT_DISPLAY: *mut c_void = ptr::null_mut();
const EGL_NO_CONTEXT: EGLContext = ptr::null_mut();
const EGL_NO_DISPLAY: EGLDisplay = ptr::null_mut();
const EGL_NO_SURFACE: EGLSurface = ptr::null_mut();
const EGL_OPENGL_ES_API: c_uint = 0x30A0;
const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
const EGL_OPENGL_ES3_BIT_KHR: EGLint = 0x0040;
const EGL_RED_SIZE: EGLint = 0x3024;
const EGL_GREEN_SIZE: EGLint = 0x3023;
const EGL_BLUE_SIZE: EGLint = 0x3022;
const EGL_ALPHA_SIZE: EGLint = 0x3021;
const EGL_DEPTH_SIZE: EGLint = 0x3025;
const EGL_STENCIL_SIZE: EGLint = 0x3026;
const EGL_NONE: EGLint = 0x3038;
const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;

unsafe extern "C" {
    fn eglGetDisplay(display_id: *mut c_void) -> EGLDisplay;
    fn eglInitialize(dpy: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> EGLBoolean;
    fn eglBindAPI(api: c_uint) -> EGLBoolean;
    fn eglChooseConfig(
        dpy: EGLDisplay,
        attrib_list: *const EGLint,
        configs: *mut EGLConfig,
        config_size: EGLint,
        num_config: *mut EGLint,
    ) -> EGLBoolean;
    fn eglCreateWindowSurface(
        dpy: EGLDisplay,
        config: EGLConfig,
        win: EGLNativeWindowType,
        attrib_list: *const EGLint,
    ) -> EGLSurface;
    fn eglCreateContext(
        dpy: EGLDisplay,
        config: EGLConfig,
        share_context: EGLContext,
        attrib_list: *const EGLint,
    ) -> EGLContext;
    fn eglMakeCurrent(
        dpy: EGLDisplay,
        draw: EGLSurface,
        read: EGLSurface,
        ctx: EGLContext,
    ) -> EGLBoolean;
    fn eglSwapBuffers(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean;
    fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    fn eglTerminate(dpy: EGLDisplay) -> EGLBoolean;
    fn eglGetProcAddress(procname: *const c_char) -> *const c_void;
}

#[derive(Clone, Copy)]
struct EglHandles {
    display: EGLDisplay,
    surface: EGLSurface,
    context: EGLContext,
}

unsafe impl Send for EglHandles {}
unsafe impl Sync for EglHandles {}

struct EglSession {
    handles: EglHandles,
}

impl EglSession {
    fn init() -> Result<Self, String> {
        unsafe {
            let win = horizon_switch_default_window();
            if win.is_null() {
                return Err("nwindowGetDefault returned null".into());
            }
            let display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
            if display == EGL_NO_DISPLAY {
                return Err("eglGetDisplay failed".into());
            }
            let mut major: EGLint = 0;
            let mut minor: EGLint = 0;
            if eglInitialize(display, &mut major, &mut minor) == EGL_FALSE {
                return Err("eglInitialize failed".into());
            }
            if eglBindAPI(EGL_OPENGL_ES_API) == EGL_FALSE {
                eglTerminate(display);
                return Err("eglBindAPI(EGL_OPENGL_ES_API) failed".into());
            }
            let cfg_attribs: [EGLint; 15] = [
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES3_BIT_KHR,
                EGL_RED_SIZE,
                8,
                EGL_GREEN_SIZE,
                8,
                EGL_BLUE_SIZE,
                8,
                EGL_ALPHA_SIZE,
                8,
                EGL_DEPTH_SIZE,
                24,
                EGL_STENCIL_SIZE,
                8,
                EGL_NONE,
            ];
            let mut config: EGLConfig = ptr::null_mut();
            let mut num_configs: EGLint = 0;
            if eglChooseConfig(
                display,
                cfg_attribs.as_ptr(),
                &mut config,
                1,
                &mut num_configs,
            ) == EGL_FALSE
                || num_configs < 1
            {
                eglTerminate(display);
                return Err("eglChooseConfig failed".into());
            }
            let surface = eglCreateWindowSurface(display, config, win, ptr::null());
            if surface == EGL_NO_SURFACE {
                eglTerminate(display);
                return Err("eglCreateWindowSurface failed".into());
            }
            let ctx_attribs: [EGLint; 3] = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
            let context =
                eglCreateContext(display, config, EGL_NO_CONTEXT, ctx_attribs.as_ptr());
            if context == EGL_NO_CONTEXT {
                eglDestroySurface(display, surface);
                eglTerminate(display);
                return Err("eglCreateContext failed".into());
            }
            if eglMakeCurrent(display, surface, surface, context) == EGL_FALSE {
                eglDestroyContext(display, context);
                eglDestroySurface(display, surface);
                eglTerminate(display);
                return Err("eglMakeCurrent failed".into());
            }
            log_line(&format!(
                "INFO EGL {}.{} initialized on Switch default NWindow",
                major, minor
            ));
            Ok(Self {
                handles: EglHandles {
                    display,
                    surface,
                    context,
                },
            })
        }
    }
}

impl Drop for EglSession {
    fn drop(&mut self) {
        unsafe {
            eglMakeCurrent(
                self.handles.display,
                EGL_NO_SURFACE,
                EGL_NO_SURFACE,
                EGL_NO_CONTEXT,
            );
            if self.handles.context != EGL_NO_CONTEXT {
                eglDestroyContext(self.handles.display, self.handles.context);
            }
            if self.handles.surface != EGL_NO_SURFACE {
                eglDestroySurface(self.handles.display, self.handles.surface);
            }
            if self.handles.display != EGL_NO_DISPLAY {
                eglTerminate(self.handles.display);
            }
        }
        log_line("INFO EGL session cleanly terminated");
    }
}

// ============================================================================
// Horizon WGSL Terrain Shader (compiled via Naga -> wgpu-hal ShaderModule)
// ============================================================================
const HORIZON_WGSL_SHADER: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    sky_color: vec4<f32>,
    sun_dir: vec4<f32>,
    camera_pos: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(0) @binding(1)
var terrain_tex: texture_2d_array<f32>;

@group(0) @binding(2)
var terrain_sampler: sampler;

struct VertexInput {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv_layer: vec3<f32>,
    @location(3) color_ao: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv_layer: vec3<f32>,
    @location(3) color_ao: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_pos = camera.view_proj * vec4<f32>(in.pos, 1.0);
    out.world_pos = in.pos;
    out.normal = in.normal;
    out.uv_layer = in.uv_layer;
    out.color_ao = in.color_ao;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let layer_idx = i32(round(in.uv_layer.z));
    let tex = textureSample(terrain_tex, terrain_sampler, in.uv_layer.xy, layer_idx);
    if (tex.a < 0.5) {
        discard;
    }

    let n = normalize(in.normal);
    let l = normalize(camera.sun_dir.xyz);
    let ndotl = max(dot(n, l), 0.0);

    // Horizon/Vanilla directional face shading + ambient + sun diffuse
    let face_shade = in.color_ao.a;
    let light = clamp((0.42 + 0.58 * ndotl) * face_shade, 0.22, 1.15);
    let tinted = tex.rgb * in.color_ao.rgb * light;

    // Distance fog blending into Horizon sky
    let dist = distance(in.world_pos, camera.camera_pos.xyz);
    let fog_factor = clamp((dist - 24.0) / 38.0, 0.0, 0.72);
    let final_rgb = mix(tinted, camera.sky_color.rgb, fog_factor);

    return vec4<f32>(final_rgb, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    sky_color: [f32; 4],
    sun_dir: [f32; 4],
    camera_pos: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct TerrainVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv_layer: [f32; 3],
    color_ao: [f32; 4],
}

// ============================================================================
// Voxel Block Palette & Real 26.3 Asset Bundle Loader (`horizon-assets` + `png`)
// ============================================================================
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum VoxelBlock {
    Air = 0,
    GrassBlock = 1,
    Dirt = 2,
    Stone = 3,
    OakLog = 4,
    OakLeaves = 5,
    Water = 6,
    Sand = 7,
    GoldBlock = 8,
    SeaLantern = 9,
    Bedrock = 10,
}

impl VoxelBlock {
    const ALL_NON_AIR: [Self; 10] = [
        Self::GrassBlock,
        Self::Dirt,
        Self::Stone,
        Self::OakLog,
        Self::OakLeaves,
        Self::Water,
        Self::Sand,
        Self::GoldBlock,
        Self::SeaLantern,
        Self::Bedrock,
    ];

    fn block_name(self) -> &'static str {
        match self {
            Self::Air => "air",
            Self::GrassBlock => "grass_block",
            Self::Dirt => "dirt",
            Self::Stone => "stone",
            Self::OakLog => "oak_log",
            Self::OakLeaves => "oak_leaves",
            Self::Water => "water",
            Self::Sand => "sand",
            Self::GoldBlock => "gold_block",
            Self::SeaLantern => "sea_lantern",
            Self::Bedrock => "bedrock",
        }
    }

    fn is_opaque(self) -> bool {
        !matches!(self, Self::Air | Self::Water | Self::OakLeaves)
    }

    fn should_cull_against(self, neighbor: Self) -> bool {
        if neighbor == Self::Air {
            return false;
        }
        if self == Self::Water {
            return neighbor == Self::Water || neighbor.is_opaque();
        }
        if self == Self::OakLeaves {
            return neighbor.is_opaque();
        }
        neighbor.is_opaque()
    }

    fn biome_tint(self, tint_index: i32) -> [f32; 3] {
        if tint_index < 0 {
            return [1.0, 1.0, 1.0];
        }
        match self {
            // Plains / Forest lush grass & foliage colors
            Self::GrassBlock => [0.49, 0.78, 0.31],
            Self::OakLeaves => [0.38, 0.72, 0.25],
            Self::Water => [0.25, 0.46, 0.89],
            _ => [1.0, 1.0, 1.0],
        }
    }
}

#[derive(serde::Deserialize)]
struct SwitchAssetManifest {
    version: String,
    client_jar_sha1: String,
    models: Vec<String>,
    textures: Vec<String>,
    files: Vec<String>,
}

struct AnimatedLayer {
    layer_index: u32,
    frames: Vec<Vec<u8>>,
    clip: AnimationClip,
    current_frame: usize,
}

struct BakedAssetRegistry {
    source_label: String,
    texture_layers: Vec<Vec<u8>>, // Each layer is 16*16*4 = 1024 bytes RGBA8
    sprite_to_layer: BTreeMap<String, u32>,
    baked_quads: BTreeMap<VoxelBlock, Vec<ModelQuad>>,
    animations: Vec<AnimatedLayer>,
}

/// Ensures `sdmc:/switch/horizon/assets/manifest.json` and all its referenced files exist on SD,
/// syncing from `romfs:/` automatically if `romfs` is mounted.
fn ensure_sdcard_assets() -> Result<String, String> {
    let sd_root = Path::new("sdmc:/switch/horizon");
    let sd_manifest = sd_root.join("assets/manifest.json");

    let needs_sync = if let Ok(text) = fs::read_to_string(&sd_manifest) {
        if let Ok(manifest) = serde_json::from_str::<SwitchAssetManifest>(&text) {
            manifest
                .files
                .iter()
                .any(|rel| !sd_root.join(rel).is_file())
        } else {
            true
        }
    } else {
        true
    };

    if needs_sync {
        if unsafe { horizon_switch_romfs_mounted() } != 0 {
            let romfs_root = Path::new("romfs:");
            let romfs_manifest_path = romfs_root.join("assets/manifest.json");
            let manifest_text = fs::read_to_string(&romfs_manifest_path)
                .map_err(|e| format!("Failed to read romfs:/assets/manifest.json: {e}"))?;
            let manifest: SwitchAssetManifest = serde_json::from_str(&manifest_text)
                .map_err(|e| format!("Invalid romfs manifest.json: {e}"))?;

            let mut synced_bytes = 0usize;
            for rel in &manifest.files {
                let src = romfs_root.join(rel);
                let dst = sd_root.join(rel);
                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let data =
                    fs::read(&src).map_err(|e| format!("Failed reading {}: {e}", src.display()))?;
                synced_bytes += data.len();
                fs::write(&dst, &data)
                    .map_err(|e| format!("Failed writing {}: {e}", dst.display()))?;
            }
            if let Some(parent) = sd_manifest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&sd_manifest, manifest_text.as_bytes())
                .map_err(|e| format!("Failed writing {}: {e}", sd_manifest.display()))?;

            log_line(&format!(
                "INFO Synced {} Minecraft {} asset files ({} bytes) to sdmc:/switch/horizon/assets/",
                manifest.files.len(),
                manifest.version,
                synced_bytes
            ));
        }
    }

    if sd_manifest.is_file() {
        Ok("sdmc:/switch/horizon".to_string())
    } else {
        Err("sdmc:/switch/horizon/assets/manifest.json not found".into())
    }
}

/// Decodes a PNG file into 16x16 RGBA8 frames (splitting vertical strips for animated sprites).
fn decode_png_frames(bytes: &[u8]) -> Result<(u32, Vec<Vec<u8>>), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.ignore_checksums(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("png read_info: {e}"))?;
    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG exceeds buffer limits".to_string())?;
    let mut raw = vec![0u8; buf_size];
    let info = reader
        .next_frame(&mut raw)
        .map_err(|e| format!("png next_frame: {e}"))?;
    let channels = info.color_type.samples();
    let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
    for px in raw[..info.buffer_size()].chunks_exact(channels) {
        let p = match info.color_type {
            png::ColorType::Rgba => [px[0], px[1], px[2], px[3]],
            png::ColorType::Rgb => [px[0], px[1], px[2], 255],
            png::ColorType::Grayscale => [px[0], px[0], px[0], 255],
            png::ColorType::GrayscaleAlpha => [px[0], px[0], px[0], px[1]],
            _ => return Err("Unsupported expanded PNG color type".into()),
        };
        rgba.extend_from_slice(&p);
    }

    let side = info.width;
    if side == 0 || info.height == 0 || info.height % side != 0 {
        return Err(format!(
            "Invalid block sprite dimensions {}x{}",
            info.width, info.height
        ));
    }
    let frame_count = (info.height / side) as usize;
    let frame_bytes = (side * side * 4) as usize;
    let mut frames = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        frames.push(rgba[i * frame_bytes..(i + 1) * frame_bytes].to_vec());
    }
    Ok((side, frames))
}

fn select_variant<'a>(def: &'a BlockStateDefinition, preferred_key: &str) -> Result<&'a Variant, String> {
    let Some(ref variants) = def.variants else {
        return Err("Blockstate has no variants".into());
    };
    let set = variants
        .iter()
        .find(|(k, _)| k == preferred_key || k.is_empty())
        .or_else(|| variants.first())
        .map(|(_, v)| v)
        .ok_or_else(|| "Empty variant list".to_string())?;
    match set {
        VariantSet::Single(v) => Ok(v),
        VariantSet::Weighted(list) => list
            .first()
            .map(|(v, _)| v)
            .ok_or_else(|| "Empty weighted variant list".to_string()),
    }
}

fn load_and_bake_assets() -> Result<BakedAssetRegistry, String> {
    let root_str = ensure_sdcard_assets()?;
    let root = Path::new(&root_str);
    let manifest_text = fs::read_to_string(root.join("assets/manifest.json"))
        .map_err(|e| format!("read manifest.json: {e}"))?;
    let manifest: SwitchAssetManifest =
        serde_json::from_str(&manifest_text).map_err(|e| format!("parse manifest.json: {e}"))?;

    // 1. Decode all 14 PNG textures and optional .mcmeta animations
    let mut texture_layers = Vec::with_capacity(manifest.textures.len());
    let mut sprite_to_layer = BTreeMap::new();
    let mut animations = Vec::new();

    for (idx, tex_rel) in manifest.textures.iter().enumerate() {
        let layer_idx = idx as u32;
        let png_path = root.join(format!("assets/minecraft/textures/{tex_rel}.png"));
        let png_bytes =
            fs::read(&png_path).map_err(|e| format!("read {}: {e}", png_path.display()))?;
        let (side, frames) = decode_png_frames(&png_bytes)?;
        if side != 16 {
            return Err(format!("Expected 16x16 texture for {tex_rel}, got {side}x{side}"));
        }

        let mcmeta_path = root.join(format!("assets/minecraft/textures/{tex_rel}.png.mcmeta"));
        let mut initial_frame = 0usize;
        if mcmeta_path.is_file() {
            let mcmeta_text = fs::read_to_string(&mcmeta_path)
                .map_err(|e| format!("read {}: {e}", mcmeta_path.display()))?;
            let meta = AnimationMetadata::from_json(&mcmeta_text)
                .map_err(|e| format!("AnimationMetadata {tex_rel}: {e}"))?;
            if let Some(clip) = meta
                .clip(frames.len() as u32)
                .map_err(|e| format!("AnimationClip {tex_rel}: {e}"))?
            {
                initial_frame = clip.sample(0).frame as usize;
                animations.push(AnimatedLayer {
                    layer_index: layer_idx,
                    frames: frames.clone(),
                    clip,
                    current_frame: initial_frame,
                });
            }
        }

        texture_layers.push(frames[initial_frame].clone());
        let full_id = format!("minecraft:{tex_rel}");
        sprite_to_layer.insert(full_id, layer_idx);
    }

    // 2. Decode all 23 CuboidModels and build ModelLibrary
    let mut models_map = BTreeMap::new();
    for model_rel in &manifest.models {
        let model_path = root.join(format!("assets/minecraft/models/{model_rel}.json"));
        let model_text =
            fs::read_to_string(&model_path).map_err(|e| format!("read {}: {e}", model_path.display()))?;
        let cuboid = CuboidModel::from_json(&model_text)
            .map_err(|e| format!("CuboidModel::from_json({model_rel}): {e}"))?;
        models_map.insert(format!("minecraft:{model_rel}"), cuboid);
    }
    let model_lib =
        ModelLibrary::new(models_map).map_err(|e| format!("ModelLibrary::new: {e}"))?;

    // 3. Decode all 10 BlockStateDefinitions and bake ModelQuads
    let mut baked_quads = BTreeMap::new();
    for block in VoxelBlock::ALL_NON_AIR {
        let name = block.block_name();
        let bs_path = root.join(format!("assets/minecraft/blockstates/{name}.json"));
        let bs_text =
            fs::read_to_string(&bs_path).map_err(|e| format!("read {}: {e}", bs_path.display()))?;
        let def = BlockStateDefinition::from_json(&bs_text)
            .map_err(|e| format!("BlockStateDefinition({name}): {e}"))?;

        let preferred_key = match block {
            VoxelBlock::GrassBlock => "snowy=false",
            VoxelBlock::OakLog => "axis=y",
            _ => "",
        };
        let variant = select_variant(&def, preferred_key)?;
        let resolved = model_lib
            .resolve(&variant.model)
            .map_err(|e| format!("resolve({}): {e}", variant.model))?;

        let mut quads = bake_block_model(
            &resolved,
            BlockModelTransform::new(variant.rotations, variant.uv_lock),
        )
        .map_err(|e| format!("bake_block_model({name}): {e}"))?;

        // `water.json` defines `particle: block/water_still` without cuboid elements;
        // bake its 6 fluid faces via `horizon_assets::bake::bake_flat_face`.
        if quads.is_empty() && block == VoxelBlock::Water {
            let mat = resolved
                .material("particle")
                .ok()
                .flatten()
                .cloned()
                .unwrap_or(Material {
                    sprite: "minecraft:block/water_still".to_string(),
                    force_translucent: false,
                });
            for dir in AssetDirection::ALL {
                let mut q = bake_flat_face(
                    [0.0, 0.0, 0.0],
                    [16.0, 16.0, 16.0],
                    [0.0, 0.0, 16.0, 16.0],
                    dir,
                    mat.clone(),
                    0,
                )
                .map_err(|e| format!("bake_flat_face(water): {e}"))?;
                q.cull = Some(dir);
                quads.push(q);
            }
        }

        if quads.is_empty() {
            return Err(format!("Block {name} produced 0 baked quads"));
        }
        baked_quads.insert(block, quads);
    }

    Ok(BakedAssetRegistry {
        source_label: format!(
            "{} (26.3 jar sha1={}..)",
            root_str,
            &manifest.client_jar_sha1[..12]
        ),
        texture_layers,
        sprite_to_layer,
        baked_quads,
        animations,
    })
}

// ============================================================================
// Horizon Voxel World & Baked ModelQuad Mesh Builder
// ============================================================================
fn block_tag(block: VoxelBlock) -> horizon_world::nbt::Tag {
    BlockState {
        name: format!("minecraft:{}", block.block_name()),
        properties: BlockProperties::new(),
    }
    .to_nbt()
}

struct HorizonVoxelScene {
    chunk_pos: ChunkPos,
    container: PalettedContainer,
    blocks: Vec<VoxelBlock>, // 16 * 32 * 16
    mesh_dirty: bool,
    mesh_generations: u64,
}

impl HorizonVoxelScene {
    fn idx(x: i32, y: i32, z: i32) -> usize {
        ((y as usize) * 16 + (z as usize)) * 16 + (x as usize)
    }

    fn new() -> Self {
        let container = PalettedContainer::uniform(block_tag(VoxelBlock::Air), 4096, 4)
            .expect("PalettedContainer::uniform");
        let mut scene = Self {
            chunk_pos: ChunkPos::new(0, 0),
            container,
            blocks: vec![VoxelBlock::Air; 16 * 32 * 16],
            mesh_dirty: true,
            mesh_generations: 0,
        };
        scene.populate_island();
        scene
    }

    fn get(&self, x: i32, y: i32, z: i32) -> VoxelBlock {
        if !(0..16).contains(&x) || !(0..32).contains(&y) || !(0..16).contains(&z) {
            return VoxelBlock::Air;
        }
        self.blocks[Self::idx(x, y, z)]
    }

    fn set(&mut self, x: i32, y: i32, z: i32, block: VoxelBlock) {
        if (0..16).contains(&x) && (0..32).contains(&y) && (0..16).contains(&z) {
            self.blocks[Self::idx(x, y, z)] = block;
            if y < 16 {
                let section_idx = BlockPos::new(x, y, z).section_index();
                let _ = self.container.set(section_idx, block_tag(block));
            }
            self.mesh_dirty = true;
        }
    }

    fn populate_island(&mut self) {
        let mut rng = LegacyRandom::new(2026_2603);
        let perlin = PerlinNoise::new(&mut rng);
        let _ = rng.next_int_bound(16);

        for x in 0..16 {
            for z in 0..16 {
                self.set(x, 0, z, VoxelBlock::Bedrock);

                let nx = (x as f64) * 0.11;
                let nz = (z as f64) * 0.11;
                let n = perlin.get2(nx, nz) as f32;
                let dx = (x as f32 - 7.5) / 7.5;
                let dz = (z as f32 - 7.5) / 7.5;
                let dist_sq = dx * dx + dz * dz;
                let hill = ((1.0 - dist_sq).max(0.0) * 6.0 + n * 2.2).round() as i32;
                let surface_y = (3 + hill).clamp(2, 11);

                for y in 1..=surface_y {
                    let blk = if y == surface_y {
                        if surface_y <= 4 {
                            VoxelBlock::Sand
                        } else {
                            VoxelBlock::GrassBlock
                        }
                    } else if y >= surface_y - 2 {
                        if surface_y <= 4 {
                            VoxelBlock::Sand
                        } else {
                            VoxelBlock::Dirt
                        }
                    } else {
                        VoxelBlock::Stone
                    };
                    self.set(x, y, z, blk);
                }

                if surface_y < 4 {
                    for y in (surface_y + 1)..=4 {
                        self.set(x, y, z, VoxelBlock::Water);
                    }
                }
            }
        }

        // Oak tree using real `oak_log` and `oak_leaves` baked models
        let tree_x = 8;
        let tree_z = 8;
        let mut base_y = 4;
        for y in (1..20).rev() {
            if self.get(tree_x, y, tree_z) != VoxelBlock::Air {
                base_y = y + 1;
                break;
            }
        }
        for ty in base_y..(base_y + 4) {
            self.set(tree_x, ty, tree_z, VoxelBlock::OakLog);
        }
        for lx in (tree_x - 2)..=(tree_x + 2) {
            for lz in (tree_z - 2)..=(tree_z + 2) {
                for ly in (base_y + 2)..=(base_y + 4) {
                    let dist = (lx - tree_x).abs() + (lz - tree_z).abs();
                    if dist <= 3 && !(lx == tree_x && lz == tree_z && ly < base_y + 4) {
                        if ly == base_y + 4 && dist > 1 {
                            continue;
                        }
                        self.set(lx, ly, lz, VoxelBlock::OakLeaves);
                    }
                }
            }
        }

        // Beacon monument (GoldBlock + animated SeaLantern)
        let bx = 4;
        let bz = 5;
        let mut by = 4;
        for y in (1..20).rev() {
            if self.get(bx, y, bz) != VoxelBlock::Air {
                by = y + 1;
                break;
            }
        }
        self.set(bx, by, bz, VoxelBlock::GoldBlock);
        self.set(bx, by + 1, bz, VoxelBlock::SeaLantern);
    }

    fn build_mesh(&mut self, assets: &BakedAssetRegistry) -> Vec<TerrainVertex> {
        let mut vertices = Vec::with_capacity(16384);
        for y in 0..32 {
            for z in 0..16 {
                for x in 0..16 {
                    let block = self.get(x, y, z);
                    if block == VoxelBlock::Air {
                        continue;
                    }
                    let Some(quads) = assets.baked_quads.get(&block) else {
                        continue;
                    };
                    let origin = [x as f32, y as f32, z as f32];

                    for quad in quads {
                        if let Some(cull_dir) = quad.cull {
                            let (nx, ny, nz) = match cull_dir {
                                AssetDirection::Down => (x, y - 1, z),
                                AssetDirection::Up => (x, y + 1, z),
                                AssetDirection::North => (x, y, z - 1),
                                AssetDirection::South => (x, y, z + 1),
                                AssetDirection::West => (x - 1, y, z),
                                AssetDirection::East => (x + 1, y, z),
                            };
                            let neighbor = self.get(nx, ny, nz);
                            if block.should_cull_against(neighbor) {
                                continue;
                            }
                        }

                        let normal = match quad.facing {
                            AssetDirection::Down => [0.0, -1.0, 0.0],
                            AssetDirection::Up => [0.0, 1.0, 0.0],
                            AssetDirection::North => [0.0, 0.0, -1.0],
                            AssetDirection::South => [0.0, 0.0, 1.0],
                            AssetDirection::West => [-1.0, 0.0, 0.0],
                            AssetDirection::East => [1.0, 0.0, 0.0],
                        };
                        let face_shade = if !quad.shade {
                            1.0
                        } else if block == VoxelBlock::SeaLantern {
                            1.15
                        } else {
                            match quad.facing {
                                AssetDirection::Up => 1.00,
                                AssetDirection::Down => 0.52,
                                AssetDirection::North | AssetDirection::South => 0.82,
                                AssetDirection::West | AssetDirection::East => 0.68,
                            }
                        };
                        let tint = block.biome_tint(quad.tint_index);
                        let color_ao = [tint[0], tint[1], tint[2], face_shade];
                        let layer = assets
                            .sprite_to_layer
                            .get(&quad.material.sprite)
                            .copied()
                            .unwrap_or(0) as f32;

                        // Two CCW triangles [0, 1, 2] and [0, 2, 3]
                        for &vi in &[0usize, 1, 2, 0, 2, 3] {
                            let p = quad.positions[vi];
                            let uv = quad.uv[vi];
                            vertices.push(TerrainVertex {
                                pos: [origin[0] + p[0], origin[1] + p[1], origin[2] + p[2]],
                                normal,
                                uv_layer: [uv[0], uv[1], layer],
                                color_ao,
                            });
                        }
                    }
                }
            }
        }
        self.mesh_dirty = false;
        self.mesh_generations += 1;
        vertices
    }
}

// ============================================================================
// Helper functions for `wgpu-hal` staging buffer writes
// ============================================================================
unsafe fn write_staging_bytes(
    device: &<GlesApi as HalApi>::Device,
    staging_buf: &<GlesApi as HalApi>::Buffer,
    data: &[u8],
) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }
    let size = data.len() as u64;
    let mapping = unsafe { device.map_buffer(staging_buf, 0..size) }
        .map_err(|e| format!("map_buffer failed: {e:?}"))?;
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), mapping.ptr.as_ptr(), data.len());
        if !mapping.is_coherent {
            device.flush_mapped_ranges(staging_buf, std::iter::once(0..size));
        }
        device.unmap_buffer(staging_buf);
    }
    Ok(())
}

// ============================================================================
// Main Entry (`rust_horizon_renderer_main`)
// ============================================================================
const BTN_A: u64 = 1 << 0;
const BTN_B: u64 = 1 << 1;
const BTN_X: u64 = 1 << 2;
const BTN_Y: u64 = 1 << 3;
const BTN_L: u64 = 1 << 6;
const BTN_R: u64 = 1 << 7;
const BTN_LEFT: u64 = 1 << 12;
const BTN_UP: u64 = 1 << 13;
const BTN_RIGHT: u64 = 1 << 14;
const BTN_DOWN: u64 = 1 << 15;

const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 720;
const MAX_VERTEX_BUFFER_BYTES: u64 = 4 * 1024 * 1024; // 4 MiB
const STAGING_BUFFER_BYTES: u64 = 4 * 1024 * 1024; // 4 MiB

#[unsafe(no_mangle)]
pub extern "C" fn rust_horizon_renderer_main() -> c_int {
    log_line("INFO Compile-time ABI assertions passed (163 SDK checks)");

    // 1. Verify Horizon Core, World, Client-Core, and Items crates
    let welcome = Text::literal(format!(
        "Horizon {} (proto {}, data {}) — wgpu-hal GLES + horizon-assets on Switch",
        VANILLA_VERSION, PROTOCOL_VERSION, DATA_VERSION
    ));
    let held_stack = ItemStack::new("minecraft:grass_block", 64);
    let up = Direction::Up;
    let spawn_pos = BlockPos::new(8, 12, 8).offset(0, 1, 0);
    if held_stack.item.is_empty()
        || held_stack.count != 64
        || spawn_pos.y != 13
        || up.opposite() != Direction::Down
    {
        log_line("FAIL Horizon item/position sanity check failed");
        return 1;
    }
    log_line(&format!("INFO {}", welcome.plain()));

    // 2. Load and bake real Minecraft 26.3 assets via `horizon-assets` + `png`
    let mut assets = match load_and_bake_assets() {
        Ok(reg) => reg,
        Err(e) => {
            log_line(&format!("FAIL Asset loading/baking failed: {e}"));
            return 2;
        }
    };
    let grass_quad_count = assets
        .baked_quads
        .get(&VoxelBlock::GrassBlock)
        .map_or(0, Vec::len);
    let total_baked_quads: usize = assets.baked_quads.values().map(Vec::len).sum();
    log_line(&format!(
        "PASS Horizon 26.3 assets loaded and baked via horizon-assets: source='{}' blocks={} textures={} animations={} grass_quads={} total_quads={}",
        assets.source_label,
        assets.baked_quads.len(),
        assets.texture_layers.len(),
        assets.animations.len(),
        grass_quad_count,
        total_baked_quads
    ));

    // 3. Build initial voxel chunk mesh from `horizon-assets` baked `ModelQuad`s
    let scene = Arc::new(Mutex::new(HorizonVoxelScene::new()));
    let initial_vertices = {
        let mut guard = scene.lock().unwrap();
        guard.build_mesh(&assets)
    };
    if initial_vertices.is_empty() {
        log_line("FAIL Initial chunk mesh is empty");
        return 3;
    }
    log_line(&format!(
        "PASS Horizon chunk meshed from baked ModelQuads: vertices={} triangles={}",
        initial_vertices.len(),
        initial_vertices.len() / 3
    ));

    // 4. Initialize EGL 1.5 + OpenGL ES 3.2 and `wgpu-hal` GLES Adapter, Device, Queue, Surface
    let egl = match EglSession::init() {
        Ok(s) => s,
        Err(e) => {
            log_line(&format!("FAIL EGL initialization error: {e}"));
            return 4;
        }
    };

    let exposed_adapter = unsafe {
        wgpu_hal::gles::Adapter::new_external(
            |name| {
                if let Ok(c_name) = CString::new(name) {
                    eglGetProcAddress(c_name.as_ptr())
                } else {
                    ptr::null()
                }
            },
            wgpu_types::GlBackendOptions::default(),
        )
    };
    let Some(exposed_adapter) = exposed_adapter else {
        log_line("FAIL wgpu_hal::gles::Adapter::new_external returned None");
        return 5;
    };

    log_line(&format!(
        "PASS wgpu-hal Adapter exposed: name='{}' backend={:?} driver_info='{}'",
        exposed_adapter.info.name, exposed_adapter.info.backend, exposed_adapter.info.driver_info
    ));

    let egl_handles = egl.handles;
    let surface = unsafe {
        wgpu_hal::gles::Surface::new_external(move || {
            let _ = &egl_handles;
            eglSwapBuffers(egl_handles.display, egl_handles.surface) != EGL_FALSE
        })
    };

    let Some(surface_caps) = (unsafe { exposed_adapter.adapter.surface_capabilities(&surface) })
    else {
        log_line("FAIL wgpu-hal Adapter::surface_capabilities returned None");
        return 6;
    };
    let surface_format = surface_caps
        .formats
        .iter()
        .find(|f| f.format == wgpu_types::TextureFormat::Rgba8Unorm)
        .map(|f| f.format)
        .unwrap_or(wgpu_types::TextureFormat::Rgba8Unorm);

    let open_device = match unsafe {
        exposed_adapter.adapter.open(
            wgpu_types::Features::empty(),
            &exposed_adapter.capabilities.limits,
            &wgpu_types::MemoryHints::Performance,
        )
    } {
        Ok(od) => od,
        Err(e) => {
            log_line(&format!("FAIL wgpu-hal Adapter::open failed: {e:?}"));
            return 7;
        }
    };
    let device = open_device.device;
    let queue = open_device.queue;

    let surface_config = SurfaceConfiguration {
        maximum_frame_latency: 2,
        present_mode: wgpu_types::PresentMode::Fifo,
        composite_alpha_mode: wgpu_types::CompositeAlphaMode::Opaque,
        format: surface_format,
        color_space: wgpu_types::SurfaceColorSpace::Srgb,
        extent: wgpu_types::Extent3d {
            width: SCREEN_WIDTH,
            height: SCREEN_HEIGHT,
            depth_or_array_layers: 1,
        },
        usage: wgpu_types::TextureUses::COLOR_TARGET,
        view_formats: Vec::new(),
    };
    if let Err(e) = unsafe { surface.configure(&device, &surface_config) } {
        log_line(&format!("FAIL wgpu-hal Surface::configure failed: {e:?}"));
        return 8;
    }
    log_line(&format!(
        "PASS wgpu-hal Device, Queue, and Surface configured ({}x{} {:?})",
        SCREEN_WIDTH, SCREEN_HEIGHT, surface_format
    ));

    // 5. Parse WGSL via Naga and create `wgpu-hal` ShaderModule, Pipeline, Textures, Buffers, BindGroup
    let naga_module = match naga::front::wgsl::parse_str(HORIZON_WGSL_SHADER) {
        Ok(m) => m,
        Err(e) => {
            log_line(&format!("FAIL Naga WGSL parse error: {e}"));
            unsafe { surface.unconfigure(&device) };
            return 9;
        }
    };
    let naga_info = match naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&naga_module)
    {
        Ok(info) => info,
        Err(e) => {
            log_line(&format!("FAIL Naga validation error: {e}"));
            unsafe { surface.unconfigure(&device) };
            return 10;
        }
    };

    let shader_module = match unsafe {
        device.create_shader_module(
            &ShaderModuleDescriptor {
                label: Some("horizon_terrain_wgsl"),
                runtime_checks: wgpu_types::ShaderRuntimeChecks::unchecked(),
            },
            ShaderInput::Naga(NagaShader {
                module: Cow::Owned(naga_module),
                info: naga_info,
                debug_source: None,
            }),
        )
    } {
        Ok(sm) => sm,
        Err(e) => {
            log_line(&format!("FAIL create_shader_module failed: {e:?}"));
            unsafe { surface.unconfigure(&device) };
            return 11;
        }
    };

    let bgl_entries = [
        wgpu_types::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu_types::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu_types::BindingType::Buffer {
                ty: wgpu_types::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64),
            },
            count: None,
        },
        wgpu_types::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu_types::ShaderStages::FRAGMENT,
            ty: wgpu_types::BindingType::Texture {
                sample_type: wgpu_types::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu_types::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu_types::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu_types::ShaderStages::FRAGMENT,
            ty: wgpu_types::BindingType::Sampler(wgpu_types::SamplerBindingType::Filtering),
            count: None,
        },
    ];

    let bind_group_layout = match unsafe {
        device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("horizon_terrain_bgl"),
            flags: BindGroupLayoutFlags::empty(),
            entries: &bgl_entries,
        })
    } {
        Ok(bgl) => bgl,
        Err(e) => {
            log_line(&format!("FAIL create_bind_group_layout failed: {e:?}"));
            unsafe {
                device.destroy_shader_module(shader_module);
                surface.unconfigure(&device);
            }
            return 12;
        }
    };

    let pipeline_layout = match unsafe {
        device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("horizon_terrain_pipeline_layout"),
            flags: PipelineLayoutFlags::empty(),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        })
    } {
        Ok(pl) => pl,
        Err(e) => {
            log_line(&format!("FAIL create_pipeline_layout failed: {e:?}"));
            unsafe {
                device.destroy_bind_group_layout(bind_group_layout);
                device.destroy_shader_module(shader_module);
                surface.unconfigure(&device);
            }
            return 13;
        }
    };

    let vertex_attributes = [
        wgpu_types::VertexAttribute {
            format: wgpu_types::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        wgpu_types::VertexAttribute {
            format: wgpu_types::VertexFormat::Float32x3,
            offset: 12,
            shader_location: 1,
        },
        wgpu_types::VertexAttribute {
            format: wgpu_types::VertexFormat::Float32x3,
            offset: 24,
            shader_location: 2,
        },
        wgpu_types::VertexAttribute {
            format: wgpu_types::VertexFormat::Float32x4,
            offset: 36,
            shader_location: 3,
        },
    ];
    let vertex_buffer_layouts = [Some(VertexBufferLayout {
        array_stride: std::mem::size_of::<TerrainVertex>() as u64,
        step_mode: wgpu_types::VertexStepMode::Vertex,
        attributes: &vertex_attributes,
    })];
    let pipeline_constants = naga::back::PipelineConstants::default();

    let render_pipeline = match unsafe {
        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("horizon_terrain_pipeline"),
            layout: &pipeline_layout,
            vertex_processor: VertexProcessor::Standard {
                vertex_buffers: &vertex_buffer_layouts,
                vertex_stage: ProgrammableStage {
                    module: &shader_module,
                    entry_point: "vs_main",
                    constants: &pipeline_constants,
                    zero_initialize_workgroup_memory: true,
                },
            },
            primitive: wgpu_types::PrimitiveState {
                topology: wgpu_types::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu_types::FrontFace::Ccw,
                cull_mode: Some(wgpu_types::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu_types::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu_types::DepthStencilState {
                format: wgpu_types::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(true),
                // LessEqual allows multi-element block models (like grass_block's
                // grass_block_side_overlay over grass_block_side) to composite cleanly.
                depth_compare: Some(wgpu_types::CompareFunction::LessEqual),
                stencil: wgpu_types::StencilState::default(),
                bias: wgpu_types::DepthBiasState::default(),
            }),
            multisample: wgpu_types::MultisampleState::default(),
            fragment_stage: Some(ProgrammableStage {
                module: &shader_module,
                entry_point: "fs_main",
                constants: &pipeline_constants,
                zero_initialize_workgroup_memory: true,
            }),
            color_targets: &[Some(wgpu_types::ColorTargetState {
                format: surface_format,
                blend: None,
                write_mask: wgpu_types::ColorWrites::ALL,
            })],
            multiview_mask: None,
            cache: None,
        })
    } {
        Ok(rp) => rp,
        Err(e) => {
            log_line(&format!("FAIL create_render_pipeline failed: {e:?}"));
            unsafe {
                device.destroy_pipeline_layout(pipeline_layout);
                device.destroy_bind_group_layout(bind_group_layout);
                device.destroy_shader_module(shader_module);
                surface.unconfigure(&device);
            }
            return 14;
        }
    };

    // Create 2D Array Texture for the 14 real 26.3 block sprites
    let layer_count = assets.texture_layers.len() as u32;
    let atlas_texture = unsafe {
        device
            .create_texture(&TextureDescriptor {
                label: Some("horizon_block_atlas_2d_array"),
                size: wgpu_types::Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: layer_count,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu_types::TextureDimension::D2,
                format: wgpu_types::TextureFormat::Rgba8Unorm,
                usage: wgpu_types::TextureUses::COPY_DST | wgpu_types::TextureUses::RESOURCE,
                memory_flags: MemoryFlags::empty(),
                view_formats: Vec::new(),
            })
            .expect("create_texture(atlas)")
    };
    let atlas_view = unsafe {
        device
            .create_texture_view(
                &atlas_texture,
                &TextureViewDescriptor {
                    label: Some("horizon_block_atlas_view"),
                    format: wgpu_types::TextureFormat::Rgba8Unorm,
                    dimension: wgpu_types::TextureViewDimension::D2Array,
                    usage: wgpu_types::TextureUses::RESOURCE,
                    range: wgpu_types::ImageSubresourceRange::default(),
                },
            )
            .expect("create_texture_view(atlas)")
    };
    let sampler = unsafe {
        device
            .create_sampler(&SamplerDescriptor {
                label: Some("horizon_nearest_sampler"),
                address_modes: [wgpu_types::AddressMode::Repeat; 3],
                mag_filter: wgpu_types::FilterMode::Nearest,
                min_filter: wgpu_types::FilterMode::Nearest,
                mipmap_filter: wgpu_types::MipmapFilterMode::Nearest,
                lod_clamp: 0.0..32.0,
                compare: None,
                anisotropy_clamp: 1,
                border_color: None,
            })
            .expect("create_sampler")
    };

    // Create 1280x720 Depth24Plus Texture & View
    let depth_texture = unsafe {
        device
            .create_texture(&TextureDescriptor {
                label: Some("horizon_depth_texture"),
                size: wgpu_types::Extent3d {
                    width: SCREEN_WIDTH,
                    height: SCREEN_HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu_types::TextureDimension::D2,
                format: wgpu_types::TextureFormat::Depth24Plus,
                usage: wgpu_types::TextureUses::DEPTH_STENCIL_WRITE,
                memory_flags: MemoryFlags::empty(),
                view_formats: Vec::new(),
            })
            .expect("create_texture(depth)")
    };
    let depth_view = unsafe {
        device
            .create_texture_view(
                &depth_texture,
                &TextureViewDescriptor {
                    label: Some("horizon_depth_view"),
                    format: wgpu_types::TextureFormat::Depth24Plus,
                    dimension: wgpu_types::TextureViewDimension::D2,
                    usage: wgpu_types::TextureUses::DEPTH_STENCIL_WRITE,
                    range: wgpu_types::ImageSubresourceRange {
                        aspect: wgpu_types::TextureAspect::DepthOnly,
                        base_mip_level: 0,
                        mip_level_count: None,
                        base_array_layer: 0,
                        array_layer_count: None,
                    },
                },
            )
            .expect("create_texture_view(depth)")
    };

    // Create Camera Uniform Buffer, Terrain Vertex Buffer, and Host Staging Buffer
    let camera_buffer = unsafe {
        device
            .create_buffer(&BufferDescriptor {
                label: Some("horizon_camera_ubo"),
                size: std::mem::size_of::<CameraUniform>() as u64,
                usage: wgpu_types::BufferUses::COPY_DST | wgpu_types::BufferUses::UNIFORM,
                memory_flags: MemoryFlags::empty(),
            })
            .expect("create_buffer(camera)")
    };
    let vertex_buffer = unsafe {
        device
            .create_buffer(&BufferDescriptor {
                label: Some("horizon_terrain_vbo"),
                size: MAX_VERTEX_BUFFER_BYTES,
                usage: wgpu_types::BufferUses::COPY_DST | wgpu_types::BufferUses::VERTEX,
                memory_flags: MemoryFlags::empty(),
            })
            .expect("create_buffer(vertex)")
    };
    let staging_buffer = unsafe {
        device
            .create_buffer(&BufferDescriptor {
                label: Some("horizon_staging_buffer"),
                size: STAGING_BUFFER_BYTES,
                usage: wgpu_types::BufferUses::MAP_WRITE | wgpu_types::BufferUses::COPY_SRC,
                memory_flags: MemoryFlags::empty(),
            })
            .expect("create_buffer(staging)")
    };

    let bind_group = unsafe {
        device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("horizon_terrain_bg"),
                layout: &bind_group_layout,
                buffers: &[BufferBinding::new_unchecked(
                    &camera_buffer,
                    0,
                    NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64),
                )],
                samplers: &[&sampler],
                textures: &[TextureBinding {
                    view: &atlas_view,
                    usage: wgpu_types::TextureUses::RESOURCE,
                }],
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource_index: 0,
                        count: 1,
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource_index: 0,
                        count: 1,
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource_index: 0,
                        count: 1,
                    },
                ],
                acceleration_structures: &[],
                external_textures: &[],
            })
            .expect("create_bind_group")
    };

    let mut encoder = unsafe {
        device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("horizon_cmd_encoder"),
                queue: &queue,
            })
            .expect("create_command_encoder")
    };
    let mut fence = unsafe { device.create_fence().expect("create_fence") };
    let mut fence_value: u64 = 1;

    // Upload all 14 texture layers + initial vertex mesh via wgpu-hal CommandEncoder
    let mut init_upload_bytes = Vec::new();
    let mut tex_copies = Vec::with_capacity(assets.texture_layers.len());
    for (layer_idx, layer_bytes) in assets.texture_layers.iter().enumerate() {
        let offset = init_upload_bytes.len() as u64;
        init_upload_bytes.extend_from_slice(layer_bytes);
        tex_copies.push(BufferTextureCopy {
            buffer_layout: wgpu_types::TexelCopyBufferLayout {
                offset,
                bytes_per_row: Some(16 * 4),
                rows_per_image: Some(16),
            },
            texture_base: TextureCopyBase {
                mip_level: 0,
                array_layer: layer_idx as u32,
                origin: wgpu_types::Origin3d::ZERO,
                aspect: FormatAspects::COLOR,
            },
            size: CopyExtent {
                width: 16,
                height: 16,
                depth: 1,
            },
        });
    }
    let vbo_src_offset = init_upload_bytes.len() as u64;
    let initial_vbo_bytes = bytemuck::cast_slice(&initial_vertices);
    init_upload_bytes.extend_from_slice(initial_vbo_bytes);

    if let Err(e) = unsafe { write_staging_bytes(&device, &staging_buffer, &init_upload_bytes) } {
        log_line(&format!("FAIL Initial staging buffer write failed: {e}"));
        return 15;
    }

    unsafe {
        encoder
            .begin_encoding(Some("horizon_init_upload"))
            .expect("begin_encoding(init)");
        encoder.copy_buffer_to_texture(&staging_buffer, &atlas_texture, tex_copies.into_iter());
        encoder.copy_buffer_to_buffer(
            &staging_buffer,
            &vertex_buffer,
            std::iter::once(BufferCopy {
                src_offset: vbo_src_offset,
                dst_offset: 0,
                size: NonZeroU64::new(initial_vbo_bytes.len() as u64).unwrap(),
            }),
        );
        let cmd_buf = encoder.end_encoding().expect("end_encoding(init)");
        queue
            .submit(&[&cmd_buf], &[], (&mut fence, fence_value))
            .expect("queue.submit(init)");
        fence_value += 1;
        encoder.reset_all(std::iter::once(cmd_buf));
    }

    let mut vertex_count = initial_vertices.len() as u32;
    log_line(&format!(
        "PASS wgpu-hal RenderPipeline, D2Array Texture ({} layers), Depth24Plus, Buffers, and BindGroup ready",
        layer_count
    ));

    // 6. Spawn background 20 TPS "Server thread"
    let running = Arc::new(AtomicBool::new(true));
    let server_ticks = Arc::new(AtomicU64::new(0));
    let running_srv = Arc::clone(&running);
    let ticks_srv = Arc::clone(&server_ticks);
    let scene_srv = Arc::clone(&scene);

    let server_handle = match thread::Builder::new()
        .name("Server thread".into())
        .spawn(move || {
            while running_srv.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
                let t = ticks_srv.fetch_add(1, Ordering::Relaxed) + 1;
                if t % 20 == 0 {
                    if let Ok(mut guard) = scene_srv.lock() {
                        let pulse_blk = if (t / 20) % 2 == 0 {
                            VoxelBlock::SeaLantern
                        } else {
                            VoxelBlock::GoldBlock
                        };
                        guard.set(4, 9, 5, pulse_blk);
                    }
                }
            }
        }) {
        Ok(h) => h,
        Err(e) => {
            log_line(&format!("FAIL Failed to spawn Server thread: {e}"));
            return 16;
        }
    };
    log_line("PASS Integrated 'Server thread' spawned (20 TPS tick loop active)");
    log_line("INFO Controls: Left Stick/D-Pad=Move | Right Stick=Look | A=Place | B=Break | X=Cycle Block | Y=Reset | PLUS=Exit");

    // 7. Interactive Camera & Real `wgpu-hal` Frame Loop
    let mut cam_pos = Vec3::new(8.0, 12.5, 22.5);
    let mut cam_yaw: f32 = -std::f32::consts::FRAC_PI_2;
    let mut cam_pitch: f32 = -0.32;
    let placeable = [
        VoxelBlock::GoldBlock,
        VoxelBlock::SeaLantern,
        VoxelBlock::OakLog,
        VoxelBlock::OakLeaves,
        VoxelBlock::GrassBlock,
        VoxelBlock::Stone,
    ];
    let mut selected_idx: usize = 0;
    let mut frames_rendered: u64 = 0;
    let mut last_frame = Instant::now();

    loop {
        let mut k_down: u64 = 0;
        let mut k_held: u64 = 0;
        let mut lx: i32 = 0;
        let mut ly: i32 = 0;
        let mut rx: i32 = 0;
        let mut ry: i32 = 0;

        let keep_running = unsafe {
            horizon_switch_poll(&mut k_down, &mut k_held, &mut lx, &mut ly, &mut rx, &mut ry)
        };
        if keep_running == 0 {
            break;
        }

        let now = Instant::now();
        let dt = (now - last_frame).as_secs_f32().clamp(0.001, 0.1);
        last_frame = now;

        // Analog stick deadzone & normalization
        let norm_stick = |v: i32| -> f32 {
            let f = (v as f32) / 32767.0;
            if f.abs() < 0.14 { 0.0 } else { f.clamp(-1.0, 1.0) }
        };
        let move_x = norm_stick(lx)
            + if (k_held & BTN_RIGHT) != 0 { 1.0 } else { 0.0 }
            - if (k_held & BTN_LEFT) != 0 { 1.0 } else { 0.0 };
        let move_z = norm_stick(ly)
            + if (k_held & BTN_UP) != 0 { 1.0 } else { 0.0 }
            - if (k_held & BTN_DOWN) != 0 { 1.0 } else { 0.0 };
        let look_x = norm_stick(rx);
        let look_y = norm_stick(ry);

        cam_yaw += look_x * 2.2 * dt;
        cam_pitch = (cam_pitch + look_y * 1.8 * dt).clamp(-1.45, 1.45);

        if move_x == 0.0 && move_z == 0.0 && look_x == 0.0 && look_y == 0.0 {
            cam_yaw += 0.12 * dt;
        }

        let forward = Vec3::new(
            cam_yaw.cos() * cam_pitch.cos(),
            cam_pitch.sin(),
            cam_yaw.sin() * cam_pitch.cos(),
        )
        .normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or_zero();

        let speed = 7.5 * dt;
        cam_pos += forward * (move_z * speed) + right * (move_x * speed);
        if (k_held & BTN_R) != 0 {
            cam_pos.y += speed;
        }
        if (k_held & BTN_L) != 0 {
            cam_pos.y -= speed;
        }
        cam_pos.y = cam_pos.y.clamp(2.0, 28.0);

        if (k_down & BTN_Y) != 0 {
            cam_pos = Vec3::new(8.0, 12.5, 22.5);
            cam_yaw = -std::f32::consts::FRAC_PI_2;
            cam_pitch = -0.32;
            log_line("EVENT Camera reset (Y button)");
        }
        if (k_down & BTN_X) != 0 {
            selected_idx = (selected_idx + 1) % placeable.len();
            log_line(&format!(
                "EVENT Selected block: {:?}",
                placeable[selected_idx]
            ));
        }

        // Raycast interaction (A = place block, B = break block)
        if (k_down & (BTN_A | BTN_B)) != 0 {
            if let Ok(mut guard) = scene.lock() {
                let mut prev_pos = None;
                for step in 1..=36 {
                    let p = cam_pos + forward * (step as f32 * 0.25);
                    let bx = p.x.floor() as i32;
                    let by = p.y.floor() as i32;
                    let bz = p.z.floor() as i32;
                    if guard.get(bx, by, bz) != VoxelBlock::Air {
                        if (k_down & BTN_B) != 0 && by > 0 {
                            guard.set(bx, by, bz, VoxelBlock::Air);
                            log_line(&format!("EVENT Broke block at ({bx}, {by}, {bz})"));
                        } else if (k_down & BTN_A) != 0 {
                            if let Some((px, py, pz)) = prev_pos {
                                let chosen = placeable[selected_idx];
                                guard.set(px, py, pz, chosen);
                                log_line(&format!(
                                    "EVENT Placed {:?} at ({px}, {py}, {pz})",
                                    chosen
                                ));
                            }
                        }
                        break;
                    }
                    prev_pos = Some((bx, by, bz));
                }
            }
        }

        // Check if chunk mesh needs rebuilding
        let new_mesh = {
            if let Ok(mut guard) = scene.lock() {
                if guard.mesh_dirty {
                    Some(guard.build_mesh(&assets))
                } else {
                    None
                }
            } else {
                None
            }
        };

        let tick = server_ticks.load(Ordering::Relaxed);
        let day_angle = (tick as f32) * 0.015;
        let sun_y = (day_angle.cos() * 0.45 + 0.65).clamp(0.2, 1.0);
        let sun_dir = Vec3::new(0.45, sun_y, 0.55).normalize();
        let sky_r = 0.36 * sun_y + 0.08;
        let sky_g = 0.58 * sun_y + 0.12;
        let sky_b = 0.88 * sun_y + 0.18;

        // WebGPU/Vulkan [0, 1] depth perspective matrix (`perspective_rh`),
        // which `wgpu-hal` converts via `ADJUST_COORDINATE_SPACE`.
        let proj = Mat4::perspective_rh(65.0_f32.to_radians(), 1280.0 / 720.0, 0.1, 120.0);
        let view = Mat4::look_to_rh(cam_pos, forward, Vec3::Y);
        let vp = proj * view;

        let cam_ubo = CameraUniform {
            view_proj: vp.to_cols_array_2d(),
            sky_color: [sky_r, sky_g, sky_b, 1.0],
            sun_dir: [sun_dir.x, sun_dir.y, sun_dir.z, 0.0],
            camera_pos: [cam_pos.x, cam_pos.y, cam_pos.z, 1.0],
        };

        // Pack per-frame staging bytes: CameraUniform + any animated texture frames + optional rebuilt mesh
        let mut frame_staging = Vec::with_capacity(4096);
        frame_staging.extend_from_slice(bytemuck::bytes_of(&cam_ubo));

        let mut anim_copies = Vec::new();
        for anim in &mut assets.animations {
            let next_frame = anim.clip.sample(tick).frame as usize % anim.frames.len();
            if next_frame != anim.current_frame {
                anim.current_frame = next_frame;
                let offset = frame_staging.len() as u64;
                frame_staging.extend_from_slice(&anim.frames[next_frame]);
                anim_copies.push(BufferTextureCopy {
                    buffer_layout: wgpu_types::TexelCopyBufferLayout {
                        offset,
                        bytes_per_row: Some(16 * 4),
                        rows_per_image: Some(16),
                    },
                    texture_base: TextureCopyBase {
                        mip_level: 0,
                        array_layer: anim.layer_index,
                        origin: wgpu_types::Origin3d::ZERO,
                        aspect: FormatAspects::COLOR,
                    },
                    size: CopyExtent {
                        width: 16,
                        height: 16,
                        depth: 1,
                    },
                });
            }
        }

        let mut mesh_copy = None;
        if let Some(ref verts) = new_mesh {
            let v_bytes = bytemuck::cast_slice(verts);
            if !v_bytes.is_empty()
                && (frame_staging.len() + v_bytes.len()) as u64 <= STAGING_BUFFER_BYTES
            {
                let offset = frame_staging.len() as u64;
                frame_staging.extend_from_slice(v_bytes);
                mesh_copy = Some(BufferCopy {
                    src_offset: offset,
                    dst_offset: 0,
                    size: NonZeroU64::new(v_bytes.len() as u64).unwrap(),
                });
                vertex_count = verts.len() as u32;
            }
        }

        unsafe {
            let _ = write_staging_bytes(&device, &staging_buffer, &frame_staging);

            let acquired = match surface.acquire_texture(None, &fence) {
                Ok(a) => a,
                Err(e) => {
                    log_line(&format!("FAIL surface.acquire_texture failed: {e:?}"));
                    break;
                }
            };
            let frame_view = device
                .create_texture_view(
                    &acquired.texture,
                    &TextureViewDescriptor {
                        label: Some("horizon_swapchain_view"),
                        format: surface_format,
                        dimension: wgpu_types::TextureViewDimension::D2,
                        usage: wgpu_types::TextureUses::COLOR_TARGET,
                        range: wgpu_types::ImageSubresourceRange::default(),
                    },
                )
                .expect("create_texture_view(swapchain)");

            encoder
                .begin_encoding(Some("horizon_frame"))
                .expect("begin_encoding(frame)");

            // Upload CameraUniform
            encoder.copy_buffer_to_buffer(
                &staging_buffer,
                &camera_buffer,
                std::iter::once(BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64).unwrap(),
                }),
            );
            // Upload animated sprite frames (`water_still` & `sea_lantern`) if changed
            if !anim_copies.is_empty() {
                encoder.copy_buffer_to_texture(
                    &staging_buffer,
                    &atlas_texture,
                    anim_copies.into_iter(),
                );
            }
            // Upload updated chunk vertex buffer if rebuilt
            if let Some(copy) = mesh_copy {
                encoder.copy_buffer_to_buffer(&staging_buffer, &vertex_buffer, std::iter::once(copy));
            }

            // Record Main RenderPass
            encoder
                .begin_render_pass(&RenderPassDescriptor::<
                    <GlesApi as HalApi>::QuerySet,
                    <GlesApi as HalApi>::TextureView,
                > {
                    label: Some("horizon_main_pass"),
                    extent: wgpu_types::Extent3d {
                        width: SCREEN_WIDTH,
                        height: SCREEN_HEIGHT,
                        depth_or_array_layers: 1,
                    },
                    sample_count: 1,
                    color_attachments: &[Some(ColorAttachment {
                        target: Attachment {
                            view: &frame_view,
                            usage: wgpu_types::TextureUses::COLOR_TARGET,
                        },
                        depth_slice: None,
                        resolve_target: None,
                        ops: AttachmentOps::LOAD_CLEAR | AttachmentOps::STORE,
                        clear_value: wgpu_types::Color {
                            r: sky_r as f64,
                            g: sky_g as f64,
                            b: sky_b as f64,
                            a: 1.0,
                        },
                    })],
                    depth_stencil_attachment: Some(DepthStencilAttachment {
                        target: Attachment {
                            view: &depth_view,
                            usage: wgpu_types::TextureUses::DEPTH_STENCIL_WRITE,
                        },
                        depth_ops: AttachmentOps::LOAD_CLEAR | AttachmentOps::STORE,
                        stencil_ops: AttachmentOps::empty(),
                        clear_value: (1.0, 0),
                    }),
                    multiview_mask: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .expect("begin_render_pass");

            encoder.set_render_pipeline(&render_pipeline);
            encoder.set_bind_group(&pipeline_layout, 0, &bind_group, &[]);
            encoder.set_vertex_buffer(
                0,
                BufferBinding::new_unchecked(&vertex_buffer, 0, None),
            );
            encoder.draw(0, vertex_count, 0, 1);
            encoder.end_render_pass();

            let cmd_buf = encoder.end_encoding().expect("end_encoding(frame)");
            queue
                .submit(&[&cmd_buf], &[&acquired.texture], (&mut fence, fence_value))
                .expect("queue.submit(frame)");
            fence_value += 1;

            if let Err(e) = queue.present(&surface, acquired.texture) {
                log_line(&format!("FAIL queue.present failed: {e:?}"));
                device.destroy_texture_view(frame_view);
                encoder.reset_all(std::iter::once(cmd_buf));
                break;
            }

            device.destroy_texture_view(frame_view);
            encoder.reset_all(std::iter::once(cmd_buf));
        }

        frames_rendered += 1;
        if frames_rendered == 1 {
            log_line("PASS First 3D frame rendered and presented via wgpu-hal Queue::submit + Queue::present");
        }
    }

    // 8. Clean shutdown: stop Server thread and destroy all wgpu-hal GPU objects
    running.store(false, Ordering::Relaxed);
    let _ = server_handle.join();
    let final_ticks = server_ticks.load(Ordering::Relaxed);
    let (mesh_gens, palette_len, chunk_coords) = scene
        .lock()
        .map(|g| {
            (
                g.mesh_generations,
                g.container.palette_len(),
                (g.chunk_pos.x, g.chunk_pos.z),
            )
        })
        .unwrap_or((0, 0, (0, 0)));

    drop(encoder);
    unsafe {
        let _ = queue.wait_for_idle();
        surface.unconfigure(&device);
        device.destroy_fence(fence);
        device.destroy_bind_group(bind_group);
        device.destroy_buffer(staging_buffer);
        device.destroy_buffer(vertex_buffer);
        device.destroy_buffer(camera_buffer);
        device.destroy_texture_view(depth_view);
        device.destroy_texture(depth_texture);
        device.destroy_sampler(sampler);
        device.destroy_texture_view(atlas_view);
        device.destroy_texture(atlas_texture);
        device.destroy_render_pipeline(render_pipeline);
        device.destroy_pipeline_layout(pipeline_layout);
        device.destroy_bind_group_layout(bind_group_layout);
        device.destroy_shader_module(shader_module);
    }

    log_line(&format!(
        "PASS Clean shutdown: frames_rendered={} server_ticks={} mesh_generations={} palette_entries={} chunk={:?} wgpu_hal_fences={}",
        frames_rendered,
        final_ticks,
        mesh_gens,
        palette_len,
        chunk_coords,
        fence_value - 1
    ));
    log_line("RESULT=0");
    0
}
