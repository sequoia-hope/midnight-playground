//! The debug overlay (the owner's request of 2026-10-05; DECISIONS D1020):
//! a dark grey, nearly opaque panel over everything with the frame rate,
//! the frame times' minimum, mean, 99th percentile and maximum over the
//! last 5 s, the last 30 s and all the time since the overlay was turned on,
//! a small graph for each of those spans, and what the client is doing.
//!
//! `debug=1` (`--debug` natively, `?debug=1` on the web) shows it from the
//! start; F3 shows and hides it at any time (no binding of the game's has
//! F3, so the key reaches nothing else). Turning it on starts its all-time
//! span afresh.
//!
//! Two times per frame, both in ms:
//! - **frame**: the interval since the last frame (`Time<Real>`'s delta),
//!   what the player sees as smoothness;
//! - **main**: the main world's CPU time for its frame, from the start of
//!   `First` to the end of `Last` (input, the race, the UI, the HUD); the
//!   render world runs beside it natively (pipelined) and after it on the
//!   web, and its own time is shown as **render** (its `Render` schedule,
//!   presentation included).
//!
//! [`FrameTimes`] holds this frame's three, for the run recording too
//! (`crate::recording`). The overlay keeps the last 30 s of samples in a
//! ring (always, so the windows are full when it is turned on) and the
//! all-time span in a histogram and in at most [`BUCKETS`] buckets that
//! merge in pairs as time goes on, so its memory is bounded. The text and
//! the graphs are redrawn four times a second, the graphs into three small
//! images updated in place.

use crate::Opts;
use crate::status::Status;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::entity::Entities;
use bevy::image::ImageSampler;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::text::{FontSource, LineHeight};
use std::collections::VecDeque;
use std::fmt::Write;
use std::sync::atomic::{AtomicU32, Ordering};

/// The key that shows and hides the overlay.
pub const TOGGLE: KeyCode = KeyCode::F3;

/// The graphs' size, image pixels (drawn at one logical pixel each).
const GW: usize = 200;
const GH: usize = 34;
/// All-time buckets (the graph's columns at most).
pub const BUCKETS: usize = GW;
/// The ring's cap: 30 s at 1000 frames a second.
const RING_MAX: usize = 30_000;
/// The all-time histogram: 0.25 ms bins to 250 ms.
const HIST_BIN: f32 = 0.25;
const HIST_N: usize = 1000;
/// The text and graphs are redrawn this often, s.
const REDRAW: f64 = 0.25;

const BG: Color = Color::srgba(0.16, 0.16, 0.17, 0.9);
const ORANGE: [u8; 3] = [255, 150, 30];
const ORANGE_DIM: [u8; 3] = [140, 82, 18];
const ORANGE_PALE: [u8; 3] = [255, 214, 140];
const GUIDE: [u8; 3] = [92, 70, 48];
const GRAPH_BG: [u8; 4] = [30, 30, 32, 255];

/// This frame's times, ms (see the module's notes).
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct FrameTimes {
    /// The interval since the last frame.
    pub frame_ms: f32,
    /// The main world's CPU time of the last whole frame (set in `Last`).
    pub main_ms: f32,
    /// The render world's latest `Render` schedule.
    pub render_ms: f32,
    begin: Option<Instant>,
}

/// The render world's latest `Render` schedule, µs.
static RENDER_US: AtomicU32 = AtomicU32::new(0);

/// When the render world's schedule began (render world only).
#[derive(Resource, Default)]
struct RenderBegin(Option<Instant>);

fn render_begin(mut b: ResMut<RenderBegin>) {
    b.0 = Some(Instant::now());
}

fn render_end(b: Res<RenderBegin>) {
    if let Some(t) = b.0 {
        let us = t.elapsed().as_micros().min(u128::from(u32::MAX)) as u32;
        RENDER_US.store(us, Ordering::Relaxed);
    }
}

fn frame_begin(time: Res<Time<Real>>, mut ft: ResMut<FrameTimes>) {
    ft.begin = Some(Instant::now());
    ft.frame_ms = time.delta_secs() * 1000.0;
    ft.render_ms = RENDER_US.load(Ordering::Relaxed) as f32 / 1000.0;
}

