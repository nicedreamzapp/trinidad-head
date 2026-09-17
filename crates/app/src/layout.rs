//! Where everything sits in the window, in pixels. Pure math so it can be tested anywhere.
//!
//! The window is larger than the body you see: a transparent margin around the body holds the
//! glow and doubles as the resize border, the way Windows 11 hides its own resize edges.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
}

impl Rect {
    pub fn new(l: f32, t: f32, w: f32, h: f32) -> Rect {
        Rect { l, t, r: l + w, b: t + h }
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
    pub fn w(&self) -> f32 {
        self.r - self.l
    }
    pub fn h(&self) -> f32 {
        self.b - self.t
    }
    pub fn cx(&self) -> f32 {
        (self.l + self.r) / 2.0
    }
    pub fn cy(&self) -> f32 {
        (self.t + self.b) / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Close,
    Minimize,
    Zoom,
    Terminal,
    Folder,
    Glow,
}

pub const BUTTONS: [Button; 6] = [
    Button::Close,
    Button::Minimize,
    Button::Zoom,
    Button::Terminal,
    Button::Folder,
    Button::Glow,
];

/// The sidebar's buttons, top to bottom. Only the glow (color) button: files are dropped
/// straight onto the window, so the terminal and folder icons were just clutter.
pub const SIDEBAR: &[Button] = &[Button::Glow];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Client,
    Caption,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub scale: f32,
    pub maximized: bool,
    pub window: Rect,
    pub body: Rect,
    pub radius: f32,
    pub lights_pill: Rect,
    pub sidebar: Rect,
    /// Where terminal text goes.
    pub text: Rect,
}

impl Layout {
    pub fn new(w: f32, h: f32, scale: f32, maximized: bool) -> Layout {
        let s = scale;
        // Extra room underneath for the light the window casts on the "floor".
        // The glow has to fade out completely before the window's real (square) edge, or the cut-off shows.
        let (margin, floor) = if maximized { (0.0, 0.0) } else { (46.0 * s, 66.0 * s) };
        let window = Rect { l: 0.0, t: 0.0, r: w, b: h };
        let body = Rect { l: margin, t: margin, r: (w - margin).max(margin + 1.0), b: (h - floor).max(margin + 1.0) };
        let radius = if maximized { 0.0 } else { (72.0 * s).min(body.h() / 2.0).min(body.w() / 2.0) };

        let pill_h = 26.0 * s;
        let top = body.t + 14.0 * s;
        let lights_pill = Rect::new(body.l + 58.0 * s, top, 74.0 * s, pill_h);

        let side_w = 40.0 * s;
        let side_h = (SIDEBAR.len() as f32 * 40.0 * s + 12.0 * s).min(body.h() - 90.0 * s).max(side_w);
        let sidebar = Rect::new(body.l + 16.0 * s, body.cy() - side_h / 2.0 + 14.0 * s, side_w, side_h);

        let text = Rect {
            l: sidebar.r + 18.0 * s,
            t: body.t + 56.0 * s,
            r: body.r - 40.0 * s,
            b: body.b - 28.0 * s,
        };
        Layout { scale, maximized, window, body, radius, lights_pill, sidebar, text }
    }

    /// Centre and radius of each round button.
    pub fn button(&self, b: Button) -> (f32, f32, f32) {
        let s = self.scale;
        let lp = self.lights_pill;
        let sb = self.sidebar;
        let slot = |i: f32| sb.t + 6.0 * s + 20.0 * s + i * 40.0 * s;
        match b {
            Button::Close => (lp.l + 16.0 * s, lp.cy(), 6.0 * s),
            Button::Minimize => (lp.l + 37.0 * s, lp.cy(), 6.0 * s),
            Button::Zoom => (lp.l + 58.0 * s, lp.cy(), 6.0 * s),
            side => match SIDEBAR.iter().position(|&b| b == side) {
                Some(i) => (sb.cx(), slot(i as f32), 15.0 * s),
                // Not on this platform's sidebar: nowhere, so it can't be hit.
                None => (-10_000.0, -10_000.0, 0.0),
            },
        }
    }

