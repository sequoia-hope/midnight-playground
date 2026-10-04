//! The loading screen (`#loading` in `index.html`), and what every
//! `.screen` shares: the
//! full-window, scrolling column over a dark radial gradient, its gaps and
//! paddings at each breakpoint, and the logo.

use super::widgets::{self as w, Bp, Control, Icons, T, Value};
use crate::status::Status;
use bevy::prelude::*;
use bevy::text::{Justify, TextLayout};

/// What a screen builder has: the breakpoints, the icons, the focus and
/// the focus order it records, the logo picture.
pub struct Cx<'a> {
    pub bp: Bp,
    pub icons: &'a Icons,
    pub focus: Option<String>,
    pub order: Vec<String>,
    pub logo: Option<(Handle<Image>, &'a w::Logo)>,
}

impl Cx<'_> {
    /// Notes a focusable control, in order; true if it has the focus.
    pub fn f(&mut self, id: &str) -> bool {
        self.order.push(id.to_owned());
        self.focus.as_deref() == Some(id)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Loading,
    Menu,
    /// Pause, results, controller setup (to come).
    #[allow(dead_code)]
    Other,
}

/// `.logo`'s font size: `clamp(48px, 9vw, 110px)`, `clamp(40px, 7vw,
/// 76px)` under 780 px tall, 34 px under 500.
pub fn logo_px(bp: &Bp) -> f32 {
    if bp.compact {
        34.0
    } else if bp.short {
        bp.vw(7.0).clamp(40.0, 76.0)
    } else {
        bp.vw(9.0).clamp(48.0, 110.0)
    }
}

/// A `.screen`: fills the window, scrolls, and centres its column when it
/// fits (`justify-content: safe center`), or starts at the top on short
/// screens. The menu on a phone held sideways is a two-column grid.
/// Returns the scrolling node.
pub fn screen(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    kind: Kind,
    body: impl FnOnce(&mut ChildSpawnerCommands, &mut Cx),
) -> Entity {
    let bp = cx.bp;
    let gap = if bp.compact {
        8.0
    } else if bp.short {
        10.0
    } else {
        16.0
    };
    let mut pad = if bp.compact {
        (10.0, 16.0, 10.0)
    } else if bp.short {
        (18.0, 16.0, 16.0)
    } else {
        (16.0, 16.0, 16.0)
    };
    let mut top_aligned = bp.short;
    if kind == Kind::Menu && bp.touch && bp.portrait {
        // `body.touch #menu` in portrait: under the rotate hint.
        pad.0 = bp.inset_top.max(12.0) + 40.0;
        top_aligned = true;
    }
    let bg = if kind == Kind::Loading {
        w::screen_bg(40.0, w::rgb(0x1c1030), w::rgb(0x05060a), Some(70.0))
    } else {
        w::screen_bg(30.0, w::rgba(0x28143c, 0.55), w::rgba(0x03040a, 0.88), None)
    };
    let grid = kind == Kind::Menu && bp.two_columns();
    let mut inner = Node {
        width: Val::Percent(100.0),
        flex_shrink: 0.0,
        margin: UiRect::new(
            Val::Px(0.0),
            Val::Px(0.0),
            if top_aligned { Val::Px(0.0) } else { Val::Auto },
            Val::Auto,
        ),
        ..default()
    };
    if grid {
        inner.display = Display::Grid;
        inner.grid_template_columns = vec![RepeatedGridTrack::flex(2, 1.0)];
        inner.column_gap = bp.px(22.0);
        inner.row_gap = bp.px(8.0);
        inner.align_content = AlignContent::Start;
        inner.justify_items = JustifyItems::Stretch;
    } else {
        inner.flex_direction = FlexDirection::Column;
        inner.align_items = AlignItems::Center;
        inner.row_gap = bp.px(gap);
    }
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(bp.px(pad.1), bp.px(pad.1), bp.px(pad.0), bp.px(pad.2)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        bg,
    ))
    .with_children(|p| {
        p.spawn(inner).with_children(|p| body(p, cx));
    })
    .id()
}

/// The logo node.
pub fn logo(p: &mut ChildSpawnerCommands, cx: &Cx, place: impl FnOnce(&mut Node)) {
    let Some((h, l)) = &cx.logo else { return };
    let e = w::logo(p, &cx.bp, h, l);
    let bp = cx.bp;
    let mut n = Node {
        width: bp.px(l.w),
        height: bp.px(l.h),
        flex_shrink: 0.0,
        ..default()
    };
    place(&mut n);
    p.commands_mut().entity(e).insert(n);
}

/// The loading line under the bar: what the client is doing.
pub fn loading_label(status: &Status) -> String {
    match status.state {
        "waiting" => "Loading the scene…".into(),
        "building" => "Building the scene…".into(),
        "warming" => "Preparing the shaders…".into(),
        "failed" => status.error.clone().unwrap_or_else(|| "Failed".into()),
        _ => "Starting…".into(),
    }
}

/// `#loading`: the logo, the bar, the line.
pub fn loading(p: &mut ChildSpawnerCommands, cx: &mut Cx, status: &Status) -> Entity {
    let bp = cx.bp;
    let frac = match status.state {
        "building" => 0.8 + 0.2 * status.progress,
        "warming" | "running" => 1.0,
        _ => 0.3,
    };
    screen(p, cx, Kind::Loading, |p, cx| {
        logo(p, cx, |_| {});
        p.spawn((
            Node {
                width: bp.px(420f32.min(bp.vw(80.0))),
                height: bp.px(6.0),
                border_radius: w::radius(&bp, 3.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(w::white(0.1)),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Percent(100.0 * frac),
                    height: Val::Percent(100.0),
                    ..default()
                },
                w::hgrad(w::accent(), w::rgb(0xff9a3c)),
            ));
        });
        let label = loading_label(status);
        p.spawn((
            w::text(label.to_uppercase(), T::new(13.0).ls(0.2).c(w::dim()), bp.k),
            TextLayout::justify(Justify::Center),
            Control::named("load-label", Value::Text(label)),
        ));
    })
}
