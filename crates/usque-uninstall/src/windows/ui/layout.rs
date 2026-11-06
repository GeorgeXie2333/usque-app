//! Geometry: footer buttons, measured text, field frames and window placement.
use super::*;

#[derive(Debug)]
pub(super) struct FooterButton {
    pub(super) id: i32,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
}

/// Keep actions inside the work area and reserve their measured text height.
/// The primary button stays in the last position when restart confirmation
/// swaps button meanings, preserving the distinct-click safety boundary.
pub(super) fn footer_positions(
    (width, height): (i32, i32),
    margin: i32,
    gap: i32,
    padding: i32,
    stacked: bool,
    captions: [(i32, bool, i32); 3],
) -> (i32, Vec<FooterButton>) {
    let action_height = captions[1].2.max(captions[2].2);
    let row_height = captions[0].2.max(action_height);
    let visible_count = captions.iter().filter(|(_, visible, _)| *visible).count() as i32;
    let footer_height = if stacked {
        captions
            .iter()
            .filter(|(_, visible, _)| *visible)
            .map(|(id, _, h)| if *id == DETAILS { *h } else { action_height })
            .sum::<i32>()
            + gap * (visible_count - 1).max(0)
            + padding * 2
    } else {
        row_height + padding * 2
    };
    let button_width = if stacked {
        width - margin * 2
    } else {
        (width - margin * 2 - gap * 2) / 3
    }
    .max(1);
    let mut y = height - footer_height + padding;
    let buttons = captions
        .into_iter()
        .enumerate()
        .filter_map(|(index, (id, visible, h))| {
            if !visible {
                return None;
            }
            let h = if id == DETAILS { h } else { action_height };
            let button = FooterButton {
                id,
                x: if stacked {
                    margin
                } else {
                    margin + index as i32 * (button_width + gap)
                },
                y: if stacked {
                    y
                } else {
                    height - padding - row_height + (row_height - h) / 2
                },
                width: button_width,
                height: h,
            };
            if stacked {
                y += h + gap;
            }
            Some(button)
        })
        .collect();
    (footer_height, buttons)
}