pub fn frame_end(mut ft: ResMut<FrameTimes>) {
    if let Some(t) = ft.begin {
        ft.main_ms = t.elapsed().as_secs_f32() * 1000.0;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Sample {
    /// Real time, s.
    t: f64,
    frame: f32,
    main: f32,
}

/// Min, max, sum, count.
#[derive(Clone, Copy, Debug)]
struct Agg {
    min: f32,
    max: f32,
    sum: f64,
    n: u32,
}

impl Default for Agg {
    fn default() -> Agg {
        Agg {
            min: f32::INFINITY,
            max: 0.0,
            sum: 0.0,
            n: 0,
        }
    }
}

impl Agg {
    fn add(&mut self, x: f32) {
        self.min = self.min.min(x);
        self.max = self.max.max(x);
        self.sum += f64::from(x);
        self.n += 1;
    }
    fn merge(&mut self, o: &Agg) {
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
        self.sum += o.sum;
        self.n += o.n;
    }
    fn mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum / f64::from(self.n)) as f32
        }
    }
}

/// One graph column (or all-time bucket): the frame interval's spread and
/// the main world's mean.
#[derive(Clone, Copy, Debug, Default)]
struct Col {
    frame: Agg,
    main: Agg,
}

/// Since the overlay was turned on.
struct AllTime {
    since: f64,
    frame: Agg,
    main: Agg,
    hist: Vec<u32>,
    /// Frames over the histogram's top.
    over: u32,
    buckets: Vec<Col>,
    /// Each bucket's span, s (doubles when the buckets are full).
    span: f64,
}

impl AllTime {
    fn new(since: f64) -> AllTime {
        AllTime {
            since,
            frame: Agg::default(),
            main: Agg::default(),
            hist: vec![0; HIST_N],
            over: 0,
            buckets: Vec::with_capacity(BUCKETS),
            span: 0.25,
        }
    }

    fn add(&mut self, s: Sample) {
        self.frame.add(s.frame);
        self.main.add(s.main);
        let bin = (s.frame / HIST_BIN) as usize;
        match self.hist.get_mut(bin) {
            Some(h) => *h += 1,
            None => self.over += 1,
        }
        let mut i = ((s.t - self.since).max(0.0) / self.span) as usize;
        while i >= BUCKETS {
            // Pairs merge: half the columns, each twice as long.
            for j in 0..self.buckets.len() / 2 {
                let mut c = self.buckets[2 * j];
                let b = self.buckets[2 * j + 1];
                c.frame.merge(&b.frame);
                c.main.merge(&b.main);
                self.buckets[j] = c;
            }
            if self.buckets.len() % 2 == 1 {
                let last = self.buckets[self.buckets.len() - 1];
                let j = self.buckets.len() / 2;
                self.buckets[j] = last;
                self.buckets.truncate(j + 1);
            } else {
                self.buckets.truncate(self.buckets.len() / 2);
            }
            self.span *= 2.0;
            i = ((s.t - self.since).max(0.0) / self.span) as usize;
        }
        while self.buckets.len() <= i {
            self.buckets.push(Col::default());
        }
        self.buckets[i].frame.add(s.frame);
        self.buckets[i].main.add(s.main);
    }

    /// The 99th percentile of the frame interval, from the histogram (the
    /// bin's upper edge); `None` when it is past the histogram's top.
    fn p99(&self) -> Option<f32> {
        let n = self.frame.n;
        if n == 0 {
            return Some(0.0);
        }
        let want = (f64::from(n) * 0.99).ceil() as u32;
        let mut acc = 0;
        for (i, h) in self.hist.iter().enumerate() {
            acc += h;
            if acc >= want {
                return Some((i + 1) as f32 * HIST_BIN);
            }
        }
        None
    }
}

