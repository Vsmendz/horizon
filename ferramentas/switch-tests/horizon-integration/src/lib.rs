include!("../../runtime-audit/abi_assertions.rs");

use core::ffi::{c_char, c_void};
use core::mem::{offset_of, size_of};
use std::ffi::CString;
use std::fs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use glow::HasContext;
use horizon_client_core::{
    gamerules::{GameRules, Value as RuleValue},
    identifier::parse_identifier,
    scoreboard::Scoreboard,
    text::Text,
};
use horizon_core::{
    geometry::Direction,
    noise::PerlinNoise,
    position::{BlockPos, ChunkPos},
    random::{LegacyRandom, RandomSource, XoroshiroRandom},
    version::{DATA_VERSION, PROTOCOL_VERSION, VANILLA_VERSION},
};
use horizon_items::{cooldowns::Cooldowns, items::ItemStack};
use horizon_world::{
    chunk::{BlockProperties, BlockState, PalettedContainer},
    level::ticks::Ticks,
};

unsafe extern "C" {
    fn probe_log(s: *const c_char);
    fn horizon_switch_default_window() -> *mut c_void;
    fn horizon_switch_poll(
        buttons_down_out: *mut u64,
        buttons_held_out: *mut u64,
        lx_out: *mut i32,
        ly_out: *mut i32,
        rx_out: *mut i32,
        ry_out: *mut i32,
    ) -> i32;
}

// libnx HidNpadButton bitmasks (switch/services/hid.h)
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