pub(super) fn layout(hwnd: HWND, state: &mut State) {
    let mut rect = RECT::default();
    // SAFETY: hwnd is a live window and rect is writable.
    unsafe {
        GetClientRect(hwnd, &mut rect);
    }
    let scale = |value: i32| ((i64::from(value) * i64::from(state.dpi)) / 96) as i32;
    let width = rect.right.max(1);
    let height = rect.bottom.max(1);
    let margin = scale(32).min(width / 8);
    let body_width = (width - 2 * margin).max(40);
    let stacked = body_width < scale(520);
    let button_width = if stacked {
        body_width
    } else {
        (body_width - scale(20)) / 3
    };
    let captions = [DETAILS, SECONDARY, PRIMARY].map(|id| {
        let child = control(hwnd, id);
        // SAFETY: these are the live native footer controls.
        let visible = unsafe { GetWindowLongW(child, GWL_STYLE) } as u32 & WS_VISIBLE != 0;
        let minimum = scale(if id == DETAILS { 36 } else { 48 });
        (
            id,
            visible,
            (measure(
                hwnd,
                child,
                state.fonts.body,
                (button_width - scale(20)).max(1),
            ) + scale(12))
            .max(minimum),
        )
    });
    let (footer, footer_buttons) = footer_positions(
        (width, height),
        margin,
        scale(10),
        scale(20),
        stacked,
        captions,
    );
    let viewport = (height - footer).max(1);
    let mut y = scale(24);
    let mut items = Vec::new();
    let mut frame = RECT::default();
    for id in [
        BADGE,
        GLYPH,
        TITLE,
        BODY,
        CHECK,
        DESCRIPTION,
        WARNING,
        STATUS,
        PROGRESS,
        DETAIL,
        SAVE,
    ] {
        // SAFETY: a child handle exists for each ID.
        let child = control(hwnd, id);
        // SAFETY: visibility query does not mutate the native control.
        if unsafe { GetWindowLongW(child, GWL_STYLE) } as u32 & WS_VISIBLE == 0 {
            continue;
        }
        let notice_text = (body_width
            - scale((NOTICE_PADDING + NOTICE_ICON + NOTICE_GAP + NOTICE_TRAILING) as i32))
        .max(1);
        let h = match id {
            GLYPH => scale(36),
            PROGRESS => scale(PROGRESS_HEIGHT),
            DETAIL => scale(112),
            SAVE => scale(36),
            CHECK => measure(
                hwnd,
                child,
                state.fonts.body,
                (body_width - scale(34)).max(30),
            )
            .max(scale(44)),
            WARNING if state.critical_warning() => {
                measure(hwnd, child, state.fonts.body, notice_text) + scale(2 * NOTICE_TOP as i32)
            }
            _ => {
                let font = if id == TITLE {
                    state.fonts.title
                } else if id == DESCRIPTION {
                    state.fonts.secondary
                } else {
                    state.fonts.body
                };
                running_variants(state, id)
                    .iter()
                    .map(|key| measure_text(hwnd, child, state.text(key), font, body_width))
                    .fold(measure(hwnd, child, font, body_width), i32::max)
                    .max(scale(18))
            }
        };
        let child_width = if id == SAVE {
            (text_width(hwnd, child, state.fonts.body) + scale(24)).min(body_width)
        } else if id == GLYPH {
            scale(36)
        } else {
            body_width
        };
        if id == WARNING && state.critical_warning() {
            // Flutter's WarningBanner: the owner-drawn accent draws the rounded
            // notice and its icon; the native text sits after the icon. Both
            // use parent coordinates, so Windows mirrors them together for RTL.
            let leading = scale((NOTICE_PADDING + NOTICE_ICON + NOTICE_GAP) as i32);
            items.push((
                child,
                margin + leading,
                y + scale(NOTICE_TOP as i32),
                (h - scale(2 * NOTICE_TOP as i32)).max(1),
                notice_text,
            ));
            items.push((control(hwnd, WARNING_ACCENT), margin, y, h, body_width));
        } else if id == DETAIL {
            // Rounded field: the viewport paints the frame around the edit.
            let inset = scale(6);
            frame = RECT {
                left: margin,
                top: y - state.scroll,
                right: margin + body_width,
                bottom: y - state.scroll + h,
            };
            items.push((
                child,
                margin + inset,
                y + inset,
                (h - 2 * inset).max(1),
                (body_width - 2 * inset).max(1),
            ));
        } else {
            items.push((child, margin, y, h, child_width));
        }
        y += h + scale(match id {
            TITLE | BODY => 20,
            CHECK => 8,
            _ => 12,
        });
    }
    state.max_scroll = (y - viewport).max(0);
    state.scroll = state.scroll.clamp(0, state.max_scroll);
    let same =
        |a: RECT, b: RECT| (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom);
    if !same(frame, state.detail_frame) {
        state.detail_frame = frame;
        refresh_paint_style(state);
        // SAFETY: the viewport is live; only its background and frame repaint.
        unsafe {
            InvalidateRect(GetDlgItem(hwnd, VIEWPORT), ptr::null(), 0);
        }
    }
    // SAFETY: native scrollbar state and child dimensions are bounded integers.
    unsafe {
        let info = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: y.max(1) - 1,
            nPage: viewport as u32,
            nPos: state.scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(hwnd, SB_VERT, &info, 1);
        MoveWindow(GetDlgItem(hwnd, VIEWPORT), 0, 0, width, viewport, 1);
        for (child, left, top, h, child_width) in items {
            MoveWindow(child, left, top - state.scroll, child_width, h, 1);
        }
        for button in footer_buttons {
            MoveWindow(
                control(hwnd, button.id),
                button.x,
                button.y,
                button.width,
                button.height,
                1,
            );
        }
    }
}

pub(super) fn measure(hwnd: HWND, child: HWND, font: HFONT, width: i32) -> i32 {
    measure_text(hwnd, child, &window_text(child), font, width)
}