/// The overlay's state.
#[derive(Resource)]
pub struct Overlay {
    pub on: bool,
    ring: VecDeque<Sample>,
    all: AllTime,
    /// Reused for the percentiles.
    scratch: Vec<f32>,
    cols: Vec<Col>,
    drawn_at: f64,
    /// The overlay's own CPU time over the last redraw period: sum, max,
    /// frames (ms); and the last period's mean and max, shown.
    cost: (f64, f32, u32),
    cost_shown: (f32, f32),
    ui: Option<OverlayUi>,
}

struct OverlayUi {
    root: Entity,
    panel: Entity,
    text: Entity,
    info: Entity,
    labels: [Entity; 3],
    imgs: [Entity; 3],
    graphs: [Handle<Image>; 3],
    /// The layout last applied: the scale and the touch placement.
    laid: Option<(f32, bool)>,
}

#[derive(Resource)]
struct MonoFont(Handle<Font>);

/// Whether the overlay is asked for at the start (`debug=1`).
pub fn wanted(o: &crate::options::Options) -> bool {
    o.param("debug").is_some_and(|v| v == "1" || v == "true")
}

pub fn plugin(app: &mut App) {
    let on = wanted(&app.world().resource::<Opts>().o);
    app.init_resource::<FrameTimes>()
        .insert_resource(Overlay {
            on,
            ring: VecDeque::with_capacity(8192),
            all: AllTime::new(0.0),
            scratch: Vec::with_capacity(8192),
            cols: vec![Col::default(); GW],
            drawn_at: f64::NEG_INFINITY,
            cost: (0.0, 0.0, 0),
            cost_shown: (0.0, 0.0),
            ui: None,
        })
        .add_systems(Startup, load_font)
        .add_systems(First, frame_begin.after(bevy::time::TimeSystems))
        .add_systems(Update, (toggle, layout).chain())
        .add_systems(Last, (frame_end, sample).chain());
    app.sub_app_mut(bevy::render::RenderApp)
        .init_resource::<RenderBegin>()
        .add_systems(
            bevy::render::Render,
            (
                render_begin.in_set(bevy::render::RenderSystems::ExtractCommands),
                render_end.in_set(bevy::render::RenderSystems::PostCleanup),
            ),
        );
}

/// Bevy's bundled Fira Mono (the `default_font` feature's; the client
/// replaces the default font with Rajdhani), registered for the overlay.
fn load_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let h = fonts.add(Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec()));
    commands.insert_resource(MonoFont(h));
}

/// F3 shows or hides it; showing starts the all-time span afresh.
fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut ov: ResMut<Overlay>,
    mut nodes: Query<&mut Node>,
) {
    if !keys.just_pressed(TOGGLE) {
        return;
    }
    ov.on = !ov.on;
    info!("debug overlay {}", if ov.on { "on" } else { "off" });
    if ov.on {
        ov.all = AllTime::new(time.elapsed_secs_f64());
        ov.drawn_at = f64::NEG_INFINITY;
    }
    if let Some(ui) = &ov.ui
        && let Ok(mut n) = nodes.get_mut(ui.root)
    {
        n.display = if ov.on { Display::Flex } else { Display::None };
    }
}

/// Font size of the text and of the graphs' labels at full scale, px.
const FONT: f32 = 11.0;
const FONT_LABEL: f32 = 10.0;

