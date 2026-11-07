use horizon_core::random::RandomSource;
use std::array;

/// ClientLevel.addBreakingParticles placement, using local outline boxes.
pub fn breaking_position(
    random: &mut impl RandomSource,
    pos: [i32; 3],
    face: [i32; 3],
    boxes: &[[f64; 6]],
) -> Option<[f64; 3]> {
    let mut bounds = *boxes.first()?;
    for b in &boxes[1..] {
        for i in 0..3 {
            bounds[i] = bounds[i].min(b[i]);
            bounds[i + 3] = bounds[i + 3].max(b[i + 3]);
        }
    }
    // Vanilla consumes all three doubles before replacing the hit-face coordinate.
    let mut p = array::from_fn(|i| {
        f64::from(pos[i])
            + random.next_double() * (bounds[i + 3] - bounds[i] - f64::from(0.2f32))
            + f64::from(0.1f32)
            + bounds[i]
    });
    for i in 0..3 {
        match face[i] {
            -1 => p[i] = f64::from(pos[i]) + bounds[i] - f64::from(0.1f32),
            1 => p[i] = f64::from(pos[i]) + bounds[i + 3] + f64::from(0.1f32),
            _ => {}
        }
    }
    Some(p)
}

/// ClientLevel.addDestroyBlockEffect placement and pre-constructor velocities.
pub fn destroy_positions(
    pos: [i32; 3],
    boxes: &[[f64; 6]],
    mut emit: impl FnMut([f64; 3], [f64; 3]),
) {
    for b in boxes {
        let widths: [f64; 3] = array::from_fn(|i| (b[i + 3] - b[i]).min(1.0));
        let counts: [usize; 3] = widths.map(|w| ((w / 0.25).ceil() as usize).max(2));
        for x in 0..counts[0] {
            for y in 0..counts[1] {
                for z in 0..counts[2] {
                    let cells = [x, y, z];
                    let relative: [f64; 3] =
                        array::from_fn(|i| (cells[i] as f64 + 0.5) / counts[i] as f64);
                    emit(
                        array::from_fn(|i| f64::from(pos[i]) + (relative[i] * widths[i] + b[i])),
                        relative.map(|v| v - 0.5),
                    );
                }
            }
        }
    }
}
