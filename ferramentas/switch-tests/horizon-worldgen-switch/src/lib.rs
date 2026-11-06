use horizon_core::position::ChunkPos;
use horizon_world::{
    DATA_VERSION,
    chunk::{BlockSection, ChunkData, PalettedContainer},
    heightmap::HeightmapType,
    nbt::{self, Limits, NamedTag, Tag},
    registry::BlockRegistry,
    worldgen::{
        generator::{GeneratorData, OverworldGenerator, ProtoChunk},
        terrain::{PreLightChunkStage, TerrainBlockStates},
    },
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::Path,
    time::Instant,
};

type ProbeResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Fingerprint(u64);

impl Fingerprint {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
}

fn snapshot(chunk: &ProtoChunk, pos: ChunkPos, min_y: i32, height: i32) -> ProbeResult<NamedTag> {
    let terrain = &chunk.terrain;
    let mut sections = Vec::new();
    for index in 0..(height / 16) as usize {
        let blocks = terrain.section(index).ok_or("missing block section")?;
        let biomes = chunk
            .biomes()
            .section(index)
            .ok_or("missing biome section")?;
        let name = |i: usize| Tag::String(chunk.data().biome_name(biomes[i]).into());
        let mut palette = PalettedContainer::uniform(name(0), 64, 1)?;
        for i in 1..64 {
            palette.set(i, name(i))?;
        }
        sections.push(Tag::compound([
            ("Y", Tag::Byte(i8::try_from(min_y / 16 + index as i32)?)),
            ("block_states", blocks.to_nbt()?),
            ("biomes", palette.to_nbt()),
        ]));
    }
    if terrain.section(sections.len()).is_some() || chunk.biomes().section(sections.len()).is_some()
    {
        return Err("unexpected extra section".into());
    }
    let mut heightmaps = Vec::new();
    for kind in HeightmapType::ALL {
        let map = terrain
            .heightmap(kind)
            .ok_or_else(|| format!("missing {kind:?}"))?;
        for z in 0..16 {
            for x in 0..16 {
                let expected = (min_y..min_y + height)
                    .rev()
                    .find(|&y| {
                        let block = chunk.block(pos.x * 16 + x, y, pos.z * 16 + z);
                        kind.is_opaque(chunk.data().block_states().heightmap_state(block))
                    })
                    .map_or(min_y, |y| y + 1);
                if map.first_available(x, z)? != expected {
                    return Err(format!("{kind:?} differs from block scan at {x},{z}").into());
                }
            }
        }
        heightmaps.push((
            kind.name(),
            Tag::LongArray(map.raw_data().iter().map(|&word| word as i64).collect()),
        ));
    }
    Ok(NamedTag {
        name: "".into(),
        tag: Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            ("xPos", Tag::Int(pos.x)),
            ("zPos", Tag::Int(pos.z)),
            ("sections", Tag::List(sections)),
            ("Heightmaps", Tag::compound(heightmaps)),
        ]),
    })
}

fn check_serialization(chunk: &ProtoChunk, root: &NamedTag) -> ProbeResult<usize> {
    let bytes = nbt::write_named(root, Limits::default())?;
    let (decoded, consumed) = nbt::read_named(&bytes, Limits::default())?;
    if consumed != bytes.len() || &decoded != root {
        return Err("NBT round trip changed the snapshot".into());
    }
    let saved = ChunkData::new(decoded)?;
    if saved.position() != chunk.position() {
        return Err("serialized position changed".into());
    }
    let Some(Tag::List(sections)) = saved.root().tag.get("sections") else {
        return Err("decoded sections missing".into());
    };
    for (i, section) in sections.iter().enumerate() {
        let original = chunk.terrain.section(i).ok_or("missing original section")?;
        let blocks = BlockSection::from_nbt(
            section.get("block_states").ok_or("missing saved blocks")?,
            chunk.data().registry(),
        )?;
        for index in 0..4096 {
            if blocks.get(index)? != original.get(index)? {
                return Err(format!("block palette changed in section {i}").into());
            }
        }
        let biomes = PalettedContainer::from_nbt(
            section.get("biomes").ok_or("missing saved biomes")?,
            64,
            1,
        )?;
        for (index, biome) in chunk
            .biomes()
            .section(i)
            .ok_or("missing original biomes")?
            .iter()
            .enumerate()
        {
            if biomes.get(index)? != &Tag::String(chunk.data().biome_name(*biome).into()) {
                return Err(format!("biome palette changed in section {i}").into());
            }
        }
    }
    Ok(bytes.len())
}

