use horizon_world::{
    collision::CachedCollisionRegistry,
    registry::{BlockRegistry, BlockTableFile},
};
use std::{fs, path::Path, time::Instant};

#[test]
#[ignore = "opt-in timing probe with prepared 26.3 data"]
fn profile_block_table_reads_and_decode() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/test8-romfs");
    let facts = fs::read(root.join("facts.json")).unwrap();
    let (registry, _) = BlockRegistry::from_facts_json_bytes(&facts).unwrap();
    let dir = root.join("worldgen/blocks");
    for name in [
        "cached-collision.nbt",
        "block-properties.nbt",
        "cached-support.nbt",
    ] {
        let start = Instant::now();
        let bytes = fs::read(dir.join(name)).unwrap();
        let read = start.elapsed();
        let start = Instant::now();
        let table = BlockTableFile::read(&bytes, &registry).unwrap();
        let decode = start.elapsed();
        println!(
            "BLOCK_TABLE {name} bytes={} read_us={} decode_and_validate_us={}",
            bytes.len(),
            read.as_micros(),
            decode.as_micros()
        );
        if name == "cached-collision.nbt" {
            let start = Instant::now();
            let collisions =
                CachedCollisionRegistry::from_table(table.table("collision").unwrap()).unwrap();
            println!(
                "BLOCK_TABLE {name} shapes={} build_us={}",
                collisions.shape_count(),
                start.elapsed().as_micros()
            );
        }
    }
}
