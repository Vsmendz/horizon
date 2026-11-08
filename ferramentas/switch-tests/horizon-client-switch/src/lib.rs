//! Etapa 5 (`test5_horizon_client`): First-Person Playable Horizon Client on Nintendo Switch.
//!
//! Integrates the real Horizon client and world subsystems:
//! 1. `crates/horizon-client-render/src/camera.rs` (included directly without modification):
//!    `Camera`, `CameraEntity`, `CameraType`, `FovPlayer`, `field_of_view_modifier`,
//!    `AvatarBob`, `bob_matrix`, `hand_projection`, `view_matrix`.
//! 2. `horizon-world` player physics & collision resolution:
//!    `player_travel::travel`, `living_air::travel_in_air`, `living_motion::jump_from_ground`,
//!    `edge_backoff::back_off_from_edge`, and `horizon_core::geometry::resolve_step_movement`.
//! 3. `horizon-world::world_ray::clip_world` + `view_direction::view_vector` voxel raycasting.
//! 4. `horizon-world::player_inventory::PlayerInventory` 9-slot hotbar + `horizon-items::cooldowns::Cooldowns`.
//! 5. Multi-chunk (`2x2` chunks = `32x32x32`) world with `PalettedContainer`, `Heightmap`,
//!    scheduled server ticks (`20 TPS` `"Server thread"`), and `.mca` Anvil region save/load
//!    (`RegionFile`, `ChunkRecord`, `Compression::Zlib`, `level.dat` NBT).
//! 6. `wgpu-hal` GLES backend + `horizon-assets` baked 26.3 block models, first-person held item,
//!    targeted block highlight box, and 2D HUD (crosshair + 9-slot hotbar with block icons).

#![allow(clippy::too_many_arguments)]

mod block_palette;
#[cfg(feature = "vanilla-visuals")]
#[path = "../../../../crates/horizon-client-render/src/block_particle_geometry.rs"]
mod block_particle_geometry;
#[cfg(all(test, feature = "vanilla"))]
mod block_table_probe;
#[path = "../../../../crates/horizon-client-render/src/camera.rs"]
pub mod camera;
#[cfg(feature = "vanilla-visuals")]
mod effects;
#[cfg(feature = "vanilla-visuals")]
mod features;
#[cfg(feature = "vanilla-water")]
#[path = "../../../../crates/horizon-client-render/src/mesher/fluid_geometry.rs"]
mod fluid_geometry;
#[cfg(feature = "vanilla")]
mod mesh_upload;
#[cfg(feature = "vanilla-visuals")]
mod state_models;
#[cfg(feature = "vanilla-visuals")]
mod visuals;
#[cfg(feature = "vanilla-water")]
mod water;

#[cfg(feature = "first-person-polish")]
#[allow(dead_code)]
#[path = "../../../../crates/horizon-client/src/first_person_shared.rs"]
mod first_person_shared;
#[cfg(all(test, target_os = "linux"))]
mod pixel_test;
#[cfg(feature = "first-person-polish")]
mod polish;
#[allow(dead_code)]
#[path = "../../../../crates/horizon-client/src/ticket_levels.rs"]
mod ticket_levels;
#[cfg(feature = "vanilla")]
mod vanilla;
mod world_stream;
pub use block_palette::VoxelBlock;
use horizon_core::fast_hash;
use world_stream::{
    HEIGHT, HorizonClientWorld, Key, MESH_HEIGHT, MIN_Y, StreamChange, StreamWorker, VIEW_DISTANCE,
    player_chunk,
};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3 as GlamVec3, Vec4};

use horizon_assets::animation::{AnimationClip, AnimationMetadata};
use horizon_assets::bake::{BlockModelTransform, ModelQuad, bake_block_model, bake_flat_face};
use horizon_assets::blockstate::{BlockStateDefinition, Variant, VariantSet};
use horizon_assets::model::{CuboidModel, Direction as AssetDirection, Material};
use horizon_assets::resolve::ModelLibrary;
use horizon_client_core::text::Text;
#[cfg(not(feature = "first-person-polish"))]
use horizon_core::geometry::trig;
use horizon_core::{
    geometry::{Aabb, BlockHit, Direction, Vec3, VoxelShape, try_resolve_step_movement},
    position::BlockPos,
    version::{DATA_VERSION, PROTOCOL_VERSION, VANILLA_VERSION},
};
use horizon_items::{cooldowns::Cooldowns, items::ItemStack};
use horizon_world::{
    component_patch::Prototype,
    edge_backoff::{EdgeBackoffInput, EdgeBackoffWorld, back_off_from_edge},
    item_stack_refs::ItemStacks,
    living_air::{LivingAirWorld, travel_in_air},
    living_motion::{LivingMotionWorld, jump_from_ground},
    player_inventory::{EntityEquipment, PlayerInventory},
    player_travel::{
        PlayerTravelWorld, flying_speed as player_flying_speed, travel as player_travel,
    },
    view_direction::view_vector,
    world_ray::{RayBlockMode, RayFluidMode, RayWorld, clip_world},
};

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

#[cfg(target_os = "horizon")]
include!("../../runtime-audit/abi_assertions.rs");

unsafe extern "C" {
    fn probe_log(s: *const c_char);
    fn horizon_switch_default_window() -> *mut c_void;
    #[cfg(all(target_os = "horizon", feature = "render-diagnostics"))]
    fn nwindowGetDimensions(window: *mut c_void, width: *mut u32, height: *mut u32) -> u32;
    #[cfg(not(feature = "vanilla"))]
    fn horizon_switch_romfs_mounted() -> c_int;
    #[cfg(feature = "vanilla")]
    fn horizon_loading_begin();
    #[cfg(feature = "vanilla")]
    fn horizon_loading_poll() -> c_int;
    #[cfg(feature = "vanilla")]
    fn horizon_loading_end();
    #[cfg(feature = "vanilla")]
    fn horizon_memory_report();
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
// 1. RAW EGL 1.5 SESSION ON NINTENDO SWITCH DEFAULT NWINDOW
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
    #[cfg(feature = "render-diagnostics")]
    fn eglQuerySurface(
        dpy: EGLDisplay,
        surface: EGLSurface,
        attribute: EGLint,
        value: *mut EGLint,
    ) -> EGLBoolean;
}

#[derive(Clone, Copy)]
struct EglHandles {
    display: EGLDisplay,
    surface: EGLSurface,
    context: EGLContext,
}

#[cfg(all(target_os = "horizon", feature = "render-diagnostics"))]
fn diagnostic_window_size(handles: EglHandles) -> Option<[i32; 2]> {
    const EGL_WIDTH: EGLint = 0x3057;
    const EGL_HEIGHT: EGLint = 0x3056;
    let mut egl = [0, 0];
    let mut native = [0, 0];
    let (width, height, status) = unsafe {
        (
            eglQuerySurface(handles.display, handles.surface, EGL_WIDTH, &mut egl[0]),
            eglQuerySurface(handles.display, handles.surface, EGL_HEIGHT, &mut egl[1]),
            nwindowGetDimensions(
                horizon_switch_default_window(),
                &mut native[0],
                &mut native[1],
            ),
        )
    };
    let size = if width != EGL_FALSE && height != EGL_FALSE && egl.iter().all(|&v| v > 0) {
        Some(egl)
    } else {
        let native_size = native.map(|v| v as i32);
        (status == 0 && native_size.iter().all(|&v| v > 0)).then_some(native_size)
    };
    log_line(&format!(
        "GLES_WINDOW egl={egl:?} query_ok=({width},{height}) native={native:?} native_status={status:#x} chosen={size:?}"
    ));
    size
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
            let context = eglCreateContext(display, config, EGL_NO_CONTEXT, ctx_attribs.as_ptr());
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
// 2. REAL HORIZON GAMEPAD & INPUT PIPELINE (`horizon-client-input` semantics)
// ============================================================================
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl PadButton {
    const fn bit(self) -> u16 {
        1u16 << (self as u16)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    pub move_x: f32,
    pub move_y: f32,
    pub look_x: f32,
    pub look_y: f32,
    pub left_trigger: f32,
    pub right_trigger: f32,
    buttons: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadEdges {
    pressed: u16,
}

impl PadEdges {
    pub fn pressed(self, button: PadButton) -> bool {
        self.pressed & button.bit() != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PadSettings {
    pub enabled: bool,
    pub sensitivity: f32,
    pub move_deadzone: f32,
    pub look_deadzone: f32,
    pub invert_y: bool,
    pub nintendo_letters: bool,
    #[cfg(feature = "first-person-polish")]
    pub fov: i32,
}

impl Default for PadSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sensitivity: 1.0,
            move_deadzone: 0.14,
            look_deadzone: 0.12,
            invert_y: false,
            nintendo_letters: true,
            #[cfg(feature = "first-person-polish")]
            fov: 70,
        }
    }
}

impl PadSettings {
    pub fn load_or_save(path: &Path) -> Self {
        let mut settings = Self::default();
        if let Ok(text) = fs::read_to_string(path) {
            for line in text.lines() {
                let Some((k, v)) = line.split_once(':') else {
                    continue;
                };
                match k.trim() {
                    "enabled" => settings.enabled = v.trim() == "true",
                    "sensitivity" => {
                        if let Ok(val) = v.trim().parse::<f32>() {
                            settings.sensitivity = val.clamp(0.1, 3.0);
                        }
                    }
                    "move_deadzone" => {
                        if let Ok(val) = v.trim().parse::<f32>() {
                            settings.move_deadzone = val.clamp(0.05, 0.40);
                        }
                    }
                    "look_deadzone" => {
                        if let Ok(val) = v.trim().parse::<f32>() {
                            settings.look_deadzone = val.clamp(0.05, 0.40);
                        }
                    }
                    #[cfg(feature = "first-person-polish")]
                    "fov" => {
                        if let Ok(value) = v.trim().parse::<i32>() {
                            settings.fov = value.clamp(30, 110);
                        }
                    }
                    "invert_y" => settings.invert_y = v.trim() == "true",
                    "nintendo_letters" => settings.nintendo_letters = v.trim() == "true",
                    _ => {}
                }
            }
        } else {
            let content = format!(
                "enabled:{}\nsensitivity:{:.2}\nmove_deadzone:{:.2}\nlook_deadzone:{:.2}\ninvert_y:{}\nnintendo_letters:{}\n",
                settings.enabled,
                settings.sensitivity,
                settings.move_deadzone,
                settings.look_deadzone,
                settings.invert_y,
                settings.nintendo_letters
            );
            #[cfg(feature = "first-person-polish")]
            let content = format!("{content}fov:{}\n", settings.fov);
            let _ = fs::write(path, content);
        }
        settings
    }
}

fn radial_deadzone(x: f32, y: f32, deadzone: f32) -> (f32, f32) {
    let dz = deadzone.clamp(0.0, 0.95);
    let len = x.hypot(y);
    if len <= dz || len <= f32::EPSILON {
        return (0.0, 0.0);
    }
    let scaled = ((len - dz) / (1.0 - dz)).clamp(0.0, 1.0);
    (x / len * scaled, y / len * scaled)
}

impl PadState {
    pub fn from_switch_hid(
        keys_held: u64,
        lx: i32,
        ly: i32,
        rx: i32,
        ry: i32,
        settings: &PadSettings,
    ) -> Self {
        const KEY_A: u64 = 1 << 0;
        const KEY_B: u64 = 1 << 1;
        const KEY_X: u64 = 1 << 2;
        const KEY_Y: u64 = 1 << 3;
        const KEY_LSTICK: u64 = 1 << 4;
        const KEY_RSTICK: u64 = 1 << 5;
        const KEY_L: u64 = 1 << 6;
        const KEY_R: u64 = 1 << 7;
        const KEY_ZL: u64 = 1 << 8;
        const KEY_ZR: u64 = 1 << 9;
        const KEY_PLUS: u64 = 1 << 10;
        const KEY_MINUS: u64 = 1 << 11;
        const KEY_DLEFT: u64 = 1 << 12;
        const KEY_DUP: u64 = 1 << 13;
        const KEY_DRIGHT: u64 = 1 << 14;
        const KEY_DDOWN: u64 = 1 << 15;

        let mut state = Self::default();
        if !settings.enabled {
            return state;
        }

        let norm = |v: i32| (v as f32 / 32767.0).clamp(-1.0, 1.0);
        let (mx, my) = radial_deadzone(norm(lx), norm(ly), settings.move_deadzone);
        let (kx, ky) = radial_deadzone(norm(rx), norm(ry), settings.look_deadzone);
        state.move_x = mx;
        state.move_y = my;
        state.look_x = kx;
        state.look_y = ky;
        state.left_trigger = if keys_held & KEY_ZL != 0 { 1.0 } else { 0.0 };
        state.right_trigger = if keys_held & KEY_ZR != 0 { 1.0 } else { 0.0 };

        let mut set_btn = |btn: PadButton, cond: bool| {
            if cond {
                state.buttons |= btn.bit();
            }
        };
        set_btn(PadButton::South, keys_held & KEY_B != 0);
        set_btn(PadButton::East, keys_held & KEY_A != 0);
        set_btn(PadButton::West, keys_held & KEY_Y != 0);
        set_btn(PadButton::North, keys_held & KEY_X != 0);
        set_btn(PadButton::LeftBumper, keys_held & KEY_L != 0);
        set_btn(PadButton::RightBumper, keys_held & KEY_R != 0);
        set_btn(PadButton::LeftTrigger, keys_held & KEY_ZL != 0);
        set_btn(PadButton::RightTrigger, keys_held & KEY_ZR != 0);
        set_btn(PadButton::Select, keys_held & KEY_MINUS != 0);
        set_btn(PadButton::Start, keys_held & KEY_PLUS != 0);
        set_btn(PadButton::LeftThumb, keys_held & KEY_LSTICK != 0);
        set_btn(PadButton::RightThumb, keys_held & KEY_RSTICK != 0);
        set_btn(PadButton::DPadUp, keys_held & KEY_DUP != 0);
        set_btn(PadButton::DPadDown, keys_held & KEY_DDOWN != 0);
        set_btn(PadButton::DPadLeft, keys_held & KEY_DLEFT != 0);
        set_btn(PadButton::DPadRight, keys_held & KEY_DRIGHT != 0);

        state
    }

    pub fn down(&self, button: PadButton) -> bool {
        self.buttons & button.bit() != 0
    }

    pub fn edges(&self, previous: &Self) -> PadEdges {
        PadEdges {
            pressed: self.buttons & !previous.buttons,
        }
    }

    pub fn look_delta(&self, dt: f32, settings: &PadSettings) -> (f32, f32) {
        if !settings.enabled {
            return (0.0, 0.0);
        }
        let curve = |v: f32| v.signum() * (0.35 * v.abs() + 0.65 * v * v);
        let rate = 165.0 * settings.sensitivity.clamp(0.1, 3.0) * dt.clamp(0.0, 0.1);
        let sign_y = if settings.invert_y { 1.0 } else { -1.0 };
        (
            curve(self.look_x) * rate,
            curve(self.look_y) * rate * sign_y,
        )
    }

    pub fn hotbar_step(&self, previous: &Self) -> i32 {
        let edges = self.edges(previous);
        (edges.pressed(PadButton::RightBumper) as i32)
            - (edges.pressed(PadButton::LeftBumper) as i32)
    }
}

/// Real `LocalPlayer.modifyInput` from `horizon-client-input::input`.
pub fn modify_move_input(
    raw_forward: f32,
    raw_left: f32,
    sneaking: bool,
    sneaking_speed_factor: f32,
) -> (f32, f32) {
    let len = (raw_forward * raw_forward + raw_left * raw_left).sqrt();
    let (mut f, mut l) = if len <= 1.0e-5 {
        (0.0, 0.0)
    } else {
        let clamped = len.min(1.0);
        let scale = (clamped * clamped) / len;
        (raw_forward * scale, raw_left * scale)
    };
    if sneaking {
        f *= sneaking_speed_factor;
        l *= sneaking_speed_factor;
    }
    (f, l)
}

// ============================================================================
// 3. WGSL SHADER & GPU BUFFER TYPES (`wgpu-hal` + `naga`)
// ============================================================================
const HORIZON_WGSL_SHADER: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    hand_projection: mat4x4<f32>,
    sky_color: vec4<f32>,
    sun_dir: vec4<f32>,
    camera_pos: vec4<f32>,
    water_fog: vec4<f32>,
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

@vertex
fn vs_hand(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_pos = camera.hand_projection * vec4<f32>(in.pos, 1.0);
    out.world_pos = in.pos;
    out.normal = in.normal;
    out.uv_layer = in.uv_layer;
    out.color_ao = in.color_ao;
    return out;
}

@fragment
fn fs_hand(in: VertexOutput) -> @location(0) vec4<f32> {
    let layer_idx = i32(round(in.uv_layer.z));
    let tex = textureSample(terrain_tex, terrain_sampler, in.uv_layer.xy, layer_idx);
    if (tex.a < 0.5) {
        discard;
    }
    let key_light = normalize(vec3<f32>(0.35, 0.8, 0.55));
    let light = (0.65 + 0.35 * max(dot(normalize(in.normal), key_light), 0.0)) * in.color_ao.a;
    return vec4<f32>(tex.rgb * in.color_ao.rgb * light, 1.0);
}

fn scene_fog(tinted: vec3<f32>, dist: f32) -> vec3<f32> {
    let air = mix(tinted, camera.sky_color.rgb, clamp((dist - 28.0) / 44.0, 0.0, 0.72));
    let water = mix(tinted, camera.water_fog.rgb, clamp((dist + 8.0) / 32.0, 0.0, 1.0));
    return mix(air, water, camera.water_fog.a);
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

    let face_shade = in.color_ao.a;
    let light = select(clamp((0.45 + 0.55 * ndotl) * face_shade, 0.25, 1.20), face_shade, camera.sun_dir.w > 0.0);
    let tinted = tex.rgb * in.color_ao.rgb * light;

    let dist = distance(in.world_pos, camera.camera_pos.xyz);
    let final_rgb = scene_fog(tinted, dist);

    return vec4<f32>(final_rgb, 1.0);
}
"#;

#[cfg(feature = "first-person-polish")]
const POLISH_WGSL: &str = r#"
@fragment
fn fs_hand_polished(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(terrain_tex, terrain_sampler, in.uv_layer.xy, i32(round(in.uv_layer.z)));
    if (tex.a < 0.5) { discard; }
    let n = normalize(in.normal);
    let l0 = normalize(vec3<f32>(0.2, 1.0, -0.7));
    let l1 = normalize(vec3<f32>(-0.2, 1.0, 0.7));
    let light = min(1.0, (max(0.0, dot(l0, n)) + max(0.0, dot(l1, n))) * 0.6 + 0.4);
    let base = tex.rgb * in.color_ao.rgb * light;
    return vec4<f32>(mix(base, base * 0.75 + camera.water_fog.rgb * 0.25, camera.water_fog.a), 1.0);
}
"#;

#[cfg(feature = "vanilla-water")]
const WATER_WGSL: &str = r#"
@fragment
fn fs_water(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(terrain_tex, terrain_sampler, in.uv_layer.xy, i32(round(in.uv_layer.z)));
    if (tex.a < 0.01) {
        discard;
    }
    let tinted = tex.rgb * in.color_ao.rgb * in.color_ao.a;
    let dist = distance(in.world_pos, camera.camera_pos.xyz);
    return vec4<f32>(scene_fog(tinted, dist), tex.a);
}
"#;

#[cfg(feature = "vanilla-visuals")]
const VISUALS_WGSL: &str = r#"
@fragment
fn fs_effect(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(terrain_tex, terrain_sampler, in.uv_layer.xy, i32(round(in.uv_layer.z)));
    let rgb = scene_fog(tex.rgb * in.color_ao.rgb, distance(in.world_pos, camera.camera_pos.xyz));
    return vec4<f32>(rgb, tex.a * in.color_ao.a);
}
"#;

#[cfg(feature = "render-diagnostics")]
const DIAGNOSTIC_WGSL: &str = r#"
@fragment
fn fs_probe_solid(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color_ao.rgb, 1.0);
}

@fragment
fn fs_probe_sample(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(terrain_tex, terrain_sampler, in.uv_layer.xy, i32(round(in.uv_layer.z)));
}

@fragment
fn fs_probe_texel(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureLoad(terrain_tex, vec2<i32>(in.uv_layer.xy * vec2<f32>(textureDimensions(terrain_tex))), i32(round(in.uv_layer.z)), 0);
}

@fragment
fn fs_probe_fixed_texel() -> @location(0) vec4<f32> {
    return textureLoad(terrain_tex, vec2<i32>(0), 0, 0);
}
"#;

#[cfg(feature = "render-diagnostics")]
const PROBE_SKY: [f64; 3] = [32.0 / 255.0, 96.0 / 255.0, 224.0 / 255.0];
#[cfg(feature = "render-diagnostics")]
const PROBE_CONTROL_TEXEL: [u8; 4] = [92, 212, 56, 255];

#[cfg(feature = "render-diagnostics")]
fn probe_panels(vertices: &mut Vec<TerrainVertex>, layer: u32) {
    for (x, y, u, color) in [
        (-2.0 / 3.0, 1.0 / 3.0, 0.25, [0.25, 0.75, 0.25]), // Solid.
        (0.0, 1.0 / 3.0, 0.25, [1.0; 3]),                  // Sample without lighting.
        (2.0 / 3.0, 1.0 / 3.0, 0.25, [1.0; 3]),            // Production fragment.
        (-2.0 / 3.0, -1.0 / 3.0, 0.25, [0.75, 0.25, 0.75]), // Solid HUD.
        (0.0, -1.0 / 3.0, 0.75, [1.0; 3]),                 // Direct texel load.
    ] {
        let start = vertices.len();
        emit_ndc_rect(
            vertices,
            Mat4::IDENTITY,
            x - 0.19,
            y - 0.17,
            x + 0.19,
            y + 0.17,
            0.2,
            layer as f32,
            color,
        );
        for vertex in &mut vertices[start..] {
            vertex.uv_layer[0] = u;
            vertex.uv_layer[1] = 0.5;
            vertex.color_ao[3] = 0.5;
        }
    }
    // The center panel reads layer 0 without UV or layer vertex attributes.
    emit_ndc_rect(
        vertices,
        Mat4::IDENTITY,
        -0.19,
        -0.10,
        0.19,
        0.10,
        0.2,
        layer as f32,
        [1.0; 3],
    );
    // Sample the separate HAL control texture at the bottom-center readback point.
    emit_ndc_rect(
        vertices,
        Mat4::IDENTITY,
        -0.19,
        -0.97,
        0.19,
        -0.79,
        0.2,
        0.0,
        [1.0; 3],
    );
}

#[cfg(feature = "render-diagnostics")]
fn probe_texture(side: usize) -> Vec<u8> {
    (0..side * side)
        .flat_map(|i| {
            if i % side < side / 2 {
                [224, 64, 32, 255]
            } else {
                [32, 192, 224, 255]
            }
        })
        .collect()
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    hand_projection: [[f32; 4]; 4],
    sky_color: [f32; 4],
    sun_dir: [f32; 4],
    camera_pos: [f32; 4],
    water_fog: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct TerrainVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv_layer: [f32; 3],
    pub color_ao: [f32; 4],
}

// ============================================================================
// 4. VOXEL BLOCK PALETTE & REAL 26.3 ASSET BAKING (`horizon-assets`)
// ============================================================================
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

const ATLAS_SIDE: u32 = if cfg!(feature = "vanilla-water") {
    32
} else {
    16
};
const ATLAS_LAYER_BYTES: usize = (ATLAS_SIDE * ATLAS_SIDE * 4) as usize;

pub struct BakedAssetRegistry {
    source_label: String,
    texture_layers: Vec<Vec<u8>>,
    #[cfg(feature = "vanilla-water")]
    water_layers: [u32; 3],
    sprite_to_layer: BTreeMap<String, u32>,
    baked_quads: BTreeMap<VoxelBlock, Vec<ModelQuad>>,
    #[cfg(feature = "first-person-polish")]
    hand_models: BTreeMap<VoxelBlock, Mat4>,
    #[cfg(feature = "vanilla-visuals")]
    bucket_layer: u32,
    #[cfg(feature = "vanilla-visuals")]
    state_quads: BTreeMap<u32, Vec<ModelQuad>>,
    #[cfg(feature = "vanilla-visuals")]
    bubble: u32,
    #[cfg(feature = "vanilla-visuals")]
    splash: [u32; 4],
    #[cfg(feature = "vanilla-visuals")]
    cracks: [u32; 10],
    animations: Vec<AnimatedLayer>,
    white_layer: u32,
}

#[cfg(not(feature = "vanilla"))]
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

    if needs_sync && unsafe { horizon_switch_romfs_mounted() } != 0 {
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
            fs::write(&dst, &data).map_err(|e| format!("Failed writing {}: {e}", dst.display()))?;
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

    if sd_manifest.is_file() {
        Ok("sdmc:/switch/horizon".to_string())
    } else {
        Err("sdmc:/switch/horizon/assets/manifest.json not found".into())
    }
}

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

fn select_variant<'a>(
    def: &'a BlockStateDefinition,
    preferred_key: &str,
) -> Result<&'a Variant, String> {
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
    #[cfg(feature = "vanilla")]
    let root_str = "romfs:/".to_string();
    #[cfg(not(feature = "vanilla"))]
    let root_str = ensure_sdcard_assets()?;
    bake_assets(root_str)
}