fn fingerprint(
    chunk: &ProtoChunk,
    pos: ChunkPos,
    min: i32,
    height: i32,
    log: &impl Fn(&str),
) -> (u64, u64) {
    let mut block_hash = Fingerprint::new();
    let mut biome_hash = Fingerprint::new();
    let mut blocks = BTreeMap::new();
    let mut biomes = BTreeMap::new();
    for y in min..min + height {
        for z in pos.z * 16..pos.z * 16 + 16 {
            for x in pos.x * 16..pos.x * 16 + 16 {
                block_hash.push(&chunk.block(x, y, z).0.to_le_bytes());
                *blocks.entry(chunk.block_name(x, y, z)).or_insert(0) += 1;
            }
        }
    }
    for y in (min..min + height).step_by(4) {
        for z in (pos.z * 16..pos.z * 16 + 16).step_by(4) {
            for x in (pos.x * 16..pos.x * 16 + 16).step_by(4) {
                let biome = chunk.biome_at(x, y, z);
                biome_hash.push(biome.as_bytes());
                biome_hash.push(&[0]);
                *biomes.entry(biome).or_insert(0) += 1;
            }
        }
    }
    log(&format!("BLOCKS {blocks:?}"));
    log(&format!("BIOMES {biomes:?}"));
    (block_hash.0, biome_hash.0)
}

pub fn run(root: &Path, log: &impl Fn(&str)) -> ProbeResult<()> {
    let expected = fs::read_to_string(root.join("expected.txt"))?;
    let fields: Vec<_> = expected.split_whitespace().collect();
    let &[seed, x, z, min, height, blocks, biomes] = fields.as_slice() else {
        return Err("invalid Java terrain fixture".into());
    };
    let seed = seed.parse::<i64>()?;
    let pos = ChunkPos::new(x.parse()?, z.parse()?);
    let (min, height) = (min.parse::<i32>()?, height.parse::<i32>()?);
    let oracle = (
        u64::from_str_radix(blocks, 16)?,
        u64::from_str_radix(biomes, 16)?,
    );
    log(&format!(
        "START Minecraft 26.3 data_version={DATA_VERSION} overworld seed={seed} chunk=({}, {})",
        pos.x, pos.z
    ));
    memory("before_load", log);
    let start = Instant::now();
    let registry = Box::leak(Box::new(BlockRegistry::from_facts_json(File::open(
        root.join("facts.json"),
    )?)?));
    let data = GeneratorData::load(&root.join("worldgen"), registry)?;
    for warning in data.warnings() {
        log(&format!("WARN worldgen {warning}"));
    }
    let generator = OverworldGenerator::new(data, seed)?;
    log(&format!(
        "SETTINGS min_y={} height={} sea_level={} states={} init_ms={}",
        generator.level().0,
        generator.level().1,
        generator.sea_level(),
        registry.len(),
        start.elapsed().as_millis()
    ));
    memory("after_load", log);
    if generator.level() != (min, height) {
        return Err("generator height differs from the Java reference".into());
    }
    let mut baseline = None;
    for attempt in 1..=2 {
        generator.release_caches();
        let start = Instant::now();
        let mut worker = generator.worker();
        let chunk = worker.generate_proto(pos)?;
        let timings = worker.stage_timings();
        log(&format!(
            "GENERATED attempt={attempt} elapsed_ms={} chunk_heap_bytes={} cache_biome_bytes={}",
            start.elapsed().as_millis(),
            chunk.heap_bytes().iter().sum::<usize>(),
            generator.cache_census().biome_bytes
        ));
        log(&format!(
            "STAGES biomes_us={} noise_us={} surface_us={} structures_us={} carvers_us={}",
            timings.biomes.as_micros(),
            timings.noise.as_micros(),
            timings.surface.as_micros(),
            timings.structures.as_micros(),
            timings.carvers.as_micros(),
        ));
        memory("after_generation", log);
        if chunk.position() != pos
            || chunk.terrain.min_y() != min
            || chunk.terrain.height() != height
            || chunk.terrain.generation_stage() != PreLightChunkStage::Terrain
        {
            return Err("unexpected chunk position, dimension or stage".into());
        }
        let (block_hash, biome_hash) = fingerprint(&chunk, pos, min, height, log);
        log(&format!(
            "FINGERPRINT blocks={block_hash:016x} biomes={biome_hash:016x}"
        ));
        if (block_hash, biome_hash) != oracle {
            return Err(format!(
                "terrain differs from Java reference {:016x}/{:016x}",
                oracle.0, oracle.1
            )
            .into());
        }
        let root = snapshot(&chunk, pos, min, height)?;
        if let Some(first) = &baseline {
            if first != &root {
                return Err("second generation changed the NBT snapshot".into());
            }
        } else {
            let bytes = check_serialization(&chunk, &root)?;
            log(&format!(
                "PASS NBT round trip sections={} heightmaps=6 bytes={bytes}",
                height / 16
            ));
            baseline = Some(root);
        }
    }
    log("PASS Java terrain and biome reference, heightmaps, NBT and determinism");
    Ok(())
}

