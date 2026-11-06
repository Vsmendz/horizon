//! Fluid surfaces following `FluidRenderer.tesselate`: corner heights, face
//! occlusion against neighbour shapes, flow-rotated top UVs, the water overlay
//! beside glass and leaves, back faces, cardinal shading and max(self, above)
//! light.
use super::{
    Info, MeshTables, Padded, SectionMesh, TerrainVertex, fluid_face,
    fluid_geometry::{FluidCell, FluidSprite, tessellate_geometry},
    lighting, pack_coords, pack_rgb, pi,
};
use horizon_core::{
    geometry::{Axis, Direction, VoxelShape},
    position::BlockPos,
};
use horizon_world::{
    fluid::{FluidKind, FluidState},
    fluid_flow::{FluidFlowWorld, flow_at},
};
use std::convert::Infallible;

/// For the occlusion face toward `direction`: the height from the bottom over
/// which its slice flush with the `direction` side covers the whole width
/// (vertical faces: 1 when the whole square is covered, else 0). This is what
/// `Shapes.blockOccludes` decides for a fluid box of a given height.
pub(super) fn coverage(shape: &VoxelShape, direction: Direction) -> f32 {
    let (axis, u, v) = match direction {
        Direction::Down | Direction::Up => (Axis::Y, Axis::X, Axis::Z),
        Direction::North | Direction::South => (Axis::Z, Axis::X, Axis::Y),
        Direction::West | Direction::East => (Axis::X, Axis::Z, Axis::Y),
    };
    let positive = matches!(
        direction,
        Direction::Up | Direction::South | Direction::East
    );
    let rects: Vec<[f64; 4]> = shape
        .boxes()
        .iter()
        .filter(|b| {
            if positive {
                b.max(axis) >= 1.0 - 1e-7
            } else {
                b.min(axis) <= 1e-7
            }
        })
        .map(|b| [b.min(u), b.max(u), b.min(v), b.max(v)])
        .collect();
    let mut cuts: Vec<f64> = rects
        .iter()
        .flat_map(|r| [r[2], r[3]])
        .filter(|c| (0.0..=1.0).contains(c))
        .chain([0.0, 1.0])
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut covered = 0.0;
    for w in cuts.windows(2) {
        let middle = (w[0] + w[1]) / 2.0;
        let mut spans: Vec<(f64, f64)> = rects
            .iter()
            .filter(|r| r[2] <= middle && middle <= r[3])
            .map(|r| (r[0], r[1]))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut reach = 0.0f64;
        for (lo, hi) in spans {
            if lo > reach + 1e-7 {
                break;
            }
            reach = reach.max(hi);
        }
        if reach < 1.0 - 1e-7 {
            break;
        }
        covered = w[1];
    }
    if matches!(direction, Direction::Down | Direction::Up) && covered < 1.0 {
        return 0.0;
    }
    covered as f32
}
/// The padded section as `FlowingFluid.getFlow` sees it (section-relative positions).
struct FlowView<'a> {
    pad: &'a Padded,
    info: &'a [Info],
}
impl FlowView<'_> {
    fn id(&self, p: BlockPos) -> u32 {
        self.pad.blocks[pi(p.x, p.y, p.z)]
    }
}
impl FluidFlowWorld for FlowView<'_> {
    type Error = Infallible;
    type Block = u32;
    fn fluid(&mut self, p: BlockPos) -> Result<FluidState, Infallible> {
        Ok(self.info[self.id(p) as usize].fluid)
    }
    fn block(&mut self, p: BlockPos) -> Result<u32, Infallible> {
        Ok(self.id(p))
    }
    fn blocks_fluid_flow(&self, state: u32) -> bool {
        self.info[state as usize].blocks_flow
    }
    fn is_ice_class(&self, state: u32) -> bool {
        self.info[state as usize].ice
    }
    fn is_face_sturdy(
        &mut self,
        state: u32,
        _: BlockPos,
        d: Direction,
    ) -> Result<bool, Infallible> {
        Ok(self.info[state as usize].sturdy & 1 << d as u8 != 0)
    }
}
/// Emits one fluid cell at section-relative `[x, y, z]`.
/// `(nether, shader_pack_lighting, separate_ao)`: the Nether's face shading, only the up-face value
/// when a shader pack lights the faces (`Mesher::shader_pack_lighting`), and whether to keep pure
/// alpha for separate AO.
pub(super) fn tesselate(
    tables: &MeshTables,
    (nether, shader_pack_lighting, separate_ao): (bool, bool, bool),
    pad: &Padded,
    water_color: u32,
    out: &mut SectionMesh,
    [x, y, z]: [i32; 3],
) {
    use horizon_assets::model::Direction as D;
    let cardinal = |d: D| {
        if shader_pack_lighting {
            lighting::pack_flat(nether)
        } else {
            lighting::cardinal(nether, d)
        }
    };
    let info = &tables.info;
    let at = |p: [i32; 3]| &info[pad.blocks[pi(p[0], p[1], p[2])] as usize];
    let me = at([x, y, z]);
    let kind = me.fluid.kind();
    let water = matches!(kind, FluidKind::Water | FluidKind::FlowingWater);
    let tint = if water {
        water_color & 0xFF_FFFF
    } else {
        0xFF_FFFF
    };
    let emission = if water {
        0
    } else {
        crate::mesher::hdr_tier("lava", 15)
    };
    let color = |by: f32| {
        let rgb = [16, 8, 0].map(|shift| lighting::scale((tint >> shift & 255) as i32, by) as u32);
        if separate_ao {
            pack_rgb(rgb)
        } else {
            crate::mesher::emitting(pack_rgb(rgb), emission)
        }
    };
    let light = |p: [i32; 3]| {
        let above = [p[0], p[1] + 1, p[2]];
        pack_coords(lighting::max_coords(
            lighting::coords(pad, at(p), p),
            lighting::coords(pad, at(above), above),
        ))
    };
    let (still, flow, overlay) = if water {
        (
            tables.water_still,
            tables.water_flow,
            Some(tables.water_overlay),
        )
    } else {
        (tables.lava_still, tables.lava_flow, None)
    };
    let target = if water {
        &mut out.translucent
    } else {
        &mut out.solid
    };
    let mut sprite_layers = Vec::new();
    tessellate_geometry(
        [x, y, z],
        |p| {
            let state = at(p);
            FluidCell {
                fluid: state.fluid,
                solid: state.solid,
                legacy_solid: state.legacy_solid,
                face: state.face,
                cover: state.cover,
                overlay: state.overlay,
            }
        },
        |state, pos| {
            flow_at(&mut FlowView { pad, info }, state, pos).unwrap_or_else(|e| match e {})
        },
        |v, sprite, face, back| {
            let layer = match sprite {
                FluidSprite::Still => still,
                FluidSprite::Flow => flow,
                FluidSprite::Overlay => overlay.expect("water overlay"),
            };
            let shade = match face {
                Direction::Up => cardinal(D::Up),
                Direction::Down => cardinal(D::Down),
                Direction::North | Direction::South => {
                    cardinal(D::Up)
                        * if shader_pack_lighting {
                            1.0
                        } else {
                            cardinal(D::North)
                        }
                }
                _ => {
                    cardinal(D::Up)
                        * if shader_pack_lighting {
                            1.0
                        } else {
                            cardinal(D::West)
                        }
                }
            };
            let color = color(shade);
            let color = if separate_ao {
                color
            } else {
                fluid_face(color, face as usize)
            };
            let light = light([x, y - i32::from(face == Direction::Down), z]);
            if tables.is_animated(layer) && !sprite_layers.contains(&layer) {
                sprite_layers.push(layer);
            }
            let vertex = |(pos, uv)| TerrainVertex::new(pos, uv, layer, color, light);
            target.extend(v.map(vertex));
            if back {
                target.extend([v[0], v[3], v[2], v[1]].map(vertex));
            }
        },
    );
    out.sprite_layers.extend(sprite_layers);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coverage_of_full_and_partial_faces() {
        assert_eq!(coverage(&VoxelShape::block(), Direction::North), 1.0);
        assert_eq!(coverage(&VoxelShape::block(), Direction::Up), 1.0);
        assert_eq!(coverage(&VoxelShape::empty(), Direction::East), 0.0);
        // A bottom slab covers the lower half of each side and the whole floor.
        let slab = VoxelShape::from_cube([1, 2, 1], &[true, false]).unwrap();
        assert_eq!(coverage(&slab, Direction::South), 0.5);
        assert_eq!(coverage(&slab, Direction::Down), 1.0);
        assert_eq!(coverage(&slab, Direction::Up), 0.0);
    }
}
