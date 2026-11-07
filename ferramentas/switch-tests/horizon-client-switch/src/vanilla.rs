use std::{fs, io, path::Path, sync::Arc, time::Instant};

use horizon_core::{
    geometry::VoxelShape,
    position::{BlockPos, ChunkPos},
};
use horizon_world::{
    chunk::{BlockSection, PalettedContainer},
    heightmap::{HeightmapState, HeightmapType},
    nbt::Tag,
    registry::{BlockRegistry, BlockStateId},
    worldgen::{
        biome::BiomeId,
        biome_chunk::BiomeChunk,
        generator::{
            GenBlockStates, GeneratorData, GeneratorWorker, OverworldGenerator, ProtoChunk,
        },
        terrain::{PreLightChunkStage, TerrainChunk},
    },
};

use crate::{
    block_palette::VoxelBlock,
    world_stream::{HEIGHT, MIN_Y, SECTIONS, SEED},
};

pub struct Vanilla {
    pub generator: OverworldGenerator,
    pub spawn: [i32; 2],
    #[cfg(feature = "vanilla-visuals")]
    pub decorated: bool,
    #[cfg(feature = "vanilla-visuals")]
    pub biome_visuals: Box<[crate::visuals::BiomeVisual]>,
    #[cfg(feature = "vanilla-visuals")]
    pub visual_states: Box<[crate::visuals::VisualState]>,
    visuals: Box<[VoxelBlock]>,
    #[cfg(feature = "vanilla-water")]
    pub fluids: horizon_world::fluid::FluidStateTable,
    #[cfg(feature = "vanilla-water")]
    pub water_colors: Box<[u32]>,
    #[cfg(feature = "first-person-polish")]
    pub water_fog_colors: Box<[u32]>,
}

impl Vanilla {
    pub fn load(root: &Path, log: impl Fn(&str)) -> io::Result<Arc<Self>> {
        Self::load_mode(root, log, false)
    }

    #[cfg(feature = "vanilla-visuals")]
    pub fn load_decorated(root: &Path, log: impl Fn(&str)) -> io::Result<Arc<Self>> {
        Self::load_mode(root, log, true)
    }

    fn load_mode(root: &Path, log: impl Fn(&str), decorated: bool) -> io::Result<Arc<Self>> {
        let start = Instant::now();
        let bytes = fs::read(root.join("facts.json"))?;
        let read = start.elapsed();
        let parse = Instant::now();
        let (registry, _) = BlockRegistry::from_facts_json_bytes(&bytes)?;
        drop(bytes);
        let registry = Box::leak(Box::new(registry));
        log(&format!(
            "INIT registry_read_ms={} registry_parse_ms={} states={}",
            read.as_millis(),
            parse.elapsed().as_millis(),
            registry.len()
        ));
        let start = Instant::now();
        let profile = |stage: &str, elapsed: std::time::Duration| {
            log(&format!(
                "INIT_TABLE stage={stage} elapsed_ms={}",
                elapsed.as_millis()
            ));
        };
        let path = root.join("worldgen");
        let data = if decorated {
            GeneratorData::load_profiled_without_structures(&path, registry, &profile)
        } else {
            GeneratorData::load_profiled(&path, registry, &profile)
        }
        .map_err(io::Error::other)?;
        log(&format!(
            "INIT worldgen_tables_ms={}",
            start.elapsed().as_millis()
        ));
        for warning in data.warnings() {
            log(&format!("WARN worldgen {warning}"));
        }
        let start = Instant::now();
        let generator = if decorated {
            OverworldGenerator::without_structures(data.clone(), SEED)
        } else {
            OverworldGenerator::new(data.clone(), SEED)
        }
        .map_err(io::Error::other)?;
        if generator.level() != (MIN_Y, HEIGHT) {
            return Err(invalid("unexpected 26.3 overworld height"));
        }
        let spawn = generator.spawn_position().map_err(io::Error::other)?;
        let visuals = (0..registry.len())
            .map(|i| {
                let id = BlockStateId(i as u32);
                let facts = data.block_states().get(id);
                if facts.is_air() {
                    return VoxelBlock::Air;
                }
                let name = &registry.state(id).expect("registered state").name;
                let visual = VoxelBlock::from_nbt_name(name);
                if visual == VoxelBlock::Air {
                    #[cfg(feature = "vanilla-visuals")]
                    if decorated
                        && data
                            .cached_collision_shape(id)
                            .is_some_and(|shape| !shape.is_block())
                    {
                        return VoxelBlock::Air;
                    }
                    VoxelBlock::Stone
                } else {
                    visual
                }
            })
            .collect();
        #[cfg(feature = "vanilla-water")]
        let fluids = horizon_world::fluid::FluidStateTable::new(registry)?;
        #[cfg(feature = "vanilla-water")]
        let water_colors = (0..data.biome_names().len())
            .map(|id| crate::water::biome_water_color(data.biome_json(BiomeId(id as u32))))
            .collect::<io::Result<_>>()?;
        #[cfg(feature = "first-person-polish")]
        let water_fog_colors = (0..data.biome_names().len())
            .map(|id| crate::water::biome_water_fog_color(data.biome_json(BiomeId(id as u32))))
            .collect::<io::Result<_>>()?;
        log(&format!(
            "INIT generator_ms={} seed={SEED} min_y={MIN_Y} height={HEIGHT} sea_level={}",
            start.elapsed().as_millis(),
            generator.sea_level()
        ));
        #[cfg(feature = "vanilla-visuals")]
        let (biome_visuals, visual_states) = if decorated {
            (
                crate::visuals::load_biomes(root, &data)?,
                crate::visuals::load_states(root, registry.len())?,
            )
        } else {
            (Box::default(), Box::default())
        };
        Ok(Arc::new(Self {
            generator,
            spawn,
            #[cfg(feature = "vanilla-visuals")]
            decorated,
            #[cfg(feature = "vanilla-visuals")]
            biome_visuals,
            #[cfg(feature = "vanilla-visuals")]
            visual_states,
            visuals,
            #[cfg(feature = "vanilla-water")]
            fluids,
            #[cfg(feature = "vanilla-water")]
            water_colors,
            #[cfg(feature = "first-person-polish")]
            water_fog_colors,
        }))
    }