/// The panel's size and place for the window: full size from 1100 × 760
/// logical px up, down to 0.7 on a phone (text no smaller than 8 px); in
/// the bottom-left corner (empty in the desktop HUD), and with the touch
/// controls (which take both bottom corners and the buttons under the
/// place and time) centred under the route's progress bar, over the
/// scenery.
fn layout(
    mut ov: ResMut<Overlay>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    play: Option<Res<crate::play::Play>>,
    mut nodes: Query<&mut Node>,
    mut fonts: Query<&mut TextFont>,
) {
    let Some(ui) = ov.ui.as_mut() else { return };
    let Ok(w) = windows.single() else { return };
    let k = (w.width() / 1100.0).min(w.height() / 760.0).clamp(0.7, 1.0);
    let k = (k * 20.0).round() / 20.0;
    let touch = play.as_ref().is_some_and(|p| p.touch_ui);
    if ui.laid == Some((k, touch)) {
        return;
    }
    ui.laid = Some((k, touch));
    if let Ok(mut n) = nodes.get_mut(ui.panel) {
        n.padding = UiRect::all(Val::Px(6.0 * k));
        n.row_gap = Val::Px(3.0 * k);
    }
    if let Ok(mut n) = nodes.get_mut(ui.root) {
        if touch {
            // Centred under the route's progress bar.
            n.left = Val::Px(0.0);
            n.right = Val::Px(0.0);
            n.bottom = Val::Auto;
            n.top = Val::Px(44.0);
            n.align_items = AlignItems::Center;
        } else {
            n.left = Val::Px(8.0);
            n.right = Val::Auto;
            n.top = Val::Auto;
            n.bottom = Val::Px(8.0);
            n.align_items = AlignItems::FlexStart;
        }
    }
    for e in ui.imgs {
        if let Ok(mut n) = nodes.get_mut(e) {
            n.width = Val::Px((GW as f32 * k).round());
            n.height = Val::Px((GH as f32 * k).round());
        }
    }
    for (e, size) in [(ui.text, FONT), (ui.info, FONT)]
        .into_iter()
        .chain(ui.labels.map(|e| (e, FONT_LABEL)))
    {
        if let Ok(mut f) = fonts.get_mut(e) {
            f.font_size = bevy::text::FontSize::Px((size * k).round().max(8.0));
        }
    }
}

/// What the overlay shows besides the frame times.
#[derive(bevy::ecs::system::SystemParam)]
struct Info<'w, 's> {
    status: Res<'w, Status>,
    opts: Res<'w, Opts>,
    entities: &'w Entities,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
    play: Option<Res<'w, crate::play::Play>>,
    ui: Option<Res<'w, crate::ui::UiState>>,
    windows: Query<'w, 's, &'static Window, With<bevy::window::PrimaryWindow>>,
}

/// Each frame: the sample into the ring (and the all-time span); four times
/// a second, the text and graphs.
#[allow(clippy::too_many_arguments)]
fn sample(
    mut commands: Commands,
    time: Res<Time<Real>>,
    ft: Res<FrameTimes>,
    mut ov: ResMut<Overlay>,
    font: Option<Res<MonoFont>>,
    mut images: ResMut<Assets<Image>>,
    mut texts: Query<&mut Text>,
    info: Info,
) {
    let t0 = Instant::now();
    let ov = &mut *ov;
    let now = time.elapsed_secs_f64();
    // The first frames' intervals are the start-up's, not frames.
    if info.status.frames > 2 {
        let s = Sample {
            t: now,
            frame: ft.frame_ms,
            main: ft.main_ms,
        };
        while ov.ring.front().is_some_and(|f| now - f.t > 30.0) || ov.ring.len() >= RING_MAX {
            ov.ring.pop_front();
        }
        ov.ring.push_back(s);
        if ov.on {
            ov.all.add(s);
        }
    }
    if !ov.on {
        return;
    }
    if ov.ui.is_none() {
        let Some(font) = font else { return };
        ov.ui = Some(spawn(&mut commands, &mut images, &font.0));
        return;
    }
    if now - ov.drawn_at >= REDRAW {
        ov.drawn_at = now;
        let (sum, max, n) = ov.cost;
        ov.cost_shown = (
            if n > 0 {
                (sum / f64::from(n)) as f32
            } else {
                0.0
            },
            max,
        );
        ov.cost = (0.0, 0.0, 0);
        redraw(ov, now, &ft, &mut images, &mut texts, &info);
    }
    let ms = t0.elapsed().as_secs_f32() * 1000.0;
    ov.cost.0 += f64::from(ms);
    ov.cost.1 = ov.cost.1.max(ms);
    ov.cost.2 += 1;
}

