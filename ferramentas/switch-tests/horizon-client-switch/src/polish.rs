use super::{HorizonClientWorld, HorizonLocalPlayer, MIN_Y, VoxelBlock};
use crate::first_person_shared::{Hand, HandHeights, SwingAnimation, SwingState};
use crate::world_stream::MAX_Y;
use horizon_core::{
    geometry::{Aabb, Axis, Vec3},
    position::BlockPos,
};
use horizon_world::fluid::{FluidHeightWorld, FluidState};
use std::convert::Infallible;

pub(super) struct HandPresentation {
    pub visible: VoxelBlock,
    pub heights: HandHeights,
    pub swing: SwingState,
}

impl HandPresentation {
    pub fn new(visible: VoxelBlock) -> Self {
        Self {
            visible,
            heights: HandHeights {
                main: 1.0,
                old_main: 1.0,
                ..HandHeights::default()
            },
            swing: SwingState::default(),
        }
    }

    pub fn tick(&mut self, selected: VoxelBlock) {
        self.swing.tick();
        if self
            .heights
            .tick(false, 1.0, [selected != self.visible, false])[0]
        {
            self.visible = selected;
        }
    }

    pub fn attack(&mut self) {
        let animation = SwingAnimation::DEFAULT;
        self.swing
            .start_if_able(Hand::Main, animation, animation.duration);
    }

    pub fn place(&mut self) {
        self.attack();
        self.heights.item_used(Hand::Main);
    }
}

struct FluidView<'a>(&'a HorizonClientWorld);

impl FluidHeightWorld for FluidView<'_> {
    type Error = Infallible;

    fn fluid(&mut self, [x, y, z]: [i32; 3]) -> Result<FluidState, Infallible> {
        Ok(self
            .0
            .state_at(BlockPos::new(x, y, z))
            .and_then(|id| self.0.vanilla.fluids.get(id))
            .unwrap_or(FluidState::EMPTY))
    }

    fn sturdy_down(&mut self, [x, y, z]: [i32; 3]) -> Result<bool, Infallible> {
        Ok(self.0.get_block(BlockPos::new(x, y, z)).is_opaque())
    }
}

fn water_surface(
    view: &mut impl FluidHeightWorld<Error = Infallible>,
    p: [i32; 3],
    camera: bool,
) -> Option<f64> {
    let state = view.fluid(p).unwrap_or_else(|e| match e {});
    if !crate::water::is_water(state) {
        return None;
    }
    let h = if camera {
        state.camera_height_at(view, p)
    } else {
        state.height_at(view, p)
    }
    .unwrap_or_else(|e| match e {});
    Some(f64::from(p[1]) + f64::from(h))
}

pub(super) fn eye_in_water(world: &HorizonClientWorld, eye: [f64; 3]) -> bool {
    let p = eye.map(|v| v.floor() as i32);
    water_surface(&mut FluidView(world), p, true).is_some_and(|surface| eye[1] < surface)
}

#[derive(Default)]
pub(super) struct WaterView {
    pub blend: f32,
    color: [f32; 3],
}

impl WaterView {
    pub fn update(&mut self, world: &HorizonClientWorld, eye: [f64; 3], dt: f32) {
        let submerged = eye_in_water(world, eye);
        if submerged {
            let [x, y, z] = eye.map(|v| v.floor() as i32);
            if let Some(column) = world.chunks.get(&(x >> 4, z >> 4)) {
                let biome = column.biomes.noise_biome([x >> 2, y >> 2, z >> 2]);
                let tint = world.vanilla.water_fog_colors[biome.0 as usize];
                self.color = [16, 8, 0].map(|shift| ((tint >> shift) & 255) as f32 / 255.0);
            }
        }
        self.blend = transition(self.blend, submerged, dt);
    }

    pub fn uniform(&self) -> [f32; 4] {
        [self.color[0], self.color[1], self.color[2], self.blend]
    }
}

fn transition(current: f32, submerged: bool, seconds: f32) -> f32 {
    let target = if submerged { 1.0 } else { 0.0 };
    target + (current - target) * (-seconds * 20.0).exp()
}

pub(super) fn water_depth(world: &HorizonClientWorld, bounds: Aabb) -> f64 {
    let bounds = bounds.deflate(Vec3::new(0.001, 0.001, 0.001));
    let low = |axis| bounds.min(axis).floor() as i32;
    let high = |axis| bounds.max(axis).ceil() as i32;
    let mut depth: f64 = 0.0;
    let mut view = FluidView(world);
    for z in low(Axis::Z)..high(Axis::Z) {
        for x in low(Axis::X)..high(Axis::X) {
            for y in low(Axis::Y)..high(Axis::Y) {
                if let Some(surface) = water_surface(&mut view, [x, y, z], false) {
                    depth = depth.max(surface - bounds.min(Axis::Y));
                }
            }
        }
    }
    depth
}

pub(super) fn intersects_blocks(world: &HorizonClientWorld, bounds: Aabb) -> bool {
    world
        .collect_colliders(bounds)
        .iter()
        .any(|shape| shape.boxes().iter().any(|&block| bounds.intersects(block)))
}

