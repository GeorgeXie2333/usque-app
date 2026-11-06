//! Anti-aliased shapes for the native windows' custom-drawn controls.
//!
//! Pure: shapes render into a top-down 32-bit DIB buffer that Win32 code
//! blits, after which it draws text natively. The setup bootstrapper's
//! `paint.h` implements the same algorithm.

use crate::state::Bar;
use crate::tokens::{MOTION_BASE_MS, MOTION_SCAN_MS, STALL_MS, Tokens};

/// An opaque 8-bit sRGB color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const WHITE: Self = Self::from_colorref(0x00ff_ffff);

    /// Converts a Win32 COLORREF (0x00BBGGRR).
    pub const fn from_colorref(value: u32) -> Self {
        Self {
            r: (value & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: ((value >> 16) & 0xff) as u8,
        }
    }

    /// The color as a 32-bit DIB pixel (0x00RRGGBB in memory order B, G, R).
    pub const fn dib(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    /// This color at `amount` opacity over `base`.
    pub fn over(self, base: Self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        let channel = |top: u8, bottom: u8| {
            (f32::from(top) * amount + f32::from(bottom) * (1.0 - amount) + 0.5) as u8
        };
        Self {
            r: channel(self.r, base.r),
            g: channel(self.g, base.g),
            b: channel(self.b, base.b),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn inset(self, amount: f32) -> Self {
        Self::new(
            self.left + amount,
            self.top + amount,
            self.right - amount,
            self.bottom - amount,
        )
    }
}

/// Signed distance from a point to a rounded rectangle; negative inside.
fn rounded_distance(x: f32, y: f32, rect: Rect, radius: f32) -> f32 {
    let half_width = ((rect.right - rect.left) / 2.0).max(0.0);
    let half_height = ((rect.bottom - rect.top) / 2.0).max(0.0);
    let radius = radius.clamp(0.0, half_width.min(half_height));
    let qx = (x - (rect.left + rect.right) / 2.0).abs() - half_width + radius;
    let qy = (y - (rect.top + rect.bottom) / 2.0).abs() - half_height + radius;
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// The part of pixel column `[x, x + 1)` inside `[start, end)`.
fn column_overlap(x: f32, start: f32, end: f32) -> f32 {
    ((x + 1.0).min(end) - x.max(start)).clamp(0.0, 1.0)
}

/// A top-down 32-bit pixel buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, background: Rgb) -> Self {
        Self {
            width,
            height,
            pixels: vec![background.dib(); width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Reverses every row, for device contexts that mirror coordinates but
    /// not DIB contents.
    pub fn mirror(&mut self) {
        if self.width > 0 {
            for row in self.pixels.chunks_mut(self.width) {
                row.reverse();
            }
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> Rgb {
        let value = self.pixels[y * self.width + x];
        Rgb {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }
    }

    /// Blends `shade(x, y)` = (color, coverage) at every pixel center that a
    /// shape bounded by `bounds` can reach.
    fn paint(&mut self, bounds: Rect, mut shade: impl FnMut(f32, f32) -> Option<(Rgb, f32)>) {
        let clamp = |value: f32, limit: usize| (value.max(0.0) as usize).min(limit);
        let (left, right) = (
            clamp(bounds.left.floor() - 1.0, self.width),
            clamp(bounds.right.ceil() + 1.0, self.width),
        );
        let (top, bottom) = (
            clamp(bounds.top.floor() - 1.0, self.height),
            clamp(bounds.bottom.ceil() + 1.0, self.height),
        );
        for y in top..bottom {
            for x in left..right {
                if let Some((color, amount)) = shade(x as f32, y as f32)
                    && amount > 0.0
                {
                    let index = y * self.width + x;
                    let base = self.pixel(x, y);
                    self.pixels[index] = color.over(base, amount).dib();
                }
            }
        }
    }

    pub fn fill_rounded(&mut self, rect: Rect, radius: f32, color: Rgb) {
        self.paint(rect, |x, y| {
            Some((
                color,
                coverage(rounded_distance(x + 0.5, y + 0.5, rect, radius)),
            ))
        });
    }

    /// A stroke of `width` lying inside the rounded rectangle's edge.
    pub fn stroke_rounded(&mut self, rect: Rect, radius: f32, width: f32, color: Rgb) {
        self.paint(rect, |x, y| {
            let distance = rounded_distance(x + 0.5, y + 0.5, rect, radius);
            Some((
                color,
                coverage((distance + width / 2.0).abs() - width / 2.0),
            ))
        });
    }

    /// A round-capped line segment.
    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), width: f32, color: Rgb) {
        let bounds = Rect::new(
            from.0.min(to.0) - width,
            from.1.min(to.1) - width,
            from.0.max(to.0) + width,
            from.1.max(to.1) + width,
        );
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length = dx * dx + dy * dy;
        self.paint(bounds, |x, y| {
            let (px, py) = (x + 0.5 - from.0, y + 0.5 - from.1);
            let t = if length > 0.0 {
                ((px * dx + py * dy) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance = (px - dx * t).hypot(py - dy * t) - width / 2.0;
            Some((color, coverage(distance)))
        });
    }
}

/// Footer and inline button roles, matching Flutter's filled, outlined and
/// text buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Filled,
    /// A filled action that deletes data; it uses the danger tone.
    Destructive,
    Outlined,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonLook {
    pub fill: Option<Rgb>,
    pub border: Option<Rgb>,
    pub ink: Rgb,
    /// Keyboard focus ring; it must contrast with the button's own fill.
    pub ring: Rgb,
}

/// Button colors from the shared tokens. High contrast uses system colors
/// in the Win32 layer instead.
pub fn button_look(kind: ButtonKind, enabled: bool, pressed: bool, tokens: &Tokens) -> ButtonLook {
    let color = Rgb::from_colorref;
    let ink = |active: u32| color(if enabled { active } else { tokens.disabled_ink });
    let pressed_fill = (enabled && pressed).then(|| color(tokens.pressed));
    match kind {
        ButtonKind::Filled => ButtonLook {
            fill: Some(color(if !enabled {
                tokens.disabled_fill
            } else if pressed {
                tokens.primary_pressed
            } else {
                tokens.primary
            })),
            border: None,
            ink: ink(tokens.on_primary),
            ring: color(tokens.on_primary),
        },
        ButtonKind::Destructive => ButtonLook {
            fill: Some(color(if !enabled {
                tokens.disabled_fill
            } else if pressed {
                tokens.danger_pressed
            } else {
                tokens.danger
            })),
            border: None,
            ink: ink(tokens.on_danger),
            ring: color(tokens.on_danger),
        },
        ButtonKind::Outlined => ButtonLook {
            fill: pressed_fill,
            border: Some(color(if enabled {
                tokens.hairline_strong
            } else {
                tokens.hairline
            })),
            ink: ink(tokens.ink),
            ring: color(tokens.primary),
        },
        ButtonKind::Text => ButtonLook {
            fill: pressed_fill,
            border: None,
            ink: ink(tokens.primary),
            ring: color(tokens.primary),
        },
    }
}

/// The result shown by a page's status mark. Decorative: the page title
/// states the same result in words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mark {
    #[default]
    Success,
    Caution,
    Danger,
    Neutral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkColors {
    pub background: Rgb,
    /// The tone at Flutter's tint over the canvas.
    pub soft: Rgb,
    pub tone: Rgb,
    /// High contrast outlines the circle instead of tinting it.
    pub outline: Option<Rgb>,
}

/// Draws a glyph inside the square `frame`.
fn glyph(canvas: &mut Canvas, mark: Mark, frame: Rect, width: f32, color: Rgb) {
    let size = frame.right - frame.left;
    let point = |x: f32, y: f32| (frame.left + x * size, frame.top + y * size);
    let mut line = |from: (f32, f32), to: (f32, f32)| {
        canvas.line(point(from.0, from.1), point(to.0, to.1), width, color);
    };
    match mark {
        Mark::Success => {
            line((0.29, 0.52), (0.44, 0.66));
            line((0.44, 0.66), (0.71, 0.37));
        }
        Mark::Danger => {
            line((0.36, 0.36), (0.64, 0.64));
            line((0.64, 0.36), (0.36, 0.64));
        }
        Mark::Caution => {
            line((0.5, 0.29), (0.5, 0.55));
            line((0.5, 0.70), (0.5, 0.70));
        }
        Mark::Neutral => line((0.34, 0.5), (0.66, 0.5)),
    }
}

/// A circular result mark for completion and failure pages.
pub fn render_mark(size: usize, mark: Mark, colors: &MarkColors) -> Canvas {
    let mut canvas = Canvas::new(size, size, colors.background);
    let edge = size as f32;
    let circle = Rect::new(0.0, 0.0, edge, edge);
    match colors.outline {
        Some(outline) => canvas.stroke_rounded(circle, edge / 2.0, (edge / 18.0).max(1.0), outline),
        None => canvas.fill_rounded(circle, edge / 2.0, colors.soft),
    }
    glyph(
        &mut canvas,
        mark,
        circle,
        (edge * 0.085).max(1.5),
        colors.tone,
    );
    canvas
}

/// Flutter `WarningBanner` geometry in logical pixels: padding before the
/// icon, its top offset, icon size, and the gap before the text.
pub const NOTICE_PADDING: f32 = 14.0;
pub const NOTICE_TOP: f32 = 13.0;
pub const NOTICE_ICON: f32 = 18.0;
pub const NOTICE_GAP: f32 = 11.0;
pub const NOTICE_TRAILING: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoticeColors {
    pub background: Rgb,
    pub fill: Rgb,
    pub border: Rgb,
    pub tone: Rgb,
}

/// A rounded caution or danger notice with its icon; text is drawn natively
/// at `NOTICE_PADDING + NOTICE_ICON + NOTICE_GAP` from the leading edge.
pub fn render_notice(
    width: usize,
    height: usize,
    danger: bool,
    colors: &NoticeColors,
    scale: f32,
) -> Canvas {
    let mut canvas = Canvas::new(width, height, colors.background);
    let bounds = Rect::new(0.0, 0.0, width as f32, height as f32);
    let radius = crate::tokens::RADIUS_CONTROL as f32 * scale;
    canvas.fill_rounded(bounds, radius, colors.fill);
    canvas.stroke_rounded(bounds, radius, scale.max(1.0), colors.border);
    let (left, top, size) = (
        NOTICE_PADDING * scale,
        NOTICE_TOP * scale,
        NOTICE_ICON * scale,
    );
    let stroke = (1.7 * scale).max(1.0);
    if danger {
        let icon = Rect::new(left, top, left + size, top + size);
        canvas.stroke_rounded(icon, size / 2.0, stroke, colors.tone);
        glyph(&mut canvas, Mark::Danger, icon, stroke, colors.tone);
    } else {
        let point = |x: f32, y: f32| (left + x * size, top + y * size);
        for (from, to) in [
            ((0.5, 0.1), (0.06, 0.88)),
            ((0.06, 0.88), (0.94, 0.88)),
            ((0.94, 0.88), (0.5, 0.1)),
        ] {
            canvas.line(
                point(from.0, from.1),
                point(to.0, to.1),
                stroke,
                colors.tone,
            );
        }
        canvas.line(point(0.5, 0.38), point(0.5, 0.60), stroke, colors.tone);
        canvas.line(point(0.5, 0.74), point(0.5, 0.74), stroke, colors.tone);
    }
    canvas
}

/// What one progress-bar frame shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BarLook {
    /// A segment travelling along the track; `phase` is in `[0, 1)`.
    Waiting { phase: f32 },
    /// The presented fraction, with an activity highlight while held.
    Filled { fraction: f32, sweep: Option<f32> },
    /// Rollback: the frozen fraction in the caution tone.
    Halted { fraction: f32 },
}

impl Default for BarLook {
    fn default() -> Self {
        Self::Waiting { phase: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarColors {
    pub background: Rgb,
    pub track: Rgb,
    pub fill: Rgb,
    pub halted: Rgb,
    /// High contrast draws an outlined track instead of a tinted one.
    pub outline: Option<Rgb>,
}

/// Peak white blend of the held-value highlight.
const SWEEP: f32 = 0.3;

fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

/// Renders a pill-shaped track with rounded ends. The fill grows from the
/// left edge; a mirrored (RTL) device context mirrors the bitmap.
pub fn render_bar(width: usize, height: usize, look: BarLook, colors: &BarColors) -> Canvas {
    let mut canvas = Canvas::new(width, height, colors.background);
    let (w, h) = (width as f32, height as f32);
    let track = Rect::new(0.0, 0.0, w, h);
    let radius = h / 2.0;
    canvas.fill_rounded(track, radius, colors.track);
    if let Some(outline) = colors.outline {
        canvas.stroke_rounded(track, radius, 1.0, outline);
    }
    let segment = |canvas: &mut Canvas, start: f32, end: f32, color: &dyn Fn(f32) -> Rgb| {
        canvas.paint(track, |x, y| {
            let amount = coverage(rounded_distance(x + 0.5, y + 0.5, track, radius))
                * column_overlap(x, start, end);
            Some((color(x + 0.5), amount))
        });
    };
    match look {
        BarLook::Waiting { phase } => {
            let length = (w * 0.3).max(h * 2.0);
            let start = -length + (w + length) * smoothstep(phase);
            segment(&mut canvas, start, start + length, &|_| colors.fill);
        }
        BarLook::Filled { fraction, sweep } => {
            let end = w * fraction.clamp(0.0, 1.0);
            let band = (w * 0.12).max(h * 8.0);
            let center = sweep.map(|phase| -band + (end + 2.0 * band) * phase.clamp(0.0, 1.0));
            segment(&mut canvas, 0.0, end, &|x| match center {
                Some(center) => {
                    let near = 1.0 - (x - center).abs() / band;
                    Rgb::WHITE.over(colors.fill, SWEEP * smoothstep(near))
                }
                None => colors.fill,
            });
        }
        BarLook::Halted { fraction } => {
            segment(&mut canvas, 0.0, w * fraction.clamp(0.0, 1.0), &|_| {
                colors.halted
            });
        }
    }
    canvas
}

/// Eases the presented fraction toward the model's value and derives the
/// waiting and held animations. Pure: the caller supplies elapsed time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BarMotion {
    shown: f32,
    target: Option<f32>,
    held_ms: u32,
    clock_ms: u32,
}

impl BarMotion {
    pub fn step(&mut self, bar: Bar, elapsed_ms: u32, motion: bool) -> BarLook {
        let scan = MOTION_SCAN_MS as u32;
        self.clock_ms = (self.clock_ms + elapsed_ms) % scan;
        let phase = self.clock_ms as f32 / scan as f32;
        let (value, halted) = match bar {
            Bar::Waiting => {
                *self = Self {
                    clock_ms: self.clock_ms,
                    ..Self::default()
                };
                return BarLook::Waiting {
                    phase: if motion { phase } else { 0.5 },
                };
            }
            Bar::Advancing(value) => (value, false),
            Bar::Halted(value) => (value, true),
        };
        let target = f32::from(value.min(crate::state::BAR_COMPLETE)) / 1000.0;
        if self.target == Some(target) {
            self.held_ms = self.held_ms.saturating_add(elapsed_ms);
        } else {
            self.held_ms = 0;
            self.target = Some(target);
        }
        // A new attempt may start lower; only easing forward is animated.
        if !motion || target < self.shown {
            self.shown = target;
        } else {
            let tau = MOTION_BASE_MS as f32 / 2.0;
            self.shown += (target - self.shown) * (1.0 - (-(elapsed_ms as f32) / tau).exp());
            if target - self.shown < 0.0005 {
                self.shown = target;
            }
        }
        if halted {
            return BarLook::Halted {
                fraction: self.shown,
            };
        }
        let held = self.held_ms >= STALL_MS as u32 && self.shown > 0.0 && self.shown == target;
        BarLook::Filled {
            fraction: self.shown,
            sweep: (motion && held).then_some(phase),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKGROUND: Rgb = Rgb::from_colorref(0x00f1f4f5);
    const TRACK: Rgb = Rgb::from_colorref(0x00dae1e4);
    const FILL: Rgb = Rgb::from_colorref(0x000c50c2);
    const HALTED: Rgb = Rgb::from_colorref(0x00005ca1);
    const COLORS: BarColors = BarColors {
        background: BACKGROUND,
        track: TRACK,
        fill: FILL,
        halted: HALTED,
        outline: None,
    };

    #[test]
    fn colors_round_trip_between_colorref_and_dib_order() {
        let color = Rgb::from_colorref(0x000c50c2);
        assert_eq!((color.r, color.g, color.b), (0xc2, 0x50, 0x0c));
        assert_eq!(color.dib(), 0x00c2_500c);
        assert_eq!(FILL.over(BACKGROUND, 1.0), FILL);
        assert_eq!(FILL.over(BACKGROUND, 0.0), BACKGROUND);
    }

    #[test]
    fn rounded_fill_is_solid_inside_and_antialiased_at_the_corner() {
        let mut canvas = Canvas::new(40, 20, BACKGROUND);
        canvas.fill_rounded(Rect::new(0.0, 0.0, 40.0, 20.0), 10.0, FILL);
        assert_eq!(canvas.pixel(20, 10), FILL);
        assert_eq!(canvas.pixel(0, 0), BACKGROUND);
        let edge = canvas.pixel(2, 3);
        assert!(
            edge != FILL && edge != BACKGROUND,
            "corner must blend: {edge:?}"
        );
    }

    #[test]
    fn strokes_stay_inside_their_rectangle() {
        let mut canvas = Canvas::new(30, 30, BACKGROUND);
        canvas.stroke_rounded(Rect::new(5.0, 5.0, 25.0, 25.0), 4.0, 2.0, FILL);
        assert_eq!(canvas.pixel(15, 5), FILL);
        assert_eq!(canvas.pixel(15, 15), BACKGROUND);
        assert_eq!(canvas.pixel(15, 3), BACKGROUND);
        let mut line = Canvas::new(20, 20, BACKGROUND);
        line.line((2.0, 10.0), (18.0, 10.0), 2.0, FILL);
        assert_eq!(line.pixel(10, 10), FILL);
        assert_eq!(line.pixel(10, 14), BACKGROUND);
    }

    #[test]
    fn mirroring_reverses_rows_for_right_to_left_layouts() {
        let mut canvas = render_bar(
            100,
            6,
            BarLook::Filled {
                fraction: 0.25,
                sweep: None,
            },
            &COLORS,
        );
        assert_eq!(canvas.pixel(10, 3), FILL);
        canvas.mirror();
        assert_eq!(canvas.pixel(10, 3), TRACK);
        assert_eq!(canvas.pixel(89, 3), FILL);
    }

    #[test]
    fn determinate_fill_ends_at_its_fraction_without_leaking_past_it() {
        let canvas = render_bar(
            200,
            6,
            BarLook::Filled {
                fraction: 0.5,
                sweep: None,
            },
            &COLORS,
        );
        assert_eq!(canvas.pixel(50, 3), FILL);
        assert_eq!(canvas.pixel(99, 3), FILL);
        assert_eq!(canvas.pixel(100, 3), TRACK);
        assert_eq!(
            canvas.pixel(199, 0),
            BACKGROUND,
            "rounded end keeps the corner"
        );
        let empty = render_bar(
            200,
            6,
            BarLook::Filled {
                fraction: 0.0,
                sweep: None,
            },
            &COLORS,
        );
        assert!((0..200).all(|x| empty.pixel(x, 3) == TRACK || !(3..=196).contains(&x)));
    }

    #[test]
    fn waiting_segment_travels_and_rollback_uses_the_halted_tone() {
        let fill_columns = |canvas: &Canvas| {
            (0..canvas.width())
                .filter(|x| canvas.pixel(*x, 3) == FILL)
                .collect::<Vec<_>>()
        };
        let early = render_bar(200, 6, BarLook::Waiting { phase: 0.3 }, &COLORS);
        let late = render_bar(200, 6, BarLook::Waiting { phase: 0.7 }, &COLORS);
        let (early, late) = (fill_columns(&early), fill_columns(&late));
        assert!(!early.is_empty() && !late.is_empty());
        assert!(late[0] > early[0], "the segment moves forward");
        let halted = render_bar(200, 6, BarLook::Halted { fraction: 0.4 }, &COLORS);
        assert_eq!(halted.pixel(40, 3), HALTED);
        assert_eq!(halted.pixel(120, 3), TRACK);
    }

    #[test]
    fn held_sweep_brightens_part_of_the_fill_only() {
        let canvas = render_bar(
            400,
            6,
            BarLook::Filled {
                fraction: 0.5,
                sweep: Some(0.5),
            },
            &COLORS,
        );
        let lit = (0..200).filter(|x| canvas.pixel(*x, 3) != FILL).count();
        assert!(lit > 0 && lit < 200);
        assert_eq!(canvas.pixel(300, 3), TRACK);
    }

    #[test]
    fn motion_eases_forward_holds_and_reports_activity_after_a_stall() {
        let mut motion = BarMotion::default();
        assert!(matches!(
            motion.step(Bar::Waiting, 16, true),
            BarLook::Waiting { .. }
        ));
        let BarLook::Filled { fraction, sweep } = motion.step(Bar::Advancing(500), 16, true) else {
            panic!("advancing must fill");
        };
        assert!(fraction > 0.0 && fraction < 0.5 && sweep.is_none());
        let mut look = motion.step(Bar::Advancing(500), 16, true);
        let mut previous = 0.0;
        for _ in 0..200 {
            let BarLook::Filled { fraction, .. } = look else {
                panic!()
            };
            assert!(fraction >= previous);
            previous = fraction;
            look = motion.step(Bar::Advancing(500), 16, true);
        }
        let BarLook::Filled { fraction, sweep } = look else {
            panic!()
        };
        assert_eq!(fraction, 0.5);
        assert!(sweep.is_some(), "a held value shows activity");
        let BarLook::Halted { fraction } = motion.step(Bar::Halted(500), 16, true) else {
            panic!("rollback halts");
        };
        assert_eq!(fraction, 0.5);
    }

    #[test]
    fn button_roles_follow_flutter_and_keep_a_visible_focus_ring() {
        for tokens in [&crate::tokens::LIGHT, &crate::tokens::DARK] {
            let color = Rgb::from_colorref;
            let filled = button_look(ButtonKind::Filled, true, false, tokens);
            assert_eq!(filled.fill, Some(color(tokens.primary)));
            assert_eq!(filled.ink, color(tokens.on_primary));
            assert_ne!(
                filled.ring,
                filled.fill.unwrap(),
                "focus contrasts with the fill"
            );
            let pressed = button_look(ButtonKind::Filled, true, true, tokens);
            assert_eq!(pressed.fill, Some(color(tokens.primary_pressed)));
            let outlined = button_look(ButtonKind::Outlined, true, false, tokens);
            assert_eq!(outlined.fill, None);
            assert_eq!(outlined.border, Some(color(tokens.hairline_strong)));
            assert_eq!(outlined.ink, color(tokens.ink));
            let text = button_look(ButtonKind::Text, true, true, tokens);
            assert_eq!(text.border, None);
            assert_eq!(text.fill, Some(color(tokens.pressed)));
            assert_eq!(text.ink, color(tokens.primary));
            for kind in [ButtonKind::Filled, ButtonKind::Outlined, ButtonKind::Text] {
                let disabled = button_look(kind, false, true, tokens);
                assert_eq!(disabled.ink, color(tokens.disabled_ink));
                assert_ne!(disabled.fill, Some(color(tokens.primary_pressed)));
            }
        }
    }

    #[test]
    fn destructive_buttons_use_the_danger_pair() {
        for tokens in [&crate::tokens::LIGHT, &crate::tokens::DARK] {
            let color = Rgb::from_colorref;
            let look = button_look(ButtonKind::Destructive, true, false, tokens);
            assert_eq!(look.fill, Some(color(tokens.danger)));
            assert_eq!(look.ink, color(tokens.on_danger));
            let pressed = button_look(ButtonKind::Destructive, true, true, tokens);
            assert_eq!(pressed.fill, Some(color(tokens.danger_pressed)));
        }
    }

    #[test]
    fn marks_tint_a_circle_and_draw_their_glyph_in_the_tone() {
        let tokens = &crate::tokens::LIGHT;
        let colors = MarkColors {
            background: BACKGROUND,
            soft: Rgb::from_colorref(tokens.success_soft),
            tone: Rgb::from_colorref(tokens.success),
            outline: None,
        };
        for mark in [Mark::Success, Mark::Caution, Mark::Danger, Mark::Neutral] {
            let canvas = render_mark(36, mark, &colors);
            assert_eq!(
                canvas.pixel(0, 0),
                BACKGROUND,
                "{mark:?} keeps square corners clear"
            );
            assert_eq!(
                canvas.pixel(18, 3),
                colors.soft,
                "{mark:?} fills its circle"
            );
            let toned = (0..36 * 36)
                .filter(|index| canvas.pixel(index % 36, index / 36) == colors.tone)
                .count();
            assert!(toned > 10, "{mark:?} draws a visible glyph");
        }
        let outlined = render_mark(
            36,
            Mark::Danger,
            &MarkColors {
                outline: Some(Rgb::WHITE),
                ..colors
            },
        );
        assert_eq!(
            outlined.pixel(18, 3),
            BACKGROUND,
            "high contrast leaves the circle unfilled"
        );
    }

    #[test]
    fn notices_round_their_frame_and_place_the_icon_at_the_leading_edge() {
        let tokens = &crate::tokens::LIGHT;
        let colors = NoticeColors {
            background: BACKGROUND,
            fill: Rgb::from_colorref(tokens.caution_fill),
            border: Rgb::from_colorref(tokens.caution_border),
            tone: Rgb::from_colorref(tokens.caution),
        };
        for danger in [false, true] {
            let canvas = render_notice(300, 60, danger, &colors, 1.0);
            assert_eq!(canvas.pixel(0, 0), BACKGROUND);
            assert_eq!(canvas.pixel(150, 30), colors.fill);
            let icon = (14..32)
                .flat_map(|x| (13..31).map(move |y| (x, y)))
                .filter(|(x, y)| canvas.pixel(*x, *y) == colors.tone)
                .count();
            assert!(icon > 8, "the icon is drawn in the tone");
            assert!((60..290).all(|x| canvas.pixel(x, 30) == colors.fill));
        }
    }

    #[test]
    fn reduced_motion_jumps_and_never_animates() {
        let mut motion = BarMotion::default();
        assert_eq!(
            motion.step(Bar::Waiting, 100, false),
            BarLook::Waiting { phase: 0.5 }
        );
        assert_eq!(
            motion.step(Bar::Advancing(250), 16, false),
            BarLook::Filled {
                fraction: 0.25,
                sweep: None
            }
        );
        for _ in 0..200 {
            motion.step(Bar::Advancing(250), 16, false);
        }
        assert_eq!(
            motion.step(Bar::Advancing(250), 16, false),
            BarLook::Filled {
                fraction: 0.25,
                sweep: None
            }
        );
    }
}