fn spawn(commands: &mut Commands, images: &mut Assets<Image>, font: &Handle<Font>) -> OverlayUi {
    let tf = |size: f32| TextFont {
        font: FontSource::Handle(font.clone()),
        font_size: bevy::text::FontSize::Px(size),
        ..default()
    };
    let orange = Color::srgb_u8(ORANGE[0], ORANGE[1], ORANGE[2]);
    let mut graph = || {
        let mut img = Image::new_fill(
            Extent3d {
                width: GW as u32,
                height: GH as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &GRAPH_BG,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        img.sampler = ImageSampler::nearest();
        images.add(img)
    };
    let graphs = [graph(), graph(), graph()];
    let text = commands
        .spawn((
            Text::new(""),
            tf(FONT),
            TextColor(orange),
            LineHeight::RelativeToFont(1.25),
        ))
        .id();
    let info = commands
        .spawn((
            Text::new(""),
            tf(FONT),
            TextColor(orange),
            LineHeight::RelativeToFont(1.25),
        ))
        .id();
    let mut labels = [Entity::PLACEHOLDER; 3];
    let mut imgs = [Entity::PLACEHOLDER; 3];
    let mut rows = Vec::new();
    for (i, g) in graphs.iter().enumerate() {
        let label = commands
            .spawn((
                Text::new(""),
                tf(FONT_LABEL),
                TextColor(Color::srgb_u8(ORANGE_DIM[0] + 60, ORANGE_DIM[1] + 40, 40)),
            ))
            .id();
        labels[i] = label;
        let img = commands
            .spawn((
                ImageNode::new(g.clone()),
                Node {
                    width: Val::Px(GW as f32),
                    height: Val::Px(GH as f32),
                    ..default()
                },
            ))
            .id();
        imgs[i] = img;
        let row = commands
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .add_children(&[label, img])
            .id();
        rows.push(row);
    }
    let panel = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::all(Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(BG),
        ))
        .add_children(&[text])
        .add_children(&rows)
        .add_children(&[info])
        .id();
    // The panel in a box that places it (`layout`).
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                bottom: Val::Px(8.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            GlobalZIndex(i32::MAX),
            Name::new("debug overlay"),
        ))
        .add_children(&[panel])
        .id();
    OverlayUi {
        root,
        panel,
        text,
        info,
        labels,
        imgs,
        graphs,
        laid: None,
    }
}

/// Mean, min, max and p99 of the samples within `span` s of `now`, and
/// the columns of the graph over that span.
fn window(ov: &mut Overlay, now: f64, span: f64) -> (Agg, Agg, f32) {
    let mut frame = Agg::default();
    let mut main = Agg::default();
    ov.scratch.clear();
    for c in ov.cols.iter_mut() {
        *c = Col::default();
    }
    let from = now - span;
    for s in ov.ring.iter().rev() {
        if s.t < from {
            break;
        }
        frame.add(s.frame);
        main.add(s.main);
        ov.scratch.push(s.frame);
        let i = (((s.t - from) / span) * GW as f64) as usize;
        let c = &mut ov.cols[i.min(GW - 1)];
        c.frame.add(s.frame);
        c.main.add(s.main);
    }
    (frame, main, p99(&mut ov.scratch))
}

fn p99(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let k = ((v.len() as f64 * 0.99).ceil() as usize).clamp(1, v.len()) - 1;
    *v.select_nth_unstable_by(k, f32::total_cmp).1
}

/// The graph's top, ms: the first of these at or over 1.25 × the span's
/// 99th percentile, so one hitch (a load, a late pipeline) does not
/// flatten the rest; a column over the top is drawn to it with a white cap.
fn scale(p99: f32) -> f32 {
    let want = p99 * 1.25;
    [20.0, 34.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 5000.0]
        .into_iter()
        .find(|s| *s >= want)
        .unwrap_or(5000.0)
}

const CLIPPED: [u8; 3] = [255, 255, 255];

