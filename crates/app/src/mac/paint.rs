//! Drawing the window chrome with CoreGraphics: glass body, neon rim and bloom, light pool,
//! glass pills, window buttons and the resize grip. Mirrors the Windows renderer in win.rs.

use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGColor, CGColorSpace, CGContext, CGGradient, CGGradientDrawingOptions, CGLineCap,
};

use crate::layout::{outline, Button, Layout, Rect, GLASS};
use crate::theme::{self, GLOWS};

pub fn rgba(hex: u32, a: f64) -> [CGFloat; 4] {
    [((hex >> 16) & 0xFF) as f64 / 255.0, ((hex >> 8) & 0xFF) as f64 / 255.0, (hex & 0xFF) as f64 / 255.0, a]
}

fn cgrect(l: f64, t: f64, r: f64, b: f64) -> CGRect {
    CGRect::new(CGPoint::new(l, t), CGSize::new(r - l, b - t))
}

fn gradient(stops: &[([CGFloat; 4], CGFloat)]) -> Option<objc2_core_foundation::CFRetained<CGGradient>> {
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let comps: Vec<CGFloat> = stops.iter().flat_map(|s| s.0).collect();
    let locs: Vec<CGFloat> = stops.iter().map(|s| s.1).collect();
    unsafe { CGGradient::with_color_components(Some(&space), comps.as_ptr(), locs.as_ptr(), stops.len()) }
}

fn set_fill(c: &CGContext, rgba: [CGFloat; 4]) {
    CGContext::set_rgb_fill_color(Some(c), rgba[0], rgba[1], rgba[2], rgba[3]);
}

fn set_stroke(c: &CGContext, rgba: [CGFloat; 4]) {
    CGContext::set_rgb_stroke_color(Some(c), rgba[0], rgba[1], rgba[2], rgba[3]);
}

fn add_outline(c: &CGContext, pts: &[(f32, f32)]) {
    CGContext::begin_path(Some(c));
    CGContext::move_to_point(Some(c), pts[0].0 as f64, pts[0].1 as f64);
    for &(x, y) in &pts[1..] {
        CGContext::add_line_to_point(Some(c), x as f64, y as f64);
    }
    CGContext::close_path(Some(c));
}

/// Stroke the current path with a gradient running from `from` to `to`.
fn stroke_gradient(c: &CGContext, pts: &[(f32, f32)], width: f64, g: &CGGradient, from: CGPoint, to: CGPoint) {
    CGContext::save_g_state(Some(c));
    add_outline(c, pts);
    CGContext::set_line_width(Some(c), width);
    CGContext::replace_path_with_stroked_path(Some(c));
    CGContext::clip(Some(c));
    CGContext::draw_linear_gradient(
        Some(c),
        Some(g),
        from,
        to,
        CGGradientDrawingOptions::DrawsBeforeStartLocation | CGGradientDrawingOptions::DrawsAfterEndLocation,
    );
    CGContext::restore_g_state(Some(c));
}

fn fill_gradient(c: &CGContext, pts: &[(f32, f32)], g: &CGGradient, from: CGPoint, to: CGPoint) {
    CGContext::save_g_state(Some(c));
    add_outline(c, pts);
    CGContext::clip(Some(c));
    CGContext::draw_linear_gradient(Some(c), Some(g), from, to, CGGradientDrawingOptions::empty());
    CGContext::restore_g_state(Some(c));
}

/// Look 06's rim: the glow's light color hot at both ends, dimmer across the middle.
fn rim06(glow: usize, alpha: f64) -> Option<objc2_core_foundation::CFRetained<CGGradient>> {
    let [c0, c1, _, c3] = GLOWS[glow].colors;
    gradient(&[
        (rgba(c0, alpha), 0.0),
        (rgba(c1, alpha), 0.12),
        (rgba(c1, alpha * 0.45), 0.36),
        (rgba(c1, alpha * 0.45), 0.64),
        (rgba(c3, alpha), 0.88),
        (rgba(c0, alpha), 1.0),
    ])
}

