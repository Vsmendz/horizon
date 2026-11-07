use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use horizon_core::{
    geometry::{Aabb, Axis, Vec3, VoxelShape},
    position::{BlockPos, ChunkPos},
    version::DATA_VERSION,
};
use horizon_world::{
    compression::Compression,
    nbt::{self, JavaString, Limits, NamedTag, Tag},
    region::{ChunkRecord, RegionFile},
    worldgen::generator::spiral_around,
};

use crate::{
    block_palette::VoxelBlock,
    ticket_levels::{ChunkTickets, Sources},
};

#[cfg(not(feature = "vanilla"))]
use crate::block_palette::block_tag;
#[cfg(feature = "vanilla")]
use crate::vanilla::{self, Column, Vanilla};
#[cfg(not(feature = "vanilla"))]
use horizon_core::{noise::PerlinNoise, random::LegacyRandom};
#[cfg(not(feature = "vanilla"))]
use horizon_world::{
    chunk::{BlockState, PalettedContainer},
    heightmap::{Heightmap, HeightmapChunk, HeightmapType},
};
#[cfg(feature = "vanilla")]
use horizon_world::{heightmap::HeightmapType, registry::BlockStateId};
#[cfg(feature = "vanilla")]
use std::sync::Arc;

#[cfg(not(feature = "vanilla"))]
const FORMAT: i32 = 1;
#[cfg(feature = "vanilla")]
const FORMAT: i32 = 2;
#[cfg(not(feature = "vanilla"))]
pub const SECTIONS: usize = 2;
#[cfg(feature = "vanilla")]
pub const SECTIONS: usize = 24;
#[cfg(not(feature = "vanilla"))]
pub const MIN_Y: i32 = 0;
#[cfg(feature = "vanilla")]
pub const MIN_Y: i32 = -64;
pub const HEIGHT: i32 = SECTIONS as i32 * 16;
pub const MAX_Y: i32 = MIN_Y + HEIGHT;
#[cfg(not(feature = "vanilla"))]
pub const MESH_HEIGHT: i32 = HEIGHT;
#[cfg(feature = "vanilla")]
pub const MESH_HEIGHT: i32 = 16;
pub const VIEW_DISTANCE: i32 = 1;
#[cfg(not(feature = "vanilla"))]
pub const SEED: i64 = 2026_2603;
#[cfg(feature = "vanilla")]
pub const SEED: i64 = 12345;
pub const SPAWN_X: i32 = 16;
pub const SPAWN_Z: i32 = 24;
pub type Key = (i32, i32);
pub type PlayerSave = (Vec3, f32, f32, usize);

#[derive(Clone)]
#[cfg(not(feature = "vanilla"))]
pub struct Column {
    sections: [PalettedContainer; SECTIONS],
    blocks: Vec<VoxelBlock>,
    surface: Heightmap,
    motion: Heightmap,
    edited: bool,
    revision: u64,
    pub dirty_sections: u32,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(not(feature = "vanilla"))]
impl Column {
    fn new() -> Self {
        Self {
            sections: std::array::from_fn(|_| {
                PalettedContainer::uniform(block_tag(VoxelBlock::Air), 4096, 4)
                    .expect("valid block palette")
            }),
            blocks: vec![VoxelBlock::Air; 16 * HEIGHT as usize * 16],
            surface: Heightmap::new(HeightmapType::WorldSurface, 0, HEIGHT).expect("valid height"),
            motion: Heightmap::new(HeightmapType::MotionBlocking, 0, HEIGHT).expect("valid height"),
            edited: false,
            revision: 0,
            dirty_sections: 1,
        }
    }

    fn index(x: i32, y: i32, z: i32) -> usize {
        ((y * 16 + z) * 16 + x) as usize
    }

    fn get(&self, x: i32, y: i32, z: i32) -> VoxelBlock {
        self.blocks[Self::index(x, y, z)]
    }

    fn set(&mut self, x: i32, y: i32, z: i32, block: VoxelBlock) -> io::Result<()> {
        self.sections[y as usize / 16]
            .set(BlockPos::new(x, y, z).section_index(), block_tag(block))?;
        self.blocks[Self::index(x, y, z)] = block;
        Ok(())
    }

    fn prime(&mut self) {
        struct Blocks<'a>(&'a [VoxelBlock]);
        impl HeightmapChunk for Blocks<'_> {
            type State = VoxelBlock;
            fn min_y(&self) -> i32 {
                0
            }
            fn height(&self) -> i32 {
                HEIGHT
            }
            fn highest_section_y(&self) -> i32 {
                HEIGHT - 16
            }
            fn block_state(&self, x: i32, y: i32, z: i32) -> VoxelBlock {
                self.0[Column::index(x, y, z)]
            }
        }
        Heightmap::prime(
            &Blocks(&self.blocks),
            &mut [&mut self.surface, &mut self.motion],
        )
        .expect("heightmaps cover the column");
    }
}

#[derive(Debug, Default)]
pub struct StreamChange {
    pub generated: usize,
    pub read: usize,
    pub evicted: usize,
    pub saved: usize,
    pub read_us: u128,
    pub save_us: u128,
    pub generated_us: Vec<(Key, u128)>,
    #[cfg(feature = "vanilla-visuals")]
    pub(crate) features: crate::features::FeatureStats,
}

struct Departure {
    key: Key,
    revision: u64,
    edited: Option<Column>,
}

struct StreamPlan {
    center: Key,
    arrivals: Vec<Key>,
    departures: Vec<Departure>,
    player: Option<PlayerSave>,
}

