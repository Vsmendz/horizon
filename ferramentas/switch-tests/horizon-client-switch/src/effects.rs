use crate::{BakedAssetRegistry, HorizonClientWorld, TerrainVertex, VoxelBlock, visuals};
use glam::Vec3;
use horizon_core::{
    geometry::{Axis, Direction},
    position::BlockPos,
    random::{LegacyRandom, RandomSource},
};

const LIMIT: usize = 256;

#[derive(Clone, Copy)]
struct Particle {
    previous: Vec3,
    position: Vec3,
    velocity: Vec3,
    size: f32,
    tint: [f32; 3],
    uv: [f32; 4],
    layer: u32,
    age: u8,
    life: u8,
    gravity: f32,
    splash: bool,
}

pub(super) struct Effects {
    particles: Vec<Particle>,
    random: LegacyRandom,
    pub emitted: usize,
    pub peak: usize,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            particles: Vec::with_capacity(LIMIT),
            random: LegacyRandom::new(12345),
            emitted: 0,
            peak: 0,
        }
    }
}

impl Effects {
    fn push(&mut self, particle: Particle) {
        if self.particles.len() < LIMIT {
            self.particles.push(particle);
            self.emitted += 1;
            self.peak = self.peak.max(self.particles.len());
        }
    }

    pub fn tick(&mut self) {
        self.particles.retain_mut(|p| {
            p.age += 1;
            if p.age >= p.life {
                return false;
            }
            p.previous = p.position;
            p.velocity.y -= p.gravity;
            p.position += p.velocity;
            p.velocity *= 0.98;
            true
        });
    }

    fn crumb(&mut self, p: [f64; 3], velocity: [f64; 3], layer: u32, tint: [f32; 3], impact: bool) {
        let position = Vec3::from_array(p.map(|v| v as f32));
        let life = (4.0 / (self.random.next_float() * 0.9 + 0.1)) as u8;
        let v = Vec3::from_array(
            velocity.map(|v| v as f32 + (self.random.next_float() * 2.0 - 1.0) * 0.4),
        );
        let speed = (self.random.next_float() + self.random.next_float() + 1.0) * 0.06;
        let velocity = v.normalize_or_zero() * speed + Vec3::Y * 0.1;
        let size = 0.1 * (self.random.next_float() * 0.5 + 0.5);
        let u = self.random.next_float() * 0.75;
        let v = self.random.next_float() * 0.75;
        self.push(Particle {
            previous: position,
            position,
            velocity: if impact {
                (velocity - Vec3::Y * 0.1) * 0.2 + Vec3::Y * 0.1
            } else {
                velocity
            },
            size: size * if impact { 0.6 } else { 1.0 },
            tint: tint.map(|v| v * 0.6),
            uv: [u + 0.25, v, u, v + 0.25],
            layer,
            age: 0,
            life,
            gravity: 0.04,
            splash: false,
        });
    }

    pub fn block(
        &mut self,
        world: &HorizonClientWorld,
        assets: &BakedAssetRegistry,
        pos: BlockPos,
        face: Direction,
        destroy: bool,
    ) {
        let block = world.get_block(pos);
        if block == VoxelBlock::Air || block == VoxelBlock::Water {
            return;
        }
        let Some(quads) = assets.baked_quads.get(&block) else {
            return;
        };
        let Some(model) = quads
            .iter()
            .find(|q| normal(q.facing) == face.normal())
            .or_else(|| quads.first())
        else {
            return;
        };
        let Some(&layer) = assets.sprite_to_layer.get(&model.material.sprite) else {
            return;
        };
        let tint = visuals::tint(world, pos, block, model.tint_index);
        let bounds = world.collision_shape(pos).boxes();
        let coords = |a: &horizon_core::geometry::Aabb| {
            [
                a.min(Axis::X),
                a.min(Axis::Y),
                a.min(Axis::Z),
                a.max(Axis::X),
                a.max(Axis::Y),
                a.max(Axis::Z),
            ]
        };
        let mut boxes: Vec<_> = bounds.iter().map(coords).collect();
        if boxes.is_empty() {
            // The prepared model has geometry even for plants with no collision.
            let mut bounds = [1.0f64, 1.0, 1.0, 0.0, 0.0, 0.0];
            for p in quads.iter().flat_map(|q| q.positions) {
                for i in 0..3 {
                    bounds[i] = bounds[i].min(f64::from(p[i]));
                    bounds[i + 3] = bounds[i + 3].max(f64::from(p[i]));
                }
            }
            boxes.push(bounds);
        }
        let p = [pos.x, pos.y, pos.z];
        if destroy {
            crate::block_particle_geometry::destroy_positions(p, &boxes, |p, v| {
                self.crumb(p, v, layer, tint, false)
            });
        } else if let Some(p) = crate::block_particle_geometry::breaking_position(
            &mut self.random,
            p,
            face.normal(),
            &boxes,
        ) {
            self.crumb(p, [0.0; 3], layer, tint, true);
        }
    }

    pub fn water(&mut self, assets: &BakedAssetRegistry, p: Vec3, bubble: bool) {
        for _ in 0..if bubble { 4 } else { 12 } {
            let x = self.random.next_float() - 0.5;
            let z = self.random.next_float() - 0.5;
            let position = p + Vec3::new(x, 0.0, z) * 0.45;
            let velocity = Vec3::new(x * 0.12, self.random.next_float() * 0.12 + 0.04, z * 0.12);
            self.push(Particle {
                previous: position,
                position,
                velocity,
                size: 0.08,
                tint: [1.0; 3],
                uv: [0.0, 0.0, 1.0, 1.0],
                layer: if bubble {
                    assets.bubble
                } else {
                    assets.splash[0]
                },
                age: 0,
                life: if bubble { 12 } else { 8 },
                gravity: if bubble { -0.002 } else { 0.03 },
                splash: !bubble,
            });
        }
    }

