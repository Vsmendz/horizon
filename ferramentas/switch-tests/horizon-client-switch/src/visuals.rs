use crate::{ATLAS_SIDE, HorizonClientWorld, TerrainVertex, VoxelBlock, decode_png_frames};
use glam::{Mat4, Vec3};
use horizon_assets::{
    item_bake::{SpriteLookup, SpriteMask},
    item_model::DisplayContext,
    resolve::ModelLibrary,
};
use horizon_core::position::BlockPos;
use horizon_world::worldgen::{biome::BiomeId, generator::GeneratorData};
use std::{collections::BTreeMap, fs, io, path::Path};

#[derive(Clone, Copy)]
pub(crate) struct BiomeVisual {
    pub grass: [f32; 3],
    pub leaves: [f32; 3],
    pub dry_foliage: [f32; 3],
    pub sky: [f32; 3],
}

#[derive(serde::Deserialize)]
pub(crate) struct VisualState(pub f32, pub bool, pub u8);

pub(super) fn load_states(root: &Path, count: usize) -> io::Result<Box<[VisualState]>> {
    let states: Box<[VisualState]> = serde_json::from_reader(std::io::BufReader::new(
        fs::File::open(root.join("visual-state-facts.json"))?,
    ))?;
    if states.len() != count {
        return Err(io::Error::other(
            "visual facts do not match the block registry",
        ));
    }
    Ok(states)
}

pub(super) fn light(
    world: &HorizonClientWorld,
    id: horizon_world::registry::BlockStateId,
    face_shade: f32,
) -> f32 {
    // Local emission is known; propagation and skylight are not yet integrated.
    let emission = world
        .vanilla
        .visual_states
        .get(id.0 as usize)
        .map_or(0, |state| state.2);
    face_shade.max(f32::from(emission) / 15.0)
}

fn rgb(color: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((color >> shift) & 255) as f32 / 255.0)
}

fn sample(map: &[u32], temp: f32, downfall: f32) -> u32 {
    // GrassColor.get and FoliageColor.get use the original climate colormap.
    let t = f64::from(temp.clamp(0.0, 1.0));
    let rain = f64::from(downfall.clamp(0.0, 1.0)) * t;
    let x = ((1.0 - t) * 255.0) as usize;
    let y = ((1.0 - rain) * 255.0) as usize;
    map[y * 256 + x]
}

pub(super) fn load_biomes(root: &Path, data: &GeneratorData) -> io::Result<Box<[BiomeVisual]>> {
    let load = |name| -> io::Result<Vec<u32>> {
        let bytes = fs::read(root.join(format!("assets/minecraft/textures/colormap/{name}.png")))?;
        let (side, frames) = decode_png_frames(&bytes).map_err(io::Error::other)?;
        if side != 256 || frames.len() != 1 {
            return Err(io::Error::other("invalid biome colormap dimensions"));
        }
        Ok(frames[0]
            .chunks_exact(4)
            .map(|p| u32::from_be_bytes([0, p[0], p[1], p[2]]))
            .collect())
    };
    let grass = load("grass")?;
    let leaves = load("foliage")?;
    let dry_foliage = load("dry_foliage").unwrap_or_else(|_| leaves.clone());
    (0..data.biome_names().len())
        .map(|id| {
            let biome: serde_json::Value =
                serde_json::from_str(data.biome_json(BiomeId(id as u32)))?;
            let climate = |name| biome[name].as_f64().unwrap_or(0.5) as f32;
            let temp = climate("temperature");
            let rain = climate("downfall");
            let mut grass = crate::water::read_color(
                &biome["effects"]["grass_color"],
                sample(&grass, temp, rain),
            );
            if biome["effects"]["grass_color_modifier"] == "dark_forest" {
                grass = ((grass & 0xfe_fefe) + 0x28_340a) >> 1;
            }
            Ok(BiomeVisual {
                grass: rgb(grass),
                leaves: rgb(crate::water::read_color(
                    &biome["effects"]["foliage_color"],
                    sample(&leaves, temp, rain),
                )),
                dry_foliage: rgb(crate::water::read_color(
                    &biome["effects"]["dry_foliage_color"],
                    sample(&dry_foliage, temp, rain),
                )),
                sky: rgb(crate::water::read_color(
                    &biome["attributes"]["minecraft:visual/sky_color"],
                    0x78_a7ff,
                )),
            })
        })
        .collect()
}

pub(super) fn biome_at(world: &HorizonClientWorld, p: BlockPos) -> Option<BiomeVisual> {
    let column = world.chunks.get(&(p.x >> 4, p.z >> 4))?;
    let biome = column.biomes.noise_biome([p.x >> 2, p.y >> 2, p.z >> 2]);
    world.vanilla.biome_visuals.get(biome.0 as usize).copied()
}