pub struct PreparedStream {
    pub center: Key,
    pub elapsed: Duration,
    arrivals: Vec<(Key, Column)>,
    departures: Vec<(Key, u64)>,
    change: StreamChange,
    pub manual_save: bool,
}

/// One in-flight plan. Storage work runs on its own thread and never locks the live world.
pub struct StreamWorker {
    send: Sender<StreamPlan>,
    receive: Receiver<io::Result<PreparedStream>>,
    thread: JoinHandle<()>,
    pending: bool,
    pub awaiting: usize,
}

impl StreamWorker {
    pub fn new(world: &HorizonClientWorld) -> io::Result<Self> {
        #[cfg(not(feature = "vanilla"))]
        let storage = HorizonClientWorld::new(world.root.clone());
        #[cfg(feature = "vanilla")]
        let storage = HorizonClientWorld::new(world.root.clone(), world.vanilla.clone());
        let (send, requests) = mpsc::channel::<StreamPlan>();
        let (responses, receive) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("Switch chunk storage".into())
            .stack_size(4 << 20)
            .spawn(move || {
                #[cfg(feature = "vanilla")]
                let mut worker = vanilla::Generation::new(&storage.vanilla);
                while let Ok(plan) = requests.recv() {
                    if responses
                        .send(storage.prepare(
                            plan,
                            #[cfg(feature = "vanilla")]
                            &mut worker,
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            send,
            receive,
            thread,
            pending: false,
            awaiting: 0,
        })
    }

    pub fn busy(&self) -> bool {
        self.pending
    }

    pub fn request(&mut self, world: &mut HorizonClientWorld, center: Key) -> io::Result<()> {
        if !self.pending
            && let Some(plan) = world.plan(center)
        {
            let count = plan.arrivals.len();
            self.send
                .send(plan)
                .map_err(|_| io::Error::other("chunk worker stopped"))?;
            self.pending = true;
            self.awaiting = count;
        }
        Ok(())
    }

    pub fn poll(&mut self) -> io::Result<Option<PreparedStream>> {
        if !self.pending {
            return Ok(None);
        }
        match self.receive.try_recv() {
            Ok(response) => {
                self.pending = false;
                self.awaiting = 0;
                response.map(Some)
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::Error::other("chunk worker stopped")),
        }
    }

    #[cfg(feature = "vanilla")]
    pub fn request_save(
        &mut self,
        world: &HorizonClientWorld,
        player: PlayerSave,
    ) -> io::Result<()> {
        debug_assert!(!self.pending);
        let departures = world
            .chunks
            .iter()
            .filter(|(_, col)| col.edited)
            .map(|(&key, col)| Departure {
                key,
                revision: col.revision,
                edited: Some(col.clone()),
            })
            .collect();
        self.send
            .send(StreamPlan {
                center: player_chunk(player.0),
                arrivals: Vec::new(),
                departures,
                player: Some(player),
            })
            .map_err(|_| io::Error::other("chunk worker stopped"))?;
        self.pending = true;
        Ok(())
    }

    pub fn shutdown(self) -> io::Result<()> {
        drop(self.send);
        self.thread
            .join()
            .map_err(|_| io::Error::other("chunk worker panicked"))?;
        if self.pending {
            self.receive
                .recv()
                .map_err(|_| io::Error::other("chunk worker stopped"))??;
        }
        Ok(())
    }
}

pub struct HorizonClientWorld {
    root: PathBuf,
    #[cfg(not(feature = "vanilla"))]
    noise: PerlinNoise,
    #[cfg(feature = "vanilla")]
    pub vanilla: Arc<Vanilla>,
    tickets: ChunkTickets,
    center: Option<Key>,
    pub chunks: BTreeMap<Key, Column>,
    #[cfg(not(feature = "vanilla"))]
    pub beacon_pos: BlockPos,
    pub solid_shape: VoxelShape,
    pub empty_shape: VoxelShape,
    pub mesh_generations: u64,
}

pub fn player_chunk(pos: Vec3) -> Key {
    let chunk = BlockPos::new(pos.x.floor() as i32, 0, pos.z.floor() as i32).chunk();
    (chunk.x, chunk.z)
}

impl HorizonClientWorld {
    pub fn spawn_xz(&self) -> [i32; 2] {
        #[cfg(feature = "vanilla")]
        return self.vanilla.spawn;
        #[cfg(not(feature = "vanilla"))]
        [SPAWN_X, SPAWN_Z]
    }

    pub fn spawn_position(&self) -> Vec3 {
        let [x, z] = self.spawn_xz();
        #[cfg(feature = "vanilla")]
        let (x, z) = (-12i32..=12)
            .flat_map(|dx| (-12i32..=12).map(move |dz| (dx, dz)))
            .filter(|&(dx, dz)| {
                let y = self.surface_y(x + dx, z + dz);
                y >= MIN_Y && self.get_block(BlockPos::new(x + dx, y, z + dz)).is_solid()
            })
            .min_by_key(|&(dx, dz)| dx * dx + dz * dz)
            .map_or((x, z), |(dx, dz)| (x + dx, z + dz));
        Vec3::new(
            f64::from(x) + 0.5,
            f64::from(self.surface_y(x, z)) + 1.05,
            f64::from(z) + 0.5,
        )
    }
    #[cfg(feature = "vanilla")]
    pub fn new(root: PathBuf, vanilla: Arc<Vanilla>) -> Self {
        Self {
            root,
            vanilla,
            tickets: ChunkTickets::default(),
            center: None,
            chunks: BTreeMap::new(),
            solid_shape: VoxelShape::block(),
            empty_shape: VoxelShape::empty(),
            mesh_generations: 0,
        }
    }

    #[cfg(not(feature = "vanilla"))]
    pub fn new(root: PathBuf) -> Self {
        let noise = PerlinNoise::new(&mut LegacyRandom::new(SEED));
        let mut world = Self {
            root,
            noise,
            tickets: ChunkTickets::default(),
            center: None,
            chunks: BTreeMap::new(),
            beacon_pos: BlockPos::new(16, 0, 16),
            solid_shape: VoxelShape::block(),
            empty_shape: VoxelShape::empty(),
            mesh_generations: 0,
        };
        world.beacon_pos.y = world.terrain_height(16, 16) + 2;
        world
    }

    #[cfg(not(feature = "vanilla"))]
    fn terrain_height(&self, x: i32, z: i32) -> i32 {
        // The prototype's existing palette and Perlin terrain, sampled in world
        // coordinates so generating in any order gives the same shared edges.
        let n = self.noise.get2(f64::from(x) * 0.085, f64::from(z) * 0.085);
        (7 + (n * 5.0).round() as i32).clamp(3, 11)
    }

    #[cfg(not(feature = "vanilla"))]
    fn generate(&self, (cx, cz): Key) -> io::Result<Column> {
        let mut col = Column::new();
        for z in 0..16 {
            for x in 0..16 {
                let h = self.terrain_height(cx * 16 + x, cz * 16 + z);
                col.set(x, 0, z, VoxelBlock::Bedrock)?;
                for y in 1..=h {
                    let block = if y == h {
                        if h <= 4 {
                            VoxelBlock::Sand
                        } else {
                            VoxelBlock::GrassBlock
                        }
                    } else if y >= h - 2 {
                        VoxelBlock::Dirt
                    } else {
                        VoxelBlock::Stone
                    };
                    col.set(x, y, z, block)?;
                }
                for y in h + 1..=4 {
                    col.set(x, y, z, VoxelBlock::Water)?;
                }
            }
        }
        let b = self.beacon_pos;
        if (cx, cz) == (b.chunk().x, b.chunk().z) {
            col.set(
                b.x.rem_euclid(16),
                b.y - 1,
                b.z.rem_euclid(16),
                VoxelBlock::GoldBlock,
            )?;
            col.set(
                b.x.rem_euclid(16),
                b.y,
                b.z.rem_euclid(16),
                VoxelBlock::SeaLantern,
            )?;
        }
        col.prime();
        Ok(col)
    }

    fn region_path(&self, pos: ChunkPos) -> PathBuf {
        let region = pos.region();
        self.root
            .join("region")
            .join(format!("r.{}.{}.mca", region.x, region.z))
    }

    fn read_column(&self, pos: ChunkPos) -> io::Result<Option<Column>> {
        let path = self.region_path(pos);
        let size = match fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        // RegionFile follows vanilla and treats a short header as empty.
        // For these saves a short existing file means a failed write.
        if size < 8192 {
            return Err(invalid("truncated streaming region header"));
        }
        let mut region = RegionFile::open(path)?;
        let Some(record) = region.read(pos)? else {
            return Ok(None);
        };
        let tag = record.nbt(Limits::default())?.tag;
        if tag.get("SwitchWorldFormat") != Some(&Tag::Int(FORMAT))
            || tag.get("xPos") != Some(&Tag::Int(pos.x))
            || tag.get("zPos") != Some(&Tag::Int(pos.z))
        {
            return Err(invalid("invalid streaming column format or coordinates"));
        }
        #[cfg(feature = "vanilla")]
        {
            if tag.get("DataVersion") != Some(&Tag::Int(DATA_VERSION))
                || tag.get("Seed") != Some(&Tag::Long(SEED))
            {
                return Err(invalid("invalid vanilla column version or seed"));
            }
            Ok(Some(Column::from_tag(
                &tag,
                pos,
                self.vanilla.generator.data().clone(),
            )?))
        }
        #[cfg(not(feature = "vanilla"))]
        {
            let Some(Tag::List(sections)) = tag.get("sections") else {
                return Err(invalid("missing streaming column sections"));
            };
            let mut col = Column::new();
            let mut seen = [false; SECTIONS];
            for section in sections {
                let Some(&Tag::Byte(y)) = section.get("Y") else {
                    return Err(invalid("missing section Y"));
                };
                let i = y as usize;
                if i >= SECTIONS || seen[i] {
                    return Err(invalid("unexpected or duplicate section Y"));
                }
                seen[i] = true;
                let blocks = section
                    .get("block_states")
                    .ok_or_else(|| invalid("missing palette"))?;
                let palette = PalettedContainer::from_nbt(blocks, 4096, 4)?;
                for y in 0..16 {
                    for z in 0..16 {
                        for x in 0..16 {
                            let p = BlockPos::new(x, y, z);
                            let state = BlockState::from_nbt(palette.get(p.section_index())?)?;
                            let block = VoxelBlock::from_nbt_name(&state.name);
                            if block == VoxelBlock::Air && state.name != "minecraft:air" {
                                return Err(invalid("unsupported block in streaming save"));
                            }
                            col.blocks[Column::index(x, y + 16 * i as i32, z)] = block;
                        }
                    }
                }
                col.sections[i] = palette;
            }
            if !seen.into_iter().all(|present| present) {
                return Err(invalid("incomplete streaming column"));
            }
            col.prime();
            Ok(Some(col))
        }
    }

    fn write_column(&self, (x, z): Key, col: &Column) -> io::Result<()> {
        let pos = ChunkPos::new(x, z);
        let path = self.region_path(pos);
        fs::create_dir_all(path.parent().expect("region has parent"))?;
        #[cfg(not(feature = "vanilla"))]
        let sections = col
            .sections
            .iter()
            .enumerate()
            .map(|(i, palette)| {
                Tag::compound([
                    ("Y", Tag::Byte(i as i8)),
                    ("block_states", palette.to_nbt()),
                ])
            })
            .collect();
        #[cfg(feature = "vanilla")]
        let sections = col.sections()?;
        let mut entries = vec![
            ("SwitchWorldFormat", Tag::Int(FORMAT)),
            ("DataVersion", Tag::Int(DATA_VERSION as i32)),
            ("xPos", Tag::Int(x)),
            ("zPos", Tag::Int(z)),
            ("yPos", Tag::Int(MIN_Y / 16)),
            ("sections", Tag::List(sections)),
        ];
        #[cfg(feature = "vanilla")]
        entries.extend([("Seed", Tag::Long(SEED)), ("Heightmaps", col.heightmaps())]);
        let named = NamedTag {
            name: JavaString::default(),
            tag: Tag::compound(entries.drain(..)),
        };
        let record = ChunkRecord::from_nbt(&named, Compression::Zlib, 0, Limits::default())?;
        let mut region = RegionFile::open_writable(path, true)?;
        region.write(pos, &record)?;
        region.sync()
    }

    fn plan(&mut self, center: Key) -> Option<StreamPlan> {
        if self.center == Some(center) {
            return None;
        }
        self.tickets.update(Some(Sources {
            players: vec![center],
            view_distance: VIEW_DISTANCE,
            simulation_distance: VIEW_DISTANCE,
            ..Default::default()
        }));
        let levels = self.tickets.levels();
        let desired = |(x, z)| levels.loaded_entity_ticking(x, z);
        let mut arrivals = Vec::new();
        for [dx, dz] in spiral_around(VIEW_DISTANCE) {
            let key = (center.0 + dx, center.1 + dz);
            if !desired(key) || self.chunks.contains_key(&key) {
                continue;
            }
            arrivals.push(key);
        }
        let departures = self
            .chunks
            .iter()
            .filter(|(key, _)| !desired(**key))
            .map(|(&key, col)| Departure {
                key,
                revision: col.revision,
                edited: col.edited.then(|| col.clone()),
            })
            .collect();
        Some(StreamPlan {
            center,
            arrivals,
            departures,
            player: None,
        })
    }

    fn prepare(
        &self,
        plan: StreamPlan,
        #[cfg(feature = "vanilla")] worker: &mut vanilla::Generation,
    ) -> io::Result<PreparedStream> {
        let start = Instant::now();
        let mut change = StreamChange::default();
        let mut arrivals = Vec::with_capacity(plan.arrivals.len());
        for key in plan.arrivals {
            let pos = ChunkPos::new(key.0, key.1);
            let started = Instant::now();
            let read = self.read_column(pos)?;
            change.read_us += started.elapsed().as_micros();
            let col = match read {
                Some(col) => {
                    change.read += 1;
                    col
                }
                None => {
                    change.generated += 1;
                    let started = Instant::now();
                    #[cfg(not(feature = "vanilla"))]
                    let col = self.generate(key)?;
                    #[cfg(feature = "vanilla")]
                    let col = worker.generate(pos)?;
                    change
                        .generated_us
                        .push((key, started.elapsed().as_micros()));
                    col
                }
            };
            arrivals.push((key, col));
        }
        #[cfg(feature = "vanilla-visuals")]
        {
            change.features = worker.take_feature_stats();
        }
        let mut departures = Vec::with_capacity(plan.departures.len());
        for departure in plan.departures {
            if let Some(col) = departure.edited {
                let started = Instant::now();
                self.write_column(departure.key, &col)?;
                change.save_us += started.elapsed().as_micros();
                change.saved += 1;
            }
            departures.push((departure.key, departure.revision));
        }
        if let Some(player) = plan.player {
            let started = Instant::now();
            self.write_player(player)?;
            change.save_us += started.elapsed().as_micros();
        }
        Ok(PreparedStream {
            center: plan.center,
            elapsed: start.elapsed(),
            arrivals,
            departures,
            change,
            manual_save: plan.player.is_some(),
        })
    }

    pub fn apply_stream(&mut self, prepared: PreparedStream) -> Option<StreamChange> {
        if prepared.manual_save {
            for (key, revision) in prepared.departures {
                if let Some(col) = self.chunks.get_mut(&key)
                    && col.revision == revision
                {
                    col.edited = false;
                }
            }
            return Some(prepared.change);
        }
        // A live edit made during I/O must be persisted by a new plan before eviction.
        if prepared.departures.iter().any(|(key, revision)| {
            self.chunks
                .get(key)
                .is_none_or(|col| col.revision != *revision)
        }) {
            return None;
        }
        let mut change = prepared.change;
        for (key, _) in prepared.departures {
            self.chunks.remove(&key);
            self.invalidate_neighbors(key);
            change.evicted += 1;
        }
        for (key, col) in prepared.arrivals {
            self.chunks.insert(key, col);
            self.invalidate_neighbors(key);
        }
        self.center = Some(prepared.center);
        Some(change)
    }

    #[cfg(feature = "vanilla")]
    pub fn current_center(&self) -> Option<Key> {
        self.center
    }

    pub fn stream_to(&mut self, center: Key) -> io::Result<Option<StreamChange>> {
        let Some(plan) = self.plan(center) else {
            return Ok(None);
        };
        let prepared = self.prepare(
            plan,
            #[cfg(feature = "vanilla")]
            &mut vanilla::Generation::new(&self.vanilla),
        )?;
        Ok(self.apply_stream(prepared))
    }

    fn invalidate_neighbors(&mut self, (x, z): Key) {
        for key in [
            (x - 1, z),
            (x + 1, z),
            (x, z - 1),
            (x, z + 1),
            #[cfg(feature = "vanilla-water")]
            (x - 1, z - 1),
            #[cfg(feature = "vanilla-water")]
            (x - 1, z + 1),
            #[cfg(feature = "vanilla-water")]
            (x + 1, z - 1),
            #[cfg(feature = "vanilla-water")]
            (x + 1, z + 1),
        ] {
            if let Some(col) = self.chunks.get_mut(&key) {
                #[cfg(feature = "vanilla")]
                {
                    col.dirty_sections |= col
                        .terrain
                        .filled_sections()
                        .fold(0, |mask, i| mask | 1 << i);
                }
                #[cfg(not(feature = "vanilla"))]
                {
                    col.dirty_sections = 1;
                }
            }
        }
    }

    pub fn get_block(&self, p: BlockPos) -> VoxelBlock {
        if !(MIN_Y..MAX_Y).contains(&p.y) {
            return VoxelBlock::Air;
        }
        let key = p.chunk();
        self.chunks
            .get(&(key.x, key.z))
            .map_or(VoxelBlock::Air, |col| {
                let state = col.get(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16));
                #[cfg(feature = "vanilla")]
                return self.vanilla.visual(state);
                #[cfg(not(feature = "vanilla"))]
                state
            })
    }

    pub fn set_block(&mut self, p: BlockPos, block: VoxelBlock) -> bool {
        #[cfg(feature = "vanilla")]
        let same = self.state_at(p) == Some(self.vanilla.state(block));
        #[cfg(not(feature = "vanilla"))]
        let same = self.get_block(p) == block;
        if !(MIN_Y..MAX_Y).contains(&p.y) || same {
            return false;
        }
        let key = p.chunk();
        let key = (key.x, key.z);
        let Some(col) = self.chunks.get_mut(&key) else {
            return false;
        };
        let x = p.x.rem_euclid(16);
        let z = p.z.rem_euclid(16);
        #[cfg(feature = "vanilla")]
        col.set(p, self.vanilla.state(block))
            .expect("valid loaded block coordinates");
        #[cfg(not(feature = "vanilla"))]
        col.set(x, p.y, z, block)
            .expect("valid loaded block coordinates");
        #[cfg(not(feature = "vanilla"))]
        col.prime();
        col.edited = true;
        col.revision += 1;
        let section = (p.y - MIN_Y) / MESH_HEIGHT;
        let mut dirty = 1 << section;
        if p.y.rem_euclid(MESH_HEIGHT) == 0 && section > 0 {
            dirty |= 1 << (section - 1);
        }
        if p.y.rem_euclid(MESH_HEIGHT) == MESH_HEIGHT - 1 && section + 1 < HEIGHT / MESH_HEIGHT {
            dirty |= 1 << (section + 1);
        }
        col.dirty_sections |= dirty;
        if x == 0 || x == 15 || z == 0 || z == 15 {
            for (neighbor, edge) in [
                ((key.0 - 1, key.1), x == 0),
                ((key.0 + 1, key.1), x == 15),
                ((key.0, key.1 - 1), z == 0),
                ((key.0, key.1 + 1), z == 15),
                #[cfg(feature = "vanilla-water")]
                ((key.0 - 1, key.1 - 1), x == 0 && z == 0),
                #[cfg(feature = "vanilla-water")]
                ((key.0 - 1, key.1 + 1), x == 0 && z == 15),
                #[cfg(feature = "vanilla-water")]
                ((key.0 + 1, key.1 - 1), x == 15 && z == 0),
                #[cfg(feature = "vanilla-water")]
                ((key.0 + 1, key.1 + 1), x == 15 && z == 15),
            ] {
                if edge && let Some(col) = self.chunks.get_mut(&neighbor) {
                    #[cfg(feature = "vanilla-water")]
                    {
                        col.dirty_sections |= dirty;
                    }
                    #[cfg(not(feature = "vanilla-water"))]
                    {
                        col.dirty_sections |= 1 << section;
                    }
                }
            }
        }
        true
    }

    pub fn surface_y(&self, x: i32, z: i32) -> i32 {
        #[cfg(feature = "vanilla")]
        {
            let pos = BlockPos::new(x, 0, z).chunk();
            return self
                .chunks
                .get(&(pos.x, pos.z))
                .and_then(|col| col.terrain.heightmap(HeightmapType::WorldSurface))
                .and_then(|map| map.first_available(x & 15, z & 15).ok())
                .unwrap_or(MIN_Y)
                - 1;
        }
        #[cfg(not(feature = "vanilla"))]
        (0..HEIGHT)
            .rev()
            .find(|&y| self.get_block(BlockPos::new(x, y, z)).is_solid())
            .unwrap_or_else(|| self.terrain_height(x, z))
    }

    #[cfg(feature = "vanilla")]
    pub fn state_at(&self, p: BlockPos) -> Option<BlockStateId> {
        if !(MIN_Y..MAX_Y).contains(&p.y) {
            return None;
        }
        let pos = p.chunk();
        self.chunks
            .get(&(pos.x, pos.z))
            .map(|col| col.get(p.x, p.y, p.z))
    }

    pub fn collision_shape(&self, pos: BlockPos) -> &VoxelShape {
        #[cfg(feature = "vanilla")]
        if let Some(shape) = self
            .state_at(pos)
            .and_then(|state| self.vanilla.collision(state))
        {
            return shape;
        }
        if self.get_block(pos).is_solid() {
            &self.solid_shape
        } else {
            &self.empty_shape
        }
    }

    pub fn collect_colliders(&self, bounds: Aabb) -> Vec<VoxelShape> {
        let lower = |axis| bounds.min(axis).floor() as i32 - 1;
        let upper = |axis| bounds.max(axis).ceil() as i32 + 1;
        let mut shapes = Vec::new();
        for y in lower(Axis::Y).max(MIN_Y)..=upper(Axis::Y).min(MAX_Y - 1) {
            for z in lower(Axis::Z)..=upper(Axis::Z) {
                for x in lower(Axis::X)..=upper(Axis::X) {
                    let pos = BlockPos::new(x, y, z);
                    let shape = self.collision_shape(pos);
                    if !shape.is_empty() {
                        shapes.push(shape.moved(Vec3::from_block(pos)));
                    }
                }
            }
        }
        shapes
    }

    pub fn read_player(&self) -> io::Result<Option<PlayerSave>> {
        let data = match fs::read(self.root.join("level.dat")).or_else(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                fs::read(self.root.join("level.dat.bak"))
            } else {
                Err(e)
            }
        }) {
            Ok(data) => data,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let tag = nbt::read_named(&data, Limits::default())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
            .0
            .tag;
        if tag.get("SwitchWorldFormat") != Some(&Tag::Int(FORMAT)) {
            return Err(invalid("unsupported streaming world format"));
        }
        #[cfg(feature = "vanilla")]
        if tag.get("Seed") != Some(&Tag::Long(SEED)) {
            return Err(invalid("save belongs to a different world seed"));
        }
        let p = tag
            .get("Player")
            .ok_or_else(|| invalid("missing player state"))?;
        let coord = |key| match p.get(key) {
            Some(&Tag::Double(bits)) if f64::from_bits(bits).abs() < 29_999_984.0 => {
                Ok(f64::from_bits(bits))
            }
            _ => Err(invalid("invalid saved coordinate")),
        };
        let angle = |key| match p.get(key) {
            Some(&Tag::Float(bits)) if f32::from_bits(bits).is_finite() => Ok(f32::from_bits(bits)),
            _ => Err(invalid("invalid saved angle")),
        };
        let pos = Vec3::new(coord("x")?, coord("y")?, coord("z")?);
        #[cfg(not(feature = "first-person-polish"))]
        if !(f64::from(MIN_Y - 12)..=f64::from(MAX_Y + 96)).contains(&pos.y) {
            return Err(invalid("saved player outside streaming height"));
        }
        let slot = match p.get("SelectedItemSlot") {
            Some(&Tag::Int(n)) if (0..9).contains(&n) => n as usize,
            _ => return Err(invalid("invalid saved hotbar slot")),
        };
        Ok(Some((pos, angle("yaw")?, angle("pitch")?, slot)))
    }

    pub fn save(&mut self, player: PlayerSave) -> io::Result<usize> {
        let mut saved = 0;
        for key in self.chunks.keys().copied().collect::<Vec<_>>() {
            if self.chunks[&key].edited {
                self.write_column(key, &self.chunks[&key])?;
                self.chunks.get_mut(&key).expect("active column").edited = false;
                saved += 1;
            }
        }
        self.write_player(player)?;
        Ok(saved)
    }

    fn write_player(&self, (pos, yaw, pitch, slot): PlayerSave) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let named = NamedTag {
            name: JavaString::default(),
            tag: Tag::compound([
                ("SwitchWorldFormat", Tag::Int(FORMAT)),
                ("Seed", Tag::Long(SEED)),
                (
                    "Player",
                    Tag::compound([
                        ("x", Tag::Double(pos.x.to_bits())),
                        ("y", Tag::Double(pos.y.to_bits())),
                        ("z", Tag::Double(pos.z.to_bits())),
                        ("yaw", Tag::Float(yaw.to_bits())),
                        ("pitch", Tag::Float(pitch.to_bits())),
                        ("SelectedItemSlot", Tag::Int(slot as i32)),
                    ]),
                ),
            ]),
        };
        let tmp = self.root.join("level.dat.tmp");
        let bytes = nbt::write_named(&named, Limits::default())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        let current = self.root.join("level.dat");
        let backup = self.root.join("level.dat.bak");
        // libnx forwards rename to the filesystem service. Leave the
        // destination absent and retain the last complete player record.
        match fs::metadata(&current) {
            Ok(_) => {
                match fs::remove_file(&backup) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
                fs::rename(&current, backup)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::rename(tmp, current)?;
        Ok(())
    }
}

#[cfg(all(test, feature = "vanilla"))]
mod vanilla_tests {
    use super::*;

    fn await_stream(worker: &mut StreamWorker) -> io::Result<PreparedStream> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) {
            if let Some(prepared) = worker.poll()? {
                return Ok(prepared);
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err(io::Error::other("vanilla stream test timed out"))
    }

    fn fingerprint(parts: impl IntoIterator<Item = u8>) -> u64 {
        parts.into_iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
    }

    #[test]
    fn vanilla_stream_preserves_oracle_states_biomes_edits_and_save_revisions() -> io::Result<()> {
        let started = Instant::now();
        let root = PathBuf::from(std::env::var_os("HORIZON_VANILLA_ROOT").unwrap());
        let original = horizon_world::registry::BlockRegistry::from_facts_json(fs::File::open(
            root.join("facts.json"),
        )?)?;
        println!("BASELINE facts_reader_ms={}", started.elapsed().as_millis());
        let vanilla = Vanilla::load(&root, |line| println!("{line}"))?;
        let registry = vanilla.generator.data().registry();
        assert_eq!(registry.len(), original.len());
        assert!(
            (0..registry.len()).all(|id| registry.state(BlockStateId(id as u32))
                == original.state(BlockStateId(id as u32)))
        );
        drop(original);

        let save =
            PathBuf::from(std::env::var_os("HORIZON_STREAM_TEST_ROOT").unwrap()).join("vanilla");
        let mut world = HorizonClientWorld::new(save.clone(), vanilla.clone());
        let key = (-2, 3);
        let mut worker = StreamWorker::new(&world)?;
        worker.request(&mut world, key)?;
        let prepared = await_stream(&mut worker)?;
        let metrics = world.apply_stream(prepared).unwrap();
        println!(
            "STARTUP_3X3 generated_us={:?} read_us={}",
            metrics.generated_us, metrics.read_us
        );
        assert_eq!(world.chunks.len(), 9);
        let col = &world.chunks[&key];
        let blocks = (MIN_Y..MAX_Y)
            .flat_map(|y| {
                (0..16).flat_map(move |z| (0..16).map(move |x| col.get(x, y, z).0.to_le_bytes()))
            })
            .flatten();
        let biomes = (0..SECTIONS).flat_map(|i| {
            col.biomes
                .section(i)
                .unwrap()
                .iter()
                .flat_map(|id| col.data.biome_name(*id).bytes().chain([0]))
        });
        let values = fs::read_to_string(root.join("expected.txt"))?;
        let mut hashes = values.split_whitespace().skip(5);
        assert_eq!(
            fingerprint(blocks),
            u64::from_str_radix(hashes.next().unwrap(), 16).unwrap()
        );
        assert_eq!(
            fingerprint(biomes),
            u64::from_str_radix(hashes.next().unwrap(), 16).unwrap()
        );

        let edit = BlockPos::new(-17, -49, 63);
        #[cfg(feature = "vanilla-water")]
        let fluid_cells =
            [(0, 8, false), (5, 3, false), (12, 8, true)].map(|(level, amount, falling)| {
                let pos = edit.offset(-1 - level / 4, 0, 0);
                let mut water = registry
                    .state(vanilla.state(VoxelBlock::Water))
                    .unwrap()
                    .clone();
                water.properties.insert("level".into(), level.to_string());
                let id = registry.id(&water).unwrap();
                world.chunks.get_mut(&key).unwrap().set(pos, id).unwrap();
                (pos, id, amount, falling)
            });
        let adjacent = (-1, 3);
        for col in world.chunks.values_mut() {
            col.dirty_sections = 0;
        }
        assert!(world.set_block(edit, VoxelBlock::GoldBlock));
        assert_eq!(world.chunks[&key].dirty_sections, 0b11);
        #[cfg(feature = "vanilla-water")]
        assert_eq!(world.chunks[&(key.0 + 1, key.1 + 1)].dirty_sections, 0b11);
        assert_eq!(
            world.chunks[&adjacent].dirty_sections,
            if cfg!(feature = "vanilla-water") {
                0b11
            } else {
                1
            }
        );
        let player = (Vec3::new(-20.5, -21.0, 60.5), 25.0, -8.0, 3);
        worker.request_save(&world, player)?;
        assert!(world.set_block(edit, VoxelBlock::SeaLantern));
        let prepared = await_stream(&mut worker)?;
        assert!(prepared.manual_save);
        assert_eq!(world.apply_stream(prepared).unwrap().saved, 1);
        assert!(
            world.chunks[&key].edited,
            "save of old revision must leave the live edit dirty"
        );

        let before = world.chunks[&key].clone();
        worker.request(&mut world, (0, 3))?;
        let prepared = await_stream(&mut worker)?;
        assert!(world.apply_stream(prepared).unwrap().saved > 0);
        assert!(!world.chunks.contains_key(&key));
        worker.request(&mut world, key)?;
        let prepared = await_stream(&mut worker)?;
        assert_eq!(world.apply_stream(prepared).unwrap().read, 1);
        assert_eq!(world.get_block(edit), VoxelBlock::SeaLantern);
        #[cfg(feature = "vanilla-water")]
        for (pos, id, amount, falling) in fluid_cells {
            assert_eq!(world.state_at(pos), Some(id));
            let fluid = vanilla.fluids.get(id).unwrap();
            assert_eq!(fluid.amount(), amount);
            assert_eq!(fluid.falling(), falling);
        }
        let after = &world.chunks[&key];
        for i in 0..SECTIONS {
            assert_eq!(before.biomes.section(i), after.biomes.section(i));
            for index in 0..4096 {
                assert_eq!(
                    before.terrain.section(i).unwrap().get(index)?,
                    after.terrain.section(i).unwrap().get(index)?
                );
            }
        }
        assert_eq!(before.heightmaps(), after.heightmaps());
        worker.shutdown()?;
        let reloaded = HorizonClientWorld::new(save, vanilla);
        assert_eq!(reloaded.read_player()?, Some(player));
        Ok(())
    }
}

#[cfg(all(test, not(feature = "vanilla")))]
mod tests {
    use super::*;

    #[test]
    fn async_eviction_retries_when_the_live_column_changes() -> io::Result<()> {
        let save = test_root("async-eviction");
        let mut world = HorizonClientWorld::new(save.clone());
        world.stream_to((0, 0))?;
        let p = BlockPos::new(-1, 20, 0);
        assert!(world.set_block(p, VoxelBlock::GoldBlock));

        let mut worker = StreamWorker::new(&world)?;
        worker.request(&mut world, (1, 0))?;
        assert!(worker.busy());
        // This edit must survive even if the worker already saved the earlier snapshot.
        assert!(world.set_block(p, VoxelBlock::Stone));
        let first = wait_for_stream(&mut worker)?;
        assert_eq!(first.center, (1, 0));
        assert!(world.apply_stream(first).is_none());
        assert_eq!(world.get_block(p), VoxelBlock::Stone);
        assert_eq!(world.chunks.len(), 9);

        worker.request(&mut world, (1, 0))?;
        let second = wait_for_stream(&mut worker)?;
        assert_eq!(world.apply_stream(second).unwrap().saved, 1);
        assert_eq!(world.get_block(p), VoxelBlock::Air);
        worker.shutdown()?;

        let mut reopened = HorizonClientWorld::new(save);
        assert_eq!(reopened.stream_to((0, 0))?.unwrap().read, 1);
        assert_eq!(reopened.get_block(p), VoxelBlock::Stone);
        Ok(())
    }

    fn wait_for_stream(worker: &mut StreamWorker) -> io::Result<PreparedStream> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(prepared) = worker.poll()? {
                return Ok(prepared);
            }
            assert!(Instant::now() < deadline, "chunk worker timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn crossing_chunk_boundaries_reloads_evicted_edits() -> io::Result<()> {
        let save = test_root("crossing");
        let mut world = HorizonClientWorld::new(save.clone());
        assert_eq!(player_chunk(Vec3::new(-0.1, 8.0, -16.1)), (-1, -2));
        assert_eq!(world.stream_to((0, 0))?.unwrap().generated, 9);
        assert_eq!(world.chunks.len(), 9);
        assert!(world.stream_to((0, 0))?.is_none());
        let negative = BlockPos::new(-1, 20, -1);
        let positive = BlockPos::new(15, 20, 15);
        assert_eq!(world.get_block(negative), VoxelBlock::Air);
        assert!(world.set_block(negative, VoxelBlock::GoldBlock));
        assert!(world.set_block(positive, VoxelBlock::Stone));
        let crossing = world.stream_to((1, 0))?.unwrap();
        assert_eq!(
            (crossing.generated, crossing.evicted, crossing.saved),
            (3, 3, 1)
        );
        assert_eq!(world.chunks.len(), 9);
        assert_eq!(world.get_block(negative), VoxelBlock::Air);
        assert_eq!(world.get_block(positive), VoxelBlock::Stone);
        assert!(save.join("region/r.-1.-1.mca").is_file());
        assert_eq!(world.stream_to((0, 0))?.unwrap().read, 1);
        assert_eq!(world.get_block(negative), VoxelBlock::GoldBlock);

        let saved = (Vec3::new(-0.5, 12.0, -0.5), 15.0, -8.0, 3);
        assert_eq!(world.save(saved)?, 1);
        assert_eq!(world.save(saved)?, 0);
        let mut reopened = HorizonClientWorld::new(save.clone());
        assert_eq!(reopened.read_player()?, Some(saved));
        assert_eq!(reopened.stream_to((0, 0))?.unwrap().read, 2);
        assert_eq!(reopened.get_block(negative), VoxelBlock::GoldBlock);
        assert_eq!(reopened.get_block(positive), VoxelBlock::Stone);
        fs::remove_file(save.join("level.dat"))?;
        assert_eq!(reopened.read_player()?, Some(saved));
        Ok(())
    }

    #[test]
    fn streaming_failure_retains_live_columns_and_edits() -> io::Result<()> {
        let save = test_root("failure");
        let mut world = HorizonClientWorld::new(save.clone());
        world.stream_to((0, 0))?;
        let p = BlockPos::new(0, 20, 0);
        assert!(world.set_block(p, VoxelBlock::GoldBlock));
        fs::create_dir_all(save.join("region"))?;
        let bad = save.join("region/r.1.0.mca");
        fs::write(&bad, b"bad region")?;
        assert!(world.stream_to((32, 0)).is_err());
        assert_eq!(world.chunks.len(), 9);
        assert_eq!(world.get_block(p), VoxelBlock::GoldBlock);
        assert_eq!(fs::read(bad)?, b"bad region");
        assert_eq!(world.save((Vec3::new(0.5, 12.0, 0.5), 0.0, 0.0, 0))?, 1);
        Ok(())
    }

    #[test]
    fn generating_from_either_side_keeps_terrain_identical() -> io::Result<()> {
        let save = test_root("generation");
        let mut world = HorizonClientWorld::new(save.clone());
        world.stream_to((0, 0))?;
        let before = world.chunks[&(1, 0)].blocks.clone();
        assert_eq!(
            world.get_block(BlockPos::new(16, 0, 0)),
            VoxelBlock::Bedrock
        );
        world.stream_to((-2, 0))?;
        assert!(!world.chunks.contains_key(&(1, 0)));
        world.stream_to((2, 0))?;
        assert_eq!(world.chunks[&(1, 0)].blocks, before);
        assert!(!save.join("region").exists());
        Ok(())
    }

    fn test_root(name: &str) -> PathBuf {
        PathBuf::from(
            std::env::var_os("HORIZON_STREAM_TEST_ROOT").expect("isolated test directory"),
        )
        .join(name)
    }
}
