//! Owner-drawn and subclassed painting. Shapes come from `crate::paint`;
//! native controls keep their input, values and accessibility.
use super::*;

pub(super) unsafe extern "system" fn viewport_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // SAFETY: the viewport is a child of the wizard for its full lifetime.
    let owner = unsafe { GetParent(hwnd) };
    if message == WM_PAINT
        && let Some(style) = paint_style(owner)
    {
        // SAFETY: matched Begin/EndPaint for this viewport's paint request.
        // WS_CLIPCHILDREN keeps every child's own pixels intact.
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            FillRect(dc, &paint.rcPaint, style.brush);
            paint_detail_frame(dc, hwnd, &style);
            EndPaint(hwnd, &paint);
        }
        return 0;
    }
    if matches!(
        message,
        WM_COMMAND
            | WM_DRAWITEM
            | WM_CTLCOLORSTATIC
            | WM_CTLCOLOREDIT
            | WM_CTLCOLORBTN
            | WM_ERASEBKGND
            | WM_MOUSEWHEEL
    ) {
        // SAFETY: the viewport is a child of the wizard for its full lifetime.
        unsafe { SendMessageW(GetParent(hwnd), message, wparam, lparam) }
    } else {
        // SAFETY: pass unhandled native-container messages to the subclass chain.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}

/// Sends progress-control messages only when the presented state changes.
/// Rewriting `PBS_MARQUEE` and resending `PBM_SETMARQUEE`/range on every
/// 100 ms timer tick restarted the native animation, and raw MSI fractions
/// switched the bar between marquee and fill; together they made it jump.
pub(super) fn apply_bar(hwnd: HWND, state: &mut State, view: BarView) {
    let previous = state.applied_bar.replace(view);
    if previous == Some(view) {
        return;
    }
    let marquee = view == BarView::Visible(Bar::Waiting);
    let mode_changed =
        previous.is_none_or(|previous| (previous == BarView::Visible(Bar::Waiting)) != marquee);
    let bar = control(hwnd, PROGRESS);
    // SAFETY: PROGRESS is a live child of this UI-thread window. Every value
    // is a bounded permille from the pure presentation model.
    unsafe {
        if mode_changed {
            let style = GetWindowLongW(bar, GWL_STYLE) as u32;
            let style = if marquee {
                style | PBS_MARQUEE
            } else {
                style & !PBS_MARQUEE
            };
            SetWindowLongW(bar, GWL_STYLE, style as i32);
            SendMessageW(bar, PBM_SETMARQUEE, usize::from(marquee), 35);
            if !marquee {
                SendMessageW(bar, PBM_SETRANGE32, 0, BAR_COMPLETE as isize);
            }
        }
        if let BarView::Visible(Bar::Advancing(value) | Bar::Halted(value)) = view {
            SendMessageW(bar, PBM_SETPOS, usize::from(value), 0);
        }
    }
}

pub(super) fn apply_progress_theme(hwnd: HWND, palette: &Palette) {
    let bar = control(hwnd, PROGRESS);
    let empty = [0u16];
    let native_theme = wide(if palette.dark {
        "DarkMode_Explorer"
    } else {
        "Explorer"
    });
    // SetWindowTheme sends WM_THEMECHANGED synchronously. This guard is
    // independent of State and is removed even if this scope unwinds. If the
    // property cannot be installed, leave the main window's system theme alone.
    let theme_request = NativeThemeRequest::begin(hwnd);
    // SAFETY: selecting the main window and edit/scrollbar themes affects only
    // appearance. High contrast retains its system theme and all native input
    // semantics stay unchanged. Rejected theme requests restore OS defaults.
    unsafe {
        for window in [hwnd, control(hwnd, DETAIL)] {
            if window == hwnd && theme_request.is_none() {
                continue;
            }
            if SetWindowTheme(
                window,
                if palette.high_contrast {
                    ptr::null()
                } else {
                    native_theme.as_ptr()
                },
                ptr::null(),
            ) < 0
            {
                SetWindowTheme(window, ptr::null(), ptr::null());
            }
        }
    }
    // SAFETY: only the native progress control's colors/theme are changed.
    // Visual styles otherwise ignore PBM_SETBARCOLOR/PBM_SETBKCOLOR. The
    // native control continues to own its range, position and marquee timer.
    unsafe {
        if palette.native_high_contrast {
            SetWindowTheme(bar, ptr::null(), ptr::null());
            SendMessageW(bar, PBM_SETBARCOLOR, 0, CLR_DEFAULT as isize);
            SendMessageW(bar, PBM_SETBKCOLOR, 0, CLR_DEFAULT as isize);
        } else {
            SetWindowTheme(bar, empty.as_ptr(), empty.as_ptr());
            let fill = if palette.high_contrast {
                GetSysColor(COLOR_HIGHLIGHT)
            } else {
                brand_accent(palette.dark)
            };
            let track = if palette.high_contrast {
                palette.background
            } else {
                tokens::theme(palette.dark).hairline
            };
            SendMessageW(bar, PBM_SETBARCOLOR, 0, fill as isize);
            SendMessageW(bar, PBM_SETBKCOLOR, 0, track as isize);
        }
        // Switching between native and custom rendering changes the frame.
        SetWindowPos(
            bar,
            ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        InvalidateRect(bar, ptr::null(), 1);
    }
}

pub(super) unsafe extern "system" fn progress_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: create_controls supplied this Cell, owned until window teardown.
    let style = unsafe { (*(data as *const Cell<PaintStyle>)).get() };
    // System high contrast keeps the native control and its system colors.
    // Every other message reaches the native control, which keeps its range,
    // position, marquee state and accessibility; only pixels are replaced.
    if style.native_high_contrast
        || !matches!(
            message,
            WM_PAINT | WM_PRINTCLIENT | WM_ERASEBKGND | WM_NCCALCSIZE | WM_NCPAINT
        )
    {
        // SAFETY: forwards to the original progress implementation.
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    }
    // The classic control adds a static edge outside its client area. The
    // custom bar draws its own rounded track, so it takes the whole window.
    if matches!(message, WM_NCCALCSIZE | WM_NCPAINT) {
        return 0;
    }
    if message == WM_ERASEBKGND {
        return 1;
    }
    // SAFETY: WM_PAINT uses a matched Begin/EndPaint; WM_PRINTCLIENT supplies
    // a caller-owned DC for this synchronous call.
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = if message == WM_PAINT {
            BeginPaint(hwnd, &mut paint)
        } else {
            wparam as HDC
        };
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let canvas = render_bar(
            rect.right.max(0) as usize,
            rect.bottom.max(0) as usize,
            style.bar,
            &bar_colors(&style),
        );
        blit(dc, 0, 0, &canvas, true);
        if message == WM_PAINT {
            EndPaint(hwnd, &paint);
        }
    }
    0
}