/// Draws columns into an RGBA image: the frame interval's min to max dim,
/// its mean bright, the main world's mean pale; guides at 16.7 and 33.3 ms.
fn draw(data: &mut [u8], cols: &[Col], top: f32) {
    for px in data.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&GRAPH_BG);
    }
    let y_of = |ms: f32| -> usize {
        let y = (ms / top * GH as f32).clamp(0.0, GH as f32 - 1.0);
        GH - 1 - y as usize
    };
    let mut put = |x: usize, y: usize, c: [u8; 3]| {
        let i = (y * GW + x) * 4;
        data[i..i + 3].copy_from_slice(&c);
    };
    for guide in [1000.0 / 60.0, 1000.0 / 30.0] {
        if guide < top {
            let y = y_of(guide);
            for x in (0..GW).step_by(2) {
                put(x, y, GUIDE);
            }
        }
    }
    for (x, c) in cols.iter().enumerate().take(GW) {
        if c.frame.n == 0 {
            continue;
        }
        let (lo, hi) = (y_of(c.frame.min), y_of(c.frame.max));
        for y in hi..=lo {
            put(x, y, ORANGE_DIM);
        }
        // Bars from the bottom to the mean.
        let m = y_of(c.frame.mean());
        for y in m..GH {
            put(x, y, ORANGE);
        }
        // The max as a bright cap, white when it is over the top.
        if c.frame.max > top {
            put(x, 0, CLIPPED);
            put(x, 1, CLIPPED);
        } else {
            put(x, hi, ORANGE);
        }
        put(x, y_of(c.main.mean()), ORANGE_PALE);
    }
}

/// Columns of a graph over its top.
fn over(cols: &[Col], top: f32) -> usize {
    cols.iter().filter(|c| c.frame.max > top).count()
}