#[cfg(not(target_os = "horizon"))]
fn memory(_: &str, _: &impl Fn(&str)) {}

#[cfg(target_os = "horizon")]
unsafe extern "C" {
    fn probe_log(line: *const libc::c_char);
    fn horizon_switch_romfs_mounted() -> i32;
    fn horizon_worldgen_refresh();
    fn horizon_worldgen_memory() -> u64;
}

#[cfg(target_os = "horizon")]
fn memory(stage: &str, log: &impl Fn(&str)) {
    let used = unsafe { horizon_worldgen_memory() };
    if used != 0 {
        log(&format!("MEMORY stage={stage} process_used_bytes={used}"));
    }
}

#[cfg(target_os = "horizon")]
#[unsafe(no_mangle)]
pub extern "C" fn rust_horizon_worldgen_main() -> i32 {
    fn log(line: &str) {
        let line = std::ffi::CString::new(line.replace('\0', "?")).unwrap();
        unsafe { probe_log(line.as_ptr()) };
    }
    if unsafe { horizon_switch_romfs_mounted() } == 0 {
        log("FAIL worldgen RomFS did not mount");
        return 1;
    }
    let (send, receive) = std::sync::mpsc::sync_channel(32);
    let worker = std::thread::Builder::new()
        .name("vanilla-worldgen".into())
        .stack_size(4 << 20)
        .spawn(move || {
            run(Path::new("romfs:/"), &|line| {
                let _ = send.send(line.to_owned());
            })
        });
    let worker = match worker {
        Ok(worker) => worker,
        Err(error) => {
            log(&format!("FAIL could not start worldgen worker: {error}"));
            return 1;
        }
    };
    loop {
        match receive.recv_timeout(std::time::Duration::from_millis(16)) {
            Ok(line) => log(&line),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => unsafe {
                horizon_worldgen_refresh()
            },
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    match worker.join() {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            log(&format!("FAIL {error:#}"));
            1
        }
        Err(_) => {
            log("FAIL worldgen worker panicked");
            1
        }
    }
}