    pub fn vertices(
        &self,
        assets: &BakedAssetRegistry,
        out: &mut Vec<TerrainVertex>,
        eye: Vec3,
        right: Vec3,
        up: Vec3,
        partial: f32,
    ) {
        for p in &self.particles {
            let pos = p.previous.lerp(p.position, partial);
            if pos.distance_squared(eye) > 32.0 * 32.0 {
                continue;
            }
            let right = right * p.size;
            let up = up * p.size;
            let layer = if p.splash {
                assets.splash[(p.age / 2).min(3) as usize]
            } else {
                p.layer
            };
            visuals::quad(
                out,
                [
                    pos - right - up,
                    pos + right - up,
                    pos + right + up,
                    pos - right + up,
                ],
                p.uv,
                layer,
                p.tint,
                (1.0 - (f32::from(p.age) + partial) / f32::from(p.life)).clamp(0.0, 1.0),
            );
        }
    }
}

fn normal(face: crate::AssetDirection) -> [i32; 3] {
    use crate::AssetDirection::*;
    match face {
        Down => [0, -1, 0],
        Up => [0, 1, 0],
        North => [0, 0, -1],
        South => [0, 0, 1],
        West => [-1, 0, 0],
        East => [1, 0, 0],
    }
}

type Target = (BlockPos, horizon_world::registry::BlockStateId, VoxelBlock);

#[derive(Default)]
pub(super) struct Mining {
    target: Option<Target>,
    progress: f32,
    block: Option<VoxelBlock>,
}

impl Mining {
    pub fn update(
        &mut self,
        target: Option<(Target, &visuals::VisualState)>,
        steps: u32,
        slowdown: f32,
        block: Option<VoxelBlock>,
    ) -> bool {
        self.block = block;
        let key = target.map(|(key, _)| key);
        if self.target != key {
            self.target = key;
            self.progress = 0.0;
        }
        let Some((_, visuals::VisualState(hardness, requires_tool, _))) = target else {
            return false;
        };
        if *hardness < 0.0 {
            return false;
        }
        if *hardness == 0.0 {
            return true;
        }
        self.progress +=
            steps as f32 / (hardness * slowdown * if *requires_tool { 100.0 } else { 30.0 });
        self.progress >= 1.0
    }

    pub fn vertices(&self, assets: &BakedAssetRegistry, out: &mut Vec<TerrainVertex>) {
        let Some((pos, _, _)) = self.target else {
            return;
        };
        if self.progress <= 0.0 || self.progress >= 1.0 {
            return;
        }
        let Some(quads) = self.block.and_then(|block| assets.baked_quads.get(&block)) else {
            return;
        };
        let layer = assets.cracks[(self.progress * 10.0) as usize] as f32;
        let origin = Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32);
        for quad in quads {
            let n = Vec3::from_array(normal(quad.facing).map(|x| x as f32));
            for i in [0, 1, 2, 0, 2, 3] {
                out.push(TerrainVertex {
                    pos: (origin + Vec3::from_array(quad.positions[i]) + n * 0.001).to_array(),
                    normal: n.to_array(),
                    uv_layer: [quad.uv[i][0], quad.uv[i][1], layer],
                    color_ao: [1.0, 1.0, 1.0, 0.7],
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mining_uses_tool_requirement_and_clears_progress_on_cancel() {
        use horizon_world::registry::BlockStateId;
        let mut mining = Mining::default();
        let target = (BlockPos::new(2, 3, 4), BlockStateId(1), VoxelBlock::Dirt);
        let dirt = visuals::VisualState(0.5, false, 0);
        assert!(!mining.update(Some((target, &dirt)), 14, 1.0, Some(VoxelBlock::Dirt)));
        assert!(mining.update(Some((target, &dirt)), 1, 1.0, Some(VoxelBlock::Dirt)));
        assert!(!mining.update(None, 0, 1.0, None));
        assert!(!mining.update(Some((target, &dirt)), 1, 1.0, Some(VoxelBlock::Dirt)));
        mining.update(None, 0, 1.0, None);
        let stone = visuals::VisualState(1.5, true, 0);
        assert!(!mining.update(Some((target, &stone)), 149, 1.0, Some(VoxelBlock::Stone)));
        assert!(mining.update(Some((target, &stone)), 1, 1.0, Some(VoxelBlock::Stone)));
    }

    #[test]
    fn burst_has_bounded_storage_and_particles_age_and_move() {
        let mut effects = Effects::default();
        for _ in 0..LIMIT * 2 {
            effects.crumb([1.0, 2.0, 3.0], [0.1, 0.0, 0.0], 42, [1.0; 3], false);
        }
        assert_eq!(effects.particles.len(), LIMIT);
        assert!(effects.particles.iter().all(|p| p.layer == 42));
        effects.tick();
        assert!(effects.particles.iter().all(|p| p.position != p.previous));
        for _ in 0..40 {
            effects.tick();
        }
        assert!(effects.particles.is_empty());
        assert_eq!(effects.particles.capacity(), LIMIT);
    }
}