    pub fn visual(&self, state: BlockStateId) -> VoxelBlock {
        self.visuals[state.0 as usize]
    }

    pub fn state(&self, block: VoxelBlock) -> BlockStateId {
        self.generator
            .data()
            .registry()
            .default_state(&format!("minecraft:{}", block.block_name()))
            .expect("prototype hotbar block in the vanilla registry")
    }

    pub fn collision(&self, state: BlockStateId) -> Option<&VoxelShape> {
        self.generator.data().cached_collision_shape(state)
    }
}

#[derive(Clone)]
pub struct Column {
    pub terrain: Arc<TerrainChunk<'static, GenBlockStates>>,
    pub biomes: Arc<BiomeChunk<BiomeId>>,
    pub data: Arc<GeneratorData>,
    pub edited: bool,
    pub revision: u64,
    pub dirty_sections: u32,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub struct Generation {
    worker: GeneratorWorker,
    #[cfg(feature = "vanilla-visuals")]
    features: Option<crate::features::FeatureCache>,
}

impl Generation {
    #[cfg(feature = "vanilla-visuals")]
    pub fn take_feature_stats(&mut self) -> crate::features::FeatureStats {
        self.features
            .as_mut()
            .map_or_else(Default::default, crate::features::FeatureCache::take_stats)
    }

    pub fn new(vanilla: &Vanilla) -> Self {
        Self {
            worker: vanilla.generator.worker(),
            #[cfg(feature = "vanilla-visuals")]
            features: vanilla
                .decorated
                .then(crate::features::FeatureCache::default),
        }
    }

    pub fn generate(&mut self, pos: ChunkPos) -> io::Result<Column> {
        #[cfg(feature = "vanilla-visuals")]
        if let Some(cache) = &mut self.features {
            return cache
                .column(&mut self.worker, pos)
                .map_err(io::Error::other);
        }
        self.worker
            .generate_proto(pos)
            .map(Column::from_proto)
            .map_err(io::Error::other)
    }
}

impl Column {
    pub(super) fn from_proto(proto: ProtoChunk) -> Self {
        let biomes = Arc::new(proto.biomes().clone());
        let data = proto.data().clone();
        Self::new(proto.terrain, biomes, data)
    }

    fn new(
        terrain: TerrainChunk<'static, GenBlockStates>,
        biomes: Arc<BiomeChunk<BiomeId>>,
        data: Arc<GeneratorData>,
    ) -> Self {
        let dirty_sections = terrain.filled_sections().fold(0, |mask, i| mask | 1 << i);
        Self {
            terrain: Arc::new(terrain),
            biomes,
            data,
            edited: false,
            revision: 0,
            dirty_sections,
        }
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        self.terrain.block(x, y, z)
    }

