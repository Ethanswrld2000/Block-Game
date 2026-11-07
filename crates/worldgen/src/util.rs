//! Small math helpers shared by the generators.

use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};

pub fn noise(seed: u64, salt: i32, freq: f32, octaves: i32, fractal: FractalType) -> FastNoiseLite {
    let mut n = FastNoiseLite::with_seed((seed as i32).wrapping_add(salt.wrapping_mul(7919)));
    n.set_noise_type(Some(NoiseType::OpenSimplex2));
    n.set_fractal_type(Some(fractal));
    n.set_fractal_octaves(Some(octaves));
    n.set_frequency(Some(freq));
    n
}

/// Piecewise-linear curve through (x, y) points sorted by x. Clamps at the ends.
pub fn spline(points: &[(f32, f32)], x: f32) -> f32 {
    if x <= points[0].0 {
        return points[0].1;
    }
    for w in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    points[points.len() - 1].1
}

/// 0 below `a`, 1 above `b`, smooth in between. Works with a > b too (reversed).
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Deterministic hash of a 2D position, for scattering features.
pub fn hash2(seed: u64, x: i32, z: i32) -> u64 {
    mix(seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
}

/// Deterministic hash of a 3D position.
pub fn hash3(seed: u64, x: i32, y: i32, z: i32) -> u64 {
    mix(hash2(seed, x, z) ^ (y as u64).wrapping_mul(0x1656_67B1_9E37_79F9))
}

fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^ (h >> 33)
}

/// A hash as a float in 0..1.
pub fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_interpolates_and_clamps() {
        let s = [(-1.0, 0.0), (0.0, 10.0), (1.0, 30.0)];
        assert_eq!(spline(&s, -5.0), 0.0);
        assert_eq!(spline(&s, 0.5), 20.0);
        assert_eq!(spline(&s, 9.0), 30.0);
    }

    #[test]
    fn smoothstep_reversed() {
        assert_eq!(smoothstep(1.0, 0.0, 2.0), 0.0);
        assert_eq!(smoothstep(1.0, 0.0, -1.0), 1.0);
    }
}
