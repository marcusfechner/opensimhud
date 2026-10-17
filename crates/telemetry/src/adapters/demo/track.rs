//! A Monza-like circuit and the speed profile the demo cars drive on it.

use std::f64::consts::PI;

/// Hand-placed outline points (arbitrary units, y down); smoothed with Catmull-Rom.
const RAW: [[f64; 2]; 38] = [
    [820., 800.],
    [600., 800.],
    [380., 800.],
    [300., 800.],
    [275., 798.],
    [262., 782.],
    [248., 776.],
    [225., 772.],
    [170., 750.],
    [130., 700.],
    [115., 630.],
    [118., 560.],
    [125., 500.],
    [115., 485.],
    [110., 470.],
    [122., 455.],
    [135., 420.],
    [150., 360.],
    [175., 320.],
    [210., 305.],
    [260., 300.],
    [300., 310.],
    [330., 335.],
    [420., 390.],
    [520., 440.],
    [560., 455.],
    [580., 475.],
    [605., 478.],
    [628., 462.],
    [655., 470.],
    [760., 490.],
    [880., 500.],
    [960., 510.],
    [1030., 540.],
    [1065., 620.],
    [1055., 710.],
    [1000., 775.],
    [920., 800.],
];

pub const LENGTH: f64 = 5793.0;
/// Samples along the lap.
pub const N: usize = 1200;
pub const SECTORS: [f64; 3] = [0.0, 0.345, 0.69];
/// Target lap time of a pace-1.0 car, seconds.
pub const LAP: f64 = 107.6;

pub struct TrackShape {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub heading: Vec<f64>,
    curv: Vec<f64>,
}