    pub fn set(&mut self, pos: BlockPos, state: BlockStateId) -> io::Result<()> {
        Arc::make_mut(&mut self.terrain)
            .set_block_worldgen(pos.x, pos.y, pos.z, state)
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub fn from_tag(tag: &Tag, pos: ChunkPos, data: Arc<GeneratorData>) -> io::Result<Self> {
        let Some(Tag::List(sections)) = tag.get("sections") else {
            return Err(invalid("missing sections"));
        };
        if sections.len() != SECTIONS || tag.get("yPos") != Some(&Tag::Int(MIN_Y / 16)) {
            return Err(invalid("invalid vanilla section range"));
        }
        let registry = data.registry();
        let mut terrain =
            TerrainChunk::new(pos, MIN_Y, HEIGHT, registry, data.block_states().clone())
                .map_err(io::Error::other)?;
        let mut quart = [[BiomeId(0); 64]; SECTIONS];
        let mut seen = 0u32;
        let air = registry
            .default_state("minecraft:air")
            .ok_or_else(|| invalid("air"))?;
        for section in sections {
            let Some(&Tag::Byte(y)) = section.get("Y") else {
                return Err(invalid("missing section Y"));
            };
            let index = i32::from(y) - MIN_Y / 16;
            if !(0..SECTIONS as i32).contains(&index) || seen & (1 << index) != 0 {
                return Err(invalid("invalid or duplicate vanilla section Y"));
            }
            seen |= 1 << index;
            let blocks = BlockSection::from_nbt(
                section
                    .get("block_states")
                    .ok_or_else(|| invalid("missing block palette"))?,
                registry,
            )?;
            if blocks.any_state(|state| state != air) {
                for i in 0..4096 {
                    let state = blocks.get(i)?;
                    if state != air {
                        terrain
                            .set_block_raw(
                                (i & 15) as i32,
                                i32::from(y) * 16 + (i >> 8) as i32,
                                ((i >> 4) & 15) as i32,
                                state,
                            )
                            .map_err(io::Error::other)?;
                    }
                }
            }
            let biomes = PalettedContainer::from_nbt(
                section
                    .get("biomes")
                    .ok_or_else(|| invalid("missing biome palette"))?,
                64,
                1,
            )?;
            for (i, out) in quart[index as usize].iter_mut().enumerate() {
                let Tag::String(name) = biomes.get(i)? else {
                    return Err(invalid("invalid biome name"));
                };
                *out = data
                    .biome_id(&name.to_string().map_err(io::Error::other)?)
                    .ok_or_else(|| invalid("unregistered biome"))?;
            }
        }
        let mut biomes =
            BiomeChunk::new(pos, MIN_Y, HEIGHT, BiomeId(0)).map_err(io::Error::other)?;
        biomes
            .fill(MIN_Y, MIN_Y + HEIGHT - 1, |[x, y, z]| {
                let ly = y - MIN_Y / 4;
                let cell = ((ly & 3) * 16 + (z & 3) * 4 + (x & 3)) as usize;
                Ok::<_, std::convert::Infallible>(quart[(ly / 4) as usize][cell])
            })
            .map_err(io::Error::other)?;
        for kind in HeightmapType::ALL {
            let Some(Tag::LongArray(words)) =
                tag.get("Heightmaps").and_then(|maps| maps.get(kind.name()))
            else {
                return Err(invalid("missing vanilla heightmap"));
            };
            let words = words.iter().map(|&word| word as u64).collect::<Vec<_>>();
            terrain
                .restore_heightmap(kind, &words)
                .map_err(io::Error::other)?;
        }
        terrain.set_generation_stage(PreLightChunkStage::Terrain);
        Ok(Self::new(terrain, Arc::new(biomes), data))
    }

    pub fn sections(&self) -> io::Result<Vec<Tag>> {
        (0..SECTIONS)
            .map(|i| {
                let blocks = self.terrain.section(i).expect("full terrain height");
                let biomes = self.biomes.section(i).expect("full biome height");
                let name = |i: usize| Tag::String(self.data.biome_name(biomes[i]).into());
                let mut palette = PalettedContainer::uniform(name(0), 64, 1)?;
                for i in 1..64 {
                    palette.set(i, name(i))?;
                }
                Ok(Tag::compound([
                    ("Y", Tag::Byte((MIN_Y / 16 + i as i32) as i8)),
                    ("block_states", blocks.to_nbt()?),
                    ("biomes", palette.to_nbt()),
                ]))
            })
            .collect()
    }

    pub fn heightmaps(&self) -> Tag {
        Tag::compound(HeightmapType::ALL.map(|kind| {
            let map = self
                .terrain
                .heightmap(kind)
                .expect("complete column heightmaps");
            (
                kind.name(),
                Tag::LongArray(map.raw_data().iter().map(|&word| word as i64).collect()),
            )
        }))
    }
}
