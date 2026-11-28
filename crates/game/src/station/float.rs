//! Things floating in the cooking pot and the cauldron. They stand upright
//! in the broth, half under, and drift: the ladle pushes them, they push
//! each other, they stop at the pot's walls, and they tip over when shoved
//! and slowly right themselves again. Pure rules; `board` draws them.

use bevy::math::Vec2;

/// One floating thing, in the station's frame (x, z across the pot).
#[derive(Debug, Clone, PartialEq)]
pub struct Floater {
    /// Which slot it's in and which of the stack it is ("made" for what was made).
    pub key: (String, usize),
    pub p: Vec2,
    pub v: Vec2,
    /// Which way and how far its top leans (radians), and how fast that changes.
    pub lean: Vec2,
    pub lean_v: Vec2,
    /// How much room it takes on the surface.
    pub r: f32,
}

impl Floater {
    pub fn new(key: (String, usize), p: Vec2, r: f32) -> Self {
        Self {
            key,
            p,
            v: Vec2::ZERO,
            lean: Vec2::ZERO,
            lean_v: Vec2::ZERO,
            r,
        }
    }
}

/// The pot's inside: its middle and half its width.
#[derive(Debug, Clone, Copy)]
pub struct Tank {
    pub centre: Vec2,
    pub half: f32,
}

/// The ladle's bowl under the surface: where, how fast, how big.
#[derive(Debug, Clone, Copy)]
pub struct Paddle {
    pub at: Vec2,
    pub vel: Vec2,
    pub r: f32,
}

/// How quickly the broth slows things down (per second).
const DRAG: f32 = 2.2;
/// How hard things spring back upright, and how quickly the wobble dies.
const UPRIGHT: f32 = 45.0;
const WOBBLE_DAMP: f32 = 7.0;
/// How far a shove tips a thing (radians per block/s of sudden push).
const TIP: f32 = 2.5;
const MAX_LEAN: f32 = 0.75;

/// Keep a floater inside the walls; returns whether it touched one.
fn walls(f: &mut Floater, tank: Tank) {
    let lim = (tank.half - f.r).max(0.0);
    for axis in 0..2 {
        let off = f.p[axis] - tank.centre[axis];
        if off.abs() > lim {
            f.p[axis] = tank.centre[axis] + off.signum() * lim;
            if f.v[axis] * off.signum() > 0.0 {
                f.v[axis] = -f.v[axis] * 0.3;
            }
        }
    }
}

/// One step of the pot.
pub fn step(fs: &mut [Floater], tank: Tank, paddle: Option<Paddle>, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    let before: Vec<Vec2> = fs.iter().map(|f| f.v).collect();
    let drag = (-DRAG * dt).exp();
    for f in fs.iter_mut() {
        f.v *= drag;
    }
    // The ladle shoves what it meets, and drags what's near along in its wake.
    if let Some(pd) = paddle {
        for f in fs.iter_mut() {
            let d = f.p - pd.at;
            let dist = d.length();
            let min = f.r + pd.r;
            if dist < min {
                let n = if dist > 1e-5 { d / dist } else { Vec2::X };
                f.p = pd.at + n * min;
                let rel = pd.vel - f.v;
                let push = rel.dot(n).max(0.0);
                f.v += n * push + (rel - n * rel.dot(n)) * 0.2;
            } else if dist < min + 0.1 {
                f.v += pd.vel * (1.2 * dt);
            }
        }
    }
    for f in fs.iter_mut() {
        f.p += f.v * dt;
    }
    // Things bump each other apart, then the walls hold them in.
    for _ in 0..3 {
        for i in 0..fs.len() {
            for j in i + 1..fs.len() {
                let d = fs[j].p - fs[i].p;
                let dist = d.length();
                let min = fs[i].r + fs[j].r;
                if dist < min {
                    // A slight sideways slant per pair, so things in a row can slip past each other.
                    let slant = ((i * 7 + j * 3) % 5) as f32 * 0.1 - 0.2;
                    let n = if dist > 1e-5 {
                        (d / dist + d.perp() / dist * slant).normalize()
                    } else {
                        Vec2::new((i as f32).cos(), (i as f32).sin())
                    };
                    let push = (min - dist) / 2.0;
                    fs[i].p -= n * push;
                    fs[j].p += n * push;
                    let rel = (fs[j].v - fs[i].v).dot(n);
                    if rel < 0.0 {
                        let k = rel * 0.75;
                        fs[i].v += n * k;
                        fs[j].v -= n * k;
                    }
                }
            }
        }
        for f in fs.iter_mut() {
            walls(f, tank);
        }
    }
    // A sudden shove tips the top back the other way; then it springs upright.
    for (f, v0) in fs.iter_mut().zip(before) {
        let kick = (f.v - v0 * drag) * -TIP;
        f.lean_v += kick + (-UPRIGHT * f.lean - WOBBLE_DAMP * f.lean_v) * dt;
        f.lean += f.lean_v * dt;
        if f.lean.length() > MAX_LEAN {
            f.lean = f.lean.normalize() * MAX_LEAN;
            f.lean_v *= 0.5;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tank() -> Tank {
        Tank {
            centre: Vec2::new(0.06, 0.0),
            half: 0.2,
        }
    }

    fn inside(f: &Floater, t: Tank) -> bool {
        (f.p - t.centre).abs().max_element() <= t.half - f.r + 1e-4
    }

    #[test]
    fn things_stay_inside_the_pot() {
        let t = tank();
        let mut fs = vec![Floater::new(
            ("pot_1".into(), 0),
            Vec2::new(0.06, 0.0),
            0.04,
        )];
        fs[0].v = Vec2::new(3.0, -2.0);
        for _ in 0..240 {
            step(&mut fs, t, None, 1.0 / 60.0);
            assert!(inside(&fs[0], t), "{:?}", fs[0].p);
        }
    }

    #[test]
    fn things_dont_overlap_each_other() {
        let t = tank();
        let mut fs: Vec<Floater> = (0..6)
            .map(|i| {
                Floater::new(
                    ("pot_1".into(), i),
                    Vec2::new(0.06 + i as f32 * 0.001, 0.0),
                    0.04,
                )
            })
            .collect();
        for _ in 0..120 {
            step(&mut fs, t, None, 1.0 / 60.0);
        }
        for i in 0..fs.len() {
            assert!(inside(&fs[i], t));
            for j in i + 1..fs.len() {
                assert!(fs[i].p.distance(fs[j].p) > 0.07, "{i} {j}");
            }
        }
    }

    #[test]
    fn the_ladle_pushes_things_and_they_tip_then_right_themselves() {
        let t = tank();
        let mut fs = vec![Floater::new(("pot_1".into(), 0), Vec2::new(0.1, 0.0), 0.04)];
        // The ladle sweeps in from the left.
        let mut max_lean: f32 = 0.0;
        for i in 0..20 {
            let at = Vec2::new(-0.1 + i as f32 * 0.012, 0.0);
            step(
                &mut fs,
                t,
                Some(Paddle {
                    at,
                    vel: Vec2::new(0.72, 0.0),
                    r: 0.05,
                }),
                1.0 / 60.0,
            );
            max_lean = max_lean.max(fs[0].lean.length());
        }
        assert!(fs[0].p.x > 0.15, "pushed along: {:?}", fs[0].p);
        assert!(max_lean > 0.05, "tipped");
        for _ in 0..300 {
            step(&mut fs, t, None, 1.0 / 60.0);
        }
        assert!(
            fs[0].lean.length() < 0.01,
            "upright again: {:?}",
            fs[0].lean
        );
        assert!(fs[0].v.length() < 0.01, "settled");
    }
}
