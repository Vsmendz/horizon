use super::{BakedAssetRegistry, HorizonClientWorld, MIN_Y, TerrainVertex, VoxelBlock};
use crate::fluid_geometry::{FluidCell, FluidSprite, tessellate_geometry};
use horizon_core::{geometry::Direction, position::BlockPos};
use horizon_world::{
    fluid::{FluidKind, FluidState},
    fluid_flow::{FluidFlowWorld, flow_at},
    registry::BlockStateId,
};
use std::{convert::Infallible, io};

const DEFAULT_WATER: u32 = 0x3f76e4;

pub(super) fn biome_water_color(json: &str) -> io::Result<u32> {
    let data: serde_json::Value = serde_json::from_str(json)?;
    let value = &data["effects"]["water_color"];
    let color = value.as_i64().map(|n| n as u32).or_else(|| {
        value
            .as_str()
            .and_then(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).ok())
    });
    Ok(color.unwrap_or(DEFAULT_WATER) & 0xff_ffff)
}

pub(super) fn is_water(fluid: FluidState) -> bool {
    matches!(fluid.kind(), FluidKind::Water | FluidKind::FlowingWater)
}

struct FlowView<'a>(&'a HorizonClientWorld);

impl FluidFlowWorld for FlowView<'_> {
    type Error = Infallible;
    type Block = Option<BlockStateId>;

    fn fluid(&mut self, position: BlockPos) -> Result<FluidState, Infallible> {
        Ok(self
            .0
            .state_at(position)
            .and_then(|id| self.0.vanilla.fluids.get(id))
            .unwrap_or(FluidState::EMPTY))
    }
    fn block(&mut self, position: BlockPos) -> Result<Self::Block, Infallible> {
        Ok(self.0.state_at(position))
    }
    fn blocks_fluid_flow(&self, state: Self::Block) -> bool {
        state.is_some_and(|id| {
            self.0
                .vanilla
                .generator
                .data()
                .block_states()
                .get(id)
                .motion
        })
    }
    fn is_ice_class(&self, state: Self::Block) -> bool {
        state.is_some_and(|id| self.0.vanilla.visual(id) == VoxelBlock::Ice)
    }
    fn is_face_sturdy(
        &mut self,
        state: Self::Block,
        _: BlockPos,
        _: Direction,
    ) -> Result<bool, Infallible> {
        Ok(state.is_some_and(|id| self.0.vanilla.visual(id).is_opaque()))
    }
}

pub(super) fn append(
    world: &HorizonClientWorld,
    assets: &BakedAssetRegistry,
    p: BlockPos,
    output: &mut Vec<TerrainVertex>,
) {
    let column = &world.chunks[&(p.x >> 4, p.z >> 4)];
    let biome = column.biomes.noise_biome([p.x >> 2, p.y >> 2, p.z >> 2]);
    let color = world.vanilla.water_colors[biome.0 as usize];
    let rgb = [16, 8, 0].map(|shift| ((color >> shift) & 255) as f32 / 255.0);
    tessellate_geometry(
        [0, 0, 0],
        |[x, y, z]| {
            let pos = p.offset(x, y, z);
            let id = world.state_at(pos);
            let visual = id.map_or(VoxelBlock::Air, |id| world.vanilla.visual(id));
            // Match the geometry available to the Switch block renderer.
            let opaque = pos.y < MIN_Y || visual.is_opaque();
            FluidCell {
                fluid: id
                    .and_then(|id| world.vanilla.fluids.get(id))
                    .unwrap_or(FluidState::EMPTY),
                solid: opaque,
                legacy_solid: visual.is_solid(),
                face: [u8::from(opaque); 6],
                cover: [f32::from(u8::from(opaque)); 6],
                overlay: visual == VoxelBlock::OakLeaves,
            }
        },
        |state, _| flow_at(&mut FlowView(world), state, p).unwrap_or_else(|e| match e {}),
        |quad, sprite, direction, back| {
            let layer = assets.water_layers[match sprite {
                FluidSprite::Still => 0,
                FluidSprite::Flow => 1,
                FluidSprite::Overlay => 2,
            }] as f32;
            let shade = match direction {
                Direction::Up => 1.0,
                Direction::Down => 0.5,
                Direction::North | Direction::South => 0.8,
                Direction::West | Direction::East => 0.6,
            };
            let origin = [p.x as f32, p.y as f32, p.z as f32];
            let normal = direction.normal().map(|v| v as f32);
            let mut emit = |order: [usize; 6], normal| {
                output.extend(order.map(|i| {
                    let (position, uv) = quad[i];
                    TerrainVertex {
                        pos: std::array::from_fn(|axis| origin[axis] + position[axis]),
                        normal,
                        uv_layer: [uv[0], uv[1], layer],
                        color_ao: [rgb[0], rgb[1], rgb[2], shade],
                    }
                }));
            };
            emit([0, 1, 2, 0, 2, 3], normal);
            if back {
                emit([0, 3, 2, 0, 2, 1], normal.map(|v| -v));
            }
        },
    );
}