fn fmt_span(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn redraw(
    ov: &mut Overlay,
    now: f64,
    ft: &FrameTimes,
    images: &mut Assets<Image>,
    texts: &mut Query<&mut Text>,
    info: &Info,
) {
    let Some(ui) = ov.ui.as_ref() else { return };
    let (text_e, info_e, labels, graphs) = (ui.text, ui.info, ui.labels, ui.graphs.clone());
    let mut out = String::with_capacity(512);
    let fps = if info.status.frame_ms > 0.0 {
        1000.0 / info.status.frame_ms
    } else {
        0.0
    };
    let _ = writeln!(
        out,
        "{fps:5.1} fps  frame {:5.1}  main {:4.1}  render {:4.1} ms",
        ft.frame_ms, ft.main_ms, ft.render_ms
    );
    let _ = writeln!(out, "frame ms   min   mean    p99    max  main avg/max");
    let mut rows = Vec::with_capacity(3);
    for (i, span) in [5.0, 30.0].into_iter().enumerate() {
        let (f, m, p) = window(ov, now, span);
        let top = scale(p);
        if let Some(mut img) = images.get_mut(&graphs[i])
            && let Some(d) = img.data.as_mut()
        {
            draw(d, &ov.cols, top);
        }
        let n_over = over(&ov.cols, top);
        rows.push((format!("{span:.0} s"), f, m, Some(p), (top, n_over)));
    }
    {
        let a = &ov.all;
        let top = scale(a.p99().unwrap_or(1000.0));
        if let Some(mut img) = images.get_mut(&graphs[2])
            && let Some(d) = img.data.as_mut()
        {
            draw(d, &a.buckets, top);
        }
        let n_over = over(&a.buckets, top);
        let row = (
            fmt_span(now - a.since),
            a.frame,
            a.main,
            a.p99(),
            (top, n_over),
        );
        rows.push(row);
    }
    for (label, f, m, p, _) in &rows {
        if f.n == 0 {
            let _ = writeln!(out, "{label:<7}     -");
            continue;
        }
        let p = p.map_or_else(|| " >250".to_string(), |p| format!("{p:6.1}"));
        let _ = writeln!(
            out,
            "{label:<7}{:6.1}{:7.1}{p}{:7.1}  {:4.1}/{:<5.1}",
            f.min,
            f.mean(),
            f.max,
            m.mean(),
            m.max
        );
    }
    out.pop();
    if let Ok(mut t) = texts.get_mut(text_e) {
        t.0 = out;
    }
    let names = ["last 5 s", "last 30 s", "since on"];
    for (i, (_, f, _, _, (top, n_over))) in rows.iter().enumerate() {
        if let Ok(mut t) = texts.get_mut(labels[i]) {
            let clipped = if *n_over > 0 {
                format!(", {n_over} over")
            } else {
                String::new()
            };
            t.0 = format!("{}  {} frames  top {top:.0} ms{clipped}", names[i], f.n);
        }
    }
    let mut s = String::with_capacity(256);
    describe(&mut s, info);
    let _ = write!(
        s,
        "\noverlay {:.3} ms/frame (max {:.2})  F3 hides",
        ov.cost_shown.0, ov.cost_shown.1
    );
    if let Ok(mut t) = texts.get_mut(info_e) {
        t.0 = s;
    }
}

/// The client's state in a few lines.
fn describe(s: &mut String, info: &Info) {
    let st = &info.status;
    let mode = match (&info.ui, &info.play) {
        (Some(ui), Some(play)) => format!(
            "{}, screen {}",
            crate::ui::mode_name(ui, play, st),
            ui.screen.name()
        ),
        _ => st.state.to_string(),
    };
    let _ = write!(s, "{mode}  level {}", info.opts.o.level);
    if let Some(play) = &info.play {
        let _ = write!(s, "  car {}", play.params.car);
        if let Some(r) = &play.race {
            let c = &r.session.curr;
            let p = &c.players[0];
            let _ = write!(
                s,
                "\ntick {} ({:.1} s, {:?})  {:.0} km/h  lap {}/{}  gear {}",
                c.tick,
                c.race.time,
                c.race.state,
                p.v.speed * 3.6,
                p.rules.lap,
                c.race.laps,
                p.phys.gear
            );
        }
    }
    let backend = if cfg!(target_arch = "wasm32") {
        if cfg!(mr_webgl2) { "WebGL2" } else { "WebGPU" }
    } else {
        "native"
    };
    match &info.adapter {
        Some(a) => {
            let _ = write!(s, "\n{backend}: {} ({:?})", a.name, a.backend);
        }
        None => {
            let _ = write!(s, "\n{backend}");
        }
    }
    if let Ok(w) = info.windows.single() {
        let _ = write!(
            s,
            "  {}x{} @{:.2}",
            w.physical_width(),
            w.physical_height(),
            w.scale_factor()
        );
    }
    let _ = write!(
        s,
        "\nentities {}  pipelines waiting {}  late frames {}",
        info.entities.count_spawned(),
        st.pipelines_waiting,
        st.late_frames
    );
    #[cfg(target_arch = "wasm32")]
    {
        let mb = core::arch::wasm32::memory_size(0) as f64 * 65536.0 / 1048576.0;
        let _ = write!(s, "  wasm {mb:.0} MB");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_time_stays_bounded_and_keeps_the_extremes() {
        let mut a = AllTime::new(0.0);
        // An hour at 60 fps with one 300 ms hitch.
        let n = 60 * 3600;
        for i in 0..n {
            let frame = if i == 1000 {
                300.0
            } else {
                16.0 + (i % 3) as f32
            };
            a.add(Sample {
                t: i as f64 / 60.0,
                frame,
                main: 2.0,
            });
        }
        assert!(a.buckets.len() <= BUCKETS);
        assert_eq!(a.frame.n, n as u32);
        assert_eq!(a.frame.max, 300.0);
        assert_eq!(a.over, 1);
        let total: u32 = a.buckets.iter().map(|b| b.frame.n).sum();
        assert_eq!(total, n as u32);
        assert!(a.buckets.iter().any(|b| b.frame.max == 300.0));
        // 16, 17, 18 in equal shares: the 99th percentile's bin ends at 18.25.
        assert_eq!(a.p99(), Some(18.25));
    }

    #[test]
    fn percentile_and_scale() {
        let mut v: Vec<f32> = (1..=100).map(|x| x as f32).collect();
        assert_eq!(p99(&mut v), 99.0);
        assert_eq!(scale(16.0), 20.0);
        assert_eq!(scale(40.0), 50.0);
        assert_eq!(fmt_span(3725.0), "1:02:05");
    }
}