pub(super) fn tint(
    world: &HorizonClientWorld,
    p: BlockPos,
    block: VoxelBlock,
    tint: i32,
) -> [f32; 3] {
    if tint < 0 {
        return [1.0; 3];
    }
    match block {
        VoxelBlock::BirchLeaves => rgb(0x80_a755),
        VoxelBlock::SpruceLeaves => rgb(0x61_9961),
        VoxelBlock::CherryLeaves => [1.0; 3],
        _ => biome_at(world, p).map_or_else(
            || block.biome_tint(tint),
            |b| {
                if block.block_name().ends_with("_leaves") {
                    b.leaves
                } else {
                    b.grass
                }
            },
        ),
    }
}

pub(super) fn tint_state(
    world: &HorizonClientWorld,
    p: BlockPos,
    kind: crate::state_models::TintKind,
    tint_index: i32,
) -> [f32; 3] {
    if tint_index < 0 {
        return [1.0; 3];
    }
    match kind {
        crate::state_models::TintKind::None => [1.0; 3],
        crate::state_models::TintKind::Fixed(color) => color,
        crate::state_models::TintKind::BiomeGrass => {
            biome_at(world, p).map_or([0.48, 0.74, 0.33], |b| b.grass)
        }
        crate::state_models::TintKind::BiomeFoliage => {
            biome_at(world, p).map_or([0.36, 0.66, 0.24], |b| b.leaves)
        }
        crate::state_models::TintKind::BiomeDryFoliage => {
            biome_at(world, p).map_or(rgb(0x5d_4930), |b| b.dry_foliage)
        }
        crate::state_models::TintKind::BiomeWater => {
            if let Some(column) = world.chunks.get(&(p.x >> 4, p.z >> 4)) {
                let biome = column.biomes.noise_biome([p.x >> 2, p.y >> 2, p.z >> 2]);
                if let Some(&c) = world.vanilla.water_colors.get(biome.0 as usize) {
                    return rgb(c);
                }
            }
            rgb(0x3f_76e4)
        }
    }
}

struct AtlasSprites<'a> {
    names: &'a BTreeMap<String, u32>,
    layers: &'a [Vec<u8>],
}

impl SpriteLookup for AtlasSprites<'_> {
    fn mask(&self, sprite: &str) -> Option<SpriteMask> {
        let rgba = self.layers.get(*self.names.get(sprite)? as usize)?;
        let side = ATLAS_SIDE as usize;
        Some(SpriteMask {
            width: 16,
            height: 16,
            frames: vec![
                (0..256)
                    .map(|i| {
                        rgba[((i / 16) * side * side / 16 + (i % 16) * side / 16) * 4 + 3] == 0
                    })
                    .collect(),
            ],
        })
    }
}

pub(super) fn bake_bucket(
    library: &ModelLibrary,
    names: &BTreeMap<String, u32>,
    layers: &[Vec<u8>],
) -> Result<(Vec<horizon_assets::bake::ModelQuad>, Mat4, u32), String> {
    let baked = horizon_assets::item_bake::bake_item_model(
        library,
        &AtlasSprites { names, layers },
        "minecraft:item/water_bucket",
    )
    .map_err(|e| e.to_string())?;
    let matrix = baked.layer_matrix(
        DisplayContext::FirstPersonRightHand,
        &horizon_assets::math::Mat4::IDENTITY,
    );
    let layer = *names
        .get("minecraft:item/water_bucket")
        .ok_or("missing water bucket sprite")?;
    Ok((baked.quads, Mat4::from_cols_array_2d(&matrix.0), layer))
}

pub(super) fn quad(
    out: &mut Vec<TerrainVertex>,
    positions: [Vec3; 4],
    uv: [f32; 4],
    layer: u32,
    tint: [f32; 3],
    alpha: f32,
) {
    let [u0, v0, u1, v1] = uv;
    let tex = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(TerrainVertex {
            pos: positions[i].to_array(),
            normal: [0.0, 1.0, 0.0],
            uv_layer: [tex[i][0], tex[i][1], layer as f32],
            color_ao: [tint[0], tint[1], tint[2], alpha],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn climate_corners_use_expected_pixels() {
        let map: Vec<u32> = (0..65_536).collect();
        assert_eq!(sample(&map, 1.0, 1.0), 0);
        assert_eq!(sample(&map, 0.0, 1.0), 65_535);
        assert_eq!(sample(&map, 1.0, 0.0), 65_280);
    }
}