/// Equal-distance samples of the smoothed outline, with heading and smoothed curvature.
pub fn build() -> TrackShape {
    let n = RAW.len();
    let mut dense = Vec::with_capacity(n * 40);
    for i in 0..n {
        let (p0, p1, p2, p3) = (
            RAW[(i + n - 1) % n],
            RAW[i],
            RAW[(i + 1) % n],
            RAW[(i + 2) % n],
        );
        for k in 0..40 {
            let t = k as f64 / 40.0;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            dense.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    let mut cum = vec![0.0];
    for i in 1..=dense.len() {
        let (a, b) = (dense[i - 1], dense[i % dense.len()]);
        cum.push(cum[i - 1] + (b[0] - a[0]).hypot(b[1] - a[1]));
    }
    let raw_len = *cum.last().unwrap();
    let scale = LENGTH / raw_len;
    let (mut x, mut y) = (vec![0.0; N], vec![0.0; N]);
    let mut j = 0;
    for i in 0..N {
        let target = i as f64 / N as f64 * raw_len;
        while cum[j + 1] < target {
            j += 1;
        }
        let (a, b) = (dense[j], dense[(j + 1) % dense.len()]);
        let t = (target - cum[j]) / (cum[j + 1] - cum[j]).max(1e-9);
        x[i] = (a[0] + (b[0] - a[0]) * t) * scale;
        y[i] = (a[1] + (b[1] - a[1]) * t) * scale;
    }
    let heading: Vec<f64> = (0..N)
        .map(|i| {
            let (a, b) = ((i + N - 1) % N, (i + 1) % N);
            (y[b] - y[a]).atan2(x[b] - x[a])
        })
        .collect();
    let ds = LENGTH / N as f64;
    let raw: Vec<f64> = (0..N)
        .map(|i| {
            let mut d = heading[(i + 1) % N] - heading[i];
            while d > PI {
                d -= 2.0 * PI;
            }
            while d < -PI {
                d += 2.0 * PI;
            }
            d / ds
        })
        .collect();
    let curv = smooth(&raw, 6);
    TrackShape {
        x,
        y,
        heading,
        curv,
    }
}

fn smooth(a: &[f64], w: usize) -> Vec<f64> {
    let n = a.len();
    (0..n)
        .map(|i| (0..=2 * w).map(|k| a[(i + n + k - w) % n]).sum::<f64>() / (2 * w + 1) as f64)
        .collect()
}

/// Speed (m/s) and driver inputs at each sample for a pace-1.0 lap of [`LAP`] seconds.
pub struct Profile {
    pub v: Vec<f64>,
    pub throttle: Vec<f64>,
    pub brake: Vec<f64>,
    pub steer: Vec<f64>,
    /// Time from the line to each sample; `t_at[N]` is the lap time.
    pub t_at: Vec<f64>,
    /// ∫(0.12 + throttle) dt over a lap, to spread fuel use.
    pub fuel_integral: f64,
}

pub fn profile(shape: &TrackShape) -> Profile {
    const VMAX: f64 = 79.0;
    const ALAT: f64 = 19.0;
    const ABRAKE: f64 = 15.0;
    let ds = LENGTH / N as f64;
    let mut v: Vec<f64> = shape
        .curv
        .iter()
        .map(|k| {
            if k.abs() > 1e-5 {
                VMAX.min((ALAT / k.abs()).sqrt())
            } else {
                VMAX
            }
        })
        .collect();
    // braking (backwards) and acceleration (forwards) limits, twice for the wrap-around
    for _ in 0..2 {
        for i in (0..N).rev() {
            let next = v[(i + 1) % N];
            v[i] = v[i].min((next * next + 2.0 * ABRAKE * ds).sqrt());
        }
        for i in 0..N {
            let prev = v[(i + N - 1) % N];
            let acc = (9.5 * (1.0 - prev / 86.0)).max(1.2);
            v[i] = v[i].min((prev * prev + 2.0 * acc * ds).sqrt());
        }
    }
    let (mut thr, mut brk, mut steer) = (vec![0.0; N], vec![0.0; N], vec![0.0; N]);
    for i in 0..N {
        let (a, b) = (v[i], v[(i + 1) % N]);
        let accel = (b * b - a * a) / (2.0 * ds);
        if accel < -4.0 {
            brk[i] = (-accel / ABRAKE).clamp(0.0, 1.0);
        } else if accel > 0.6 || a > VMAX - 1.0 {
            thr[i] = 1.0;
        } else {
            thr[i] = 0.55 + 0.3 * (a / VMAX);
        }
        steer[i] = (shape.curv[i] * 55.0).clamp(-1.0, 1.0);
    }
    let (throttle, brake, steer) = (smooth(&thr, 3), smooth(&brk, 2), smooth(&steer, 4));
    let mut t_at = vec![0.0; N + 1];
    for i in 0..N {
        t_at[i + 1] = t_at[i] + ds / ((v[i] + v[(i + 1) % N]) / 2.0);
    }
    let k = t_at[N] / LAP;
    v.iter_mut().for_each(|s| *s *= k);
    t_at.iter_mut().for_each(|t| *t /= k);
    let fuel_integral = (0..N)
        .map(|i| (0.12 + throttle[i]) * (t_at[i + 1] - t_at[i]))
        .sum();
    Profile {
        v,
        throttle,
        brake,
        steer,
        t_at,
        fuel_integral,
    }
}

/// Sample index pair and blend factor for a lap fraction.
pub fn sample(pct: f64) -> (usize, usize, f64) {
    let f = pct.rem_euclid(1.0) * N as f64;
    let i = (f as usize) % N;
    (i, (i + 1) % N, f - f.floor())
}

pub fn lerp_at(a: &[f64], pct: f64) -> f64 {
    let (i, j, t) = sample(pct);
    a[i] + (a[j] - a[i]) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_hits_target_lap_and_sane_speeds() {
        let shape = build();
        let p = profile(&shape);
        assert!((p.t_at[N] - LAP).abs() < 1e-9);
        let kmh: Vec<f64> = p.v.iter().map(|v| v * 3.6).collect();
        let (lo, hi) = kmh
            .iter()
            .fold((f64::MAX, 0.0f64), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(lo > 60.0 && lo < 120.0, "slowest corner {lo} km/h");
        assert!(hi > 230.0 && hi < 300.0, "top speed {hi} km/h");
    }
}