fn log_line(msg: &str) {
    if let Ok(cstr) = CString::new(msg) {
        unsafe { probe_log(cstr.as_ptr()) };
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VoxelVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SceneUniforms {
    mvp: [[f32; 4]; 4],
    sun_dir: [f32; 4],
    fog_color: [f32; 4],
}

/// Real WGSL shader compiled at runtime on Nintendo Switch via Horizon's `third_party/naga`
/// into GLSL ES 300 (`#version 300 es`).
const VOXEL_SHADER_WGSL: &str = r#"
struct SceneUniforms {
    mvp: mat4x4<f32>,
    sun_dir: vec4<f32>,
    fog_color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) lit_color: vec3<f32>,
    @location(1) height_factor: f32,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_pos = scene.mvp * vec4<f32>(in.position, 1.0);

    let light_dir = normalize(scene.sun_dir.xyz);
    let ndotl = max(dot(in.normal, light_dir), 0.0);
    // Vanilla-inspired directional face shading + sun diffuse
    let face_shade = select(0.75, 1.0, in.normal.y > 0.5)
                   * select(1.0, 0.55, in.normal.y < -0.5)
                   * select(1.0, 0.82, abs(in.normal.x) > 0.5);
    let ambient = 0.42;
    let lighting = min(ambient + ndotl * 0.65, 1.15) * face_shade;

    out.lit_color = in.color * lighting;
    out.height_factor = clamp((in.position.y + 8.0) / 16.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let aerial_mix = (1.0 - in.height_factor) * 0.14;
    let final_rgb = mix(in.lit_color, scene.fog_color.xyz, aerial_mix);
    return vec4<f32>(final_rgb, 1.0);
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum VoxelBlock {
    Air = 0,
    Bedrock = 1,
    Stone = 2,
    Dirt = 3,
    GrassBlock = 4,
    Water = 5,
    Sand = 6,
    OakLog = 7,
    OakLeaves = 8,
    GoldBlock = 9,
    SeaLantern = 10,
}

impl VoxelBlock {
    fn from_id(name: &str) -> Self {
        match name {
            "minecraft:bedrock" => Self::Bedrock,
            "minecraft:stone" => Self::Stone,
            "minecraft:dirt" => Self::Dirt,
            "minecraft:grass_block" => Self::GrassBlock,
            "minecraft:water" => Self::Water,
            "minecraft:sand" => Self::Sand,
            "minecraft:oak_log" => Self::OakLog,
            "minecraft:oak_leaves" => Self::OakLeaves,
            "minecraft:gold_block" => Self::GoldBlock,
            "minecraft:sea_lantern" => Self::SeaLantern,
            _ => Self::Air,
        }
    }

    fn registry_name(self) -> &'static str {
        match self {
            Self::Air => "minecraft:air",
            Self::Bedrock => "minecraft:bedrock",
            Self::Stone => "minecraft:stone",
            Self::Dirt => "minecraft:dirt",
            Self::GrassBlock => "minecraft:grass_block",
            Self::Water => "minecraft:water",
            Self::Sand => "minecraft:sand",
            Self::OakLog => "minecraft:oak_log",
            Self::OakLeaves => "minecraft:oak_leaves",
            Self::GoldBlock => "minecraft:gold_block",
            Self::SeaLantern => "minecraft:sea_lantern",
        }
    }

    fn face_color(self, dir: Direction) -> [f32; 3] {
        match self {
            Self::Air => [0.0, 0.0, 0.0],
            Self::Bedrock => [0.22, 0.22, 0.24],
            Self::Stone => [0.50, 0.50, 0.52],
            Self::Dirt => [0.54, 0.37, 0.24],
            Self::GrassBlock => match dir {
                Direction::Up => [0.38, 0.74, 0.27],
                Direction::Down => [0.54, 0.37, 0.24],
                _ => [0.44, 0.60, 0.28],
            },
            Self::Water => [0.18, 0.45, 0.85],
            Self::Sand => [0.86, 0.82, 0.62],
            Self::OakLog => match dir {
                Direction::Up | Direction::Down => [0.65, 0.52, 0.32],
                _ => [0.42, 0.32, 0.19],
            },
            Self::OakLeaves => [0.24, 0.58, 0.18],
            Self::GoldBlock => [0.96, 0.82, 0.20],
            Self::SeaLantern => [0.68, 0.92, 0.88],
        }
    }
}

fn block_tag(block: VoxelBlock) -> horizon_world::nbt::Tag {
    BlockState {
        name: block.registry_name().to_owned(),
        properties: BlockProperties::new(),
    }
    .to_nbt()
}

/// Generates a 16x16x16 voxel chunk section using `horizon-core` (`LegacyRandom`, `PerlinNoise`,
/// `BlockPos`) and stores it in `horizon-world`'s `PalettedContainer`.
fn build_horizon_chunk_section(seed: i64, pulse_lantern: bool) -> Result<(PalettedContainer, [VoxelBlock; 4096]), String> {
    let mut rng = LegacyRandom::new(seed);
    let perlin = PerlinNoise::new(&mut rng);
    let mut container = PalettedContainer::uniform(block_tag(VoxelBlock::Air), 4096, 4)
        .map_err(|e| format!("PalettedContainer::uniform failed: {e}"))?;
    let mut dense = [VoxelBlock::Air; 4096];

    let sea_level = 5i32;
    for z in 0..16i32 {
        for x in 0..16i32 {
            let nx = (x as f64) * 0.11;
            let nz = (z as f64) * 0.11;
            let n = perlin.get2(nx, nz);
            // Island falloff so edges reveal water/sand and center forms a hill
            let dx = (x as f32 - 7.5) / 8.0;
            let dz = (z as f32 - 7.5) / 8.0;
            let radial = (1.0 - (dx * dx + dz * dz) * 0.65).clamp(0.1, 1.0);
            let height = ((4.5 + (n * 4.5 + 3.5) * radial) as i32).clamp(2, 11);

            for y in 0..16i32 {
                let block = if y == 0 {
                    VoxelBlock::Bedrock
                } else if y < height - 2 {
                    VoxelBlock::Stone
                } else if y < height {
                    if height <= sea_level + 1 {
                        VoxelBlock::Sand
                    } else {
                        VoxelBlock::Dirt
                    }
                } else if y == height {
                    if height <= sea_level + 1 {
                        VoxelBlock::Sand
                    } else {
                        VoxelBlock::GrassBlock
                    }
                } else if y <= sea_level {
                    VoxelBlock::Water
                } else {
                    VoxelBlock::Air
                };

                if block != VoxelBlock::Air {
                    let idx = BlockPos::new(x, y, z).section_index();
                    dense[idx] = block;
                    container
                        .set(idx, block_tag(block))
                        .map_err(|e| format!("container.set failed: {e}"))?;
                }
            }
        }
    }

    // Place an oak tree at (4, surface+1, 4)
    let tree_x = 4i32;
    let tree_z = 4i32;
    let mut tree_base = 6i32;
    for y in (1..12).rev() {
        let idx = BlockPos::new(tree_x, y, tree_z).section_index();
        if dense[idx] == VoxelBlock::GrassBlock || dense[idx] == VoxelBlock::Sand {
            tree_base = y + 1;
            break;
        }
    }
    for dy in 0..3i32 {
        let y = (tree_base + dy).min(15);
        let idx = BlockPos::new(tree_x, y, tree_z).section_index();
        dense[idx] = VoxelBlock::OakLog;
        let _ = container.set(idx, block_tag(VoxelBlock::OakLog));
    }
    for lx in (tree_x - 1)..=(tree_x + 1) {
        for lz in (tree_z - 1)..=(tree_z + 1) {
            for ly in (tree_base + 2)..=(tree_base + 3) {
                if (0..16).contains(&lx) && (0..16).contains(&ly) && (0..16).contains(&lz) {
                    let idx = BlockPos::new(lx, ly, lz).section_index();
                    if dense[idx] == VoxelBlock::Air {
                        dense[idx] = VoxelBlock::OakLeaves;
                        let _ = container.set(idx, block_tag(VoxelBlock::OakLeaves));
                    }
                }
            }
        }
    }

    // Place a 2x2 Horizon beacon monument near the center (8, surface+1, 8)
    let beacon_block = if pulse_lantern {
        VoxelBlock::SeaLantern
    } else {
        VoxelBlock::GoldBlock
    };
    for bx in 7..=8i32 {
        for bz in 7..=8i32 {
            let mut top_y = 7i32;
            for y in (1..13).rev() {
                let idx = BlockPos::new(bx, y, bz).section_index();
                if dense[idx] != VoxelBlock::Air && dense[idx] != VoxelBlock::Water {
                    top_y = (y + 1).min(14);
                    break;
                }
            }
            let idx = BlockPos::new(bx, top_y, bz).section_index();
            dense[idx] = beacon_block;
            let _ = container.set(idx, block_tag(beacon_block));
        }
    }

    Ok((container, dense))
}

/// Meshes the 16x16x16 voxel grid into 3D triangles with hidden-face culling.
fn mesh_voxel_section(dense: &[VoxelBlock; 4096]) -> Vec<VoxelVertex> {
    let mut vertices = Vec::with_capacity(6144);
    let get = |x: i32, y: i32, z: i32| -> VoxelBlock {
        if (0..16).contains(&x) && (0..16).contains(&y) && (0..16).contains(&z) {
            dense[BlockPos::new(x, y, z).section_index()]
        } else {
            VoxelBlock::Air
        }
    };

    for y in 0..16i32 {
        for z in 0..16i32 {
            for x in 0..16i32 {
                let block = get(x, y, z);
                if block == VoxelBlock::Air {
                    continue;
                }
                let x0 = x as f32 - 8.0;
                let y0 = y as f32 - 6.0;
                let z0 = z as f32 - 8.0;
                let x1 = x0 + 1.0;
                let y1 = y0 + 1.0;
                let z1 = z0 + 1.0;

                for dir in Direction::ALL {
                    let [dx, dy, dz] = dir.normal();
                    let neighbor = get(x + dx, y + dy, z + dz);
                    if neighbor != VoxelBlock::Air {
                        continue;
                    }
                    let color = block.face_color(dir);
                    let normal = [dx as f32, dy as f32, dz as f32];
                    // CCW winding when viewed from outside the block
                    let quad: [[f32; 3]; 4] = match dir {
                        Direction::Up => [
                            [x0, y1, z1],
                            [x1, y1, z1],
                            [x1, y1, z0],
                            [x0, y1, z0],
                        ],
                        Direction::Down => [
                            [x0, y0, z0],
                            [x1, y0, z0],
                            [x1, y0, z1],
                            [x0, y0, z1],
                        ],
                        Direction::North => [
                            // -Z
                            [x1, y0, z0],
                            [x0, y0, z0],
                            [x0, y1, z0],
                            [x1, y1, z0],
                        ],
                        Direction::South => [
                            // +Z
                            [x0, y0, z1],
                            [x1, y0, z1],
                            [x1, y1, z1],
                            [x0, y1, z1],
                        ],
                        Direction::West => [
                            // -X
                            [x0, y0, z0],
                            [x0, y0, z1],
                            [x0, y1, z1],
                            [x0, y1, z0],
                        ],
                        Direction::East => [
                            // +X
                            [x1, y0, z1],
                            [x1, y0, z0],
                            [x1, y1, z0],
                            [x1, y1, z1],
                        ],
                    };
                    for &idx in &[0usize, 1, 2, 0, 2, 3] {
                        vertices.push(VoxelVertex {
                            position: quad[idx],
                            normal,
                            color,
                        });
                    }
                }
            }
        }
    }
    vertices
}

struct SharedWorldState {
    running: bool,
    request_new_seed: bool,
    seed: i64,
    server_ticks: u64,
    mesh_generation: u64,
    vertices: Vec<VoxelVertex>,
    palette_entries: usize,
}

fn verify_horizon_subsystems() -> Result<(PalettedContainer, Vec<VoxelVertex>), String> {
    if VANILLA_VERSION != "26.3" || PROTOCOL_VERSION != 777 || DATA_VERSION != 5023 {
        return Err(format!(
            "unexpected Horizon constants: version={VANILLA_VERSION} proto={PROTOCOL_VERSION} data={DATA_VERSION}"
        ));
    }

    // 1. Verify horizon-core random & position packing
    let pos = BlockPos::new(-123, 64, 456);
    if BlockPos::unpack(pos.pack()) != pos {
        return Err("BlockPos pack/unpack mismatch".into());
    }
    let mut xoro = XoroshiroRandom::new(0x484F_5249_5A4F_4E26);
    let draw = xoro.next_long();
    if draw == 0 {
        return Err("XoroshiroRandom returned zero draw".into());
    }

    // 2. Verify horizon-world PalettedContainer + NBT round-trip
    let (container, dense) = build_horizon_chunk_section(26032026, false)?;
    let nbt = container.to_nbt();
    let roundtrip = PalettedContainer::from_nbt(&nbt, 4096, 4)
        .map_err(|e| format!("PalettedContainer::from_nbt failed: {e}"))?;
    let sample_idx = BlockPos::new(8, 0, 8).section_index();
    let tag = roundtrip
        .get(sample_idx)
        .map_err(|e| format!("roundtrip.get failed: {e}"))?;
    let state = BlockState::from_nbt(tag).map_err(|e| format!("BlockState::from_nbt failed: {e}"))?;
    if VoxelBlock::from_id(&state.name) != VoxelBlock::Bedrock {
        return Err(format!("expected bedrock at y=0, got {}", state.name));
    }

    // 3. Verify horizon-world Ticks + horizon-client-core GameRules, Scoreboard, Identifier, Text + horizon-items Cooldowns
    let mut ticks = Ticks::new(12345);
    ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .map_err(|e| format!("Ticks::add_empty_chunk failed: {e}"))?;
    let rules = GameRules::default();
    if rules.get("keep_inventory") != Some(RuleValue::Bool(false)) {
        return Err("GameRules default keep_inventory mismatch".into());
    }
    let id = parse_identifier("overworld").ok_or("parse_identifier failed")?;
    if id != "minecraft:overworld" {
        return Err(format!("unexpected identifier: {id}"));
    }
    let _scoreboard = Scoreboard::default();
    let banner = Text::literal("Horizon 26.3 on Nintendo Switch");
    if banner.plain() != "Horizon 26.3 on Nintendo Switch" {
        return Err("Text::plain mismatch".into());
    }
    let stack = ItemStack::new("minecraft:ender_pearl", 1);
    let mut cooldowns = Cooldowns::default();
    cooldowns.add(Arc::clone(&stack.item), 4);
    for _ in 0..4 {
        cooldowns.tick();
    }
    if cooldowns.is_on_cooldown("minecraft:ender_pearl") {
        return Err("ItemCooldowns did not expire after 4 ticks".into());
    }

    // 4. Verify SD card write/read of NBT/state probe
    let probe_path = "sdmc:/switch/horizon-tests/test3_chunk_probe.bin";
    let probe_bytes = format!(
        "version={VANILLA_VERSION};protocol={PROTOCOL_VERSION};data={DATA_VERSION};palette={}\n",
        roundtrip.palette_len()
    );
    fs::write(probe_path, probe_bytes.as_bytes())
        .map_err(|e| format!("SD write failed: {e}"))?;
    let read_back = fs::read_to_string(probe_path)
        .map_err(|e| format!("SD read failed: {e}"))?;
    let _ = fs::remove_file(probe_path);
    if read_back != probe_bytes {
        return Err("SD round-trip content mismatch".into());
    }

    let vertices = mesh_voxel_section(&dense);
    if vertices.is_empty() {
        return Err("mesh_voxel_section produced 0 vertices".into());
    }

    log_line(&format!(
        "PASS Horizon crates: version={VANILLA_VERSION} proto={PROTOCOL_VERSION} data={DATA_VERSION} palette={} vertices={}",
        roundtrip.palette_len(),
        vertices.len()
    ));
    Ok((roundtrip, vertices))
}

/// Translates `VOXEL_SHADER_WGSL` into GLSL ES 300 (`#version 300 es`) using `third_party/naga`.
fn compile_wgsl_to_glsl_es(stage: naga::ShaderStage, entry_point: &str) -> Result<String, String> {
    let module = naga::front::wgsl::parse_str(VOXEL_SHADER_WGSL)
        .map_err(|e| format!("naga WGSL parse error: {e}"))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("naga validation error: {e}"))?;

    let mut options = naga::back::glsl::Options {
        version: naga::back::glsl::Version::Embedded {
            version: 300,
            is_webgl: false,
        },
        writer_flags: naga::back::glsl::WriterFlags::empty(),
        binding_map: Default::default(),
        zero_initialize_workgroup_memory: true,
    };
    options.binding_map.insert(
        naga::ResourceBinding {
            group: 0,
            binding: 0,
        },
        0,
    );

    let pipeline_options = naga::back::glsl::PipelineOptions {
        shader_stage: stage,
        entry_point: entry_point.to_owned(),
        multiview: None,
    };
    let policies = naga::proc::BoundsCheckPolicies {
        index: naga::proc::BoundsCheckPolicy::Unchecked,
        buffer: naga::proc::BoundsCheckPolicy::Unchecked,
        image_load: naga::proc::BoundsCheckPolicy::Unchecked,
        binding_array: naga::proc::BoundsCheckPolicy::Unchecked,
    };

    let mut output = String::new();
    let mut writer = naga::back::glsl::Writer::new(
        &mut output,
        &module,
        &info,
        &options,
        &pipeline_options,
        policies,
    )
    .map_err(|e| format!("naga GLSL Writer::new failed: {e}"))?;
    writer
        .write()
        .map_err(|e| format!("naga GLSL Writer::write failed: {e}"))?;
    Ok(output)
}

mod egl_bridge {
    use core::ffi::{c_char, c_void};

    pub type Handle = *mut c_void;

    unsafe extern "C" {
        pub fn eglGetDisplay(native: Handle) -> Handle;
        pub fn eglInitialize(display: Handle, major: *mut i32, minor: *mut i32) -> u32;
        pub fn eglBindAPI(api: u32) -> u32;
        pub fn eglChooseConfig(
            display: Handle,
            attrs: *const i32,
            config: *mut Handle,
            size: i32,
            count: *mut i32,
        ) -> u32;
        pub fn eglCreateWindowSurface(
            display: Handle,
            config: Handle,
            window: Handle,
            attrs: *const i32,
        ) -> Handle;
        pub fn eglCreateContext(
            display: Handle,
            config: Handle,
            share: Handle,
            attrs: *const i32,
        ) -> Handle;
        pub fn eglMakeCurrent(
            display: Handle,
            draw: Handle,
            read: Handle,
            context: Handle,
        ) -> u32;
        pub fn eglSwapInterval(display: Handle, interval: i32) -> u32;
        pub fn eglGetProcAddress(name: *const c_char) -> *const c_void;
        pub fn eglSwapBuffers(display: Handle, surface: Handle) -> u32;
        pub fn eglDestroyContext(display: Handle, context: Handle) -> u32;
        pub fn eglDestroySurface(display: Handle, surface: Handle) -> u32;
        pub fn eglTerminate(display: Handle) -> u32;
    }

    pub struct EglSession {
        pub display: Handle,
        pub surface: Handle,
        pub context: Handle,
    }

    impl EglSession {
        pub unsafe fn init(native_window: Handle) -> Result<Self, i32> {
            unsafe {
                let null = core::ptr::null_mut();
                let display = eglGetDisplay(null);
                if display.is_null() || eglInitialize(display, null.cast(), null.cast()) == 0 {
                    return Err(11);
                }
                if eglBindAPI(0x30A0) == 0 {
                    // EGL_OPENGL_ES_API
                    eglTerminate(display);
                    return Err(12);
                }
                let attrs = [
                    0x3040, 0x0040, // EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT
                    0x3033, 4,      // EGL_SURFACE_TYPE, EGL_WINDOW_BIT
                    0x3024, 8,      // EGL_RED_SIZE, 8
                    0x3023, 8,      // EGL_GREEN_SIZE, 8
                    0x3022, 8,      // EGL_BLUE_SIZE, 8
                    0x3021, 8,      // EGL_ALPHA_SIZE, 8
                    0x3025, 24,     // EGL_DEPTH_SIZE, 24
                    0x3026, 8,      // EGL_STENCIL_SIZE, 8
                    0x3038,         // EGL_NONE
                ];
                let mut config = null;
                let mut count = 0;
                if eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut count) == 0
                    || count != 1
                {
                    eglTerminate(display);
                    return Err(13);
                }
                let surface =
                    eglCreateWindowSurface(display, config, native_window, core::ptr::null());
                if surface.is_null() {
                    eglTerminate(display);
                    return Err(14);
                }
                let ctx_attrs = [
                    0x3098, 3, // EGL_CONTEXT_CLIENT_VERSION, 3
                    0x3038,    // EGL_NONE
                ];
                let context = eglCreateContext(display, config, null, ctx_attrs.as_ptr());
                if context.is_null() {
                    eglDestroySurface(display, surface);
                    eglTerminate(display);
                    return Err(15);
                }
                if eglMakeCurrent(display, surface, surface, context) == 0 {
                    eglDestroyContext(display, context);
                    eglDestroySurface(display, surface);
                    eglTerminate(display);
                    return Err(16);
                }
                let _ = eglSwapInterval(display, 1);
                Ok(Self {
                    display,
                    surface,
                    context,
                })
            }
        }

        pub unsafe fn swap_buffers(&self) -> bool {
            unsafe { eglSwapBuffers(self.display, self.surface) != 0 }
        }
    }

    impl Drop for EglSession {
        fn drop(&mut self) {
            unsafe {
                let null = core::ptr::null_mut();
                eglMakeCurrent(self.display, null, null, null);
                if !self.context.is_null() {
                    eglDestroyContext(self.display, self.context);
                }
                if !self.surface.is_null() {
                    eglDestroySurface(self.display, self.surface);
                }
                if !self.display.is_null() {
                    eglTerminate(self.display);
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_horizon_integration_main() -> i32 {
    std::panic::set_hook(Box::new(|info| {
        log_line(&format!("FAIL Rust panic: {info}"));
    }));
    log_line("BEGIN Horizon Switch Integration (std + horizon-core + horizon-world + naga + EGL/GLES3)");

    // Step 1: Verify Horizon crates, NBT serialization, RNG, noise, and SD card I/O
    let (initial_container, initial_vertices) = match verify_horizon_subsystems() {
        Ok(res) => res,
        Err(err) => {
            log_line(&format!("FAIL verify_horizon_subsystems: {err}"));
            return 21;
        }
    };

    // Step 2: Translate WGSL shader to GLSL ES 300 using Horizon's `third_party/naga`
    let vs_glsl = match compile_wgsl_to_glsl_es(naga::ShaderStage::Vertex, "vs_main") {
        Ok(src) => src,
        Err(err) => {
            log_line(&format!("FAIL naga vertex shader translation: {err}"));
            return 22;
        }
    };
    let fs_glsl = match compile_wgsl_to_glsl_es(naga::ShaderStage::Fragment, "fs_main") {
        Ok(src) => src,
        Err(err) => {
            log_line(&format!("FAIL naga fragment shader translation: {err}"));
            return 23;
        }
    };
    log_line(&format!(
        "PASS Naga WGSL->GLSL ES 300 translation (vs={} bytes, fs={} bytes)",
        vs_glsl.len(),
        fs_glsl.len()
    ));

    // Step 3: Initialize EGL + OpenGL ES 3.0 on nwindowGetDefault()
    let window = unsafe { horizon_switch_default_window() };
    if window.is_null() {
        log_line("FAIL horizon_switch_default_window returned null");
        return 24;
    }
    let egl = match unsafe { egl_bridge::EglSession::init(window) } {
        Ok(session) => session,
        Err(code) => {
            log_line(&format!("FAIL EglSession::init failed with code {code}"));
            return code;
        }
    };

    let gl = unsafe {
        glow::Context::from_loader_function(|name| {
            let Ok(cstr) = CString::new(name) else {
                return core::ptr::null();
            };
            egl_bridge::eglGetProcAddress(cstr.as_ptr())
        })
    };

    let renderer_name = unsafe { gl.get_parameter_string(glow::RENDERER) };
    let version_name = unsafe { gl.get_parameter_string(glow::VERSION) };
    log_line(&format!(
        "PASS EGL/GLES3 context active: renderer='{renderer_name}' version='{version_name}'"
    ));

    // Step 4: Compile and link Naga-generated GLSL ES 300 shaders on the Tegra X1 GPU
    let program = unsafe {
        let Ok(vs) = gl.create_shader(glow::VERTEX_SHADER) else {
            log_line("FAIL gl.create_shader(VERTEX_SHADER)");
            return 25;
        };
        gl.shader_source(vs, &vs_glsl);
        gl.compile_shader(vs);
        if !gl.get_shader_compile_status(vs) {
            let info = gl.get_shader_info_log(vs);
            log_line(&format!("FAIL vertex shader compile: {info}"));
            gl.delete_shader(vs);
            return 26;
        }

        let Ok(fs) = gl.create_shader(glow::FRAGMENT_SHADER) else {
            log_line("FAIL gl.create_shader(FRAGMENT_SHADER)");
            gl.delete_shader(vs);
            return 27;
        };
        gl.shader_source(fs, &fs_glsl);
        gl.compile_shader(fs);
        if !gl.get_shader_compile_status(fs) {
            let info = gl.get_shader_info_log(fs);
            log_line(&format!("FAIL fragment shader compile: {info}"));
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            return 28;
        }

        let Ok(prog) = gl.create_program() else {
            log_line("FAIL gl.create_program");
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            return 29;
        };
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        gl.link_program(prog);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        if !gl.get_program_link_status(prog) {
            let info = gl.get_program_info_log(prog);
            log_line(&format!("FAIL program link: {info}"));
            gl.delete_program(prog);
            return 30;
        }
        prog
    };
    log_line("PASS GPU program compiled and linked from Naga GLSL ES 300 output");

    // Step 5: Create VAO, VBO, and Uniform Buffer Object (UBO)
    let (vao, vbo, ubo) = unsafe {
        let Ok(vao) = gl.create_vertex_array() else {
            log_line("FAIL create_vertex_array");
            gl.delete_program(program);
            return 31;
        };
        let Ok(vbo) = gl.create_buffer() else {
            log_line("FAIL create_buffer VBO");
            gl.delete_vertex_array(vao);
            gl.delete_program(program);
            return 32;
        };
        let Ok(ubo) = gl.create_buffer() else {
            log_line("FAIL create_buffer UBO");
            gl.delete_buffer(vbo);
            gl.delete_vertex_array(vao);
            gl.delete_program(program);
            return 33;
        };

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&initial_vertices),
            glow::DYNAMIC_DRAW,
        );

        let stride = size_of::<VoxelVertex>() as i32;
        // @location(0) position: vec3<f32>
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(
            0,
            3,
            glow::FLOAT,
            false,
            stride,
            offset_of!(VoxelVertex, position) as i32,
        );
        // @location(1) normal: vec3<f32>
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(
            1,
            3,
            glow::FLOAT,
            false,
            stride,
            offset_of!(VoxelVertex, normal) as i32,
        );
        // @location(2) color: vec3<f32>
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(
            2,
            3,
            glow::FLOAT,
            false,
            stride,
            offset_of!(VoxelVertex, color) as i32,
        );

        gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
        gl.buffer_data_size(
            glow::UNIFORM_BUFFER,
            size_of::<SceneUniforms>() as i32,
            glow::DYNAMIC_DRAW,
        );
        gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(ubo));

        // Bind any uniform block index to binding point 0
        if let Some(block_idx) = gl.get_uniform_block_index(program, "_group_0_binding_0_vs") {
            gl.uniform_block_binding(program, block_idx, 0);
        }
        if let Some(block_idx) = gl.get_uniform_block_index(program, "_group_0_binding_0_fs") {
            gl.uniform_block_binding(program, block_idx, 0);
        }

        gl.viewport(0, 0, 1280, 720);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.enable(glow::CULL_FACE);
        gl.cull_face(glow::BACK);
        gl.front_face(glow::CCW);

        (vao, vbo, ubo)
    };

    // Step 6: Spawn the integrated "Server thread" ticking at 20 TPS (50 ms)
    let shared = Arc::new(Mutex::new(SharedWorldState {
        running: true,
        request_new_seed: false,
        seed: 26032026,
        server_ticks: 0,
        mesh_generation: 1,
        vertices: initial_vertices,
        palette_entries: initial_container.palette_len(),
    }));

    let server_shared = Arc::clone(&shared);
    let server_handle = match thread::Builder::new()
        .name("Server thread".into())
        .spawn(move || {
            let mut ticks = Ticks::new(26032026);
            let _ = ticks.add_empty_chunk(ChunkPos::new(0, 0));
            let mut local_ticks = 0u64;
            let mut current_seed = 26032026i64;

            loop {
                let tick_start = Instant::now();
                let should_rebuild = {
                    let Ok(mut guard) = server_shared.lock() else {
                        break;
                    };
                    if !guard.running {
                        break;
                    }
                    local_ticks += 1;
                    guard.server_ticks = local_ticks;
                    let _ = ticks.next_sub_tick();
                    if guard.request_new_seed {
                        guard.request_new_seed = false;
                        current_seed = current_seed
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        guard.seed = current_seed;
                        true
                    } else {
                        // Every 10 server ticks (0.5 s), pulse the beacon between GoldBlock and SeaLantern
                        local_ticks % 10 == 0
                    }
                };

                if should_rebuild {
                    let pulse_lantern = (local_ticks / 10) % 2 == 1;
                    if let Ok((container, dense)) =
                        build_horizon_chunk_section(current_seed, pulse_lantern)
                    {
                        let new_vertices = mesh_voxel_section(&dense);
                        if let Ok(mut guard) = server_shared.lock() {
                            guard.vertices = new_vertices;
                            guard.palette_entries = container.palette_len();
                            guard.mesh_generation += 1;
                        }
                    }
                }

                let elapsed = tick_start.elapsed();
                if elapsed < Duration::from_millis(50) {
                    thread::sleep(Duration::from_millis(50) - elapsed);
                }
            }
        }) {
        Ok(h) => h,
        Err(err) => {
            log_line(&format!("FAIL spawning Server thread: {err}"));
            return 34;
        }
    };
    log_line("PASS Integrated 'Server thread' spawned (20 TPS loop running)");

    // Step 7: Main render & controller loop (60 FPS)
    let sky_presets: [[f32; 4]; 4] = [
        [0.47, 0.66, 1.00, 1.0], // Overworld Day Sky (Vanilla 26.3 plains)
        [0.92, 0.48, 0.28, 1.0], // Sunset Horizon
        [0.05, 0.07, 0.16, 1.0], // Starlight Night
        [0.14, 0.06, 0.22, 1.0], // The End Atmosphere
    ];
    let mut sky_index = 0usize;
    let mut auto_rotate = true;
    let mut yaw: f32 = 0.65;
    let mut pitch: f32 = 0.48;
    let mut distance: f32 = 22.0;
    let mut uploaded_generation = 1u64;
    let mut vertex_count = {
        let guard = shared.lock().unwrap();
        guard.vertices.len() as i32
    };
    let mut frames_rendered = 0u64;

    log_line(
        "READY Interactive Horizon 3D voxel chunk running! Controls: Sticks/DPad=Camera, A=Sky, B=Pause Orbit, X=New Seed, Y=Reset View, PLUS=Exit",
    );

    loop {
        let mut k_down = 0u64;
        let mut k_held = 0u64;
        let mut lx = 0i32;
        let mut ly = 0i32;
        let mut rx = 0i32;
        let mut ry = 0i32;

        let keep_going = unsafe {
            horizon_switch_poll(
                &mut k_down,
                &mut k_held,
                &mut lx,
                &mut ly,
                &mut rx,
                &mut ry,
            )
        };
        if keep_going == 0 {
            break;
        }

        // Button actions
        if (k_down & BTN_A) != 0 {
            sky_index = (sky_index + 1) % sky_presets.len();
            log_line(&format!("INPUT Button A: switched sky preset to #{sky_index}"));
        }
        if (k_down & BTN_B) != 0 {
            auto_rotate = !auto_rotate;
            log_line(&format!("INPUT Button B: auto_rotate={auto_rotate}"));
        }
        if (k_down & BTN_X) != 0 {
            if let Ok(mut guard) = shared.lock() {
                guard.request_new_seed = true;
            }
            log_line("INPUT Button X: requested new chunk seed on Server thread");
        }
        if (k_down & BTN_Y) != 0 {
            yaw = 0.65;
            pitch = 0.48;
            distance = 22.0;
            log_line("INPUT Button Y: reset camera view");
        }

        // Analog stick & D-pad camera orbit
        let norm_stick = |v: i32| -> f32 {
            let f = (v as f32) / 32767.0;
            if f.abs() < 0.15 { 0.0 } else { f }
        };
        let stick_yaw = norm_stick(lx) + norm_stick(rx);
        let stick_pitch = norm_stick(ly) + norm_stick(ry);

        if stick_yaw != 0.0 {
            yaw += stick_yaw * 0.04;
        } else if (k_held & BTN_LEFT) != 0 {
            yaw -= 0.035;
        } else if (k_held & BTN_RIGHT) != 0 {
            yaw += 0.035;
        } else if auto_rotate {
            yaw += 0.012;
        }

        if stick_pitch != 0.0 {
            pitch = (pitch + stick_pitch * 0.03).clamp(0.12, 1.35);
        } else if (k_held & BTN_UP) != 0 {
            pitch = (pitch + 0.025).clamp(0.12, 1.35);
        } else if (k_held & BTN_DOWN) != 0 {
            pitch = (pitch - 0.025).clamp(0.12, 1.35);
        }

        if (k_held & BTN_L) != 0 {
            distance = (distance + 0.25).clamp(12.0, 36.0);
        }
        if (k_held & BTN_R) != 0 {
            distance = (distance - 0.25).clamp(12.0, 36.0);
        }

        // Check if Server thread produced a new voxel mesh
        if let Ok(guard) = shared.lock() {
            if guard.mesh_generation != uploaded_generation {
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                    gl.buffer_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        bytemuck::cast_slice(&guard.vertices),
                        glow::DYNAMIC_DRAW,
                    );
                }
                vertex_count = guard.vertices.len() as i32;
                uploaded_generation = guard.mesh_generation;
            }
        }

        // Compute camera MVP matrix
        let aspect = 1280.0 / 720.0;
        let proj = Mat4::perspective_rh_gl(45.0_f32.to_radians(), aspect, 0.1, 100.0);
        let eye = Vec3::new(
            distance * pitch.cos() * yaw.sin(),
            distance * pitch.sin(),
            distance * pitch.cos() * yaw.cos(),
        );
        let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
        let mvp = proj * view;

        let sky = sky_presets[sky_index];
        let uniforms = SceneUniforms {
            mvp: mvp.to_cols_array_2d(),
            sun_dir: Vec4::new(0.45, 0.82, 0.35, 0.0).to_array(),
            fog_color: sky,
        };

        unsafe {
            gl.clear_color(sky[0], sky[1], sky[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);

            gl.use_program(Some(program));
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
            gl.buffer_sub_data_u8_slice(
                glow::UNIFORM_BUFFER,
                0,
                bytemuck::bytes_of(&uniforms),
            );

            gl.bind_vertex_array(Some(vao));
            gl.draw_arrays(glow::TRIANGLES, 0, vertex_count);
        }

        if !unsafe { egl.swap_buffers() } {
            log_line("FAIL eglSwapBuffers returned false");
            break;
        }
        frames_rendered += 1;
    }

    // Step 8: Clean shutdown of Server thread and GL/EGL resources
    if let Ok(mut guard) = shared.lock() {
        guard.running = false;
    }
    if server_handle.join().is_err() {
        log_line("FAIL joining Server thread");
        return 35;
    }

    let (final_ticks, final_gen, final_palette) = {
        let guard = shared.lock().unwrap();
        (guard.server_ticks, guard.mesh_generation, guard.palette_entries)
    };

    unsafe {
        gl.delete_buffer(ubo);
        gl.delete_buffer(vbo);
        gl.delete_vertex_array(vao);
        gl.delete_program(program);
    }
    drop(egl);

    log_line(&format!(
        "PASS Clean shutdown: frames_rendered={frames_rendered} server_ticks={final_ticks} mesh_generations={final_gen} palette_entries={final_palette}"
    ));
    log_line("RESULT=0");
    0
}