pub(super) fn valid_restore(world: &HorizonClientWorld, position: Vec3) -> bool {
    position.y >= f64::from(MIN_Y)
        && position.y <= f64::from(MAX_Y + 96)
        && !intersects_blocks(
            world,
            HorizonLocalPlayer::bounds(position, HorizonLocalPlayer::SNEAKING_HEIGHT),
        )
}

pub(super) fn dry_spawn(world: &HorizonClientWorld, x: i32, z: i32) -> Option<Vec3> {
    let mut best = None;
    let mut distance = i32::MAX;
    for dz in -12i32..=12 {
        for dx in -12i32..=12 {
            let d = dx * dx + dz * dz;
            if d >= distance {
                continue;
            }
            let (x, z) = (x + dx, z + dz);
            let y = world.surface_y(x, z);
            if y < MIN_Y || world.state_at(BlockPos::new(x, y, z)).is_none() {
                continue;
            }
            let pos = Vec3::new(f64::from(x) + 0.5, f64::from(y + 1), f64::from(z) + 0.5);
            let bounds = HorizonLocalPlayer::standing_bounds(pos);
            if !intersects_blocks(world, bounds)
                && intersects_blocks(world, bounds.move_by(Vec3::new(0.0, -0.01, 0.0)))
                && water_depth(world, bounds.move_by(Vec3::new(0.0, -0.01, 0.0))) == 0.0
            {
                best = Some(pos);
                distance = d;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equip_animation_switches_model_at_the_bottom_and_rebounds_on_place() {
        let mut hand = HandPresentation::new(VoxelBlock::Stone);
        for _ in 0..2 {
            hand.tick(VoxelBlock::Dirt);
            assert_eq!(hand.visible, VoxelBlock::Stone);
        }
        hand.tick(VoxelBlock::Dirt);
        assert_eq!(hand.visible, VoxelBlock::Dirt);
        for _ in 0..3 {
            hand.tick(VoxelBlock::Dirt);
        }
        assert_eq!(hand.heights.interpolated(1.0)[0], 1.0);
        hand.place();
        hand.tick(VoxelBlock::Dirt);
        assert!((hand.heights.interpolated(0.5)[0] - 0.2).abs() < 1e-6);
        hand.tick(VoxelBlock::Dirt);
        assert!((hand.swing.animation(0.5) - 1.0 / 12.0).abs() < 1e-6);
    }

    #[test]
    fn fluid_surface_uses_actual_states_and_not_altitude() {
        struct Cells(bool);
        impl FluidHeightWorld for Cells {
            type Error = Infallible;
            fn fluid(&mut self, [_, y, _]: [i32; 3]) -> Result<FluidState, Infallible> {
                Ok(if y == 48 || (y == 49 && self.0) {
                    FluidState::WATER
                } else {
                    FluidState::EMPTY
                })
            }
            fn sturdy_down(&mut self, _: [i32; 3]) -> Result<bool, Infallible> {
                Ok(false)
            }
        }
        assert_eq!(water_surface(&mut Cells(false), [0, 47, 0], true), None);
        let surface = water_surface(&mut Cells(false), [0, 48, 0], true).unwrap();
        assert!((surface - (48.0 + 8.0 / 9.0)).abs() < 1e-6);
        assert_eq!(
            water_surface(&mut Cells(true), [0, 48, 0], false),
            Some(49.0)
        );
    }

    #[test]
    fn fog_transition_tracks_time_at_thirty_and_sixty_fps() {
        let half_second = |fps: u32, submerged| {
            (0..fps / 2).fold(if submerged { 0.0 } else { 1.0 }, |value, _| {
                transition(value, submerged, 1.0 / fps as f32)
            })
        };
        for submerged in [false, true] {
            assert!((half_second(30, submerged) - half_second(60, submerged)).abs() < 1e-6);
        }
        assert!(half_second(30, true) > 0.99);
        assert!(half_second(30, false) < 0.01);
    }

    #[test]
    fn vanilla_hand_anchor_and_sprint_fov_have_expected_endpoints() {
        use crate::{camera, first_person_shared};
        use glam::Vec3 as GVec;
        let anchor = first_person_shared::item_anchor(1.0, 0.0);
        let idle = first_person_shared::apply_swing(anchor, 1.0, 0.0);
        let center = idle.transform_point3(GVec::ZERO);
        assert!((center - GVec::new(0.56, -0.52, -0.72)).length() < 1e-6);
        let active = first_person_shared::apply_swing(anchor, 1.0, 0.25);
        assert!(active.transform_point3(GVec::ZERO).x < center.x - 0.35);

        let mut view = camera::Camera {
            fov_modifier: 1.0,
            old_fov_modifier: 1.0,
            ..Default::default()
        };
        let fov = |view: &camera::Camera, partial| {
            view.calculate_fov(partial, 90, 1.0, None, camera::CameraFluid::None, false)
        };
        assert_eq!(fov(&view, 0.0), 90.0);
        let target = camera::field_of_view_modifier(
            &camera::FovPlayer {
                flying: false,
                walking_speed: 0.10,
                movement_speed: 0.13,
                using: None,
            },
            true,
            1.0,
        );
        view.tick(Some(1.27), target);
        assert!(fov(&view, 0.0) < fov(&view, 0.5) && fov(&view, 0.5) < fov(&view, 1.0));
        assert_eq!(view.eye_height_old, 0.0);
        assert!(view.eye_height > 0.0 && view.eye_height < 1.27);
    }
}