pub(super) fn client_area_animation() -> bool {
    let mut enabled = 1i32;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to this storage.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            ptr::from_mut(&mut enabled).cast(),
            0,
        )
    };
    read == 0 || enabled != 0
}

/// Advances the custom-drawn bar, repainting only that control. The 16 ms
/// timer runs only while an animated bar is visible.
pub(super) fn update_bar_look(hwnd: HWND, state: &mut State) {
    let now = Instant::now();
    let elapsed = state
        .frame_at
        .map_or(0, |at| now.duration_since(at).as_millis().min(100) as u32);
    state.frame_at = Some(now);
    let visible = match state.applied_bar {
        Some(BarView::Visible(bar)) => Some(bar),
        _ => None,
    };
    let animate = visible.is_some() && state.motion && !state.palette.native_high_contrast;
    if animate != state.animating {
        // SAFETY: the animation timer belongs to this live UI-thread window.
        unsafe {
            if animate {
                SetTimer(hwnd, ANIMATION, 16, None);
            } else {
                KillTimer(hwnd, ANIMATION);
            }
        }
        state.animating = animate;
    }
    let Some(bar) = visible else {
        return;
    };
    let look = state.bar_motion.step(bar, elapsed, state.motion);
    if look != state.bar_look {
        state.bar_look = look;
        refresh_paint_style(state);
        // SAFETY: PROGRESS is a live child; only its pixels are invalidated.
        unsafe {
            InvalidateRect(control(hwnd, PROGRESS), ptr::null(), 0);
        }
    }
}