    /// The resize grip: a spot just inside the bottom-right curve.
    pub fn grip(&self) -> (f32, f32) {
        let k = self.radius * (1.0 - std::f32::consts::FRAC_1_SQRT_2) + 22.0 * self.scale;
        (self.body.r - k, self.body.b - k)
    }

    pub fn button_at(&self, x: f32, y: f32) -> Option<Button> {
        BUTTONS.into_iter().find(|&b| {
            let (cx, cy, r) = self.button(b);
            if r == 0.0 {
                return false; // not on this platform
            }
            // Small targets get a finger-friendly minimum.
            let r = r.max(9.0 * self.scale) + 2.0 * self.scale;
            (x - cx).powi(2) + (y - cy).powi(2) <= r * r
        })
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        if !self.maximized {
            let s = self.scale;
            let e = 6.0 * s;
            let (gx, gy) = self.grip();
            if (x - gx).powi(2) + (y - gy).powi(2) < (18.0 * s).powi(2) {
                return Hit::BottomRight;
            }
            let (l, r, t, b) = (
                x < self.body.l + e,
                x >= self.body.r - e,
                y < self.body.t + e,
                y >= self.body.b - e,
            );
            // The rounded corners are generous, so the diagonal grab zone is too.
            let near = |cx: f32, cy: f32| (x - cx).powi(2) + (y - cy).powi(2) < (self.radius * 0.6 + e).powi(2);
            let (bl, bt, br, bb) = (self.body.l, self.body.t, self.body.r, self.body.b);
            if near(bl, bt) && (l || t || !self.in_body(x, y)) {
                return Hit::TopLeft;
            }
            if near(br, bt) && (r || t || !self.in_body(x, y)) {
                return Hit::TopRight;
            }
            if near(bl, bb) && (l || b || !self.in_body(x, y)) {
                return Hit::BottomLeft;
            }
            if near(br, bb) && (r || b || !self.in_body(x, y)) {
                return Hit::BottomRight;
            }
            if l {
                return Hit::Left;
            }
            if r {
                return Hit::Right;
            }
            if t {
                return Hit::Top;
            }
            if b {
                return Hit::Bottom;
            }
        }
        if self.button_at(x, y).is_some() || self.sidebar.contains(x, y) {
            return Hit::Client;
        }
        if y < self.text.t {
            return Hit::Caption;
        }
        Hit::Client
    }

