//! Bounded worker-side FEATURES batches with a pristine two-chunk terrain halo.
use crate::vanilla::Column;
use horizon_core::position::ChunkPos;
use horizon_world::worldgen::generator::{GeneratorError, GeneratorWorker, ProtoChunk};
use std::collections::{BTreeMap, VecDeque};

type Key = (i32, i32);
const TILE: i32 = 3;
const CACHED_TILES: usize = 4;
const CACHED_TERRAIN: usize = 98;

#[derive(Debug, Default)]
pub(super) struct FeatureStats {
    pub tiles: usize,
    pub new_terrain: usize,
    pub placed: usize,
    pub unsupported: usize,
    pub failed: usize,
    pub peak_terrain_bytes: usize,
}

#[derive(Default)]
pub(super) struct FeatureCache {
    terrain: BTreeMap<Key, ProtoChunk>,
    tiles: VecDeque<(Key, BTreeMap<Key, Column>)>,
    stats: FeatureStats,
}

fn origin((x, z): Key) -> Key {
    (x.div_euclid(TILE) * TILE, z.div_euclid(TILE) * TILE)
}

impl FeatureCache {
    pub fn take_stats(&mut self) -> FeatureStats {
        std::mem::take(&mut self.stats)
    }

    pub fn column(
        &mut self,
        worker: &mut GeneratorWorker,
        pos: ChunkPos,
    ) -> Result<Column, GeneratorError> {
        let key = (pos.x, pos.z);
        let tile = origin(key);
        let entry = if let Some(i) = self.tiles.iter().position(|(o, _)| *o == tile) {
            self.tiles.remove(i).unwrap()
        } else {
            (tile, self.build(worker, tile)?)
        };
        let column = entry.1[&key].clone();
        self.tiles.push_front(entry);
        self.tiles.truncate(CACHED_TILES);
        Ok(column)
    }

    fn build(
        &mut self,
        worker: &mut GeneratorWorker,
        (ox, oz): Key,
    ) -> Result<BTreeMap<Key, Column>, GeneratorError> {
        let mut batch = BTreeMap::new();
        for z in oz - 2..oz + TILE + 2 {
            for x in ox - 2..ox + TILE + 2 {
                let key = (x, z);
                let proto = match self.terrain.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        self.stats.new_terrain += 1;
                        entry.insert(worker.generate_proto(ChunkPos::new(x, z))?)
                    }
                };
                batch.insert(key, proto.clone());
            }
        }
        // A tile's row-major schedule is independent of travel direction or cache hits.
        // Like the original generator probe, include every center that can write into it.
        for z in oz - 1..=oz + TILE {
            for x in ox - 1..=ox + TILE {
                let mut region = std::array::from_fn(|i| {
                    batch
                        .remove(&(x + i as i32 % 3 - 1, z + i as i32 / 3 - 1))
                        .expect("feature terrain halo")
                });
                let stats = worker.decorate(&mut region)?;
                self.stats.placed += stats.placed;
                self.stats.unsupported += stats.unsupported;
                self.stats.failed += stats.failed;
                for chunk in region {
                    let pos = chunk.position();
                    batch.insert((pos.x, pos.z), chunk);
                }
            }
        }
        self.stats.tiles += 1;
        self.stats.peak_terrain_bytes = self.stats.peak_terrain_bytes.max(
            self.terrain
                .values()
                .chain(batch.values())
                .map(|chunk| chunk.heap_bytes().into_iter().sum::<usize>())
                .sum(),
        );
        if self.terrain.len() > CACHED_TERRAIN {
            let mut keys: Vec<_> = self.terrain.keys().copied().collect();
            keys.sort_unstable_by_key(|&(x, z)| {
                (x.abs_diff(ox + TILE / 2) + z.abs_diff(oz + TILE / 2), x, z)
            });
            for key in keys.into_iter().skip(CACHED_TERRAIN) {
                self.terrain.remove(&key);
            }
        }
        Ok(batch
            .into_iter()
            .filter(|&(key, _)| origin(key) == (ox, oz))
            .map(|(key, proto)| (key, Column::from_proto(proto)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vanilla::Vanilla;
    use std::path::Path;

    #[test]
    fn decoration_changes_terrain_and_is_independent_of_request_order() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/test10-romfs");
        let data = Vanilla::load_decorated(&root, |_| {}).unwrap();
        let mut worker = data.generator.worker();
        let mut cache = FeatureCache::default();
        let [x, z] = data.spawn;
        let (cx, cz) = origin((x >> 4, z >> 4));
        let keys = [(cx, cz), (cx + 1, cz), (cx + 2, cz), (cx + 3, cz)];
        let columns = keys.map(|(x, z)| cache.column(&mut worker, ChunkPos::new(x, z)).unwrap());
        let stats = cache.take_stats();
        eprintln!("FEATURE_TEST {stats:?}");
        assert_eq!(stats.tiles, 2);
        assert!(stats.placed > 0 && stats.new_terrain < 98);
        use horizon_world::nbt::Tag;
        let registry = data.generator.data().registry();
        let mut changed = 0;
        let mut decoration = BTreeMap::new();
        let mut reverse = FeatureCache::default();
        for (i, (x, z)) in keys.into_iter().enumerate().rev() {
            let tag = Tag::compound([
                ("yPos", Tag::Int(-4)),
                ("sections", Tag::List(columns[i].sections().unwrap())),
                ("Heightmaps", columns[i].heightmaps()),
            ]);
            let roundtrip =
                Column::from_tag(&tag, ChunkPos::new(x, z), data.generator.data().clone()).unwrap();
            let other = reverse.column(&mut worker, ChunkPos::new(x, z)).unwrap();
            let plain = Column::from_proto(worker.generate_proto(ChunkPos::new(x, z)).unwrap());
            for y in -64..320 {
                for local_z in 0..16 {
                    for local_x in 0..16 {
                        let id = columns[i].get(local_x, y, local_z);
                        if id != plain.get(local_x, y, local_z) {
                            changed += 1;
                            let name = registry.state(id).unwrap().name.as_str();
                            *decoration.entry(name).or_insert(0) += 1;
                        }
                        assert_eq!(id, roundtrip.get(local_x, y, local_z));
                        assert_eq!(id, other.get(local_x, y, local_z));
                    }
                }
            }
            assert_eq!(
                columns[i].biomes.noise_biome([x * 4, 20, z * 4]),
                other.biomes.noise_biome([x * 4, 20, z * 4])
            );
        }
        eprintln!("FEATURE_STATES {decoration:?}");
        assert!(changed > 0, "decorated columns must differ from terrain");
    }
}