fn pill_path(c: &CGContext, r: Rect) {
    let rad = (r.w().min(r.h()) / 2.0) as f64;
    let rect = cgrect(r.l as f64, r.t as f64, r.r as f64, r.b as f64);
    let path = unsafe { objc2_core_graphics::CGPath::with_rounded_rect(rect, rad, rad, std::ptr::null()) };
    CGContext::begin_path(Some(c));
    CGContext::add_path(Some(c), Some(&path));
}

pub struct ChromeState {
    pub glow: usize,
    pub focused: bool,
    pub hover: Option<Button>,
    pub pressed: Option<Button>,
}

/// Everything except the terminal text and the sidebar icons.
pub fn chrome(c: &CGContext, l: &Layout, st: &ChromeState) {
    let b = l.body;
    let tl = CGPoint::new(b.l as f64, b.t as f64);
    let br = CGPoint::new(b.r as f64, b.b as f64);
    // Look 06 is smooth glass, not the old rippled outline.
    let shape = outline(l, 0.0, 0.0, 480);
    let strength = if st.focused { 1.0 } else { 0.5 };

    // Look 06 (Matt, 2026-09-17): thick black glass, a thin neon rim that burns brightest at the
    // left and right ends, a faint glow, and a glossy sheen along the top.
    let across = |a: f64| rim06(st.glow, a);
    let (lx, rx) = (CGPoint::new(b.l as f64, 0.0), CGPoint::new(b.r as f64, 0.0));

    // 1. Faint glow around the rim and a soft pool of light under the window.
    if !l.maximized {
        for i in 0..6 {
            let w = 22.0 - i as f64 * 3.2;
            if let Some(g) = across(0.035 * (0.4 + 0.6 * strength)) {
                stroke_gradient(c, &shape, w, &g, lx, rx);
            }
        }
        if let Some(g) = across(1.0) {
            let (cx, cy) = (b.cx() as f64, b.b as f64 + 10.0);
            for (rxf, ry, a) in [(0.40, 10.0, 0.035), (0.30, 6.0, 0.06)] {
                let rr = b.w() as f64 * rxf;
                CGContext::save_g_state(Some(c));
                CGContext::begin_path(Some(c));
                CGContext::add_ellipse_in_rect(Some(c), cgrect(cx - rr, cy - ry, cx + rr, cy + ry));
                CGContext::clip(Some(c));
                CGContext::set_alpha(Some(c), a);
                CGContext::draw_linear_gradient(Some(c), Some(&g), lx, rx, CGGradientDrawingOptions::empty());
                CGContext::restore_g_state(Some(c));
            }
        }
    }

    // 2. The glass: charcoal at the top fading to black, then the sunken window inside it.
    let window = outline(l, GLASS, 0.0, 480);
    if let Some(g) = gradient(&[
        (rgba(0x2A2A31, 0.98), 0.0),
        (rgba(0x0E0E12, 0.98), 0.22),
        (rgba(0x08080B, 0.98), 0.8),
        (rgba(0x16110D, 0.98), 1.0),
    ]) {
        fill_gradient(c, &shape, &g, CGPoint::new(0.0, b.t as f64), CGPoint::new(0.0, b.b as f64));
    }
    // Colored light caught inside the glass at both ends.
    if let Some(g) = across(0.22 * strength) {
        stroke_gradient(c, &shape, GLASS as f64 * 2.4, &g, lx, rx);
    }
    add_outline(c, &window);
    set_fill(c, rgba(theme::BODY, theme::BODY_OPACITY as f64));
    CGContext::fill_path(Some(c));
    if let Some(g) = gradient(&[
        (rgba(theme::TINT_A, 0.2), 0.0),
        (rgba(theme::TINT_A, 0.0), 0.45),
        (rgba(theme::TINT_B, 0.0), 0.62),
        (rgba(theme::TINT_B, 0.18), 1.0),
    ]) {
        fill_gradient(c, &window, &g, tl, br);
    }
    // The window's edge sits in shadow, with a hairline of glass light.
    for (w, a) in [(5.0, 0.55), (2.0, 0.8)] {
        add_outline(c, &window);
        set_stroke(c, rgba(0x000000, a));
        CGContext::set_line_width(Some(c), w);
        CGContext::stroke_path(Some(c));
    }
    add_outline(c, &outline(l, GLASS - 0.5, 0.0, 480));
    set_stroke(c, rgba(0xFFFFFF, 0.07));
    CGContext::set_line_width(Some(c), 1.0);
    CGContext::stroke_path(Some(c));

    // 3. The neon rim: a soft band and a hot core line, then a pale inner hairline.
    for (w, a) in [(4.5, 0.45), (1.8, 1.0)] {
        if let Some(g) = across(a * strength) {
            stroke_gradient(c, &shape, w, &g, lx, rx);
        }
    }
    if let Some(g) = gradient(&[(rgba(0xFFE9D2, 0.0), 0.0), (rgba(0xFFE9D2, 0.55 * strength), 0.5), (rgba(0xFFE9D2, 0.0), 1.0)]) {
        let edge = outline(l, 1.6, 0.0, 480);
        stroke_gradient(c, &edge, 0.8, &g, CGPoint::new(0.0, b.t as f64), CGPoint::new(0.0, b.b as f64));
    }
    // The top of the tube is turned away from the light.
    if let Some(g) = gradient(&[(rgba(0x000000, 0.5), 0.0), (rgba(0x000000, 0.0), 1.0)]) {
        let to = CGPoint::new(0.0, b.t as f64 + b.h() as f64 * 0.4);
        stroke_gradient(c, &shape, 5.0, &g, CGPoint::new(0.0, b.t as f64), to);
    }

    // 4. Gloss: a soft sheen across the top of the glass.
    if let Some(g) = gradient(&[(rgba(0xFFFFFF, 0.16), 0.0), (rgba(0xFFFFFF, 0.0), 1.0)]) {
        let sheen = outline(l, GLASS * 0.45, 0.0, 480);
        let to = CGPoint::new(0.0, b.t as f64 + l.radius as f64 * 0.9);
        stroke_gradient(c, &sheen, GLASS as f64 * 0.5, &g, CGPoint::new(0.0, b.t as f64), to);
    }

    // 5. Glass pills for the window buttons and the sidebar.
    for r in [l.lights_pill, l.sidebar] {
        pill_path(c, r);
        set_fill(c, rgba(0xFFFFFF, 0.07));
        CGContext::fill_path(Some(c));
        if let Some(g) = gradient(&[(rgba(0xFFFFFF, 0.45), 0.0), (rgba(0xFFFFFF, 0.08), 1.0)]) {
            CGContext::save_g_state(Some(c));
            pill_path(c, r);
            CGContext::set_line_width(Some(c), 1.0);
            CGContext::replace_path_with_stroked_path(Some(c));
            CGContext::clip(Some(c));
            CGContext::draw_linear_gradient(
                Some(c),
                Some(&g),
                CGPoint::new(0.0, r.t as f64),
                CGPoint::new(0.0, r.b as f64),
                CGGradientDrawingOptions::empty(),
            );
            CGContext::restore_g_state(Some(c));
        }
    }

    // 6. Window buttons, macOS style: red closes, yellow minimizes, green zooms.
    let over_lights = matches!(st.hover, Some(Button::Close | Button::Minimize | Button::Zoom));
    for (btn, color) in [(Button::Close, 0xFF5F57), (Button::Minimize, 0xFEBC2E), (Button::Zoom, 0x28C840)] {
        let (x, y, r) = l.button(btn);
        let (x, y, r) = (x as f64, y as f64, r as f64);
        let lit = st.focused || over_lights;
        let mut col = rgba(if lit { color } else { 0x5A5E68 }, 1.0);
        if st.pressed == Some(btn) {
            col = [col[0] * 0.75, col[1] * 0.75, col[2] * 0.75, 1.0];
        }
        set_fill(c, col);
        CGContext::fill_ellipse_in_rect(Some(c), cgrect(x - r, y - r, x + r, y + r));
        if over_lights {
            set_stroke(c, rgba(0x000000, 0.6));
            CGContext::set_line_width(Some(c), 1.2);
            CGContext::set_line_cap(Some(c), CGLineCap::Round);
            let k = 2.6;
            let line = |a: (f64, f64), z: (f64, f64)| {
                CGContext::begin_path(Some(c));
                CGContext::move_to_point(Some(c), x + a.0, y + a.1);
                CGContext::add_line_to_point(Some(c), x + z.0, y + z.1);
                CGContext::stroke_path(Some(c));
            };
            match btn {
                Button::Close => {
                    line((-k, -k), (k, k));
                    line((-k, k), (k, -k));
                }
                Button::Minimize => line((-k - 0.5, 0.0), (k + 0.5, 0.0)),
                _ => {
                    line((-k - 0.5, 0.0), (k + 0.5, 0.0));
                    line((0.0, -k - 0.5), (0.0, k + 0.5));
                }
            }
        }
    }

    // 7. Resize grip: three short strokes in the bottom-right curve.
    if !l.maximized {
        let (gx, gy) = l.grip();
        set_stroke(c, rgba(0xE6E9FF, if st.focused { 0.8 } else { 0.45 }));
        CGContext::set_line_width(Some(c), 2.0);
        CGContext::set_line_cap(Some(c), CGLineCap::Round);
        for i in 0..3 {
            let o = (i as f64 - 1.0) * 6.0;
            let len = 9.0 - 2.5 * i as f64;
            let (cx, cy) = (gx as f64 + o, gy as f64 + o);
            CGContext::begin_path(Some(c));
            CGContext::move_to_point(Some(c), cx - len, cy + len);
            CGContext::add_line_to_point(Some(c), cx + len, cy - len);
            CGContext::stroke_path(Some(c));
        }
    }
}