    /// Inside the visible rounded body.
    pub fn in_body(&self, x: f32, y: f32) -> bool {
        let b = self.body;
        if !b.contains(x, y) {
            return false;
        }
        let r = self.radius;
        let cx = x.clamp(b.l + r, b.r - r);
        let cy = y.clamp(b.t + r, b.b - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }
}

/// Points around the window outline: a rounded rectangle of the layout's radius, shrunk by
/// `inset`, with its edges pushed in and out by a slow ripple of height `wobble`.
pub fn outline(l: &Layout, inset: f32, wobble: f32, n: usize) -> Vec<(f32, f32)> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let b = l.body;
    let (x0, y0, x1, y1) = (b.l + inset + wobble, b.t + inset + wobble, b.r - inset - wobble, b.b - inset - wobble);
    let r = (l.radius - inset - wobble).max(0.0).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let (w, h) = (x1 - x0 - 2.0 * r, y1 - y0 - 2.0 * r);
    let arc = FRAC_PI_2 * r;
    let total = 2.0 * (w + h) + 4.0 * arc;
    // Segments in order: top edge, top-right arc, right edge, bottom-right arc, bottom, bottom-left, left, top-left.
    let point = |d: f32| -> (f32, f32, f32, f32) {
        let mut d = d;
        let corner = |cx: f32, cy: f32, start: f32, d: f32| {
            let a = start + if r > 0.0 { d / r } else { 0.0 };
            (cx + r * a.cos(), cy + r * a.sin(), a.cos(), a.sin())
        };
        if d < w {
            return (x0 + r + d, y0, 0.0, -1.0);
        }
        d -= w;
        if d < arc {
            return corner(x1 - r, y0 + r, -FRAC_PI_2, d);
        }
        d -= arc;
        if d < h {
            return (x1, y0 + r + d, 1.0, 0.0);
        }
        d -= h;
        if d < arc {
            return corner(x1 - r, y1 - r, 0.0, d);
        }
        d -= arc;
        if d < w {
            return (x1 - r - d, y1, 0.0, 1.0);
        }
        d -= w;
        if d < arc {
            return corner(x0 + r, y1 - r, FRAC_PI_2, d);
        }
        d -= arc;
        if d < h {
            return (x0, y1 - r - d, -1.0, 0.0);
        }
        d -= h;
        corner(x0 + r, y0 + r, PI, d.min(arc))
    };
    (0..n)
        .map(|i| {
            let d = total * i as f32 / n as f32;
            let (x, y, nx, ny) = point(d);
            let u = d / total * 2.0 * PI;
            // Two slow waves so the shape never looks machine-regular.
            let k = wobble * (0.65 * (3.0 * u + 0.7).sin() + 0.35 * (5.0 * u + 2.1).sin());
            (x + nx * k, y + ny * k)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_corners_caption_and_buttons() {
        let l = Layout::new(1000.0, 640.0, 1.0, false);
        assert_eq!(l.hit(48.0, 320.0), Hit::Left);
        assert_eq!(l.hit(952.0, 320.0), Hit::Right);
        assert_eq!(l.hit(500.0, 5.0), Hit::Top);
        assert_eq!(l.hit(50.0, 50.0), Hit::TopLeft);
        assert_eq!(l.hit(952.0, 576.0), Hit::BottomRight);
        assert_eq!(l.hit(500.0, 620.0), Hit::Bottom);
        assert_eq!(l.hit(500.0, 70.0), Hit::Caption);
        assert_eq!(l.hit(500.0, 300.0), Hit::Client);
        let (gx, gy) = l.grip();
        assert!(l.in_body(gx, gy));
        assert_eq!(l.hit(gx, gy), Hit::BottomRight);
        let (cx, cy, _) = l.button(Button::Close);
        assert_eq!(l.button_at(cx, cy), Some(Button::Close));
        assert_eq!(l.hit(cx, cy), Hit::Client);
        let (gx, gy, _) = l.button(Button::Glow);
        assert_eq!(l.button_at(gx, gy), Some(Button::Glow));
    }

    #[test]
    fn maximized_has_no_margin_or_resize() {
        let l = Layout::new(1000.0, 640.0, 1.0, true);
        assert_eq!(l.body.l, 0.0);
        assert_eq!(l.radius, 0.0);
        assert_eq!(l.hit(1.0, 320.0), Hit::Client);
    }

    #[test]
    fn outline_stays_inside_the_window() {
        let l = Layout::new(1000.0, 640.0, 1.0, false);
        let pts = outline(&l, 0.0, 5.0, 480);
        assert_eq!(pts.len(), 480);
        for (x, y) in pts {
            assert!(x >= l.body.l - 0.01 && x <= l.body.r + 0.01 && y >= l.body.t - 0.01 && y <= l.body.b + 0.01, "{x},{y}");
        }
    }

    #[test]
    fn body_shape() {
        let l = Layout::new(1000.0, 640.0, 1.0, false);
        assert!(l.in_body(500.0, 320.0));
        assert!(!l.in_body(l.body.l + 1.0, l.body.t + 1.0));
        assert!(l.text.l > l.sidebar.r && l.text.t > l.lights_pill.b);
    }
}