/// Copies a rendered canvas to `dc` at (`left`, `top`). A mirrored (RTL)
/// device context maps the destination but not DIB contents, so the canvas
/// is reversed to match: the progress fill then grows from the right.
pub(super) fn blit(dc: HDC, left: i32, top: i32, canvas: &Canvas, mirror: bool) {
    let (width, height) = (canvas.width() as i32, canvas.height() as i32);
    if width == 0 || height == 0 {
        return;
    }
    // SAFETY: GetLayout only reads the live paint DC's layout flags.
    let layout = unsafe { GetLayout(dc) };
    let mirrored;
    // Glyphs pass `mirror = false` to keep their orientation.
    let canvas = if mirror && layout & LAYOUT_RTL != 0 {
        let mut copy = canvas.clone();
        copy.mirror();
        mirrored = copy;
        &mirrored
    } else {
        canvas
    };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: the top-down 32-bit header describes exactly the canvas buffer,
    // which outlives this synchronous copy.
    unsafe {
        SetDIBitsToDevice(
            dc,
            left,
            top,
            width as u32,
            height as u32,
            0,
            0,
            0,
            height as u32,
            canvas.pixels().as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
        );
    }
}

pub(super) fn bar_colors(style: &PaintStyle) -> BarColors {
    let background = Rgb::from_colorref(style.background);
    if style.high_contrast {
        // Only simulated high contrast reaches custom paint; system high
        // contrast keeps the native control.
        // SAFETY: documented system-color index.
        let highlight = unsafe { GetSysColor(COLOR_HIGHLIGHT) };
        let text = Rgb::from_colorref(style.foreground);
        return BarColors {
            background,
            track: background,
            fill: Rgb::from_colorref(highlight),
            halted: text,
            outline: Some(text),
        };
    }
    let tokens = tokens::theme(style.dark);
    BarColors {
        background,
        track: Rgb::from_colorref(tokens.hairline),
        fill: Rgb::from_colorref(tokens.primary),
        halted: Rgb::from_colorref(tokens.caution),
        outline: None,
    }
}

/// High-contrast buttons keep system colors with the same shapes.
pub(super) fn system_button_look(kind: ButtonKind, enabled: bool) -> ButtonLook {
    // SAFETY: documented system-color indices.
    let system = |index| Rgb::from_colorref(unsafe { GetSysColor(index) });
    let ink = if !enabled {
        system(COLOR_GRAYTEXT)
    } else if kind == ButtonKind::Filled {
        system(COLOR_HIGHLIGHTTEXT)
    } else {
        system(COLOR_BTNTEXT)
    };
    ButtonLook {
        fill: Some(system(if kind == ButtonKind::Filled && enabled {
            COLOR_HIGHLIGHT
        } else {
            COLOR_BTNFACE
        })),
        border: (kind != ButtonKind::Text).then(|| system(COLOR_WINDOWTEXT)),
        ink,
        ring: system(if kind == ButtonKind::Filled {
            COLOR_HIGHLIGHTTEXT
        } else {
            COLOR_HIGHLIGHT
        }),
    }
}