/// A plain filled rectangle (selection, cell backgrounds, cursor, scroll bar).
pub fn fill_rect(c: &CGContext, l: f64, t: f64, r: f64, b: f64, color: [CGFloat; 4]) {
    set_fill(c, color);
    CGContext::fill_rect(Some(c), cgrect(l, t, r, b));
}

pub fn stroke_rect(c: &CGContext, l: f64, t: f64, r: f64, b: f64, color: [CGFloat; 4]) {
    set_stroke(c, color);
    CGContext::set_line_width(Some(c), 1.0);
    CGContext::stroke_rect(Some(c), cgrect(l + 0.5, t + 0.5, r - 0.5, b - 0.5));
}

pub fn rounded_fill(c: &CGContext, r: Rect, radius: f64, color: [CGFloat; 4]) {
    let rect = cgrect(r.l as f64, r.t as f64, r.r as f64, r.b as f64);
    let path = unsafe { objc2_core_graphics::CGPath::with_rounded_rect(rect, radius, radius, std::ptr::null()) };
    CGContext::begin_path(Some(c));
    CGContext::add_path(Some(c), Some(&path));
    set_fill(c, color);
    CGContext::fill_path(Some(c));
}

pub fn rounded_stroke(c: &CGContext, r: Rect, radius: f64, width: f64, color: [CGFloat; 4]) {
    let rect = cgrect(r.l as f64, r.t as f64, r.r as f64, r.b as f64);
    let path = unsafe { objc2_core_graphics::CGPath::with_rounded_rect(rect, radius, radius, std::ptr::null()) };
    CGContext::begin_path(Some(c));
    CGContext::add_path(Some(c), Some(&path));
    set_stroke(c, color);
    CGContext::set_line_width(Some(c), width);
    CGContext::stroke_path(Some(c));
}

pub fn clear(c: &CGContext, w: f64, h: f64) {
    CGContext::clear_rect(Some(c), cgrect(0.0, 0.0, w, h));
}

#[allow(dead_code)]
fn _uses(_: &CGColor) {}