fn bake_assets(root_str: String) -> Result<BakedAssetRegistry, String> {
    let root = Path::new(&root_str);
    let manifest_text = fs::read_to_string(root.join("assets/manifest.json"))
        .map_err(|e| format!("read manifest.json: {e}"))?;
    let manifest: SwitchAssetManifest =
        serde_json::from_str(&manifest_text).map_err(|e| format!("parse manifest.json: {e}"))?;

    let mut texture_layers = Vec::with_capacity(manifest.textures.len() + 1);
    let mut sprite_to_layer = BTreeMap::new();
    let mut animations = Vec::new();

    for (idx, tex_rel) in manifest.textures.iter().enumerate() {
        let layer_idx = idx as u32;
        let png_path = root.join(format!("assets/minecraft/textures/{tex_rel}.png"));
        let png_bytes =
            fs::read(&png_path).map_err(|e| format!("read {}: {e}", png_path.display()))?;
        let (side, mut frames) = decode_png_frames(&png_bytes)?;
        if side == 0 || !ATLAS_SIDE.is_multiple_of(side) {
            return Err(format!(
                "Unsupported texture dimensions for {tex_rel}: {side}x{side}"
            ));
        }

        if side != ATLAS_SIDE {
            for frame in &mut frames {
                *frame = (0..ATLAS_SIDE * ATLAS_SIDE)
                    .flat_map(|pixel| {
                        let x = (pixel % ATLAS_SIDE) * side / ATLAS_SIDE;
                        let y = (pixel / ATLAS_SIDE) * side / ATLAS_SIDE;
                        let i = ((y * side + x) * 4) as usize;
                        <[u8; 4]>::try_from(&frame[i..i + 4]).unwrap()
                    })
                    .collect();
            }
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

    // Add pure white 16x16 layer for HUD crosshair, hotbar frame, and block target highlight
    let white_layer = texture_layers.len() as u32;
    texture_layers.push(vec![255u8; ATLAS_LAYER_BYTES]);
    sprite_to_layer.insert("horizon:ui/white".to_string(), white_layer);

    let mut models_map = BTreeMap::new();
    for model_rel in &manifest.models {
        let model_path = root.join(format!("assets/minecraft/models/{model_rel}.json"));
        let model_text = fs::read_to_string(&model_path)
            .map_err(|e| format!("read {}: {e}", model_path.display()))?;
        let cuboid = CuboidModel::from_json(&model_text)
            .map_err(|e| format!("CuboidModel::from_json({model_rel}): {e}"))?;
        models_map.insert(format!("minecraft:{model_rel}"), cuboid);
    }
    let model_lib = ModelLibrary::new(models_map).map_err(|e| format!("ModelLibrary::new: {e}"))?;

    #[cfg(feature = "vanilla-visuals")]
    let (state_quads, defaults) = state_models::bake(root, &model_lib)?;
    let mut baked_quads = BTreeMap::new();
    #[cfg(feature = "first-person-polish")]
    let mut hand_models = BTreeMap::new();
    let blocks = VoxelBlock::ALL_NON_AIR.into_iter();
    #[cfg(feature = "vanilla")]
    let blocks = blocks.chain(VoxelBlock::TERRAIN_MODELS);
    #[cfg(feature = "vanilla-visuals")]
    let blocks = blocks.chain(VoxelBlock::VISUAL_MODELS);
    for block in blocks {
        let name = block.block_name();
        #[cfg(feature = "vanilla-visuals")]
        if name.contains("mushroom_block") || name == "mushroom_stem" {
            baked_quads.insert(block, state_quads[&defaults[name]].clone());
            hand_models.insert(block, Mat4::IDENTITY);
            continue;
        }
        let bs_path = root.join(format!("assets/minecraft/blockstates/{name}.json"));
        let bs_text =
            fs::read_to_string(&bs_path).map_err(|e| format!("read {}: {e}", bs_path.display()))?;
        let def = BlockStateDefinition::from_json(&bs_text)
            .map_err(|e| format!("BlockStateDefinition({name}): {e}"))?;

        let preferred_key = match block {
            VoxelBlock::GrassBlock => "snowy=false",
            VoxelBlock::OakLog => "axis=y",
            #[cfg(feature = "vanilla-visuals")]
            _ if name.ends_with("_log") => "axis=y",
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

        let liquid = !block.is_solid();
        if quads.is_empty() && liquid {
            let mat = resolved
                .material("particle")
                .ok()
                .flatten()
                .cloned()
                .unwrap_or(Material {
                    sprite: format!("minecraft:block/{}_still", block.block_name()),
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
        #[cfg(feature = "first-person-polish")]
        {
            use horizon_assets::model::DisplayTransform;
            let display = match &resolved.display.contexts[3] {
                DisplayTransform::Explicit(transform) => transform.as_ref(),
                DisplayTransform::Inherit => None,
            };
            let matrix = horizon_assets::item_bake::item_transform_matrix(display, false);
            hand_models.insert(block, Mat4::from_cols_array_2d(&matrix.0));
        }
        baked_quads.insert(block, quads);
    }

    #[cfg(feature = "vanilla-water")]
    let water_layers = ["water_still", "water_flow", "water_overlay"].map(|name| {
        sprite_to_layer
            .get(&format!("minecraft:block/{name}"))
            .copied()
            .ok_or_else(|| format!("missing fluid sprite {name}"))
    });
    #[cfg(feature = "vanilla-water")]
    let [still, flow, overlay] = water_layers;
    #[cfg(feature = "vanilla-visuals")]
    let (bucket_layer, bubble, splash, cracks) = {
        let (bucket, hand, bucket_layer) =
            visuals::bake_bucket(&model_lib, &sprite_to_layer, &texture_layers)?;
        baked_quads.insert(VoxelBlock::Water, bucket);
        hand_models.insert(VoxelBlock::Water, hand);
        let layer = |name: &str| {
            sprite_to_layer
                .get(name)
                .copied()
                .ok_or_else(|| format!("missing visual sprite {name}"))
        };
        let bubble = layer("minecraft:particle/bubble")?;
        let mut splash = [0; 4];
        for (i, dest) in splash.iter_mut().enumerate() {
            *dest = layer(&format!("minecraft:particle/splash_{i}"))?;
        }
        let mut cracks = [0; 10];
        for (i, dest) in cracks.iter_mut().enumerate() {
            *dest = layer(&format!("minecraft:block/destroy_stage_{i}"))?;
        }
        (bucket_layer, bubble, splash, cracks)
    };
    #[cfg(feature = "vanilla-visuals")]
    for (block, quads) in &baked_quads {
        for quad in quads {
            if !sprite_to_layer.contains_key(&quad.material.sprite) {
                return Err(format!(
                    "{block:?}: missing atlas sprite {}",
                    quad.material.sprite
                ));
            }
        }
    }
    Ok(BakedAssetRegistry {
        source_label: format!(
            "{} (26.3 jar sha1={}..)",
            root_str,
            &manifest.client_jar_sha1[..12]
        ),
        texture_layers,
        #[cfg(feature = "vanilla-water")]
        water_layers: [still?, flow?, overlay?],
        sprite_to_layer,
        baked_quads,
        #[cfg(feature = "first-person-polish")]
        hand_models,
        #[cfg(feature = "vanilla-visuals")]
        bucket_layer,
        #[cfg(feature = "vanilla-visuals")]
        state_quads,
        #[cfg(feature = "vanilla-visuals")]
        bubble,
        #[cfg(feature = "vanilla-visuals")]
        splash,
        #[cfg(feature = "vanilla-visuals")]
        cracks,
        animations,
        white_layer,
    })
}

#[cfg(all(test, feature = "vanilla"))]
#[test]
fn vanilla_romfs_bakes_models_and_meshes_the_spawn() -> Result<(), Box<dyn std::error::Error>> {
    let stage = if cfg!(feature = "vanilla-visuals") {
        "../out/test10-romfs"
    } else if cfg!(feature = "vanilla-water") {
        "../out/test8f-romfs"
    } else {
        "../out/test8-romfs"
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(stage);
    let assets = bake_assets(root.display().to_string())?;
    #[cfg(feature = "vanilla-water")]
    {
        let [still, flow, _] = assets.water_layers;
        assert!(
            assets.texture_layers[still as usize]
                .chunks_exact(4)
                .any(|p| (1..255).contains(&p[3]))
        );
        let animation = assets
            .animations
            .iter()
            .find(|a| a.layer_index == flow)
            .unwrap();
        assert!(animation.frames.windows(2).any(|pair| pair[0] != pair[1]));
        assert!(
            animation
                .frames
                .iter()
                .all(|frame| frame.len() == 32 * 32 * 4)
        );
    }
    assert!(assets.baked_quads.contains_key(&VoxelBlock::Deepslate));
    for (block, quads) in &assets.baked_quads {
        for quad in quads {
            assert!(
                assets.sprite_to_layer.contains_key(&quad.material.sprite),
                "{block:?}: missing {}",
                quad.material.sprite
            );
        }
    }
    let data = vanilla::Vanilla::load(&root, |line| println!("{line}"))?;
    let [x, z] = data.spawn;
    let center = (x >> 4, z >> 4);
    let scratch =
        std::env::temp_dir().join(format!("horizon-switch-mesh-smoke-{}", std::process::id()));
    let mut world = HorizonClientWorld::new(scratch, data);
    let started = Instant::now();
    let generated = world.stream_to(center)?.unwrap();
    let spawn = world.spawn_position();
    assert_eq!(world.chunks.len(), 9);
    assert!(
        world
            .get_block(BlockPos::new(
                spawn.x.floor() as i32,
                spawn.y.floor() as i32 - 1,
                spawn.z.floor() as i32
            ))
            .is_solid()
    );
    let mut meshes = BTreeMap::new();
    let mesh_start = Instant::now();
    let rebuilt = world.refresh_meshes(&assets, &mut meshes, center, spawn.y, usize::MAX);
    let vertices: usize = meshes.values().map(|mesh| mesh.vertices.len()).sum();
    assert!(vertices > 0 && vertices % 3 == 0);
    println!(
        "SPAWN_MESH spawn={spawn:?} generated_us={:?} generation_and_mesh_ms={} mesh_ms={} sections={} nonempty={} vertices={} bytes={}",
        generated.generated_us,
        started.elapsed().as_millis(),
        mesh_start.elapsed().as_millis(),
        rebuilt,
        meshes.len(),
        vertices,
        vertices * std::mem::size_of::<TerrainVertex>()
    );
    let vp = Mat4::perspective_rh(1.1, 1.7, 0.05, 120.0);
    assert!(section_visible(vp, ((0, -1), (-MIN_Y) / MESH_HEIGHT)));
    assert!(!section_visible(vp, ((0, 2), (-MIN_Y) / MESH_HEIGHT)));
    #[cfg(feature = "first-person-polish")]
    {
        let model = first_person_shared::item_anchor(1.0, 0.0)
            * assets.hand_models[&VoxelBlock::GrassBlock];
        let hand_proj = Mat4::from_cols_array(&camera::hand_projection(70.0, 1280.0, 720.0, 120.0));
        let vertices = assets.baked_quads[&VoxelBlock::GrassBlock]
            .iter()
            .flat_map(|quad| quad.positions)
            .map(|p| hand_proj.project_point3(model.transform_point3(GlamVec3::from_array(p))));
        // The original pose clips part of the block at the lower screen edge.
        let top = vertices
            .clone()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (-0.9..0.0).contains(&top)
                && vertices
                    .clone()
                    .any(|p| (0.0..1.0).contains(&p.x) && (-1.0..0.0).contains(&p.y)),
            "the held block must reach into the lower-right quadrant, top={top}"
        );

        let [sx, sz] = world.spawn_xz();
        let safe = polish::dry_spawn(&world, sx, sz).unwrap();
        assert!(polish::valid_restore(&world, safe));
        let foot = BlockPos::new(
            safe.x.floor() as i32,
            safe.y.floor() as i32,
            safe.z.floor() as i32,
        );
        assert!(world.set_block(foot, VoxelBlock::Stone));
        assert!(!polish::valid_restore(&world, safe));
        assert_ne!(polish::dry_spawn(&world, sx, sz), Some(safe));
        assert!(world.set_block(foot, VoxelBlock::Air));
        let registry = world.vanilla.generator.data().registry();
        let mut slab = registry
            .state(registry.default_state("minecraft:stone_slab").unwrap())
            .unwrap()
            .clone();
        slab.properties.insert("type".into(), "top".into());
        world
            .chunks
            .get_mut(&player_chunk(safe))
            .unwrap()
            .set(foot.offset(0, 1, 0), registry.id(&slab).unwrap())?;
        assert!(polish::intersects_blocks(
            &world,
            HorizonLocalPlayer::standing_bounds(safe)
        ));
        assert!(
            polish::valid_restore(&world, safe),
            "a valid crouched position is preserved"
        );
        let pool = foot.offset(0, 6, 0);
        for dy in 0..=2 {
            assert!(world.set_block(pool.offset(0, dy, 0), VoxelBlock::Water));
        }
        let pool_pos = Vec3::new(
            f64::from(pool.x) + 0.5,
            f64::from(pool.y),
            f64::from(pool.z) + 0.5,
        );
        let mut swimmer = HorizonLocalPlayer::new(pool_pos);
        assert!(
            polish::valid_restore(&world, pool_pos),
            "a valid submerged pose is preserved"
        );
        assert!(polish::eye_in_water(
            &world,
            [pool_pos.x, pool_pos.y + 1.62, pool_pos.z]
        ));
        swimmer.tick_physics(&world, 0.0, 0.0, true);
        assert!(swimmer.position.y > pool_pos.y && swimmer.velocity.y > 0.0);
    }
    Ok(())
}

// ============================================================================
// 5. MULTI-CHUNK WORLD (`2x2` CHUNKS = `32x32x32`), HEIGHTMAPS & `.MCA` SAVE/LOAD
// ============================================================================
impl HorizonClientWorld {
    fn mesh_section(
        &self,
        ((cx, cz), section): (Key, i32),
        assets: &BakedAssetRegistry,
    ) -> SectionMesh {
        let mut mesh = SectionMesh {
            vertices: Vec::new(),
            #[cfg(feature = "vanilla")]
            revision: self.mesh_generations,
            #[cfg(feature = "vanilla-water")]
            solid_vertices: 0,
        };
        #[cfg(feature = "vanilla-water")]
        let mut translucent = Vec::new();
        #[cfg(feature = "vanilla")]
        if !self.chunks[&(cx, cz)]
            .terrain
            .section(section as usize)
            .is_some_and(|blocks| blocks.any_state(|id| self.vanilla.visual(id) != VoxelBlock::Air))
        {
            return mesh;
        }
        #[cfg(feature = "vanilla")]
        {
            let opaque = |key, sy| {
                if sy < 0 {
                    return true;
                }
                self.chunks.get(&key).is_none_or(|col| {
                    col.terrain.section(sy as usize).is_some_and(|blocks| {
                        !blocks.any_state(|id| !self.vanilla.visual(id).is_opaque())
                    })
                })
            };
            if opaque((cx, cz), section)
                && opaque((cx, cz), section - 1)
                && opaque((cx, cz), section + 1)
                && [(cx - 1, cz), (cx + 1, cz), (cx, cz - 1), (cx, cz + 1)]
                    .into_iter()
                    .all(|key| opaque(key, section))
            {
                return mesh;
            }
        }
        let bottom = MIN_Y + section * MESH_HEIGHT;
        #[cfg(feature = "vanilla-water")]
        let column = &self.chunks[&(cx, cz)];
        for y in bottom..bottom + MESH_HEIGHT {
            for z in cz * 16..cz * 16 + 16 {
                for x in cx * 16..cx * 16 + 16 {
                    let pos = BlockPos::new(x, y, z);
                    #[cfg(not(feature = "vanilla-water"))]
                    let block = self.get_block(pos);
                    #[cfg(feature = "vanilla-water")]
                    let id = column.get(x & 15, y, z & 15);
                    #[cfg(feature = "vanilla-water")]
                    let block = self.vanilla.visual(id);
                    #[cfg(feature = "vanilla-water")]
                    {
                        if block != VoxelBlock::Air
                            && !block.is_opaque()
                            && self.vanilla.fluids.get(id).is_some_and(water::is_water)
                        {
                            water::append(self, assets, pos, &mut translucent);
                        }
                        if block == VoxelBlock::Water {
                            continue;
                        }
                    }
                    if block == VoxelBlock::Air {
                        continue;
                    }
                    #[cfg(feature = "vanilla-visuals")]
                    let quads = assets
                        .state_quads
                        .get(&id.0)
                        .or_else(|| assets.baked_quads.get(&block));
                    #[cfg(not(feature = "vanilla-visuals"))]
                    let quads = assets.baked_quads.get(&block);
                    let Some(quads) = quads else {
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
                            #[cfg(feature = "vanilla")]
                            if ny < MIN_Y || !self.chunks.contains_key(&(nx >> 4, nz >> 4)) {
                                continue;
                            }
                            let neighbor = self.get_block(BlockPos::new(nx, ny, nz));
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
                                AssetDirection::Down => {
                                    if cfg!(feature = "first-person-polish") {
                                        0.5
                                    } else {
                                        0.52
                                    }
                                }
                                AssetDirection::North | AssetDirection::South => {
                                    if cfg!(feature = "first-person-polish") {
                                        0.8
                                    } else {
                                        0.82
                                    }
                                }
                                AssetDirection::West | AssetDirection::East => {
                                    if cfg!(feature = "first-person-polish") {
                                        0.6
                                    } else {
                                        0.68
                                    }
                                }
                            }
                        };
                        #[cfg(not(feature = "vanilla-visuals"))]
                        let tint = block.biome_tint(quad.tint_index);
                        #[cfg(feature = "vanilla-visuals")]
                        let tint = visuals::tint(self, pos, block, quad.tint_index);
                        #[cfg(feature = "vanilla-visuals")]
                        let face_shade = visuals::light(self, id, face_shade);
                        #[cfg(feature = "vanilla-visuals")]
                        let vertices = if block.is_blended() {
                            &mut translucent
                        } else {
                            &mut mesh.vertices
                        };
                        #[cfg(not(feature = "vanilla-visuals"))]
                        let vertices = &mut mesh.vertices;
                        let color_ao = [tint[0], tint[1], tint[2], face_shade];
                        let layer = assets
                            .sprite_to_layer
                            .get(&quad.material.sprite)
                            .copied()
                            .unwrap_or(0) as f32;

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
        #[cfg(feature = "vanilla-water")]
        {
            mesh.solid_vertices = mesh.vertices.len() as u32;
            mesh.vertices.append(&mut translucent);
        }
        mesh
    }

    fn refresh_meshes(
        &mut self,
        assets: &BakedAssetRegistry,
        meshes: &mut BTreeMap<(Key, i32), SectionMesh>,
        center: Key,
        eye_y: f64,
        budget: usize,
    ) -> usize {
        meshes.retain(|(key, _), _| self.chunks.contains_key(key));
        let eye_section = (eye_y as i32 - MIN_Y).div_euclid(MESH_HEIGHT);
        let mut dirty: Vec<_> = self
            .chunks
            .iter()
            .flat_map(|(&key, col)| {
                (0..HEIGHT / MESH_HEIGHT)
                    .filter(move |&section| col.dirty_sections & (1 << section) != 0)
                    .map(move |section| (key, section))
            })
            .collect();
        dirty.sort_unstable_by_key(|&((x, z), section)| {
            (
                (x - center.0).abs() + (z - center.1).abs() + (section - eye_section).abs(),
                x,
                z,
                section,
            )
        });
        let rebuilt = dirty.len().min(budget);
        for (key, section) in dirty.into_iter().take(budget) {
            let mesh = self.mesh_section((key, section), assets);
            if mesh.vertices.is_empty() {
                meshes.remove(&(key, section));
            } else {
                meshes.insert((key, section), mesh);
            }
            self.chunks
                .get_mut(&key)
                .expect("loaded column")
                .dirty_sections &= !(1 << section);
            self.mesh_generations += 1;
        }
        rebuilt
    }
}

struct SectionMesh {
    vertices: Vec<TerrainVertex>,
    #[cfg(feature = "vanilla-water")]
    solid_vertices: u32,
    #[cfg(feature = "vanilla")]
    revision: u64,
}

#[cfg(not(feature = "vanilla"))]
fn update_stream(world: &mut HorizonClientWorld, center: Key) -> std::io::Result<()> {
    let start = Instant::now();
    if let Some(change) = world.stream_to(center)? {
        log_line(&format!(
            "STREAM center=({},{}) resident={} generated={} read={} evicted={} saved={} elapsed_us={}",
            center.0,
            center.1,
            world.chunks.len(),
            change.generated,
            change.read,
            change.evicted,
            change.saved,
            start.elapsed().as_micros()
        ));
        log_stream_metrics(&change);
    }
    Ok(())
}

#[cfg(feature = "vanilla-visuals")]
fn log_features(change: &StreamChange, report: impl Fn(&str)) {
    if change.features.tiles != 0 {
        let stats = &change.features;
        report(&format!(
            "FEATURES tiles={} new_terrain={} placed={} unsupported={} failed={} peak_terrain_bytes={}",
            stats.tiles,
            stats.new_terrain,
            stats.placed,
            stats.unsupported,
            stats.failed,
            stats.peak_terrain_bytes
        ));
    }
}
fn log_stream_metrics(change: &StreamChange) {
    #[cfg(feature = "vanilla-visuals")]
    log_features(change, log_line);
    #[cfg(feature = "vanilla")]
    log_line(&format!(
        "CHUNK_TIMING generated_us={:?} read_us={} save_us={}",
        change.generated_us, change.read_us, change.save_us,
    ));
    #[cfg(not(feature = "vanilla"))]
    let _ = change;
}

#[cfg(feature = "vanilla")]
fn section_visible(vp: Mat4, ((x, z), section): (Key, i32)) -> bool {
    let origin = Vec4::new(
        (x * 16) as f32,
        (MIN_Y + section * MESH_HEIGHT) as f32,
        (z * 16) as f32,
        1.0,
    );
    let mut outside = 0b11_1111;
    for corner in 0..8 {
        let p = vp
            * (origin
                + Vec4::new(
                    ((corner & 1) * 16) as f32,
                    (((corner >> 1) & 1) * MESH_HEIGHT) as f32,
                    (((corner >> 2) & 1) * 16) as f32,
                    0.0,
                ));
        let mask = u8::from(p.x < -p.w)
            | u8::from(p.x > p.w) << 1
            | u8::from(p.y < -p.w) << 2
            | u8::from(p.y > p.w) << 3
            | u8::from(p.z < 0.0) << 4
            | u8::from(p.z > p.w) << 5;
        outside &= mask;
    }
    outside == 0
}

#[cfg(feature = "vanilla")]
fn open_vanilla_world()
-> std::io::Result<Option<(HorizonClientWorld, Option<world_stream::PlayerSave>, Key)>> {
    let (messages, log) = std::sync::mpsc::channel::<String>();
    let worker = thread::Builder::new().name("Switch world startup".into()).stack_size(4 << 20)
        .spawn(move || {
            let start = Instant::now();
            let report = |s: &str| { let _ = messages.send(s.to_owned()); };
            #[cfg(feature = "vanilla-visuals")]
            let (vanilla, save_dir) = (
                vanilla::Vanilla::load_decorated(Path::new("romfs:/"), report)?,
                "sdmc:/switch/horizon/saves/switch_vanilla_test10",
            );
            #[cfg(not(feature = "vanilla-visuals"))]
            let (vanilla, save_dir) = (
                vanilla::Vanilla::load(Path::new("romfs:/"), report)?,
                "sdmc:/switch/horizon/saves/switch_vanilla_test8",
            );
            let mut world = HorizonClientWorld::new(
                PathBuf::from(save_dir), vanilla);
            let saved = world.read_player()?;
            let [x, z] = world.spawn_xz();
            #[cfg(feature = "first-person-polish")]
            let mut center = saved.map_or((x >> 4, z >> 4), |state| player_chunk(state.0));
            #[cfg(not(feature = "first-person-polish"))]
            let center = saved.map_or((x >> 4, z >> 4), |state| player_chunk(state.0));
            report("INFO Preparing initial 3x3 view");
            if let Some(change) = world.stream_to(center)? {
                #[cfg(feature = "vanilla-visuals")]
                log_features(&change, report);
                report(&format!(
                    "STREAM_START center={center:?} resident={} generated={} read={} worker_ms={} generated_us={:?} read_us={}",
                    world.chunks.len(), change.generated, change.read, start.elapsed().as_millis(),
                    change.generated_us, change.read_us,
                ));
            }
            #[cfg(feature = "first-person-polish")]
            let saved = if let Some((pos, yaw, pitch, slot)) = saved {
                if polish::valid_restore(&world, pos) {
                    Some((pos, yaw, pitch, slot))
                } else {
                    let mut recovery = polish::dry_spawn(&world, pos.x.floor() as i32, pos.z.floor() as i32);
                    if recovery.is_none() {
                        center = (x >> 4, z >> 4);
                        world.stream_to(center)?;
                        recovery = polish::dry_spawn(&world, x, z);
                    }
                    let recovered = recovery.ok_or_else(|| std::io::Error::other("no dry recovery position in loaded terrain"))?;
                    report(&format!("SPAWN recovered invalid saved position {pos:?} to {recovered:?}"));
                    Some((recovered, yaw, pitch, slot))
                }
            } else { None };
            Ok::<_, std::io::Error>((world, saved, center))
        })?;
    unsafe { horizon_loading_begin() };
    let mut keep_running = true;
    while !worker.is_finished() {
        for line in log.try_iter() {
            log_line(&line);
        }
        keep_running &= unsafe { horizon_loading_poll() } != 0;
        thread::sleep(Duration::from_millis(10));
    }
    for line in log.try_iter() {
        log_line(&line);
    }
    unsafe { horizon_loading_end() };
    let world = worker
        .join()
        .map_err(|_| std::io::Error::other("world startup worker panicked"))??;
    Ok(keep_running.then_some(world))
}

impl RayWorld for HorizonClientWorld {
    type Error = Infallible;
    type Block = VoxelBlock;
    type Fluid = bool;

    fn block_state(&mut self, p: BlockPos) -> Result<Self::Block, Self::Error> {
        Ok(self.get_block(p))
    }

    fn fluid_state(&mut self, p: BlockPos) -> Result<Self::Fluid, Self::Error> {
        Ok(self.get_block(p) == VoxelBlock::Water)
    }

    fn block_shape(
        &mut self,
        _state: Self::Block,
        p: BlockPos,
        _mode: RayBlockMode,
    ) -> Result<VoxelShape, Self::Error> {
        Ok(self.collision_shape(p).clone())
    }

    fn interaction_shape(
        &mut self,
        _state: Self::Block,
        _p: BlockPos,
    ) -> Result<VoxelShape, Self::Error> {
        Ok(self.empty_shape.clone())
    }

    fn fluid_is_source(&mut self, state: Self::Fluid) -> Result<bool, Self::Error> {
        Ok(state)
    }

    fn fluid_is_empty(&mut self, state: Self::Fluid) -> Result<bool, Self::Error> {
        Ok(!state)
    }

    fn fluid_is_water(&mut self, state: Self::Fluid) -> Result<bool, Self::Error> {
        Ok(state)
    }

    fn fluid_shape(&mut self, state: Self::Fluid, _p: BlockPos) -> Result<VoxelShape, Self::Error> {
        Ok(if state {
            self.solid_shape.clone()
        } else {
            self.empty_shape.clone()
        })
    }
}

// ============================================================================
// 6. REAL HORIZON LOCAL PLAYER, CAMERA, PHYSICS & INVENTORY
// ============================================================================
pub struct HorizonLocalPlayer {
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub yaw_o: f32,
    pub pitch_o: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    pub sprinting: bool,
    pub sneaking: bool,
    pub jumping: bool,
    pub fall_distance: f64,
    pub jump_cooldown: u8,
    pub camera: camera::Camera,
    pub camera_type: camera::CameraType,
    pub bob: camera::AvatarBob,
    #[cfg(not(feature = "first-person-polish"))]
    pub hand_swing: f32,
    #[cfg(feature = "first-person-polish")]
    hand: polish::HandPresentation,
    pub inventory: PlayerInventory<VoxelBlock, u8, u8>,
    pub cooldowns: Cooldowns,
    pub blocks_broken: u32,
    pub blocks_placed: u32,
}

impl HorizonLocalPlayer {
    pub const WIDTH: f64 = 0.6;
    pub const STANDING_HEIGHT: f64 = 1.8;
    pub const SNEAKING_HEIGHT: f64 = 1.5;
    pub const STANDING_EYE_HEIGHT: f32 = 1.62;
    pub const SNEAKING_EYE_HEIGHT: f32 = 1.27;
    pub const BLOCK_INTERACTION_RANGE: f64 = 4.5;

    pub fn new(spawn_pos: Vec3) -> Self {
        let stacks = ItemStacks::<VoxelBlock, u8, u8>::new(VoxelBlock::Air);
        let equipment = EntityEquipment::new(stacks.empty());
        let mut inventory = PlayerInventory::new(stacks.clone(), equipment);
        for (slot, &block) in VoxelBlock::HOTBAR_BLOCKS.iter().enumerate() {
            let stack = stacks.create(block, 64, Prototype::new([]));
            let _ = inventory.set(slot as i32, stack);
        }
        let _ = inventory.set_selected(0);

        Self {
            position: spawn_pos,
            previous: spawn_pos,
            velocity: Vec3::ZERO,
            yaw: 180.0,
            pitch: 10.0,
            yaw_o: 180.0,
            pitch_o: 10.0,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
            sprinting: false,
            sneaking: false,
            jumping: false,
            fall_distance: 0.0,
            jump_cooldown: 0,
            camera: camera::Camera {
                eye_height: Self::STANDING_EYE_HEIGHT,
                eye_height_old: Self::STANDING_EYE_HEIGHT,
                fov_modifier: 1.0,
                old_fov_modifier: 1.0,
                ..Default::default()
            },
            camera_type: camera::CameraType::FirstPerson,
            bob: camera::AvatarBob::default(),
            #[cfg(not(feature = "first-person-polish"))]
            hand_swing: 0.0,
            #[cfg(feature = "first-person-polish")]
            hand: polish::HandPresentation::new(VoxelBlock::HOTBAR_BLOCKS[0]),
            inventory,
            cooldowns: Cooldowns::default(),
            blocks_broken: 0,
            blocks_placed: 0,
        }
    }

    pub fn height(&self) -> f64 {
        if self.sneaking {
            Self::SNEAKING_HEIGHT
        } else {
            Self::STANDING_HEIGHT
        }
    }

    #[cfg(feature = "first-person-polish")]
    fn standing_bounds(pos: Vec3) -> Aabb {
        Self::bounds(pos, Self::STANDING_HEIGHT)
    }

    pub fn aabb_at(&self, pos: Vec3) -> Aabb {
        Self::bounds(pos, self.height())
    }

    fn bounds(pos: Vec3, height: f64) -> Aabb {
        let half_w = Self::WIDTH * 0.5;
        Aabb::new(
            Vec3::new(pos.x - half_w, pos.y, pos.z - half_w),
            Vec3::new(pos.x + half_w, pos.y + height, pos.z + half_w),
        )
    }

    pub fn aabb(&self) -> Aabb {
        self.aabb_at(self.position)
    }

    pub fn eye_position(&self) -> Vec3 {
        Vec3::new(
            self.position.x,
            self.position.y + f64::from(self.to_camera_entity().eye_height),
            self.position.z,
        )
    }

    pub fn selected_block(&self) -> VoxelBlock {
        let stack = self.inventory.selected_item();
        let data = stack.read();
        if data.is_empty() {
            VoxelBlock::GrassBlock
        } else {
            data.item()
        }
    }

    pub fn to_camera_entity(&self) -> camera::CameraEntity {
        camera::CameraEntity {
            previous: [self.previous.x, self.previous.y, self.previous.z],
            position: [self.position.x, self.position.y, self.position.z],
            eye_height: if self.sneaking {
                Self::SNEAKING_EYE_HEIGHT
            } else {
                Self::STANDING_EYE_HEIGHT
            },
            y_rot_o: self.yaw_o,
            y_rot: self.yaw,
            x_rot_o: self.pitch_o,
            x_rot: self.pitch,
            living: Some(camera::LivingCamera {
                scale: 1.0,
                camera_distance: 4.0,
                sleeping: None,
                dead: None,
            }),
            mount: None,
        }
    }

    pub fn sync_camera(&mut self, partial_tick: f32, world: &mut HorizonClientWorld) {
        let entity = self.to_camera_entity();
        self.camera
            .align_with_entity(&entity, partial_tick, self.camera_type, |from, to| {
                let hit = clip_world(
                    world,
                    Vec3::new(from[0], from[1], from[2]),
                    Vec3::new(to[0], to[1], to[2]),
                    RayBlockMode::Visual,
                    RayFluidMode::None,
                    32,
                )
                .ok()?;
                let p = hit.hit.location;
                (!hit.miss).then_some([p.x, p.y, p.z])
            });
    }

    pub fn raycast_block(&self, world: &mut HorizonClientWorld) -> Option<BlockHit> {
        let start = self.eye_position();
        let dir = view_vector(self.pitch, self.yaw);
        let end = start.add_vector(dir.scale(Self::BLOCK_INTERACTION_RANGE));
        match clip_world(
            world,
            start,
            end,
            RayBlockMode::Outline,
            RayFluidMode::None,
            64,
        ) {
            Ok(res) if !res.miss => Some(res.hit),
            _ => None,
        }
    }

    pub fn tick_physics(
        &mut self,
        world: &HorizonClientWorld,
        raw_forward: f32,
        raw_left: f32,
        jump_held: bool,
    ) {
        self.previous = self.position;
        self.yaw_o = self.yaw;
        self.pitch_o = self.pitch;
        self.cooldowns.tick();
        self.bob
            .tick([self.position.x, self.position.y, self.position.z]);

        #[cfg(feature = "first-person-polish")]
        self.hand.tick(self.selected_block());
        #[cfg(not(feature = "first-person-polish"))]
        if self.hand_swing > 0.0 {
            self.hand_swing = (self.hand_swing - 0.18).max(0.0);
        }
        if self.jump_cooldown > 0 {
            self.jump_cooldown -= 1;
        }

        let (forward, strafe) = modify_move_input(raw_forward, raw_left, self.sneaking, 0.3);
        if forward <= 0.05 || self.sneaking || self.horizontal_collision {
            self.sprinting = false;
        }

        self.jumping = jump_held;
        #[cfg(feature = "first-person-polish")]
        let water_depth = polish::water_depth(world, self.aabb());
        #[cfg(not(feature = "first-person-polish"))]
        let water_depth = 0.0;
        if self.jumping && self.on_ground && self.jump_cooldown == 0 && water_depth <= 0.4 {
            struct JumpCtx<'a>(&'a mut HorizonLocalPlayer);
            impl LivingMotionWorld for JumpCtx<'_> {
                type Error = Infallible;
                fn velocity(&self) -> Vec3 {
                    self.0.velocity
                }
                fn set_velocity(&mut self, velocity: Vec3) -> Result<(), Self::Error> {
                    self.0.velocity = velocity;
                    Ok(())
                }
                fn jump_strength(&mut self) -> Result<f64, Self::Error> {
                    Ok(0.42)
                }
                fn block_jump_factor(&mut self) -> Result<f32, Self::Error> {
                    Ok(1.0)
                }
                fn has_jump_boost(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn jump_boost_amplifier(&mut self) -> Result<i32, Self::Error> {
                    Ok(0)
                }
                fn sprinting(&mut self) -> Result<bool, Self::Error> {
                    Ok(self.0.sprinting)
                }
                fn yaw(&mut self) -> Result<f32, Self::Error> {
                    Ok(self.0.yaw)
                }
                fn mark_needs_sync(&mut self) {}
                fn no_gravity(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn gravity_attribute(&mut self) -> Result<f64, Self::Error> {
                    Ok(0.08)
                }
                fn has_slow_falling(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn controlling_passenger_is_player(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn speed(&mut self) -> Result<f32, Self::Error> {
                    Ok(if self.0.sprinting { 0.13 } else { 0.10 })
                }
                fn omnidirectional_air_mover(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn air_drag_modifier(&mut self) -> Result<f64, Self::Error> {
                    Ok(1.0)
                }
            }
            let mut ctx = JumpCtx(self);
            let _ = jump_from_ground(&mut ctx);
            self.on_ground = false;
            self.jump_cooldown = 6;
        }

        // Real Horizon PlayerTravelWorld -> travel_in_air -> edge_backoff -> resolve_step_movement
        {
            struct TravelCtx<'a> {
                player: &'a mut HorizonLocalPlayer,
                world: &'a HorizonClientWorld,
            }

            impl LivingAirWorld for TravelCtx<'_> {
                type Error = Infallible;
                fn movement_support_position(&mut self) -> Result<BlockPos, Self::Error> {
                    Ok(BlockPos::new(
                        self.player.position.x.floor() as i32,
                        (self.player.position.y - 0.500001).floor() as i32,
                        self.player.position.z.floor() as i32,
                    ))
                }
                fn on_ground(&mut self) -> Result<bool, Self::Error> {
                    Ok(self.player.on_ground)
                }
                fn block_friction(&mut self, _position: BlockPos) -> Result<f32, Self::Error> {
                    Ok(0.6)
                }
                fn friction_modifier(&mut self) -> Result<f64, Self::Error> {
                    Ok(1.0)
                }
                fn speed(&mut self) -> Result<f32, Self::Error> {
                    Ok(if self.player.sprinting { 0.13 } else { 0.10 })
                }
                fn flying_speed(&mut self) -> Result<f32, Self::Error> {
                    player_flying_speed(self)
                }
                fn yaw(&mut self) -> Result<f32, Self::Error> {
                    Ok(self.player.yaw)
                }
                fn velocity(&self) -> Vec3 {
                    self.player.velocity
                }
                fn set_velocity(&mut self, velocity: Vec3) -> Result<(), Self::Error> {
                    self.player.velocity = velocity;
                    Ok(())
                }
                fn on_climbable(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn reset_fall_distance(&mut self) -> Result<(), Self::Error> {
                    self.player.fall_distance = 0.0;
                    Ok(())
                }
                fn in_scaffolding(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn suppressing_ladder_slide(&mut self) -> Result<bool, Self::Error> {
                    Ok(self.player.sneaking)
                }
                fn is_player_class(&self) -> bool {
                    true
                }
                fn move_self(&mut self, movement: Vec3) -> Result<(), Self::Error> {
                    let adjusted = if self.player.sneaking {
                        struct BackoffCtx<'b> {
                            sneaking: bool,
                            on_ground: bool,
                            world: &'b HorizonClientWorld,
                        }
                        impl EdgeBackoffWorld for BackoffCtx<'_> {
                            type Error = Infallible;
                            fn max_up_step(&mut self) -> Result<f32, Self::Error> {
                                Ok(0.6)
                            }
                            fn staying_on_ground_surface(&mut self) -> Result<bool, Self::Error> {
                                Ok(self.sneaking)
                            }
                            fn on_ground(&mut self) -> Result<bool, Self::Error> {
                                Ok(self.on_ground)
                            }
                            fn no_collision(&mut self, bounds: Aabb) -> Result<bool, Self::Error> {
                                let colliders = self.world.collect_colliders(bounds);
                                Ok(!colliders.iter().any(|shape| {
                                    shape.boxes().iter().any(|b| b.intersects(bounds))
                                }))
                            }
                        }
                        let mut backoff = BackoffCtx {
                            sneaking: self.player.sneaking,
                            on_ground: self.player.on_ground,
                            world: self.world,
                        };
                        back_off_from_edge(
                            &mut backoff,
                            EdgeBackoffInput {
                                movement,
                                bounds: self.player.aabb(),
                                flying: false,
                                self_or_player: true,
                                fall_distance: self.player.fall_distance,
                                query_budget: 64,
                            },
                        )
                        .unwrap_or(movement)
                    } else {
                        movement
                    };

                    let aabb = self.player.aabb();
                    let world_ref = self.world;
                    let resolved = try_resolve_step_movement(
                        adjusted,
                        aabb,
                        0.6,
                        self.player.on_ground,
                        |query_box| Ok::<_, Infallible>(world_ref.collect_colliders(query_box)),
                    )
                    .unwrap_or_else(|never| match never {});
                    self.player.position = self.player.position.add_vector(resolved);

                    let hx = (adjusted.x - resolved.x).abs() > 1.0e-5;
                    let hz = (adjusted.z - resolved.z).abs() > 1.0e-5;
                    let vy = (adjusted.y - resolved.y).abs() > 1.0e-5;
                    self.player.horizontal_collision = hx || hz;
                    self.player.vertical_collision = vy;
                    self.player.vertical_collision_below = vy && adjusted.y < 0.0;
                    self.player.on_ground = self.player.vertical_collision_below;

                    if hx {
                        self.player.velocity.x = 0.0;
                    }
                    if hz {
                        self.player.velocity.z = 0.0;
                    }
                    if vy {
                        self.player.velocity.y = 0.0;
                    }
                    if resolved.y < 0.0 {
                        self.player.fall_distance += -resolved.y;
                    } else if self.player.on_ground {
                        self.player.fall_distance = 0.0;
                    }
                    Ok(())
                }
                fn horizontal_collision(&self) -> bool {
                    self.player.horizontal_collision
                }
                fn jumping(&self) -> bool {
                    self.player.jumping
                }
                fn was_in_powder_snow(&self) -> bool {
                    false
                }
                fn can_walk_powder_snow(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn levitation_amplifier(&mut self) -> Result<Option<i32>, Self::Error> {
                    Ok(None)
                }
                fn is_client_side(&mut self) -> Result<bool, Self::Error> {
                    Ok(true)
                }
                fn has_chunk_at(&mut self, _position: BlockPos) -> Result<bool, Self::Error> {
                    Ok(true)
                }
                fn effective_gravity(&mut self) -> Result<f64, Self::Error> {
                    Ok(0.08)
                }
                fn y(&self) -> f64 {
                    self.player.position.y
                }
                fn minimum_y(&mut self) -> Result<i32, Self::Error> {
                    Ok(0)
                }
                fn discard_friction(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn air_drag_modifier(&mut self) -> Result<f64, Self::Error> {
                    Ok(1.0)
                }
                fn omnidirectional_air_mover(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
            }

            #[cfg(feature = "first-person-polish")]
            impl horizon_world::living_fluid::LivingFluidWorld for TravelCtx<'_> {
                fn in_water(&mut self) -> Result<bool, Self::Error> {
                    Ok(polish::water_depth(self.world, self.player.aabb()) > 0.0)
                }
                fn sprinting(&mut self) -> Result<bool, Self::Error> {
                    Ok(self.player.sprinting)
                }
                fn water_slow_down(&mut self) -> Result<f32, Self::Error> {
                    Ok(0.8)
                }
                fn water_movement_efficiency(&mut self) -> Result<f64, Self::Error> {
                    Ok(0.0)
                }
                fn dolphins_grace(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn fluid_height(
                    &mut self,
                    tag: horizon_world::living_fluid::TravelFluidTag,
                ) -> Result<f64, Self::Error> {
                    use horizon_world::living_fluid::TravelFluidTag;
                    Ok(match tag {
                        TravelFluidTag::Lava => 0.0,
                        TravelFluidTag::EntityFloatable => {
                            polish::water_depth(self.world, self.player.aabb())
                        }
                    })
                }
                fn fluid_jump_threshold(&mut self) -> Result<f64, Self::Error> {
                    Ok(0.4)
                }
                fn is_free(&mut self, offset: Vec3) -> Result<bool, Self::Error> {
                    let bounds = self.player.aabb().move_by(offset);
                    Ok(!polish::intersects_blocks(self.world, bounds)
                        && polish::water_depth(self.world, bounds) == 0.0)
                }
                fn is_vehicle(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn can_float_while_ridden(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
            }

            impl PlayerTravelWorld for TravelCtx<'_> {
                type Error = Infallible;
                fn is_passenger(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn is_swimming(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn look_angle(&mut self) -> Result<Vec3, Self::Error> {
                    Ok(view_vector(self.player.pitch, self.player.yaw))
                }
                fn jumping(&self) -> bool {
                    self.player.jumping
                }
                fn position(&self) -> Vec3 {
                    self.player.position
                }
                fn fluid_is_empty(&mut self, position: BlockPos) -> Result<bool, Self::Error> {
                    Ok(self.world.get_block(position) != VoxelBlock::Water)
                }
                fn velocity(&self) -> Vec3 {
                    self.player.velocity
                }
                fn set_velocity(&mut self, velocity: Vec3) -> Result<(), Self::Error> {
                    self.player.velocity = velocity;
                    Ok(())
                }
                fn query_flying_ability(&mut self) -> Result<bool, Self::Error> {
                    Ok(false)
                }
                fn flying_ability(&self) -> bool {
                    false
                }
                fn ability_flying_speed(&mut self) -> Result<f32, Self::Error> {
                    Ok(0.05)
                }
                fn sprinting(&mut self) -> Result<bool, Self::Error> {
                    Ok(self.player.sprinting)
                }
                fn living_travel(&mut self, input: Vec3) -> Result<(), Self::Error> {
                    #[cfg(feature = "first-person-polish")]
                    if polish::water_depth(self.world, self.player.aabb()) > 0.0 {
                        use horizon_world::living_fluid::travel_in_water;
                        let falling = self.player.velocity.y <= 0.0;
                        let old_y = self.player.position.y;
                        self.player.fall_distance = 0.0;
                        return travel_in_water(self, input, 0.08, falling, old_y);
                    }
                    travel_in_air(self, input)
                }
            }

            #[cfg(feature = "first-person-polish")]
            if water_depth > 0.0 {
                // LivingEntity's float/sink step, before fluid drag.
                let vertical = f64::from(0.04f32);
                if self.sneaking {
                    self.velocity.y -= vertical;
                } else if self.jumping && (!self.on_ground || water_depth > 0.4) {
                    self.velocity.y += vertical;
                }
            }
            let mut travel_ctx = TravelCtx {
                player: self,
                world,
            };
            let _ = player_travel(
                &mut travel_ctx,
                Vec3::new(strafe as f64, 0.0, forward as f64),
            );
        }

        // Update view bobbing & FOV modifier
        let dx = self.position.x - self.previous.x;
        let dz = self.position.z - self.previous.z;
        self.bob.add_walked(dx as f32, dz as f32);
        self.bob.update_bob(
            self.on_ground,
            [self.velocity.x, self.velocity.y, self.velocity.z],
        );

        let fov_state = camera::FovPlayer {
            flying: false,
            walking_speed: 0.10,
            movement_speed: if self.sprinting { 0.13 } else { 0.10 },
            using: None,
        };
        let target_fov_mod =
            camera::field_of_view_modifier(&fov_state, self.camera_type.is_first_person(), 1.0);
        self.camera
            .tick(Some(self.to_camera_entity().eye_height), target_fov_mod);

        #[cfg(not(feature = "vanilla"))]
        if self.position.y < -12.0 {
            let sy = world.surface_y(16, 24) as f64 + 1.05;
            self.position = Vec3::new(16.5, sy, 24.5);
            self.previous = self.position;
            self.velocity = Vec3::ZERO;
            self.on_ground = false;
        }
    }
}

// ============================================================================
// 7. TARGET HIGHLIGHT, 3D FIRST-PERSON HELD ITEM & 2D HUD OVERLAY
// ============================================================================
fn direction_offset(dir: Direction) -> (i32, i32, i32) {
    match dir {
        Direction::Down => (0, -1, 0),
        Direction::Up => (0, 1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::West => (-1, 0, 0),
        Direction::East => (1, 0, 0),
    }
}

fn emit_box_beam(
    out: &mut Vec<TerrainVertex>,
    min: [f32; 3],
    max: [f32; 3],
    white_layer: f32,
    color: [f32; 3],
) {
    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    let faces: [([usize; 4], [f32; 3]); 6] = [
        ([0, 3, 2, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([3, 7, 6, 2], [0.0, 1.0, 0.0]),
        ([0, 4, 7, 3], [-1.0, 0.0, 0.0]),
        ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
    ];
    let color_ao = [color[0], color[1], color[2], 1.15];
    for (idx, normal) in faces {
        for &i in &[idx[0], idx[1], idx[2], idx[0], idx[2], idx[3]] {
            out.push(TerrainVertex {
                pos: corners[i],
                normal,
                uv_layer: [0.5, 0.5, white_layer],
                color_ao,
            });
        }
    }
}

pub fn append_target_highlight(out: &mut Vec<TerrainVertex>, pos: BlockPos, white_layer: u32) {
    let x0 = pos.x as f32 - 0.005;
    let y0 = pos.y as f32 - 0.005;
    let z0 = pos.z as f32 - 0.005;
    let x1 = pos.x as f32 + 1.005;
    let y1 = pos.y as f32 + 1.005;
    let z1 = pos.z as f32 + 1.005;
    let t = 0.020;
    let col = [1.0, 0.95, 0.25];
    let wl = white_layer as f32;

    for &(y, z) in &[(y0, z0), (y1, z0), (y0, z1), (y1, z1)] {
        emit_box_beam(out, [x0, y - t, z - t], [x1, y + t, z + t], wl, col);
    }
    for &(x, z) in &[(x0, z0), (x1, z0), (x0, z1), (x1, z1)] {
        emit_box_beam(out, [x - t, y0, z - t], [x + t, y1, z + t], wl, col);
    }
    for &(x, y) in &[(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        emit_box_beam(out, [x - t, y - t, z0], [x + t, y + t, z1], wl, col);
    }
}

pub fn append_first_person_hand(
    out: &mut Vec<TerrainVertex>,
    block: VoxelBlock,
    assets: &BakedAssetRegistry,
    #[cfg(not(feature = "first-person-polish"))] bob: &camera::AvatarBob,
    #[cfg(not(feature = "first-person-polish"))] partial_tick: f32,
    swing: f32,
    #[cfg(feature = "first-person-polish")] pose: (Mat4, f32),
) {
    let Some(quads) = assets.baked_quads.get(&block) else {
        return;
    };
    #[cfg(feature = "first-person-polish")]
    let model_mat = pose.0
        * first_person_shared::apply_swing(
            first_person_shared::item_anchor(1.0, pose.1),
            1.0,
            swing,
        )
        * assets.hand_models[&block];
    #[cfg(not(feature = "first-person-polish"))]
    let model_mat = {
        let walk_angle = f64::from(
            bob.backwards_interpolated_walk_distance(partial_tick) * std::f32::consts::PI,
        );
        let amplitude = bob.interpolated_bob(partial_tick);
        let bob_x = trig::sin(walk_angle) * amplitude * 0.18;
        let bob_y = -(trig::cos(walk_angle) * amplitude).abs() * 0.22;
        let swing_arc = (swing * std::f32::consts::PI).sin();

        let model_mat = Mat4::from_translation(GlamVec3::new(
            0.56 + bob_x - swing_arc * 0.16,
            -0.50 + bob_y - swing_arc * 0.12,
            -0.95 - swing_arc * 0.18,
        )) * Mat4::from_rotation_y(-0.62 - swing_arc * 0.35)
            * Mat4::from_rotation_x(0.22 - swing_arc * 0.45)
            * Mat4::from_scale(GlamVec3::splat(0.36));
        model_mat
    };
    for quad in quads {
        let normal = match quad.facing {
            AssetDirection::Up => GlamVec3::Y,
            AssetDirection::Down => -GlamVec3::Y,
            AssetDirection::North => -GlamVec3::Z,
            AssetDirection::South => GlamVec3::Z,
            AssetDirection::West => -GlamVec3::X,
            AssetDirection::East => GlamVec3::X,
        };
        let normal = model_mat.transform_vector3(normal).normalize().to_array();
        let face_shade = match quad.facing {
            AssetDirection::Up => 1.05,
            AssetDirection::Down => 0.60,
            AssetDirection::North | AssetDirection::South => 0.88,
            AssetDirection::West | AssetDirection::East => 0.74,
        };
        let tint = block.biome_tint(quad.tint_index);
        let color_ao = [tint[0], tint[1], tint[2], face_shade];
        let layer = assets
            .sprite_to_layer
            .get(&quad.material.sprite)
            .copied()
            .unwrap_or(0) as f32;

        for &vi in &[0usize, 1, 2, 0, 2, 3] {
            let p = quad.positions[vi];
            let uv = quad.uv[vi];
            #[cfg(not(feature = "first-person-polish"))]
            let local = GlamVec3::from_array(p) - GlamVec3::splat(0.5);
            #[cfg(feature = "first-person-polish")]
            let local = GlamVec3::from_array(p);
            out.push(TerrainVertex {
                pos: model_mat.transform_point3(local).to_array(),
                normal,
                uv_layer: [uv[0], uv[1], layer],
                color_ao,
            });
        }
    }
}

fn emit_ndc_rect(
    out: &mut Vec<TerrainVertex>,
    inv_vp: Mat4,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    z_ndc: f32,
    layer: f32,
    color: [f32; 3],
) {
    let unproj = |nx: f32, ny: f32| -> [f32; 3] {
        let p = inv_vp * Vec4::new(nx, ny, z_ndc, 1.0);
        let w = if p.w.abs() > 1.0e-6 { p.w } else { 1.0 };
        [p.x / w, p.y / w, p.z / w]
    };
    let p00 = unproj(x0, y0);
    let p10 = unproj(x1, y0);
    let p11 = unproj(x1, y1);
    let p01 = unproj(x0, y1);
    let normal = [0.0, 1.0, 0.0];
    let color_ao = [color[0], color[1], color[2], 1.12];

    // Counter-clockwise winding in screen space so Back-face culling keeps all HUD quads visible
    let verts = [
        (p00, [0.0, 1.0]),
        (p10, [1.0, 1.0]),
        (p11, [1.0, 0.0]),
        (p00, [0.0, 1.0]),
        (p11, [1.0, 0.0]),
        (p01, [0.0, 0.0]),
    ];
    for (pos, uv) in verts {
        out.push(TerrainVertex {
            pos,
            normal,
            uv_layer: [uv[0], uv[1], layer],
            color_ao,
        });
    }
}

pub fn append_hud_overlay(
    out: &mut Vec<TerrainVertex>,
    inv_vp: Mat4,
    selected_slot: usize,
    assets: &BakedAssetRegistry,
) {
    let wl = assets.white_layer as f32;

    // 1. Center Crosshair
    let cx = 0.011;
    let cy = 0.019;
    let thick_x = 0.0022;
    let thick_y = 0.0038;
    emit_ndc_rect(
        out,
        inv_vp,
        -cx,
        -thick_y,
        cx,
        thick_y,
        0.02,
        wl,
        [0.96, 0.96, 0.96],
    );
    emit_ndc_rect(
        out,
        inv_vp,
        -thick_x,
        -cy,
        thick_x,
        cy,
        0.02,
        wl,
        [0.96, 0.96, 0.96],
    );

    // 2. 9-Slot Hotbar at Bottom Center
    let slot_w = 0.068;
    let slot_h = 0.115;
    let gap = 0.006;
    let total_w = 9.0 * slot_w + 8.0 * gap;
    let start_x = -total_w * 0.5;
    let base_y = -0.94;

    emit_ndc_rect(
        out,
        inv_vp,
        start_x - 0.014,
        base_y - 0.018,
        start_x + total_w + 0.014,
        base_y + slot_h + 0.018,
        0.04,
        wl,
        [0.12, 0.13, 0.15],
    );

    for (slot_idx, &block) in VoxelBlock::HOTBAR_BLOCKS.iter().enumerate() {
        let sx0 = start_x + slot_idx as f32 * (slot_w + gap);
        let sx1 = sx0 + slot_w;
        let sy0 = base_y;
        let sy1 = base_y + slot_h;

        if slot_idx == selected_slot {
            emit_ndc_rect(
                out,
                inv_vp,
                sx0 - 0.006,
                sy0 - 0.010,
                sx1 + 0.006,
                sy1 + 0.010,
                0.03,
                wl,
                [0.98, 0.88, 0.28],
            );
        }

        emit_ndc_rect(
            out,
            inv_vp,
            sx0,
            sy0,
            sx1,
            sy1,
            0.025,
            wl,
            [0.22, 0.24, 0.28],
        );

        #[cfg(feature = "vanilla-visuals")]
        if block == VoxelBlock::Water {
            emit_ndc_rect(
                out,
                inv_vp,
                sx0 + 0.010,
                sy0 + 0.016,
                sx1 - 0.010,
                sy1 - 0.016,
                0.015,
                assets.bucket_layer as f32,
                [1.0; 3],
            );
            continue;
        }
        if let Some(quads) = assets.baked_quads.get(&block) {
            let icon_quad = quads
                .iter()
                .find(|q| q.facing == AssetDirection::South && q.tint_index < 0)
                .or_else(|| quads.iter().find(|q| q.facing == AssetDirection::Up))
                .or_else(|| quads.first());
            if let Some(q) = icon_quad {
                let layer = assets
                    .sprite_to_layer
                    .get(&q.material.sprite)
                    .copied()
                    .unwrap_or(0) as f32;
                let tint = block.biome_tint(q.tint_index);
                emit_ndc_rect(
                    out,
                    inv_vp,
                    sx0 + 0.010,
                    sy0 + 0.016,
                    sx1 - 0.010,
                    sy1 - 0.016,
                    0.015,
                    layer,
                    tint,
                );
            }
        }
    }
}

// ============================================================================
// 8. HELPER FUNCTIONS & MAIN ENTRY (`rust_horizon_client_main`)
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

const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 720;
const MAX_VERTEX_BUFFER_BYTES: u64 = 6 * 1024 * 1024; // 6 MiB
const STAGING_BUFFER_BYTES: u64 = 6 * 1024 * 1024; // 6 MiB

#[unsafe(no_mangle)]
#[cfg(target_os = "horizon")]
pub extern "C" fn rust_horizon_client_main() -> c_int {
    let startup = Instant::now();
    log_line("INFO Compile-time ABI assertions passed (180 SDK checks)");
    match horizon_platform::available_space(Path::new("sdmc:/")) {
        Ok(bytes) => log_line(&format!("PLATFORM_STORAGE available_bytes={bytes}")),
        Err(error) => log_line(&format!("PLATFORM_STORAGE error={error}")),
    }

    let welcome = Text::literal(format!(
        "Horizon {} (proto {}, data {}) — Real First-Person Client on Switch",
        VANILLA_VERSION, PROTOCOL_VERSION, DATA_VERSION
    ));
    let held_stack = ItemStack::new("minecraft:grass_block", 64);
    if held_stack.item.is_empty() || held_stack.count != 64 {
        log_line("FAIL Horizon item sanity check failed");
        return 1;
    }
    log_line(&format!("INFO {}", welcome.plain()));

    // 1. Load Gamepad settings & bake real Minecraft 26.3 assets via `horizon-assets`
    #[cfg(feature = "vanilla")]
    unsafe {
        horizon_memory_report()
    };
    let pad_settings =
        PadSettings::load_or_save(Path::new("sdmc:/switch/horizon/horizon-controller.txt"));
    #[cfg(feature = "vanilla")]
    let (mut world, saved, center) = match open_vanilla_world() {
        Ok(Some(world)) => world,
        Ok(None) => {
            log_line("INFO Startup cancelled before play");
            return 20;
        }
        Err(error) => {
            log_line(&format!("FAIL Initial vanilla world: {error}"));
            return 3;
        }
    };
    #[cfg(feature = "first-person-polish")]
    let assets_start = Instant::now();
    let mut assets = match load_and_bake_assets() {
        Ok(reg) => reg,
        Err(e) => {
            log_line(&format!("FAIL Asset loading/baking failed: {e}"));
            return 2;
        }
    };
    let total_baked_quads: usize = assets.baked_quads.values().map(Vec::len).sum();
    #[cfg(feature = "first-person-polish")]
    log_line(&format!(
        "INIT asset_bake_ms={}",
        assets_start.elapsed().as_millis()
    ));
    log_line(&format!(
        "PASS Horizon 26.3 assets loaded and baked via horizon-assets: source='{}' blocks={} textures={} animations={} total_quads={}",
        assets.source_label,
        assets.baked_quads.len(),
        assets.texture_layers.len(),
        assets.animations.len(),
        total_baked_quads
    ));

    // Use a versioned, separate save for the streaming prototype.
    #[cfg(not(feature = "vanilla"))]
    let mut world = HorizonClientWorld::new(PathBuf::from(
        "sdmc:/switch/horizon/saves/switch_streaming_v1",
    ));
    #[cfg(not(feature = "vanilla"))]
    let saved = match world.read_player() {
        Ok(saved) => saved,
        Err(e) => {
            log_line(&format!("FAIL Reading streaming world: {e}"));
            return 3;
        }
    };
    let [spawn_x, spawn_z] = world.spawn_xz();
    let spawn_center = (spawn_x >> 4, spawn_z >> 4);
    #[cfg(not(feature = "vanilla"))]
    let center = saved.map_or(spawn_center, |state| player_chunk(state.0));
    #[cfg(not(feature = "vanilla"))]
    if let Err(e) = update_stream(&mut world, center) {
        log_line(&format!("FAIL Loading initial streaming columns: {e}"));
        return 3;
    }
    #[cfg(feature = "first-person-polish")]
    let mut player = match saved
        .map(|state| state.0)
        .or_else(|| polish::dry_spawn(&world, spawn_x, spawn_z))
    {
        Some(pos) => HorizonLocalPlayer::new(pos),
        None => {
            log_line("FAIL No dry supported initial spawn in loaded terrain");
            return 3;
        }
    };
    #[cfg(not(feature = "first-person-polish"))]
    let mut player =
        HorizonLocalPlayer::new(saved.map_or_else(|| world.spawn_position(), |state| state.0));
    if let Some((pos, yaw, pitch, slot)) = saved {
        player.position = pos;
        player.previous = pos;
        player.yaw = yaw;
        player.yaw_o = yaw;
        player.pitch = pitch.clamp(-90.0, 90.0);
        player.pitch_o = player.pitch;
        let _ = player.inventory.set_selected(slot as i32);
    }
    log_line(&format!(
        "INFO Streaming world ready view_distance={VIEW_DISTANCE} restored={}",
        saved.is_some()
    ));

    // Step initial player physics & raycast verification
    #[cfg(not(feature = "first-person-polish"))]
    player.tick_physics(&world, 0.0, 0.0, false);
    #[cfg(feature = "first-person-polish")]
    {
        player.hand = polish::HandPresentation::new(player.selected_block());
        if polish::intersects_blocks(&world, HorizonLocalPlayer::standing_bounds(player.position)) {
            player.sneaking = true;
            player.camera.eye_height = HorizonLocalPlayer::SNEAKING_EYE_HEIGHT;
            player.camera.eye_height_old = player.camera.eye_height;
        }
        let eye = player.eye_position();
        let eye_block = BlockPos::new(
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        );
        log_line(&format!(
            "SPAWN eye_state={:?} eye_water={} body_water_depth={:.3}",
            world.state_at(eye_block),
            polish::eye_in_water(&world, [eye.x, eye.y, eye.z]),
            polish::water_depth(&world, player.aabb())
        ));
    }
    player.sync_camera(1.0, &mut world);
    let initial_ray = player.raycast_block(&mut world);
    log_line(&format!(
        "PASS Horizon LocalPlayer + Camera + Physics ready: pos=({:.2},{:.2},{:.2}) eye=({:.2},{:.2},{:.2}) hotbar={} ray_hit={}",
        player.position.x,
        player.position.y,
        player.position.z,
        player.eye_position().x,
        player.eye_position().y,
        player.eye_position().z,
        player.selected_block().block_name(),
        initial_ray.is_some()
    ));

    // 3. Build the initial column meshes.
    let mut column_meshes = BTreeMap::new();
    let initial_budget = if cfg!(feature = "vanilla") {
        1
    } else {
        usize::MAX
    };
    world.refresh_meshes(
        &assets,
        &mut column_meshes,
        center,
        player.position.y,
        initial_budget,
    );
    let initial_vertices: usize = column_meshes.values().map(|mesh| mesh.vertices.len()).sum();
    if !cfg!(feature = "vanilla") && initial_vertices == 0 {
        log_line("FAIL Initial multi-chunk mesh is empty");
        return 3;
    }
    log_line(&format!(
        "PASS Horizon initial columns meshed: chunks={} vertices={} triangles={}",
        world.chunks.len(),
        initial_vertices,
        initial_vertices / 3
    ));

    let mut stream_worker = match StreamWorker::new(&world) {
        Ok(worker) => worker,
        Err(e) => {
            log_line(&format!("FAIL Starting chunk worker: {e}"));
            return 19;
        }
    };
    let world_arc = Arc::new(Mutex::new(world));

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
        "PASS wgpu-hal Adapter exposed: name='{}' backend={:?}",
        exposed_adapter.info.name, exposed_adapter.info.backend
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

    // 5. Parse WGSL via Naga and create `wgpu-hal` ShaderModule, Pipeline, Textures, Buffers, BindGroup
    let wgsl = [
        HORIZON_WGSL_SHADER,
        #[cfg(feature = "first-person-polish")]
        POLISH_WGSL,
        #[cfg(feature = "vanilla-water")]
        WATER_WGSL,
        #[cfg(feature = "vanilla-visuals")]
        VISUALS_WGSL,
        #[cfg(feature = "render-diagnostics")]
        DIAGNOSTIC_WGSL,
    ]
    .concat();
    let naga_module = match naga::front::wgsl::parse_str(wgsl.as_ref()) {
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
                label: Some("horizon_client_wgsl"),
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

    let bind_group_layout = unsafe {
        device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("horizon_client_bgl"),
                flags: BindGroupLayoutFlags::empty(),
                entries: &bgl_entries,
            })
            .expect("create_bind_group_layout")
    };

    let pipeline_layout = unsafe {
        device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("horizon_client_pipeline_layout"),
                flags: PipelineLayoutFlags::empty(),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            })
            .expect("create_pipeline_layout")
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

    let make_pipeline = |label, vertex_entry, fragment_entry, blend, write_depth| unsafe {
        device
            .create_render_pipeline(&RenderPipelineDescriptor {
                label: Some(label),
                layout: &pipeline_layout,
                vertex_processor: VertexProcessor::Standard {
                    vertex_buffers: &vertex_buffer_layouts,
                    vertex_stage: ProgrammableStage {
                        module: &shader_module,
                        entry_point: vertex_entry,
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
                    depth_write_enabled: Some(write_depth),
                    depth_compare: Some(wgpu_types::CompareFunction::LessEqual),
                    stencil: wgpu_types::StencilState::default(),
                    bias: wgpu_types::DepthBiasState::default(),
                }),
                multisample: wgpu_types::MultisampleState::default(),
                fragment_stage: Some(ProgrammableStage {
                    module: &shader_module,
                    entry_point: fragment_entry,
                    constants: &pipeline_constants,
                    zero_initialize_workgroup_memory: true,
                }),
                color_targets: &[Some(wgpu_types::ColorTargetState {
                    format: surface_format,
                    blend,
                    write_mask: wgpu_types::ColorWrites::ALL,
                })],
                multiview_mask: None,
                cache: None,
            })
            .expect("create_render_pipeline")
    };
    let render_pipeline =
        make_pipeline("horizon_client_pipeline", "vs_main", "fs_main", None, true);
    let hand_entry = if cfg!(feature = "first-person-polish") {
        "fs_hand_polished"
    } else {
        "fs_hand"
    };
    let hand_pipeline = make_pipeline("horizon_hand_pipeline", "vs_hand", hand_entry, None, true);
    #[cfg(feature = "vanilla-water")]
    let water_pipeline = make_pipeline(
        "horizon water",
        "vs_main",
        "fs_water",
        Some(wgpu_types::BlendState::ALPHA_BLENDING),
        true,
    );
    #[cfg(feature = "vanilla-visuals")]
    let effect_pipeline = make_pipeline(
        "horizon effects",
        "vs_main",
        "fs_effect",
        Some(wgpu_types::BlendState::ALPHA_BLENDING),
        false,
    );
    #[cfg(feature = "render-diagnostics")]
    let probe_pipelines = [
        make_pipeline("probe solid", "vs_main", "fs_probe_solid", None, true),
        make_pipeline("probe sampler", "vs_main", "fs_probe_sample", None, true),
        make_pipeline(
            "probe direct texel",
            "vs_main",
            "fs_probe_texel",
            None,
            true,
        ),
        make_pipeline(
            "probe fixed texel",
            "vs_main",
            "fs_probe_fixed_texel",
            None,
            true,
        ),
    ];

    let layer_count = assets.texture_layers.len() as u32;
    let make_atlas = |label| unsafe {
        let texture = device
            .create_texture_2d_array(&TextureDescriptor {
                label: Some(label),
                size: wgpu_types::Extent3d {
                    width: ATLAS_SIDE,
                    height: ATLAS_SIDE,
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
            .expect("create_texture_2d_array(atlas)");
        let view = device
            .create_texture_view(
                &texture,
                &TextureViewDescriptor {
                    label: Some("horizon_client_atlas_view"),
                    format: wgpu_types::TextureFormat::Rgba8Unorm,
                    dimension: wgpu_types::TextureViewDimension::D2Array,
                    usage: wgpu_types::TextureUses::RESOURCE,
                    range: wgpu_types::ImageSubresourceRange::default(),
                },
            )
            .expect("create_texture_view(atlas)");
        (texture, view)
    };
    let (atlas_texture, atlas_view) = make_atlas("horizon_client_atlas_2d_array");
    #[cfg(feature = "render-diagnostics")]
    let (control_texture, control_view) = make_atlas("horizon_atlas_control");
    let sampler = unsafe {
        device
            .create_sampler(&SamplerDescriptor {
                label: Some("horizon_client_nearest_sampler"),
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

    let depth_texture = unsafe {
        device
            .create_texture(&TextureDescriptor {
                label: Some("horizon_client_depth_texture"),
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
                    label: Some("horizon_client_depth_view"),
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

    let camera_buffer = unsafe {
        device
            .create_buffer(&BufferDescriptor {
                label: Some("horizon_client_camera_ubo"),
                size: std::mem::size_of::<CameraUniform>() as u64,
                usage: wgpu_types::BufferUses::COPY_DST | wgpu_types::BufferUses::UNIFORM,
                memory_flags: MemoryFlags::empty(),
            })
            .expect("create_buffer(camera)")
    };
    let vertex_buffer = unsafe {
        device
            .create_buffer(&BufferDescriptor {
                label: Some("horizon_client_terrain_vbo"),
                size: MAX_VERTEX_BUFFER_BYTES,
                usage: wgpu_types::BufferUses::COPY_DST | wgpu_types::BufferUses::VERTEX,
                memory_flags: MemoryFlags::empty(),
            })
            .expect("create_buffer(vertex)")
    };
    let staging_buffer = unsafe {
        device
            .create_buffer(&BufferDescriptor {
                label: Some("horizon_client_staging_buffer"),
                size: STAGING_BUFFER_BYTES,
                usage: wgpu_types::BufferUses::MAP_WRITE | wgpu_types::BufferUses::COPY_SRC,
                memory_flags: MemoryFlags::empty(),
            })
            .expect("create_buffer(staging)")
    };

    let make_atlas_group = |view| unsafe {
        device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("horizon_client_bg"),
                layout: &bind_group_layout,
                buffers: &[BufferBinding::new_unchecked(
                    &camera_buffer,
                    0,
                    NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64),
                )],
                samplers: &[&sampler],
                textures: &[TextureBinding {
                    view,
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
    let bind_group = make_atlas_group(&atlas_view);
    #[cfg(feature = "render-diagnostics")]
    let control_group = make_atlas_group(&control_view);

    let mut encoder = unsafe {
        device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("horizon_client_cmd_encoder"),
                queue: &queue,
            })
            .expect("create_command_encoder")
    };
    let mut fence = unsafe { device.create_fence().expect("create_fence") };
    let mut fence_value: u64 = 1;

    // Upload all texture layers once
    let mut init_upload_bytes = Vec::new();
    let mut tex_copies = Vec::with_capacity(assets.texture_layers.len());
    for (layer_idx, layer_bytes) in assets.texture_layers.iter().enumerate() {
        let offset = init_upload_bytes.len() as u64;
        init_upload_bytes.extend_from_slice(layer_bytes);
        tex_copies.push(BufferTextureCopy {
            buffer_layout: wgpu_types::TexelCopyBufferLayout {
                offset,
                bytes_per_row: Some(ATLAS_SIDE * 4),
                rows_per_image: Some(ATLAS_SIDE),
            },
            texture_base: TextureCopyBase {
                mip_level: 0,
                array_layer: layer_idx as u32,
                origin: wgpu_types::Origin3d::ZERO,
                aspect: FormatAspects::COLOR,
            },
            size: CopyExtent {
                width: ATLAS_SIDE,
                height: ATLAS_SIDE,
                depth: 1,
            },
        });
    }

    #[cfg(feature = "render-diagnostics")]
    let control_copy = {
        let mut copy = tex_copies[0].clone();
        copy.buffer_layout.offset = init_upload_bytes.len() as u64;
        init_upload_bytes
            .extend_from_slice(&PROBE_CONTROL_TEXEL.repeat((ATLAS_SIDE * ATLAS_SIDE) as usize));
        copy
    };
    if let Err(e) = unsafe { write_staging_bytes(&device, &staging_buffer, &init_upload_bytes) } {
        log_line(&format!("FAIL Initial staging buffer write failed: {e}"));
        return 15;
    }

    unsafe {
        encoder
            .begin_encoding(Some("horizon_client_init_upload"))
            .expect("begin_encoding(init)");
        encoder.copy_buffer_to_texture(&staging_buffer, &atlas_texture, tex_copies.into_iter());
        #[cfg(feature = "render-diagnostics")]
        encoder.copy_buffer_to_texture(
            &staging_buffer,
            &control_texture,
            std::iter::once(control_copy),
        );
        let cmd_buf = encoder.end_encoding().expect("end_encoding(init)");
        queue
            .submit(&[&cmd_buf], &[], (&mut fence, fence_value))
            .expect("queue.submit(init)");
        fence_value += 1;
        encoder.reset_all(std::iter::once(cmd_buf));
    }

    log_line(&format!(
        "PASS wgpu-hal RenderPipeline, D2Array Texture ({} layers), Depth24Plus, Buffers, and BindGroup ready",
        layer_count
    ));
    #[cfg(feature = "render-diagnostics")]
    {
        let mut mismatches = 0;
        let mut first_error = None;
        for (layer, expected) in assets.texture_layers.iter().enumerate() {
            let mut actual = [0u8; ATLAS_LAYER_BYTES];
            let (status, error) =
                device.diagnostic_atlas_layer(&atlas_texture, layer as i32, &mut actual);
            if actual.as_slice() != expected || status != 0x8cd5 || error != 0 {
                mismatches += 1;
                first_error.get_or_insert((layer, status, error));
            }
        }
        log_line(&format!(
            "GLES_ATLAS layers={layer_count} mismatches={mismatches} first_error={first_error:x?}"
        ));
    }

    // 6. Spawn background 20 TPS "Server thread"
    let running = Arc::new(AtomicBool::new(true));
    let server_ticks = Arc::new(AtomicU64::new(0));
    let running_srv = Arc::clone(&running);
    let ticks_srv = Arc::clone(&server_ticks);
    #[cfg(not(feature = "vanilla"))]
    let world_srv = Arc::clone(&world_arc);

    let server_handle = match thread::Builder::new()
        .name("Server thread".into())
        .spawn(move || {
            while running_srv.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
                let _t = ticks_srv.fetch_add(1, Ordering::Relaxed) + 1;
                #[cfg(not(feature = "vanilla"))]
                if _t % 20 == 0 {
                    if let Ok(mut guard) = world_srv.lock() {
                        let beacon_pos = guard.beacon_pos;
                        let cur = guard.get_block(beacon_pos);
                        if cur == VoxelBlock::SeaLantern || cur == VoxelBlock::GoldBlock {
                            let next_blk = if (_t / 20) % 2 == 0 {
                                VoxelBlock::SeaLantern
                            } else {
                                VoxelBlock::GoldBlock
                            };
                            let _ = guard.set_block(beacon_pos, next_blk);
                        }
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
    log_line(
        "INFO Controls: Left Stick=Walk/Strafe (L3=Sprint) | Right Stick=Look (R3=Sneak) | B=Jump | ZR=Break | A/ZL=Place | L/R=Hotbar | DPadUp=Camera | X=Save | Y=Respawn | PLUS=Exit",
    );

    // 7. Interactive First-Person Client Loop
    let mut prev_pad = PadState::default();
    let mut frames_rendered: u64 = 0;
    let mut result = 0;
    let mut manual_saves: u32 = 0;
    let mut save_requested = false;
    let mut respawn_requested = false;
    let mut last_frame = Instant::now();
    let mut physics_accum = 0.0f32;
    #[cfg(feature = "vanilla-visuals")]
    let (mut effects, mut mining) = (effects::Effects::default(), effects::Mining::default());
    #[cfg(feature = "vanilla-visuals")]
    let mut biome_sky = [0.47, 0.65, 1.0];
    #[cfg(feature = "vanilla-visuals")]
    let mut was_in_water = false;
    #[cfg(feature = "first-person-polish")]
    let mut water_view = polish::WaterView::default();
    let mut frame_vertices: Vec<TerrainVertex> = Vec::with_capacity(65536);
    let mut frame_staging = Vec::new();
    let mut max_frame_us = 0;
    let mut slow_frames = 0;
    let mut max_mesh_us = 0;
    let mut interval_samples = 0;
    let mut interval_total_us = 0;
    let mut frame_gaps_over_1s = 0;
    let mut max_upload_us = 0;
    let mut max_upload_bytes = 0;
    #[cfg(feature = "vanilla")]
    let mut gpu_sections = mesh_upload::SectionUploads::default();
    let mut peak_mesh_bytes = 0;

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
        let frame_interval = now - last_frame;
        if frames_rendered > 0 && frame_interval > Duration::from_secs(1) {
            frame_gaps_over_1s += 1;
        } else if frames_rendered > 0 {
            let us = frame_interval.as_micros();
            max_frame_us = max_frame_us.max(us);
            slow_frames += u64::from(us > 33_333);
            interval_samples += 1;
            interval_total_us += us;
        }
        let dt = if frame_interval > Duration::from_secs(1) {
            physics_accum = 0.0;
            0.0
        } else {
            frame_interval.as_secs_f32().min(0.1)
        };
        last_frame = now;

        let pad = PadState::from_switch_hid(k_held, lx, ly, rx, ry, &pad_settings);
        let edges = pad.edges(&prev_pad);

        // Camera Look (Right Stick)
        let (dyaw, dpitch) = pad.look_delta(dt, &pad_settings);
        player.yaw = (player.yaw + dyaw).rem_euclid(360.0);
        // look_delta already converts positive stick Y to negative Horizon pitch.
        player.pitch = (player.pitch + dpitch).clamp(-90.0, 90.0);

        // Hotbar Slot Selection (L / R Bumpers)
        let step = pad.hotbar_step(&prev_pad);
        if step != 0 {
            let cur = player.inventory.selected() as i32;
            let next = (cur + step).rem_euclid(9);
            let _ = player.inventory.set_selected(next);
            log_line(&format!(
                "EVENT Hotbar slot {} -> {:?}",
                next,
                player.selected_block()
            ));
        }

        #[cfg(feature = "first-person-polish")]
        let was_sneaking = player.sneaking;
        // L3 toggles Sprint, R3 toggles Sneak/Crouch
        if edges.pressed(PadButton::LeftThumb) {
            player.sprinting = !player.sprinting;
            if player.sprinting {
                player.sneaking = false;
            }
        }
        if edges.pressed(PadButton::RightThumb) {
            player.sneaking = !player.sneaking;
            if player.sneaking {
                player.sprinting = false;
            }
        }

        // D-Pad Up cycles FirstPerson / ThirdPersonBack / ThirdPersonFront
        if edges.pressed(PadButton::DPadUp) {
            player.camera_type = player.camera_type.cycle();
            log_line(&format!(
                "EVENT Camera perspective: {:?}",
                player.camera_type
            ));
        }

        let mut targeted_hit: Option<BlockHit> = None;

        if let Ok(mut guard) = world_arc.lock() {
            #[cfg(feature = "first-person-polish")]
            if was_sneaking
                && !player.sneaking
                && polish::intersects_blocks(
                    &guard,
                    HorizonLocalPlayer::standing_bounds(player.position),
                )
            {
                player.sneaking = true;
                player.sprinting = false;
            }
            match stream_worker.poll() {
                Ok(Some(prepared)) if prepared.manual_save => {
                    if let Some(change) = guard.apply_stream(prepared) {
                        manual_saves += 1;
                        log_line(&format!(
                            "EVENT Saved {} edited columns save_us={} manual_saves={manual_saves}",
                            change.saved, change.save_us
                        ));
                    }
                }
                Ok(Some(prepared))
                    if prepared.center == player_chunk(player.position)
                        || (cfg!(feature = "vanilla")
                            && respawn_requested
                            && prepared.center == spawn_center) =>
                {
                    let center = prepared.center;
                    let worker_us = prepared.elapsed.as_micros();
                    let apply_start = Instant::now();
                    if let Some(change) = guard.apply_stream(prepared) {
                        log_line(&format!(
                            "STREAM_ASYNC center=({},{}) resident={} generated={} read={} evicted={} saved={} worker_us={} apply_us={}",
                            center.0,
                            center.1,
                            guard.chunks.len(),
                            change.generated,
                            change.read,
                            change.evicted,
                            change.saved,
                            worker_us,
                            apply_start.elapsed().as_micros()
                        ));
                        log_stream_metrics(&change);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    log_line(&format!("FAIL Streaming columns: {e}"));
                    result = 19;
                    break;
                }
            }

            respawn_requested |= edges.pressed(PadButton::West);
            save_requested |= edges.pressed(PadButton::North);
            #[cfg(feature = "vanilla")]
            {
                respawn_requested |= player.position.y < f64::from(MIN_Y - 12);
            }
            if respawn_requested && !stream_worker.busy() {
                #[cfg(not(feature = "vanilla"))]
                if let Err(e) = update_stream(&mut guard, spawn_center) {
                    log_line(&format!("FAIL Loading spawn columns: {e}"));
                    result = 19;
                    break;
                }
                #[cfg(feature = "vanilla")]
                let ready = guard.current_center() == Some(spawn_center);
                #[cfg(not(feature = "vanilla"))]
                let ready = true;
                if ready {
                    respawn_requested = false;
                    #[cfg(not(feature = "first-person-polish"))]
                    {
                        player.position = guard.spawn_position();
                    }
                    #[cfg(feature = "first-person-polish")]
                    if let Some(pos) = polish::dry_spawn(&guard, spawn_x, spawn_z) {
                        player.position = pos;
                    } else {
                        log_line("FAIL No dry supported recovery position near world spawn");
                        result = 19;
                        break;
                    }
                    player.previous = player.position;
                    player.velocity = Vec3::ZERO;
                    player.yaw = 180.0;
                    player.pitch = 10.0;
                    log_line("EVENT Player respawned at spawn point");
                }
            }

            if save_requested && !stream_worker.busy() {
                save_requested = false;
                let snapshot = (
                    player.position,
                    player.yaw,
                    player.pitch,
                    player.inventory.selected(),
                );
                #[cfg(feature = "vanilla")]
                if let Err(e) = stream_worker.request_save(&guard, snapshot) {
                    log_line(&format!("FAIL Scheduling manual save: {e}"));
                    result = 19;
                    break;
                }
                #[cfg(not(feature = "vanilla"))]
                match guard.save(snapshot) {
                    Ok(chunks) => {
                        manual_saves += 1;
                        log_line(&format!(
                            "EVENT Saved {chunks} edited columns (manual_saves={manual_saves})"
                        ));
                    }
                    Err(e) => {
                        log_line(&format!("FAIL Manual save: {e}"));
                        result = 19;
                        break;
                    }
                }
            }

            #[cfg(feature = "vanilla-visuals")]
            let mut steps = 0;
            // Step 20 Hz player physics
            physics_accum = (physics_accum + dt).min(0.20);
            while physics_accum >= 0.05 {
                physics_accum -= 0.05;
                if cfg!(feature = "vanilla") && respawn_requested {
                    break;
                }
                let before = player.position;
                if !guard.chunks.contains_key(&player_chunk(before)) {
                    break;
                }
                player.tick_physics(&guard, pad.move_y, -pad.move_x, pad.down(PadButton::South));
                #[cfg(feature = "vanilla-visuals")]
                {
                    steps += 1;
                    effects.tick();
                    let depth = polish::water_depth(&guard, player.aabb());
                    if (depth > 0.0) != was_in_water {
                        let [x, y, z] = [player.position.x, player.position.y, player.position.z];
                        effects.water(
                            &assets,
                            GlamVec3::new(x as f32, (y + depth) as f32, z as f32),
                            false,
                        );
                        if depth > 0.0 {
                            effects.water(
                                &assets,
                                GlamVec3::new(x as f32, y as f32, z as f32),
                                true,
                            );
                        }
                        was_in_water = depth > 0.0;
                    }
                }
                // A slow read must not let the player walk into unloaded air.
                // The fall recovery can teleport to spawn and waits there for its view.
                if !guard.chunks.contains_key(&player_chunk(player.position))
                    && player.previous == before
                {
                    player.position = before;
                    player.velocity = Vec3::ZERO;
                }
            }

            let partial_tick = (physics_accum / 0.05).clamp(0.0, 1.0);
            let center = player_chunk(player.position);
            let requested_center = if cfg!(feature = "vanilla") && respawn_requested {
                spawn_center
            } else {
                center
            };
            let was_busy = stream_worker.busy();
            if let Err(e) = stream_worker.request(&mut guard, requested_center) {
                log_line(&format!("FAIL Requesting streaming columns: {e}"));
                result = 19;
                break;
            }
            if cfg!(feature = "vanilla") && !was_busy && stream_worker.awaiting != 0 {
                log_line(&format!(
                    "STREAM_PENDING center={requested_center:?} resident={} awaiting={}",
                    guard.chunks.len(),
                    stream_worker.awaiting
                ));
            }
            #[cfg(feature = "first-person-polish")]
            {
                // Stick look is already integrated per frame. Interpolate movement, not this input twice.
                player.yaw_o = player.yaw;
                player.pitch_o = player.pitch;
            }
            player.sync_camera(partial_tick, &mut guard);
            #[cfg(feature = "first-person-polish")]
            water_view.update(&guard, player.camera.position, dt);

            // Voxel raycast from player eyes
            targeted_hit = player.raycast_block(&mut guard);

            let attack_edge =
                edges.pressed(PadButton::RightTrigger) || edges.pressed(PadButton::DPadLeft);
            #[cfg(not(feature = "vanilla-visuals"))]
            let break_now = attack_edge;
            #[cfg(feature = "vanilla-visuals")]
            let break_now = {
                let eye = player.camera.position.map(|v| v.floor() as i32);
                if let Some(biome) =
                    visuals::biome_at(&guard, BlockPos::new(eye[0], eye[1], eye[2]))
                {
                    biome_sky = std::array::from_fn(|i| {
                        camera::lerp((dt * 3.0).min(1.0), biome_sky[i], biome.sky[i])
                    });
                }
                let hit = targeted_hit.filter(|_| pad.down(PadButton::RightTrigger));
                let target = hit.and_then(|hit| {
                    let id = guard.state_at(hit.block)?;
                    Some((
                        (hit.block, id, player.selected_block()),
                        guard.vanilla.visual_states.get(id.0 as usize)?,
                    ))
                });
                if (attack_edge || steps > 0 && hit.is_some())
                    && let Some(hit) = targeted_hit
                {
                    effects.block(&guard, &assets, hit.block, hit.direction, false);
                }
                let slow = if player.on_ground { 1.0 } else { 5.0 }
                    * if water_view.blend > 0.5 { 5.0 } else { 1.0 };
                mining.update(target, steps, slow, hit.map(|h| guard.get_block(h.block)))
                    || edges.pressed(PadButton::DPadLeft)
            };
            if attack_edge {
                #[cfg(feature = "vanilla-visuals")]
                if let Some(hit) = targeted_hit
                    && let Some(id) = guard.state_at(hit.block)
                {
                    let state = guard.vanilla.generator.data().registry().state(id).unwrap();
                    let visual = guard.get_block(hit.block);
                    let quads = assets
                        .state_quads
                        .get(&id.0)
                        .or_else(|| assets.baked_quads.get(&visual));
                    let materials: Vec<_> = quads
                        .into_iter()
                        .flatten()
                        .map(|quad| {
                            (
                                &quad.material.sprite,
                                assets.sprite_to_layer.get(&quad.material.sprite),
                            )
                        })
                        .collect();
                    log_line(&format!(
                        "BLOCK_MODEL pos={:?} state={} name={} properties={:?} visual={visual:?} materials={materials:?}",
                        hit.block, id.0, state.name, state.properties
                    ));
                }
                #[cfg(feature = "first-person-polish")]
                player.hand.attack();
            }
            if break_now {
                #[cfg(not(feature = "first-person-polish"))]
                {
                    player.hand_swing = 1.0;
                }
                if let Some(hit) = targeted_hit {
                    if hit.block.y > MIN_Y && guard.get_block(hit.block) != VoxelBlock::Bedrock {
                        #[cfg(feature = "vanilla-visuals")]
                        effects.block(&guard, &assets, hit.block, hit.direction, true);
                        if guard.set_block(hit.block, VoxelBlock::Air) {
                            #[cfg(feature = "vanilla-visuals")]
                            {
                                mining = effects::Mining::default();
                            }
                            player.blocks_broken += 1;
                            log_line(&format!(
                                "EVENT Broke block at ({}, {}, {})",
                                hit.block.x, hit.block.y, hit.block.z
                            ));
                        }
                    }
                }
            }

            // Place block (A or ZL)
            if edges.pressed(PadButton::East) || edges.pressed(PadButton::LeftTrigger) {
                #[cfg(not(feature = "first-person-polish"))]
                {
                    player.hand_swing = 1.0;
                }
                if let Some(hit) = targeted_hit {
                    let (dx, dy, dz) = direction_offset(hit.direction);
                    let place_pos = hit.block.offset(dx, dy, dz);
                    let block_box = Aabb::block(place_pos).deflate(Vec3::new(0.001, 0.001, 0.001));
                    if !player.aabb().intersects(block_box) {
                        let chosen = player.selected_block();
                        if guard.set_block(place_pos, chosen) {
                            player.blocks_placed += 1;
                            #[cfg(feature = "first-person-polish")]
                            player.hand.place();
                            log_line(&format!(
                                "EVENT Placed {:?} at ({}, {}, {})",
                                chosen, place_pos.x, place_pos.y, place_pos.z
                            ));
                        }
                    }
                }
            }

            let mesh_start = Instant::now();
            if guard.refresh_meshes(&assets, &mut column_meshes, center, player.position.y, 1) != 0
            {
                max_mesh_us = max_mesh_us.max(mesh_start.elapsed().as_micros());
                peak_mesh_bytes = peak_mesh_bytes.max(
                    column_meshes
                        .values()
                        .map(|mesh| mesh.vertices.capacity() * std::mem::size_of::<TerrainVertex>())
                        .sum::<usize>(),
                );
            }
        }

        prev_pad = pad;

        let tick = server_ticks.load(Ordering::Relaxed);
        let day_angle = (tick as f32) * 0.012;
        let sun_y = (day_angle.cos() * 0.42 + 0.68).clamp(0.25, 1.0);
        let sun_dir = GlamVec3::new(0.45, sun_y, 0.55).normalize();
        #[cfg(feature = "vanilla-visuals")]
        let [sky_r, sky_g, sky_b] = biome_sky.map(|v| v * sun_y);
        #[cfg(not(feature = "vanilla-visuals"))]
        let sky_r = 0.36 * sun_y + 0.08;
        #[cfg(not(feature = "vanilla-visuals"))]
        let sky_g = 0.58 * sun_y + 0.12;
        #[cfg(not(feature = "vanilla-visuals"))]
        let sky_b = 0.88 * sun_y + 0.18;

        // Build View-Projection Matrix using real `camera::view_matrix` + `AvatarBob`
        let partial_tick = (physics_accum / 0.05).clamp(0.0, 1.0);
        #[cfg(feature = "first-person-polish")]
        let base_fov = pad_settings.fov;
        #[cfg(not(feature = "first-person-polish"))]
        let base_fov = 70;
        let fov_deg = player.camera.calculate_fov(
            partial_tick,
            base_fov,
            1.0,
            None,
            camera::CameraFluid::None,
            false,
        );
        #[cfg(feature = "first-person-polish")]
        let fov_deg = camera::lerp(
            water_view.blend,
            fov_deg,
            player.camera.calculate_fov(
                partial_tick,
                base_fov,
                1.0,
                None,
                camera::CameraFluid::Water,
                false,
            ),
        );
        let proj = Mat4::perspective_rh(fov_deg.to_radians(), 1280.0 / 720.0, 0.05, 120.0);
        let bob_mat = if !player.camera.detached {
            let state = camera::BobState {
                living: None,
                player: Some((
                    player
                        .bob
                        .backwards_interpolated_walk_distance(partial_tick),
                    player.bob.interpolated_bob(partial_tick),
                )),
            };
            camera::bob_matrix(&state, 1.0, true).to_glam()
        } else {
            Mat4::IDENTITY
        };
        let [cam_x, cam_y, cam_z] = player.camera.position.map(|coord| coord as f32);
        let view_rot = camera::view_matrix(player.camera.y_rot, player.camera.x_rot);
        let view =
            bob_mat * view_rot * Mat4::from_translation(GlamVec3::new(-cam_x, -cam_y, -cam_z));
        let vp = proj * view;
        let inv_vp = vp.inverse();

        let controlled = cfg!(feature = "render-diagnostics") && frames_rendered < 2;
        #[cfg(feature = "first-person-polish")]
        let fog = water_view.uniform();
        #[cfg(not(feature = "first-person-polish"))]
        let fog = [0.0; 4];
        let sky = std::array::from_fn::<_, 3, _>(|i| {
            camera::lerp(fog[3], [sky_r, sky_g, sky_b][i], fog[i])
        });
        let mut cam_ubo = CameraUniform {
            view_proj: vp.to_cols_array_2d(),
            hand_projection: Mat4::from_cols_array(&camera::hand_projection(
                70.0,
                SCREEN_WIDTH as f32,
                SCREEN_HEIGHT as f32,
                120.0,
            ))
            .to_cols_array_2d(),
            sky_color: [sky_r, sky_g, sky_b, 1.0],
            sun_dir: [
                sun_dir.x,
                sun_dir.y,
                sun_dir.z,
                if cfg!(feature = "first-person-polish") {
                    1.0
                } else {
                    0.0
                },
            ],
            camera_pos: [cam_x, cam_y, cam_z, 1.0],
            water_fog: fog,
        };
        if controlled {
            cam_ubo.view_proj = Mat4::IDENTITY.to_cols_array_2d();
            cam_ubo.sun_dir = [0.0, 1.0, 0.0, 0.0];
            cam_ubo.camera_pos = [0.0, 0.0, 0.0, 1.0];
        }

        // Share the upload buffer while keeping terrain, hand, and HUD draw ranges separate.
        frame_vertices.clear();
        #[cfg(not(feature = "vanilla"))]
        for mesh in column_meshes.values() {
            frame_vertices.extend_from_slice(&mesh.vertices);
        }
        if !controlled && let Some(hit) = targeted_hit {
            append_target_highlight(&mut frame_vertices, hit.block, assets.white_layer);
        }
        let world_end = frame_vertices.len() as u32;
        #[cfg(feature = "vanilla-visuals")]
        if !controlled {
            mining.vertices(&assets, &mut frame_vertices);
            let camera_basis = view_rot.transpose();
            effects.vertices(
                &assets,
                &mut frame_vertices,
                GlamVec3::new(cam_x, cam_y, cam_z),
                camera_basis.transform_vector3(GlamVec3::X),
                camera_basis.transform_vector3(GlamVec3::Y),
                partial_tick,
            );
        }
        let effects_end = frame_vertices.len() as u32;
        if !controlled && !player.camera.detached {
            append_first_person_hand(
                &mut frame_vertices,
                #[cfg(not(feature = "first-person-polish"))]
                player.selected_block(),
                #[cfg(feature = "first-person-polish")]
                player.hand.visible,
                &assets,
                #[cfg(not(feature = "first-person-polish"))]
                &player.bob,
                #[cfg(not(feature = "first-person-polish"))]
                partial_tick,
                #[cfg(not(feature = "first-person-polish"))]
                player.hand_swing,
                #[cfg(feature = "first-person-polish")]
                player.hand.swing.animation(partial_tick),
                #[cfg(feature = "first-person-polish")]
                (
                    bob_mat,
                    1.0 - player.hand.heights.interpolated(partial_tick)[0],
                ),
            );
        }
        let hand_end = frame_vertices.len() as u32;
        if !controlled {
            append_hud_overlay(
                &mut frame_vertices,
                inv_vp,
                player.inventory.selected(),
                &assets,
            );
        }
        #[cfg(feature = "render-diagnostics")]
        if frames_rendered == 1 {
            probe_panels(&mut frame_vertices, assets.white_layer);
        }

        frame_staging.clear();
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
                        bytes_per_row: Some(ATLAS_SIDE * 4),
                        rows_per_image: Some(ATLAS_SIDE),
                    },
                    texture_base: TextureCopyBase {
                        mip_level: 0,
                        array_layer: anim.layer_index,
                        origin: wgpu_types::Origin3d::ZERO,
                        aspect: FormatAspects::COLOR,
                    },
                    size: CopyExtent {
                        width: ATLAS_SIDE,
                        height: ATLAS_SIDE,
                        depth: 1,
                    },
                });
            }
        }

        #[cfg(feature = "render-diagnostics")]
        if matches!(frames_rendered, 1 | 2) {
            let offset = frame_staging.len() as u64;
            if frames_rendered == 1 {
                frame_staging.extend_from_slice(&probe_texture(ATLAS_SIDE as usize));
            } else {
                frame_staging
                    .extend_from_slice(&assets.texture_layers[assets.white_layer as usize]);
            }
            anim_copies.push(BufferTextureCopy {
                buffer_layout: wgpu_types::TexelCopyBufferLayout {
                    offset,
                    bytes_per_row: Some(ATLAS_SIDE * 4),
                    rows_per_image: Some(ATLAS_SIDE),
                },
                texture_base: TextureCopyBase {
                    mip_level: 0,
                    array_layer: assets.white_layer,
                    origin: wgpu_types::Origin3d::ZERO,
                    aspect: FormatAspects::COLOR,
                },
                size: CopyExtent {
                    width: ATLAS_SIDE,
                    height: ATLAS_SIDE,
                    depth: 1,
                },
            });
        }
        let v_bytes = bytemuck::cast_slice(&frame_vertices);
        let mut mesh_copy = None;
        let mut vertex_count = 0u32;
        if v_bytes.len() as u64 > MAX_VERTEX_BUFFER_BYTES
            || (frame_staging.len() + v_bytes.len()) as u64 > STAGING_BUFFER_BYTES
        {
            log_line("FAIL Frame exceeds the GPU upload buffer capacity");
            result = 17;
            break;
        }
        if !v_bytes.is_empty() {
            let offset = frame_staging.len() as u64;
            frame_staging.extend_from_slice(v_bytes);
            mesh_copy = Some(BufferCopy {
                src_offset: offset,
                dst_offset: 0,
                size: NonZeroU64::new(v_bytes.len() as u64).unwrap(),
            });
            vertex_count = frame_vertices.len() as u32;
        }

        #[cfg(feature = "vanilla")]
        let section_copy = match unsafe {
            gpu_sections.stage(
                &device,
                &column_meshes,
                &mut frame_staging,
                player_chunk(player.position),
                (cam_y.floor() as i32 - MIN_Y).div_euclid(MESH_HEIGHT),
            )
        } {
            Ok(copy) => copy,
            Err(e) => {
                log_line(&format!("FAIL Section upload allocation: {e:?}"));
                result = 17;
                break;
            }
        };

        unsafe {
            let upload_start = Instant::now();
            if let Err(e) = write_staging_bytes(&device, &staging_buffer, &frame_staging) {
                log_line(&format!("FAIL Frame upload: {e}"));
                result = 17;
                break;
            }
            max_upload_us = max_upload_us.max(upload_start.elapsed().as_micros());
            max_upload_bytes = max_upload_bytes.max(frame_staging.len());

            let acquired = match surface.acquire_texture(None, &fence) {
                Ok(a) => a,
                Err(e) => {
                    log_line(&format!("FAIL surface.acquire_texture failed: {e:?}"));
                    result = 17;
                    break;
                }
            };
            let frame_view = device
                .create_texture_view(
                    &acquired.texture,
                    &TextureViewDescriptor {
                        label: Some("horizon_client_swapchain_view"),
                        format: surface_format,
                        dimension: wgpu_types::TextureViewDimension::D2,
                        usage: wgpu_types::TextureUses::COLOR_TARGET,
                        range: wgpu_types::ImageSubresourceRange::default(),
                    },
                )
                .expect("create_texture_view(swapchain)");

            encoder
                .begin_encoding(Some("horizon_client_frame"))
                .expect("begin_encoding(frame)");

            encoder.copy_buffer_to_buffer(
                &staging_buffer,
                &camera_buffer,
                std::iter::once(BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64).unwrap(),
                }),
            );
            if !anim_copies.is_empty() {
                encoder.copy_buffer_to_texture(
                    &staging_buffer,
                    &atlas_texture,
                    anim_copies.into_iter(),
                );
            }
            if let Some(copy) = mesh_copy {
                encoder.copy_buffer_to_buffer(
                    &staging_buffer,
                    &vertex_buffer,
                    std::iter::once(copy),
                );
            }
            #[cfg(feature = "vanilla")]
            if let Some((key, copy)) = section_copy {
                encoder.copy_buffer_to_buffer(
                    &staging_buffer,
                    &gpu_sections.buffer(key),
                    std::iter::once(copy),
                );
            }

            let clear = sky.map(f64::from);
            #[cfg(feature = "render-diagnostics")]
            let clear = if controlled { PROBE_SKY } else { clear };
            let color = ColorAttachment {
                target: Attachment {
                    view: &frame_view,
                    usage: wgpu_types::TextureUses::COLOR_TARGET,
                },
                depth_slice: None,
                resolve_target: None,
                ops: AttachmentOps::LOAD_CLEAR | AttachmentOps::STORE,
                clear_value: wgpu_types::Color {
                    r: clear[0],
                    g: clear[1],
                    b: clear[2],
                    a: 1.0,
                },
            };
            let world_color = [Some(color.clone())];
            let mut pass = RenderPassDescriptor::<
                <GlesApi as HalApi>::QuerySet,
                <GlesApi as HalApi>::TextureView,
            > {
                label: Some("horizon_client_main_pass"),
                extent: wgpu_types::Extent3d {
                    width: SCREEN_WIDTH,
                    height: SCREEN_HEIGHT,
                    depth_or_array_layers: 1,
                },
                sample_count: 1,
                color_attachments: &world_color,
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
            };
            encoder.begin_render_pass(&pass).expect("begin_render_pass");

            encoder.set_render_pipeline(&render_pipeline);
            encoder.set_bind_group(&pipeline_layout, 0, &bind_group, &[]);
            #[cfg(feature = "vanilla")]
            if !controlled {
                gpu_sections.draw(&mut encoder, vp);
            }
            encoder.set_vertex_buffer(0, BufferBinding::new_unchecked(&vertex_buffer, 0, None));
            if world_end > 0 {
                encoder.draw(0, world_end, 0, 1);
            }
            #[cfg(feature = "vanilla-water")]
            if !controlled {
                encoder.set_render_pipeline(&water_pipeline);
                encoder.set_bind_group(&pipeline_layout, 0, &bind_group, &[]);
                gpu_sections.draw_water(&mut encoder, vp, [cam_x, cam_y, cam_z]);
            }
            #[cfg(feature = "vanilla-visuals")]
            if effects_end > world_end {
                encoder.set_render_pipeline(&effect_pipeline);
                encoder.set_vertex_buffer(0, BufferBinding::new_unchecked(&vertex_buffer, 0, None));
                encoder.draw(world_end, effects_end - world_end, 0, 1);
            }
            #[cfg(feature = "render-diagnostics")]
            if frames_rendered == 1 {
                encoder.set_render_pipeline(&probe_pipelines[1]);
                encoder.set_bind_group(&pipeline_layout, 0, &control_group, &[]);
                encoder.draw(36, 6, 0, 1);
                for (start, pipeline) in [
                    (0, &probe_pipelines[0]),
                    (6, &probe_pipelines[1]),
                    (12, &render_pipeline),
                    (24, &probe_pipelines[2]),
                    (30, &probe_pipelines[3]),
                ] {
                    encoder.set_render_pipeline(pipeline);
                    encoder.set_bind_group(&pipeline_layout, 0, &bind_group, &[]);
                    encoder.draw(start, 6, 0, 1);
                }
            }
            encoder.end_render_pass();

            #[cfg(feature = "render-diagnostics")]
            if controlled {
                let world_commands = encoder.end_encoding().expect("end_encoding(probe world)");
                queue
                    .submit(
                        &[&world_commands],
                        &[&acquired.texture],
                        (&mut fence, fence_value),
                    )
                    .expect("queue.submit(probe world)");
                fence_value += 1;
                if let Some(probe) = surface.diagnostic_frame(&device, None) {
                    log_line(&format!(
                        "GLES_WORLD frame={} {probe:?}",
                        frames_rendered + 1
                    ));
                }
                if frames_rendered == 1 {
                    device.diagnostic_atlas_binding(
                        &probe_pipelines[3],
                        &atlas_view,
                        &sampler,
                        |line| {
                            log_line(&format!(
                                "{line} frame=2 pipeline=fixed_textureLoad expected_pixel={:?}",
                                &assets.texture_layers[0][..4]
                            ));
                        },
                    );
                    log_line(&format!(
                        "GLES_ATLAS_HAL_CONTROL frame=2 sample=bottom_center expected={PROBE_CONTROL_TEXEL:?}"
                    ));
                    let expected = probe_texture(ATLAS_SIDE as usize);
                    let mut actual = [0; ATLAS_LAYER_BYTES];
                    let (status, error) = device.diagnostic_atlas_layer(
                        &atlas_texture,
                        assets.white_layer as i32,
                        &mut actual,
                    );
                    log_line(&format!(
                        "GLES_ATLAS_PANEL layer={} matches={} status={status:#x} error={error:#x} left={:?} right={:?}",
                        assets.white_layer,
                        actual.as_slice() == expected,
                        &actual[..4],
                        &actual[(ATLAS_SIDE * 2) as usize..(ATLAS_SIDE * 2 + 4) as usize]
                    ));
                }
                encoder.reset_all(std::iter::once(world_commands));
                encoder
                    .begin_encoding(Some("horizon_client_probe_overlay"))
                    .expect("begin_encoding(probe overlay)");
            }

            // Keep world color and clear world depth before drawing the camera-space hand.
            let overlay_color = [Some(ColorAttachment {
                ops: AttachmentOps::LOAD | AttachmentOps::STORE,
                ..color
            })];
            pass.label = Some("horizon_hand_hud_pass");
            pass.color_attachments = &overlay_color;
            encoder
                .begin_render_pass(&pass)
                .expect("begin_render_pass(hand/HUD)");
            encoder.set_vertex_buffer(0, BufferBinding::new_unchecked(&vertex_buffer, 0, None));
            if hand_end > effects_end {
                encoder.set_render_pipeline(&hand_pipeline);
                encoder.set_bind_group(&pipeline_layout, 0, &bind_group, &[]);
                encoder.draw(effects_end, hand_end - effects_end, 0, 1);
            }
            if !controlled && vertex_count > hand_end {
                encoder.set_render_pipeline(&render_pipeline);
                encoder.set_bind_group(&pipeline_layout, 0, &bind_group, &[]);
                encoder.draw(hand_end, vertex_count - hand_end, 0, 1);
            }
            #[cfg(feature = "render-diagnostics")]
            if frames_rendered == 1 {
                encoder.set_render_pipeline(&probe_pipelines[0]);
                encoder.set_bind_group(&pipeline_layout, 0, &bind_group, &[]);
                encoder.draw(18, 6, 0, 1);
            }
            encoder.end_render_pass();

            let cmd_buf = encoder.end_encoding().expect("end_encoding(frame)");
            queue
                .submit(&[&cmd_buf], &[&acquired.texture], (&mut fence, fence_value))
                .expect("queue.submit(frame)");
            fence_value += 1;

            #[cfg(feature = "render-diagnostics")]
            if matches!(frames_rendered, 0..=3 | 179) {
                if frames_rendered == 2 {
                    device.diagnostic_atlas_binding(
                        &render_pipeline,
                        &atlas_view,
                        &sampler,
                        |line| {
                            log_line(&format!("{line} frame=3 pipeline=terrain_hud"));
                        },
                    );
                }
                if let Some(probe) =
                    surface.diagnostic_frame(&device, diagnostic_window_size(egl_handles))
                {
                    log_line(&format!("GLES_PRE frame={} {probe:?}", frames_rendered + 1));
                }
            }
            if let Err(e) = queue.present(&surface, acquired.texture) {
                log_line(&format!("FAIL queue.present failed: {e:?}"));
                device.destroy_texture_view(frame_view);
                encoder.reset_all(std::iter::once(cmd_buf));
                result = 17;
                break;
            }

            #[cfg(feature = "render-diagnostics")]
            if let Some(probe) = surface.take_diagnostic_present() {
                log_line(&format!(
                    "GLES_POST frame={} {probe:?}",
                    frames_rendered + 1
                ));
            }
            #[cfg(feature = "render-diagnostics")]
            if frames_rendered == 2 {
                let mut first_texels: Vec<_> = assets
                    .texture_layers
                    .iter()
                    .map(|layer| layer[..4].try_into().unwrap())
                    .collect();
                for anim in &assets.animations {
                    first_texels[anim.layer_index as usize] =
                        anim.frames[anim.current_frame][..4].try_into().unwrap();
                }
                if let Err(error) = device.diagnostic_atlas_sampling(
                    &atlas_texture,
                    &sampler,
                    &first_texels,
                    log_line,
                ) {
                    log_line(&format!("GLES_ATLAS_NATIVE shader_error={error:?}"));
                }
            }
            device.destroy_texture_view(frame_view);
            encoder.reset_all(std::iter::once(cmd_buf));
        }

        frames_rendered += 1;
        if frames_rendered == 1 {
            log_line(&format!(
                "INIT first_frame_ms={}",
                startup.elapsed().as_millis()
            ));
            #[cfg(feature = "vanilla")]
            unsafe {
                horizon_memory_report()
            };
            log_line(&format!(
                "PASS First playable first-person Horizon frame rendered and presented (vertices={})",
                vertex_count
            ));
            #[cfg(feature = "vanilla-water")]
            log_line(&format!(
                "WATER atlas_side={ATLAS_SIDE} layers={:?} cached_vertices={}",
                assets.water_layers,
                column_meshes
                    .values()
                    .map(|m| m.vertices.len() - m.solid_vertices as usize)
                    .sum::<usize>(),
            ));
        }
    }

    // 8. Save world & player state on shutdown, stop Server thread, and destroy GPU resources
    running.store(false, Ordering::Relaxed);
    if server_handle.join().is_err() {
        log_line("FAIL Server thread join");
        result = 18;
    }
    let final_ticks = server_ticks.load(Ordering::Relaxed);
    // The worker and the shutdown save must never write the same region concurrently.
    if let Err(e) = stream_worker.shutdown() {
        log_line(&format!("FAIL Chunk worker shutdown: {e}"));
        result = 19;
    }
    log_line(&format!(
        "FRAME_TIMING max_interval_us={max_frame_us} intervals_over_33ms={slow_frames} max_column_mesh_us={max_mesh_us} interval_samples={interval_samples} interval_total_us={interval_total_us} frame_gaps_over_1s={frame_gaps_over_1s}"
    ));
    #[cfg(feature = "vanilla-visuals")]
    log_line(&format!(
        "EFFECTS emitted={} peak={}",
        effects.emitted, effects.peak
    ));
    log_line(&format!(
        "UPLOAD max_staging_write_us={max_upload_us} max_staging_bytes={max_upload_bytes} peak_cpu_mesh_bytes={peak_mesh_bytes}"
    ));
    #[cfg(feature = "vanilla")]
    unsafe {
        gpu_sections.shutdown(&device)
    };
    #[cfg(feature = "vanilla")]
    unsafe {
        horizon_memory_report()
    };

    let (mesh_gens, saved_on_exit) = if let Ok(mut guard) = world_arc.lock() {
        let saved = match guard.save((
            player.position,
            player.yaw,
            player.pitch,
            player.inventory.selected(),
        )) {
            Ok(saved) => saved,
            Err(e) => {
                log_line(&format!("FAIL Shutdown save: {e}"));
                result = 18;
                0
            }
        };
        (guard.mesh_generations, saved)
    } else {
        log_line("FAIL Could not lock the world for shutdown save");
        result = 18;
        (0, 0)
    };

    drop(encoder);
    unsafe {
        if let Err(e) = queue.wait_for_idle() {
            log_line(&format!("FAIL Waiting for the GPU on shutdown: {e:?}"));
            result = 18;
        }
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
        #[cfg(feature = "render-diagnostics")]
        {
            device.destroy_bind_group(control_group);
            device.destroy_texture_view(control_view);
            device.destroy_texture(control_texture);
        }
        device.destroy_render_pipeline(render_pipeline);
        device.destroy_render_pipeline(hand_pipeline);
        #[cfg(feature = "vanilla-water")]
        device.destroy_render_pipeline(water_pipeline);
        #[cfg(feature = "render-diagnostics")]
        for pipeline in probe_pipelines {
            device.destroy_render_pipeline(pipeline);
        }
        device.destroy_pipeline_layout(pipeline_layout);
        device.destroy_bind_group_layout(bind_group_layout);
        device.destroy_shader_module(shader_module);
    }

    let status = if result == 0 { "PASS" } else { "FAIL" };
    log_line(&format!(
        "{status} Shutdown: frames_rendered={} server_ticks={} mesh_generations={} blocks_broken={} blocks_placed={} saved_chunks={}",
        frames_rendered,
        final_ticks,
        mesh_gens,
        player.blocks_broken,
        player.blocks_placed,
        saved_on_exit
    ));
    result
}