pub(super) fn draw_button(item: &DRAWITEMSTRUCT, style: PaintStyle) {
    let enabled = item.itemState & ODS_DISABLED == 0;
    let pressed = item.itemState & ODS_SELECTED != 0;
    let focused = item.itemState & ODS_FOCUS != 0 && item.itemState & ODS_NOFOCUSRECT == 0;
    let kind = if item.CtlID == style.accent_button && style.destructive {
        ButtonKind::Destructive
    } else if item.CtlID == style.accent_button {
        ButtonKind::Filled
    } else if matches!(item.CtlID as i32, DETAILS | SAVE) {
        ButtonKind::Text
    } else {
        ButtonKind::Outlined
    };
    let look = if style.high_contrast {
        system_button_look(kind, enabled)
    } else {
        button_look(kind, enabled, pressed, tokens::theme(style.dark))
    };
    let scale = |value: i32| value * style.dpi as i32 / 96;
    let scalef = |value: f32| value * style.dpi as f32 / 96.0;
    let reading = if style.rtl { DT_RTLREADING } else { 0 };
    let rect = item.rcItem;
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    let mut canvas = Canvas::new(
        width.max(0) as usize,
        height.max(0) as usize,
        Rgb::from_colorref(style.background),
    );
    let bounds = Rect::new(0.0, 0.0, width as f32, height as f32);
    let radius = scalef(if kind == ButtonKind::Text {
        RADIUS_CHIP
    } else {
        RADIUS_CONTROL
    } as f32);
    if let Some(fill) = look.fill {
        canvas.fill_rounded(bounds, radius, fill);
    }
    if let Some(border) = look.border {
        canvas.stroke_rounded(bounds, radius, scalef(1.0).max(1.0), border);
    }
    if focused {
        // A filled button's ring sits inside its edge in the on-accent color;
        // other buttons replace their outline with the accent ring.
        let ring = scalef(FOCUS_RING as f32).max(1.0);
        let inset = if kind == ButtonKind::Filled {
            scalef(3.0)
        } else {
            0.0
        };
        canvas.stroke_rounded(
            bounds.inset(inset),
            (radius - inset).max(0.0),
            ring,
            look.ring,
        );
    }
    blit(item.hDC, rect.left, rect.top, &canvas, true);
    let ink = u32::from(look.ink.r) | u32::from(look.ink.g) << 8 | u32::from(look.ink.b) << 16;
    // SAFETY: owner-draw supplies this DC and rectangle for the callback. The
    // selected font is restored before returning.
    unsafe {
        let old_font = SelectObject(item.hDC, style.font);
        SetBkMode(item.hDC, TRANSPARENT as i32);
        SetTextColor(item.hDC, ink);
        let mut bounds = item.rcItem;
        InflateRect(&mut bounds, -scale(10), -scale(4));
        let caption = wide(&window_text(item.hwndItem));
        let mut measured = RECT {
            right: bounds.right - bounds.left,
            ..Default::default()
        };
        DrawTextW(
            item.hDC,
            caption.as_ptr(),
            -1,
            &mut measured,
            DT_CALCRECT | DT_WORDBREAK | DT_CENTER | DT_NOPREFIX | reading,
        );
        bounds.top += ((bounds.bottom - bounds.top - measured.bottom) / 2).max(0);
        DrawTextW(
            item.hDC,
            caption.as_ptr(),
            -1,
            &mut bounds,
            DT_WORDBREAK | DT_CENTER | DT_NOPREFIX | reading,
        );
        SelectObject(item.hDC, old_font);
    }
}

/// Text and background for native children: notices and fields have their
/// own surfaces; everything else sits on the window canvas.
pub(super) fn child_colors(dc: HDC, child: HWND, style: &PaintStyle) -> LRESULT {
    // SAFETY: GetDlgCtrlID only reads the live child's identifier.
    let id = unsafe { GetDlgCtrlID(child) };
    let (text, background, brush) = match id {
        DESCRIPTION => (style.muted, style.background, style.brush),
        WARNING if style.critical => (style.foreground, style.notice_fill, style.notice_brush),
        DETAIL => (style.foreground, style.field, style.field_brush),
        _ => (style.foreground, style.background, style.brush),
    };
    // SAFETY: Windows supplies this live DC for the child's painting.
    unsafe {
        SetTextColor(dc, text);
        SetBkColor(dc, background);
        SetBkMode(dc, TRANSPARENT as i32);
    }
    brush as LRESULT
}

