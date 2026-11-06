//! Platform-independent geometry from the vanilla fluid tessellator.
use horizon_core::{
    geometry::{Direction, Vec3, trig},
    position::BlockPos,
};
use horizon_world::fluid::{FluidKind, FluidState};
use std::sync::OnceLock;

const MAX_HEIGHT: f32 = 0.888_888_9;
const EPSILON: f32 = 0.001;

#[derive(Clone, Copy)]
pub(super) struct FluidCell {
    pub fluid: FluidState,
    pub solid: bool,
    pub legacy_solid: bool,
    pub face: [u8; 6],
    pub cover: [f32; 6],
    pub overlay: bool,
}

#[derive(Clone, Copy)]
pub(super) enum FluidSprite {
    Still,
    Flow,
    Overlay,
}

pub(super) type FluidQuad = [([f32; 3], [f32; 2]); 4];

/// `isFaceOccludedByState` for a fluid box `height` tall against `state`'s
/// face opposite `direction`.
fn occluded_by(direction: Direction, height: f32, state: &FluidCell) -> bool {
    let opposite = direction.opposite() as usize;
    match state.face[opposite] {
        0 => false,
        1 => direction != Direction::Up || height == 1.0,
        _ => match direction {
            Direction::Up => {
                f64::from(height) >= 1.0 - 1e-7 && state.cover[Direction::Down as usize] >= 1.0
            }
            Direction::Down => state.cover[Direction::Up as usize] >= 1.0,
            _ => f64::from(height) <= f64::from(state.cover[opposite]) + 1e-7,
        },
    }
}
/// `isFaceOccludedBySelf`: the cell's own occlusion face toward `direction`.
fn occluded_by_self(state: &FluidCell, direction: Direction) -> bool {
    match state.face[direction as usize] {
        0 => false,
        1 => true,
        _ => state.cover[direction as usize] >= 1.0,
    }
}
/// `Mth.atan2`: the client's table-driven arctangent.
fn atan2(mut y: f64, mut x: f64) -> f64 {
    static TABLES: OnceLock<(Vec<f64>, Vec<f64>)> = OnceLock::new();
    let (asin, cos) = TABLES.get_or_init(|| {
        (0..257)
            .map(|i| {
                let a = (f64::from(i) / 256.0).asin();
                (a, a.cos())
            })
            .unzip()
    });
    let frac_bias = f64::from_bits(4_805_340_802_404_319_232);
    let d2 = x * x + y * y;
    if d2.is_nan() {
        return f64::NAN;
    }
    let negative_y = y < 0.0;
    if negative_y {
        y = -y;
    }
    let negative_x = x < 0.0;
    if negative_x {
        x = -x;
    }
    let steep = y > x;
    if steep {
        std::mem::swap(&mut x, &mut y);
    }
    // Mth.fastInvSqrt
    let half = 0.5 * d2;
    let mut r = f64::from_bits((6_910_469_410_427_058_090i64 - (d2.to_bits() as i64 >> 1)) as u64);
    r *= 1.5 - half * r * r;
    x *= r;
    y *= r;
    let yp = frac_bias + y;
    let index = yp.to_bits() as i32 as usize;
    let s_phi = yp - frac_bias;
    let sd = y * cos[index] - x * s_phi;
    let mut theta = asin[index] + (6.0 + sd * sd) * sd * 0.166_666_666_666_666_66;
    if steep {
        theta = std::f64::consts::FRAC_PI_2 - theta;
    }
    if negative_x {
        theta = std::f64::consts::PI - theta;
    }
    if negative_y {
        theta = -theta;
    }
    theta
}
pub(super) fn tessellate_geometry(
    [x, y, z]: [i32; 3],
    at: impl Fn([i32; 3]) -> FluidCell,
    flow: impl Fn(FluidState, BlockPos) -> Vec3,
    mut quad: impl FnMut(FluidQuad, FluidSprite, Direction, bool),
) {
    let me = at([x, y, z]);
    let kind = me.fluid.kind();
    if kind == FluidKind::Empty {
        return;
    }
    let water = matches!(kind, FluidKind::Water | FluidKind::FlowingWater);
    let same = |s: FluidCell| s.fluid.kind().same_family(kind);
    let neighbor = |d: Direction| {
        let [dx, dy, dz] = d.normal();
        at([x + dx, y + dy, z + dz])
    };
    let (down, up) = (neighbor(Direction::Down), neighbor(Direction::Up));
    let should_render = |d: Direction| !same(neighbor(d)) && !occluded_by_self(&me, d);
    let render_up = !same(up);
    let render_down =
        should_render(Direction::Down) && !occluded_by(Direction::Down, MAX_HEIGHT, &down);
    let sides = [
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ]
    .map(should_render);
    if !render_up && !render_down && !sides.iter().any(|&s| s) {
        return;
    }
    let height = |p: [i32; 3]| -> f32 {
        let s = at(p);
        if same(s) {
            if same(at([p[0], p[1] + 1, p[2]])) {
                1.0
            } else {
                s.fluid.own_height()
            }
        } else if !s.legacy_solid {
            0.0
        } else {
            -1.0
        }
    };
    let own = height([x, y, z]);
    let (mut ne, mut nw, mut se, mut sw) = (1.0f32, 1.0f32, 1.0f32, 1.0f32);
    if own < 1.0 {
        let n = height([x, y, z - 1]);
        let s = height([x, y, z + 1]);
        let e = height([x + 1, y, z]);
        let w = height([x - 1, y, z]);
        // calculateAverageHeight(self, height2, height1, corner)
        let average = |h2: f32, h1: f32, corner: [i32; 3]| -> f32 {
            if h1 >= 1.0 || h2 >= 1.0 {
                return 1.0;
            }
            let mut sum = [0f32; 2];
            let mut add = |h: f32| {
                if h >= 0.8 {
                    sum[0] += h * 10.0;
                    sum[1] += 10.0;
                } else if h >= 0.0 {
                    sum[0] += h;
                    sum[1] += 1.0;
                }
            };
            if h1 > 0.0 || h2 > 0.0 {
                let hc = height(corner);
                if hc >= 1.0 {
                    return 1.0;
                }
                add(hc);
            }
            add(own);
            add(h1);
            add(h2);
            sum[0] / sum[1]
        };
        ne = average(n, e, [x + 1, y, z - 1]);
        nw = average(n, w, [x - 1, y, z - 1]);
        se = average(s, e, [x + 1, y, z + 1]);
        sw = average(s, w, [x - 1, y, z + 1]);
    }
    let (fx, fy, fz) = (x as f32, y as f32, z as f32);
    let bottom = if render_down { EPSILON } else { 0.0 };
    if render_up && !occluded_by(Direction::Up, nw.min(sw).min(se.min(ne)), &up) {
        nw -= EPSILON;
        sw -= EPSILON;
        se -= EPSILON;
        ne -= EPSILON;
        let flow_vector = flow(me.fluid, BlockPos::new(x, y, z));
        let (layer, uv) = if flow_vector.x == 0.0 && flow_vector.z == 0.0 {
            (
                FluidSprite::Still,
                [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            )
        } else {
            let angle =
                atan2(flow_vector.z, flow_vector.x) as f32 - std::f64::consts::FRAC_PI_2 as f32;
            let s = trig::sin(f64::from(angle)) * 0.25;
            let c = trig::cos(f64::from(angle)) * 0.25;
            (
                FluidSprite::Flow,
                [
                    [0.5 + (-c - s), 0.5 + (-c + s)],
                    [0.5 + (-c + s), 0.5 + (c + s)],
                    [0.5 + (c + s), 0.5 + (c - s)],
                    [0.5 + (c - s), 0.5 + (-c - s)],
                ],
            )
        };
        // FluidState.shouldRenderBackwardUpFace over the 3x3 cells above.
        let backward = (-1..=1).any(|ox| {
            (-1..=1).any(|oz| {
                let s = at([x + ox, y + 1, z + oz]);
                !same(s) && !s.solid
            })
        });
        quad(
            [
                ([fx, fy + nw, fz], uv[0]),
                ([fx, fy + sw, fz + 1.0], uv[1]),
                ([fx + 1.0, fy + se, fz + 1.0], uv[2]),
                ([fx + 1.0, fy + ne, fz], uv[3]),
            ],
            layer,
            Direction::Up,
            backward,
        );
    }
    if render_down {
        let y0 = fy + bottom;
        quad(
            [
                ([fx, y0, fz], [0.0, 0.0]),
                ([fx + 1.0, y0, fz], [1.0, 0.0]),
                ([fx + 1.0, y0, fz + 1.0], [1.0, 1.0]),
                ([fx, y0, fz + 1.0], [0.0, 1.0]),
            ],
            FluidSprite::Still,
            Direction::Down,
            false,
        );
    }
    for (i, d) in [
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ]
    .into_iter()
    .enumerate()
    {
        let (h0, h1, [x0, z0], [x1, z1]) = match d {
            Direction::North => (nw, ne, [fx, fz + EPSILON], [fx + 1.0, fz + EPSILON]),
            Direction::South => (
                se,
                sw,
                [fx + 1.0, fz + 1.0 - EPSILON],
                [fx, fz + 1.0 - EPSILON],
            ),
            Direction::West => (sw, nw, [fx + EPSILON, fz + 1.0], [fx + EPSILON, fz]),
            _ => (
                ne,
                se,
                [fx + 1.0 - EPSILON, fz],
                [fx + 1.0 - EPSILON, fz + 1.0],
            ),
        };
        let face_state = neighbor(d);
        if !sides[i] || occluded_by(d, h0.max(h1), &face_state) {
            continue;
        }
        let is_overlay = water && face_state.overlay;
        let layer = if is_overlay {
            FluidSprite::Overlay
        } else {
            FluidSprite::Flow
        };
        let (v0, v1) = ((1.0 - h0) * 0.5, (1.0 - h1) * 0.5);
        quad(
            [
                ([x0, fy + h0, z0], [0.0, v0]),
                ([x1, fy + h1, z1], [0.5, v1]),
                ([x1, fy + bottom, z1], [0.5, 0.5]),
                ([x0, fy + bottom, z0], [0.0, 0.5]),
            ],
            layer,
            d,
            !is_overlay,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_geometry_respects_surface_height_and_neighbor_faces() {
        let water = FluidCell {
            fluid: FluidState::WATER,
            solid: false,
            legacy_solid: false,
            face: [0; 6],
            cover: [0.0; 6],
            overlay: false,
        };
        let air = FluidCell {
            fluid: FluidState::EMPTY,
            ..water
        };
        let stone = FluidCell {
            solid: true,
            legacy_solid: true,
            face: [1; 6],
            cover: [1.0; 6],
            ..air
        };
        let collect = |neighbors: &[([i32; 3], FluidCell)]| {
            let mut faces = Vec::new();
            tessellate_geometry(
                [0; 3],
                |pos| {
                    if pos == [0; 3] {
                        return water;
                    }
                    neighbors
                        .iter()
                        .find(|(at, _)| *at == pos)
                        .map_or(air, |(_, cell)| *cell)
                },
                |_, _| Vec3::ZERO,
                |quad, sprite, face, back| faces.push((quad, sprite, face, back)),
            );
            faces
        };
        let isolated = collect(&[]);
        assert_eq!(isolated.len(), 6);
        let (top, sprite, _, back) = isolated
            .iter()
            .find(|(_, _, d, _)| *d == Direction::Up)
            .unwrap();
        assert!(matches!(sprite, FluidSprite::Still) && *back);
        assert!(top.iter().all(|(p, _)| (p[1] - 0.739_740_7).abs() < 1e-6));
        let (side, sprite, _, _) = isolated
            .iter()
            .find(|(_, _, d, _)| *d == Direction::East)
            .unwrap();
        assert!(matches!(sprite, FluidSprite::Flow));
        assert!(
            side.iter()
                .all(|(p, uv)| p[0] == 0.999 && uv[0] <= 0.5 && uv[1] <= 0.5)
        );
        assert_eq!(side[0].0[1], top[0].0[1]);

        let bank = collect(&[([1, 0, 0], stone), ([0, -1, 0], stone), ([0, 0, 1], water)]);
        assert_eq!(bank.len(), 3);
        assert!(bank.iter().all(|(_, _, d, _)| !matches!(
            d,
            Direction::East | Direction::Down | Direction::South
        )));
        let stacked = collect(&[([0, 1, 0], water)]);
        assert!(stacked.iter().all(|(_, _, d, _)| *d != Direction::Up));
    }

    #[test]
    fn atan2_follows_the_table_approximation() {
        for (y, x) in [
            (1.0, 0.0),
            (0.0, 1.0),
            (-0.5, 0.25),
            (0.3, -0.9),
            (-1.0, -1.0),
            (0.1, 0.7),
        ] {
            let exact = f64::atan2(y, x);
            assert!((atan2(y, x) - exact).abs() < 1e-6, "{y} {x}");
        }
        assert!(atan2(f64::NAN, 1.0).is_nan());
    }
}
