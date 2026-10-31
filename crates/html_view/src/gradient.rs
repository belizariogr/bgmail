//! Gradient sampling for the gradients GPUI cannot draw directly.
//!
//! GPUI backgrounds support two-stop linear gradients only. Radial, sweep
//! (conic) and multi-stop linear gradients are sampled here into a small BGRA
//! texture that GPUI then draws as an image (cached per gradient and size).

use kurbo::{Affine, Point, Rect};
use peniko::{Color, Extend, Gradient, GradientKind};

/// Position along the gradient (0 = first stop, 1 = last) for a point given
/// in the gradient's own coordinate space.
pub fn gradient_t(kind: &GradientKind, p: Point) -> f32 {
    match kind {
        GradientKind::Linear(line) => {
            let d = line.end - line.start;
            let len2 = d.hypot2();
            if len2 == 0.0 {
                0.0
            } else {
                ((p - line.start).dot(d) / len2) as f32
            }
        }
        GradientKind::Radial(radial) => {
            // Concentric approximation of the two-point conical gradient, which
            // covers CSS radial-gradient (start radius 0, shared centre).
            let center = radial.end_center;
            let (r0, r1) = (radial.start_radius as f64, radial.end_radius as f64);
            if r1 == r0 {
                0.0
            } else {
                (((p - center).hypot() - r0) / (r1 - r0)) as f32
            }
        }
        GradientKind::Sweep(sweep) => {
            let v = p - sweep.center;
            let tau = std::f64::consts::TAU;
            let angle = v.y.atan2(v.x).rem_euclid(tau);
            let (start, end) = (sweep.start_angle as f64, sweep.end_angle as f64);
            if end == start {
                0.0
            } else {
                ((angle - start) / (end - start)) as f32
            }
        }
    }
}

/// Applies the gradient's extend mode to `t`.
pub fn extend_t(t: f32, extend: Extend) -> f32 {
    match extend {
        Extend::Pad => t.clamp(0.0, 1.0),
        Extend::Repeat => t.rem_euclid(1.0),
        Extend::Reflect => {
            let m = t.rem_euclid(2.0);
            if m > 1.0 {
                2.0 - m
            } else {
                m
            }
        }
    }
}

/// Colour at `t` (already extended), interpolated in premultiplied sRGB like
/// browsers do, returned as straight-alpha RGBA in 0..=1.
pub fn sample_stops(stops: &[(f32, Color)], t: f32) -> [f32; 4] {
    let Some(first) = stops.first() else {
        return [0.0; 4];
    };
    if t <= first.0 {
        return first.1.components;
    }
    for pair in stops.windows(2) {
        let ((t0, c0), (t1, c1)) = (pair[0], pair[1]);
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            let [r0, g0, b0, a0] = c0.components;
            let [r1, g1, b1, a1] = c1.components;
            let a = a0 + (a1 - a0) * f;
            let mix = |x0: f32, x1: f32| {
                if a == 0.0 {
                    0.0
                } else {
                    (x0 * a0 + (x1 * a1 - x0 * a0) * f) / a
                }
            };
            return [mix(r0, r1), mix(g0, g1), mix(b0, b1), a];
        }
    }
    stops.last().map(|s| s.1.components).unwrap_or([0.0; 4])
}

/// Samples `gradient` (mapped to document space by `transform`) over `area`
/// into a straight-alpha BGRA buffer of `width x height` pixels.
pub fn rasterize(
    gradient: &Gradient,
    transform: &Affine,
    area: Rect,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let stops: Vec<(f32, Color)> = gradient
        .stops
        .iter()
        .map(|s| (s.offset, s.color.to_alpha_color()))
        .collect();
    let inverse = transform.inverse();
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let doc = Point::new(
                area.x0 + (x as f64 + 0.5) * area.width() / width as f64,
                area.y0 + (y as f64 + 0.5) * area.height() / height as f64,
            );
            let t = extend_t(gradient_t(&gradient.kind, inverse * doc), gradient.extend);
            let [r, g, b, a] = sample_stops(&stops, t);
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            out.extend_from_slice(&[byte(b), byte(g), byte(r), byte(a)]);
        }
    }
    out
}

/// Whether GPUI can draw this gradient natively (two-stop linear).
pub fn is_native(gradient: &Gradient) -> bool {
    matches!(gradient.kind, GradientKind::Linear(_)) && gradient.stops.len() <= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_t_projects_onto_the_line() {
        let g = Gradient::new_linear((0.0, 0.0), (10.0, 0.0));
        assert_eq!(gradient_t(&g.kind, Point::new(5.0, 3.0)), 0.5);
    }

    #[test]
    fn radial_t_grows_with_distance() {
        let g = Gradient::new_radial((0.0, 0.0), 10.0);
        assert_eq!(gradient_t(&g.kind, Point::new(0.0, 0.0)), 0.0);
        assert_eq!(gradient_t(&g.kind, Point::new(5.0, 0.0)), 0.5);
    }

    #[test]
    fn sweep_t_follows_angle() {
        let g = Gradient::new_sweep((0.0, 0.0), 0.0, std::f32::consts::TAU);
        let t = gradient_t(&g.kind, Point::new(0.0, 1.0));
        assert!((t - 0.25).abs() < 1e-4, "{t}");
    }

    #[test]
    fn extend_modes() {
        assert_eq!(extend_t(1.5, Extend::Pad), 1.0);
        assert_eq!(extend_t(1.25, Extend::Repeat), 0.25);
        assert_eq!(extend_t(1.25, Extend::Reflect), 0.75);
    }

    #[test]
    fn stops_interpolate_in_premultiplied_space() {
        let red = Color::new([1.0, 0.0, 0.0, 1.0]);
        let clear = Color::new([0.0, 0.0, 1.0, 0.0]);
        // Fading to transparent keeps the hue (no muddy blue tint).
        let mid = sample_stops(&[(0.0, red), (1.0, clear)], 0.5);
        assert_eq!(mid, [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(sample_stops(&[(0.2, red)], 0.0), red.components);
    }

    #[test]
    fn rasterizes_bgra_pixels() {
        let g = Gradient::new_linear((0.0, 0.0), (2.0, 0.0)).with_stops([
            Color::new([0.0, 0.0, 0.0, 1.0]),
            Color::new([1.0, 1.0, 1.0, 1.0]),
        ]);
        let px = rasterize(&g, &Affine::IDENTITY, Rect::new(0.0, 0.0, 2.0, 1.0), 2, 1);
        assert_eq!(px.len(), 8);
        assert!(px[0] < px[4]); // darker on the left
        assert_eq!(px[3], 255);
    }

    #[test]
    fn native_only_for_two_stop_linear() {
        let two = Gradient::new_linear((0.0, 0.0), (1.0, 0.0))
            .with_stops([Color::new([0.0; 4]), Color::new([1.0; 4])]);
        assert!(is_native(&two));
        let radial = Gradient::new_radial((0.0, 0.0), 1.0);
        assert!(!is_native(&radial));
    }
}