/// The critical notice: a rounded caution or danger banner with its icon,
/// behind WARNING's native text.
pub(super) fn draw_warning_accent(item: &DRAWITEMSTRUCT, style: PaintStyle) {
    let tones = tokens::theme(style.dark);
    let color = Rgb::from_colorref;
    let colors = if style.high_contrast {
        NoticeColors {
            background: color(style.background),
            fill: color(style.background),
            border: color(style.foreground),
            tone: color(style.foreground),
        }
    } else if style.warning_danger {
        NoticeColors {
            background: color(style.background),
            fill: color(tones.danger_fill),
            border: color(tones.danger_border),
            tone: color(tones.danger),
        }
    } else {
        NoticeColors {
            background: color(style.background),
            fill: color(tones.caution_fill),
            border: color(tones.caution_border),
            tone: color(tones.caution),
        }
    };
    let rect = item.rcItem;
    let canvas = render_notice(
        (rect.right - rect.left).max(0) as usize,
        (rect.bottom - rect.top).max(0) as usize,
        style.warning_danger,
        &colors,
        style.dpi as f32 / 96.0,
    );
    blit(item.hDC, rect.left, rect.top, &canvas, true);
}

/// The result mark above a finished page's title. Decorative: the title
/// states the result. Glyphs keep their orientation in RTL layouts.
pub(super) fn draw_mark(item: &DRAWITEMSTRUCT, style: PaintStyle) {
    let Some(mark) = style.mark else {
        return;
    };
    let tones = tokens::theme(style.dark);
    let color = Rgb::from_colorref;
    let (soft, tone) = match mark {
        Mark::Success => (tones.success_soft, tones.success),
        Mark::Caution => (tones.caution_soft, tones.caution),
        Mark::Danger => (tones.danger_soft, tones.danger),
        Mark::Neutral => (tones.pressed, tones.muted),
    };
    let colors = if style.high_contrast {
        MarkColors {
            background: color(style.background),
            soft: color(style.background),
            tone: color(style.foreground),
            outline: Some(color(style.foreground)),
        }
    } else {
        MarkColors {
            background: color(style.background),
            soft: color(soft),
            tone: color(tone),
            outline: None,
        }
    };
    let rect = item.rcItem;
    let canvas = render_mark((rect.right - rect.left).max(0) as usize, mark, &colors);
    blit(item.hDC, rect.left, rect.top, &canvas, false);
}

/// Repaints DETAIL's rounded frame when focus changes its border.
pub(super) unsafe extern "system" fn field_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: forwards to the native edit, which keeps input and accessibility.
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if matches!(message, WM_SETFOCUS | WM_KILLFOCUS) {
        // SAFETY: create_controls supplied this Cell, owned until teardown;
        // the parent viewport is live while its child receives focus messages.
        unsafe {
            let style = (*(data as *const Cell<PaintStyle>)).get();
            InvalidateRect(GetParent(hwnd), &style.detail_frame, 0);
        }
    }
    result
}

/// Paints the rounded field frame around a visible DETAIL edit.
pub(super) fn paint_detail_frame(dc: HDC, viewport: HWND, style: &PaintStyle) {
    // SAFETY: DETAIL is a child of this live viewport; these calls only read.
    let (visible, focused) = unsafe {
        let detail = GetDlgItem(viewport, DETAIL);
        (IsWindowVisible(detail) != 0, GetFocus() == detail)
    };
    let frame = style.detail_frame;
    if !visible || frame.right <= frame.left || frame.bottom <= frame.top {
        return;
    }
    let tones = tokens::theme(style.dark);
    let color = Rgb::from_colorref;
    let scale = style.dpi as f32 / 96.0;
    let mut canvas = Canvas::new(
        (frame.right - frame.left) as usize,
        (frame.bottom - frame.top) as usize,
        color(style.background),
    );
    let bounds = Rect::new(0.0, 0.0, canvas.width() as f32, canvas.height() as f32);
    let radius = RADIUS_CONTROL as f32 * scale;
    canvas.fill_rounded(bounds, radius, color(style.field));
    let border = if style.high_contrast {
        style.foreground
    } else if focused {
        tones.primary
    } else {
        tones.hairline
    };
    let width = if focused { STROKE_STRONG } else { 1.0 };
    canvas.stroke_rounded(bounds, radius, (width * scale).max(1.0), color(border));
    blit(dc, frame.left, frame.top, &canvas, true);
}