/// Text that can replace a running-page label without a page change. Its
/// tallest variant is reserved so that MSI cancellation and stage messages
/// cannot move the progress bar.
pub(super) fn running_variants(state: &State, id: i32) -> &'static [&'static str] {
    if state.page != Page::Running || state.prompt.is_some() {
        return &[];
    }
    match id {
        BODY => &[
            "uninstall_progress_note",
            "wait_no_cancel",
            "uninstall_registration",
        ],
        STATUS => &[
            "uninstall_preparing",
            "uninstall_closing_apps",
            "uninstall_restoring_network",
            "uninstall_deleting_data",
            "uninstall_removing_files",
            "uninstall_registration",
            "uninstall_rolling_back",
            "uninstall_stopping",
        ],
        _ => &[],
    }
}

pub(super) fn measure_text(hwnd: HWND, child: HWND, text: &str, font: HFONT, width: i32) -> i32 {
    let text = wide(text);
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: 0,
    };
    // SAFETY: select only a live font, then restore the borrowed DC before release.
    unsafe {
        let dc = GetDC(hwnd);
        let old = SelectObject(dc, font);
        let reading = if GetWindowLongW(child, GWL_EXSTYLE) as u32 & WS_EX_RTLREADING != 0 {
            DT_RTLREADING
        } else {
            0
        };
        DrawTextW(
            dc,
            text.as_ptr(),
            -1,
            &mut rect,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | reading,
        );
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
    }
    rect.bottom.max(1)
}

pub(super) fn text_width(hwnd: HWND, child: HWND, font: HFONT) -> i32 {
    let text = wide(&window_text(child));
    let mut rect = RECT::default();
    // SAFETY: measure a single line with the live font, restoring the borrowed
    // DC before release. The caller bounds the result to the available width.
    unsafe {
        let dc = GetDC(hwnd);
        let old = SelectObject(dc, font);
        DrawTextW(
            dc,
            text.as_ptr(),
            -1,
            &mut rect,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
    }
    rect.right.max(1)
}

/// Opens where the setup window was after a hand-off, clamped into the work
/// area of the monitor nearest that point; otherwise centers the window.
pub(super) fn place(hwnd: HWND, dpi: u32, origin: Option<(i32, i32)>) {
    let Some((x, y)) = origin else {
        return center(hwnd, dpi);
    };
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: nearest-monitor query with a writable output structure.
    let work = unsafe {
        if GetMonitorInfoW(
            MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        ) == 0
        {
            return center(hwnd, dpi);
        }
        monitor.rcWork
    };
    let (width, height) = window_size(hwnd, dpi);
    let width = width.min((work.right - work.left).max(1));
    let height = height.min((work.bottom - work.top).max(1));
    // SAFETY: the geometry lies inside that monitor's work area. A different
    // monitor DPI arrives as WM_DPICHANGED, which resizes the window.
    unsafe {
        SetWindowPos(
            hwnd,
            ptr::null_mut(),
            x.clamp(work.left, work.right - width),
            y.clamp(work.top, work.bottom - height),
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub(super) fn center(hwnd: HWND, dpi: u32) {
    let work = work_area(hwnd);
    let (width, height) = window_size(hwnd, dpi);
    // SAFETY: window geometry is limited to the current monitor's work area.
    unsafe {
        let width = width.min((work.right - work.left).max(1));
        let height = height.min((work.bottom - work.top).max(1));
        SetWindowPos(
            hwnd,
            ptr::null_mut(),
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub(super) fn window_size(hwnd: HWND, dpi: u32) -> (i32, i32) {
    let mut rect = RECT {
        right: 680 * dpi as i32 / 96,
        bottom: 480 * dpi as i32 / 96,
        ..Default::default()
    };
    // SAFETY: calculate the native frame around a 680 by 480 logical client
    // area. Callers then clamp it to the monitor work area; content can scroll.
    unsafe {
        AdjustWindowRectExForDpi(
            &mut rect,
            GetWindowLongW(hwnd, GWL_STYLE) as u32,
            0,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
            dpi,
        );
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

pub(super) fn work_area(hwnd: HWND) -> RECT {
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: nearest-monitor query and output structure are valid.
    unsafe {
        if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        ) != 0
        {
            return monitor.rcWork;
        }
    }
    RECT {
        left: 0,
        top: 0,
        right: 680,
        bottom: 480,
    }
}