pub(super) unsafe extern "system" fn checkbox_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: create_controls passes a stable boxed Cell, retained until after
    // this checkbox is destroyed. Copying it does not borrow the UI state.
    let style = unsafe { (*(data as *const Cell<PaintStyle>)).get() };
    if message == WM_PAINT && !style.native_high_contrast {
        let scale = |value: i32| value * style.dpi as i32 / 96;
        let reading = if style.rtl { DT_RTLREADING } else { 0 };
        let mut paint = PAINTSTRUCT::default();
        // SAFETY: this is the checkbox's WM_PAINT, with a matched Begin/EndPaint.
        // Only pixels are replaced; BS_AUTOCHECKBOX input and UIA stay native.
        unsafe {
            let dc = BeginPaint(hwnd, &mut paint);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            FillRect(dc, &rect, style.brush);
            let checked = SendMessageW(hwnd, BM_GETCHECK, 0, 0) as u32 == BST_CHECKED;
            let box_size = scale(20);
            let left = scale(2);
            let top = (rect.bottom - box_size) / 2;
            let ink = style.foreground;
            let tones = tokens::theme(style.dark);
            let checked_fill = Rgb::from_colorref(if style.high_contrast {
                GetSysColor(COLOR_HIGHLIGHT)
            } else {
                tones.primary
            });
            let mark = Rgb::from_colorref(if style.high_contrast {
                GetSysColor(COLOR_HIGHLIGHTTEXT)
            } else {
                tones.on_primary
            });
            let outline = Rgb::from_colorref(if style.high_contrast {
                ink
            } else {
                tones.muted
            });
            let scalef = |value: f32| value * style.dpi as f32 / 96.0;
            let size = box_size.max(1) as usize;
            let mut canvas = Canvas::new(size, size, Rgb::from_colorref(style.background));
            let bounds = Rect::new(0.0, 0.0, size as f32, size as f32);
            let radius = scalef(RADIUS_CHECKBOX as f32);
            if checked {
                canvas.fill_rounded(bounds, radius, checked_fill);
                let (edge, stroke) = (size as f32, scalef(2.0).max(1.0));
                canvas.line(
                    (edge * 0.24, edge * 0.52),
                    (edge * 0.43, edge * 0.70),
                    stroke,
                    mark,
                );
                canvas.line(
                    (edge * 0.43, edge * 0.70),
                    (edge * 0.77, edge * 0.31),
                    stroke,
                    mark,
                );
            } else {
                canvas.stroke_rounded(bounds, radius, scalef(STROKE_STRONG).max(1.0), outline);
            }
            blit(dc, left, top, &canvas, false);
            let old_font = SelectObject(dc, style.font);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, ink);
            let mut text = rect;
            text.left = left + box_size + scale(12);
            let caption = wide(&window_text(hwnd));
            let mut measured = RECT {
                right: text.right - text.left,
                ..Default::default()
            };
            DrawTextW(
                dc,
                caption.as_ptr(),
                -1,
                &mut measured,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | reading,
            );
            text.top += ((text.bottom - text.top - measured.bottom) / 2).max(0);
            DrawTextW(
                dc,
                caption.as_ptr(),
                -1,
                &mut text,
                DT_WORDBREAK | DT_NOPREFIX | reading,
            );
            if GetFocus() == hwnd
                && SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS as isize == 0
            {
                let focus = RECT {
                    left: scale(1),
                    top: (top.min(text.top) - scale(4)).max(scale(1)),
                    right: (text.left + measured.right + scale(6)).min(rect.right - scale(1)),
                    bottom: ((top + box_size).max(text.top + measured.bottom) + scale(4))
                        .min(rect.bottom - scale(1)),
                };
                DrawFocusRect(dc, &focus);
            }
            SelectObject(dc, old_font);
            EndPaint(hwnd, &paint);
        }
        return 0;
    }
    // SAFETY: all non-paint messages retain the native checkbox implementation.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
