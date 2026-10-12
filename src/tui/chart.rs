// SPDX-FileCopyrightText: 2026 Igor Santos
// SPDX-License-Identifier: Apache-2.0

//! A bar chart with axes, for a spend or token series: eighth blocks for the
//! bars, a y axis with round values, an x axis with dates and weekdays, a
//! cursor that reads one bar out, and a line of totals.
//!
//! From the top, inside the box: a totals line, the cursor readout, the plot,
//! a row of weekday letters, a row of dates. Rows are dropped from the least
//! useful when the panel is short, and the y axis when it is narrow, so the
//! chart still draws at any size. `Plot` is the geometry and the data; drawing
//! uses it, so the cursor lands on the bar it points at.

use super::data::{DayPoint, Slots};
use super::fmt::{axis_money, axis_tokens, compact, fit, money, WEEKDAYS};
use super::scale::Axis;
use super::theme::{Fill, Ink, Palette, SERIES};
use crate::aggregate::date_key;
use crate::query::weekday;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::collections::BTreeMap;

/// Vertical eighth blocks, one to eight eighths of a cell.
const EIGHTHS: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];

/// The shortest plot worth drawing, in rows.
const MIN_PLOT: u16 = 3;
/// A bar is never drawn wider than this, so a few days do not become slabs.
/// A week or more of bars always fills the plot instead.
const MAX_CELL: usize = 16;
/// The narrowest inner width that keeps a y axis.
const MIN_AXIS_WIDTH: u16 = 16;

/// What a chart plots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Cost,
    Tokens,
}

impl Metric {
    /// A value as it reads in text: `$1.25` or `23.8K`.
    pub fn show(self, v: f64) -> String {
        match self {
            Metric::Cost => money(v),
            Metric::Tokens => compact(v.round() as u64),
        }
    }

    fn axis_label(self, v: f64) -> String {
        match self {
            Metric::Cost => axis_money(v),
            Metric::Tokens => axis_tokens(v),
        }
    }
}

/// One bar: a day, or several days added up when the range is wider than the
/// plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub first: i64,
    pub last: i64,
    pub cost: f64,
    pub tokens: u64,
    pub messages: usize,
    /// Cost per model, by model name.
    pub models: Vec<(String, f64)>,
}

impl Bar {
    pub fn value(&self, metric: Metric) -> f64 {
        match metric {
            Metric::Cost => self.cost,
            Metric::Tokens => self.tokens as f64,
        }
    }

    pub fn days(&self) -> i64 {
        self.last - self.first + 1
    }

    pub fn contains(&self, day: i64) -> bool {
        (self.first..=self.last).contains(&day)
    }

    /// The model that cost the most, with its share of the bar's cost.
    pub fn top_model(&self) -> Option<(&str, f64)> {
        let (name, cost) = self
            .models
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))?;
        (self.cost > 0.0).then(|| (name.as_str(), cost / self.cost))
    }
}

/// The days of `series` in groups of `per`, each added up into a bar.
pub fn bars(series: &[DayPoint], per: usize) -> Vec<Bar> {
    series
        .chunks(per.max(1))
        .map(|days| {
            let mut models: BTreeMap<&str, f64> = BTreeMap::new();
            for d in days {
                for (name, cost) in &d.models {
                    *models.entry(name.as_str()).or_default() += cost;
                }
            }
            Bar {
                first: days[0].day,
                last: days[days.len() - 1].day,
                cost: days.iter().map(|d| d.cost_usd).sum(),
                tokens: days.iter().map(|d| d.tokens).sum(),
                messages: days.iter().map(|d| d.messages).sum(),
                models: models
                    .into_iter()
                    .map(|(name, cost)| (name.to_string(), cost))
                    .collect(),
            }
        })
        .collect()
}

/// Which rows the chart shows, for an inner height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rows {
    pub stats: bool,
    pub readout: bool,
    pub weekdays: bool,
    pub dates: bool,
    pub plot: u16,
}

impl Rows {
    /// Keeps as many rows as fit while the plot stays at least `MIN_PLOT`
    /// tall. The weekday row needs room for a plot of five. Rows go in this
    /// order: weekdays, totals, readout, dates.
    pub fn for_height(height: u16, weekdays: bool) -> Rows {
        let (mut stats, mut readout, mut dates, mut weekdays) = (true, true, true, weekdays);
        let used = |s: bool, r: bool, d: bool, w: bool| {
            u16::from(s) + u16::from(r) + u16::from(d) + u16::from(w)
        };
        if weekdays && height < used(stats, readout, dates, true) + 5 {
            weekdays = false;
        }
        if height < used(stats, readout, dates, weekdays) + MIN_PLOT {
            stats = false;
        }
        if height < used(stats, readout, dates, weekdays) + MIN_PLOT {
            readout = false;
        }
        if height < used(stats, readout, dates, weekdays) + MIN_PLOT {
            dates = false;
        }
        Rows {
            stats,
            readout,
            weekdays,
            dates,
            plot: height.saturating_sub(used(stats, readout, dates, weekdays)),
        }
    }
}

/// Where everything goes inside a chart panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Geometry {
    pub stats_y: Option<u16>,
    pub readout_y: Option<u16>,
    pub plot: Rect,
    pub weekdays_y: Option<u16>,
    pub dates_y: Option<u16>,
    /// Columns of the y axis: its labels and the axis line.
    pub gutter: u16,
    pub inner: Rect,
    /// Days added up into one bar.
    pub per: usize,
    /// For each bar, its first column in the plot and the columns it takes,
    /// with its gap.
    pub cells: Vec<(u16, u16)>,
}

impl Geometry {
    /// The layout for `days` days in a panel, with y labels `label_width`
    /// columns wide.
    pub fn new(panel: Rect, days: usize, label_width: u16) -> Geometry {
        let inner = Rect::new(
            panel.x + 1,
            panel.y + 1,
            panel.width.saturating_sub(2),
            panel.height.saturating_sub(2),
        );
        let wide = inner.width >= MIN_AXIS_WIDTH;
        // The labels, a blank column before them so they do not touch the box,
        // and the axis line.
        let gutter = if wide { label_width + 2 } else { 0 };
        let plot_width = usize::from(inner.width.saturating_sub(gutter));
        let (per, bars) = if days == 0 || plot_width == 0 {
            (1, 0)
        } else if days > plot_width {
            let per = days.div_ceil(plot_width);
            (per, days.div_ceil(per))
        } else {
            (1, days)
        };
        // A few bars get a fixed width. Many share the plot, a column or two
        // each, so the chart always fills its width.
        let cells: Vec<(u16, u16)> = if bars == 0 {
            Vec::new()
        } else if plot_width / bars >= MAX_CELL {
            (0..bars)
                .map(|i| ((i * MAX_CELL) as u16, MAX_CELL as u16))
                .collect()
        } else {
            (0..bars)
                .map(|i| {
                    let (from, to) = (i * plot_width / bars, (i + 1) * plot_width / bars);
                    (from as u16, (to - from) as u16)
                })
                .collect()
        };
        // A weekday label needs its own column and a gap, so crowded bars
        // (a month in a narrow panel) go without the row.
        let roomy = cells.iter().all(|c| c.1 >= 2);
        let rows = Rows::for_height(inner.height, per == 1 && bars > 0 && roomy);
        let mut y = inner.y;
        let mut take = |rows: u16| {
            y += rows;
            y - rows
        };
        let stats_y = rows.stats.then(|| take(1));
        let readout_y = rows.readout.then(|| take(1));
        let plot_y = take(rows.plot);
        let weekdays_y = rows.weekdays.then(|| take(1));
        let dates_y = rows.dates.then(|| take(1));
        Geometry {
            stats_y,
            readout_y,
            plot: Rect::new(inner.x + gutter, plot_y, plot_width as u16, rows.plot),
            weekdays_y,
            dates_y,
            gutter,
            inner,
            per,
            cells,
        }
    }

    pub fn bars(&self) -> usize {
        self.cells.len()
    }

    /// The first column of bar `i` on screen.
    pub fn x_of(&self, i: usize) -> u16 {
        self.plot.x + self.cells[i].0
    }

    /// The columns the bar itself fills: its cell less a gap of one when the
    /// cell is wider than a column.
    pub fn bar_width(&self, i: usize) -> u16 {
        match self.cells[i].1 {
            0 | 1 => self.cells[i].1,
            w => w - 1,
        }
    }

    /// The columns from the plot start to the end of the last bar.
    pub fn span(&self) -> usize {
        self.cells
            .len()
            .checked_sub(1)
            .map_or(0, |i| usize::from(self.cells[i].0 + self.bar_width(i)))
    }
}

/// A series laid out in a panel: geometry, bars and the scale.
#[derive(Debug, Clone)]
pub struct Plot {
    pub geo: Geometry,
    pub bars: Vec<Bar>,
    pub axis: Axis,
    pub metric: Metric,
}

impl Plot {
    pub fn new(panel: Rect, series: &[DayPoint], metric: Metric) -> Plot {
        // The label width depends on the scale, which depends on how many
        // days a bar holds, which depends on the label width. It settles in a
        // pass or two; a third pass is the last.
        let mut label_width = 2;
        let mut plot = None;
        for _ in 0..3 {
            let geo = Geometry::new(panel, series.len(), label_width);
            let bars = bars(series, geo.per);
            let peak = bars.iter().map(|b| b.value(metric)).fold(0.0, f64::max);
            let axis = Axis::of(peak);
            let width = axis
                .ticks()
                .iter()
                .map(|t| metric.axis_label(*t).chars().count() as u16)
                .max()
                .unwrap_or(2);
            let settled = width == label_width;
            label_width = width;
            plot = Some(Plot {
                geo,
                bars,
                axis,
                metric,
            });
            if settled {
                break;
            }
        }
        plot.unwrap_or_else(|| unreachable_plot(panel, metric))
    }

    /// The index of the bar holding `day`.
    pub fn index_of(&self, day: i64) -> Option<usize> {
        self.bars.iter().position(|b| b.contains(day))
    }

    /// The bar the readout shows before the cursor is moved: the one holding
    /// today, else the newest.
    pub fn default_index(&self, today: i64) -> Option<usize> {
        self.index_of(today)
            .or_else(|| self.bars.len().checked_sub(1))
    }

    /// The index of the tallest bar, if any bar has a value.
    pub fn peak_index(&self) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, b) in self.bars.iter().enumerate() {
            let v = b.value(self.metric);
            if v > 0.0 && best.is_none_or(|(_, top)| v > top) {
                best = Some((i, v));
            }
        }
        best.map(|(i, _)| i)
    }
}

/// `Plot::new` always runs its loop at least once; this is the same plot for
/// an empty series, so no code path needs an `unwrap`.
fn unreachable_plot(panel: Rect, metric: Metric) -> Plot {
    Plot {
        geo: Geometry::new(panel, 0, 2),
        bars: Vec::new(),
        axis: Axis::of(0.0),
        metric,
    }
}

/// What the chart shows besides the bars.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub fill: Fill,
    /// Stack each bar by model, in the colors of these slots, instead of one
    /// color along the fill.
    pub stack: Option<&'a Slots>,
    /// The bar the cursor is on, if it was moved or hovered.
    pub cursor: Option<usize>,
    /// The day number of today, to mark its bar.
    pub today: i64,
}

fn dim(pal: &Palette) -> Style {
    Style::default().fg(pal.color(pal.graph_text))
}

fn text(pal: &Palette) -> Style {
    Style::default().fg(pal.color(pal.fg))
}

/// The weekday label style: the cursor, today and the peak stand out, with
/// modifiers as well as colors so the mono theme shows them too.
fn mark_style(pal: &Palette, cursor: bool, today: bool, peak: bool) -> Style {
    if cursor {
        return if pal.mono {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
                .fg(pal.color(pal.sel_fg))
                .bg(pal.color(pal.sel_bg))
                .add_modifier(Modifier::BOLD)
        };
    }
    if today {
        return Style::default()
            .fg(pal.color(pal.hi))
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    }
    if peak {
        return Style::default()
            .fg(pal.color(pal.warn))
            .add_modifier(Modifier::BOLD);
    }
    dim(pal)
}

/// `Thu 2026-09-03`.
pub fn day_label(day: i64) -> String {
    format!("{} {}", WEEKDAYS[weekday(day)], date_key(day))
}

/// `09-03`, the date without its year.
fn short_date(day: i64) -> String {
    date_key(day)[5..].to_string()
}

/// The readout line for a bar: its date and tags, then its numbers, each part
/// added only while it fits.
pub fn readout(plot: &Plot, bar: &Bar, today: bool, peak: bool, width: usize) -> String {
    let mut head = if bar.days() == 1 {
        day_label(bar.first)
    } else {
        format!(
            "{} to {} ({} days)",
            short_date(bar.first),
            short_date(bar.last),
            bar.days()
        )
    };
    if today {
        head.push_str(" today");
    }
    if peak {
        head.push_str(" peak");
    }
    let cost = money(bar.cost);
    let tokens = format!("{} tok", compact(bar.tokens));
    let msgs = format!("{} msgs", bar.messages);
    let mut parts = vec![head];
    match plot.metric {
        Metric::Cost => parts.extend([cost, msgs, tokens]),
        Metric::Tokens => parts.extend([tokens, cost, msgs]),
    }
    if let (Metric::Cost, Some((model, share))) = (plot.metric, bar.top_model()) {
        let name = model.strip_prefix("claude-").unwrap_or(model);
        parts.push(format!("{name} {:.0}%", share * 100.0));
    }
    let mut out = String::new();
    for part in parts {
        let next = if out.is_empty() {
            part
        } else {
            format!("{out}  {part}")
        };
        if next.chars().count() > width {
            if out.is_empty() {
                return fit(&next, width);
            }
            break;
        }
        out = next;
    }
    out
}

/// The totals line, the longest form that fits.
pub fn stats(plot: &Plot, series: &[DayPoint], today: i64, width: usize) -> String {
    let m = plot.metric;
    let value = |d: &DayPoint| match m {
        Metric::Cost => d.cost_usd,
        Metric::Tokens => d.tokens as f64,
    };
    let total: f64 = series.iter().map(value).sum();
    let peak = series.iter().map(value).fold(0.0, f64::max);
    let peak_day = series.iter().find(|d| value(d) == peak && peak > 0.0);
    // The average is per day that has happened, so a week still in progress
    // is not pulled down by the days to come.
    let elapsed = series.iter().filter(|d| d.day <= today).count();
    let avg = if elapsed == 0 {
        0.0
    } else {
        total / elapsed as f64
    };
    let on = peak_day.map_or(String::new(), |d| format!(" on {}", short_date(d.day)));
    let forms = [
        format!(
            "total {}  peak {}{on}  avg {}/day",
            m.show(total),
            m.show(peak),
            m.show(avg)
        ),
        format!(
            "total {}  peak {}  avg {}/day",
            m.show(total),
            m.show(peak),
            m.show(avg)
        ),
        format!(
            "total {}  peak {}  avg {}",
            m.show(total),
            m.show(peak),
            m.show(avg)
        ),
        format!("total {}  peak {}", m.show(total), m.show(peak)),
        format!("total {}", m.show(total)),
    ];
    for form in &forms {
        if form.chars().count() <= width {
            return form.clone();
        }
    }
    fit(&forms[4], width)
}

/// The three date labels under the plot: first, middle and last, as many as
/// fit without touching. Years are shown when there is room for them.
fn date_labels(plot: &Plot, width: usize) -> Vec<(usize, String)> {
    let bars = &plot.bars;
    let (Some(first), Some(last)) = (bars.first(), bars.last()) else {
        return Vec::new();
    };
    let span = plot.geo.span();
    let long = |d: i64| date_key(d);
    let short = |d: i64| short_date(d);
    let pick = |fmt: &dyn Fn(i64) -> String| {
        let mut out = vec![(0, fmt(first.first))];
        if bars.len() > 1 {
            out.push((
                span.saturating_sub(fmt(last.last).chars().count()),
                fmt(last.last),
            ));
        }
        if bars.len() > 2 {
            let mid = &bars[bars.len() / 2];
            let label = fmt(mid.first);
            let at = usize::from(plot.geo.cells[bars.len() / 2].0);
            let at = at.min(span.saturating_sub(label.chars().count()));
            out.push((at, label));
        }
        out.sort_by_key(|(at, _)| *at);
        out
    };
    let fits = |labels: &[(usize, String)]| {
        labels
            .windows(2)
            .all(|w| w[0].0 + w[0].1.chars().count() < w[1].0)
            && labels
                .last()
                .is_some_and(|(at, l)| at + l.chars().count() <= width)
    };
    let drop_middle = |mut labels: Vec<(usize, String)>| {
        if labels.len() == 3 {
            labels.remove(1);
        }
        labels
    };
    for labels in [pick(&long), pick(&short), drop_middle(pick(&short))] {
        if fits(&labels) {
            return labels;
        }
    }
    vec![(0, short(first.first))]
}

/// The day of the month, `1` to `31`.
fn day_number(day: i64) -> String {
    date_key(day)[8..].trim_start_matches('0').to_string()
}

/// Whether bar `i` carries a day number when every `step` bars do. The first
/// and the last bar always do.
fn shows_number(i: usize, last: usize, step: usize) -> bool {
    i.is_multiple_of(step) || i == last
}

/// Whether the number of bar `i` leaves a column free before the next number
/// that is shown, so two numbers never touch.
fn number_fits(plot: &Plot, i: usize, last: usize, step: usize) -> bool {
    let end = usize::from(plot.geo.cells[i].0) + day_number(plot.bars[i].first).len();
    match (i + 1..=last).find(|j| shows_number(*j, last, step)) {
        Some(j) => end < usize::from(plot.geo.cells[j].0) || i == last,
        None => true,
    }
}

/// How many bars apart the day numbers are: every day when each cell holds a
/// number and a gap, else every second, third or seventh day, the smallest
/// step that keeps the regular numbers clear of each other. The first and the
/// last bar always show one, and when the last is too close to the one before
/// it the one before is left out.
fn number_step(plot: &Plot) -> usize {
    let last = plot.bars.len().saturating_sub(1);
    for step in [1, 2, 3, 7] {
        let shown: Vec<usize> = (0..=last).filter(|i| i.is_multiple_of(step)).collect();
        let clear = shown.windows(2).all(|w| {
            let end = usize::from(plot.geo.cells[w[0]].0) + day_number(plot.bars[w[0]].first).len();
            end < usize::from(plot.geo.cells[w[1]].0)
        });
        if clear {
            return step;
        }
    }
    usize::MAX
}

/// The weekday label: three letters, two or one, so the labels of one chart
/// all look alike.
fn weekday_label(letters: usize, day: i64) -> String {
    WEEKDAYS[weekday(day)][..letters].to_string()
}

/// How many letters of a weekday fit every cell of the chart, leaving a
/// column between labels.
fn weekday_letters(geo: &Geometry) -> usize {
    let narrowest = geo.cells.iter().map(|c| c.1).min().unwrap_or(1);
    usize::from(narrowest.saturating_sub(1)).clamp(1, 3)
}

/// Draws the chart inside `plot.geo.inner`. The caller draws the box.
pub fn render(buf: &mut Buffer, plot: &Plot, series: &[DayPoint], view: &View, pal: &Palette) {
    let geo = &plot.geo;
    if geo.inner.width == 0 || geo.inner.height == 0 {
        return;
    }
    let width = usize::from(geo.inner.width);
    let view = &View {
        cursor: view.cursor.filter(|i| *i < plot.bars.len()),
        ..*view
    };
    let peak = plot.peak_index();
    if let Some(y) = geo.stats_y {
        let line = stats(plot, series, view.today, width);
        buf.set_stringn(geo.inner.x, y, line, width, text(pal));
    }
    if let Some(y) = geo.readout_y {
        // With no cursor yet the readout shows the newest bar.
        let shown = view.cursor.or_else(|| plot.default_index(view.today));
        if let Some(i) = shown {
            let bar = &plot.bars[i];
            let line = readout(plot, bar, bar.contains(view.today), peak == Some(i), width);
            let style = if view.cursor.is_some() {
                text(pal).add_modifier(Modifier::BOLD)
            } else {
                text(pal)
            };
            buf.set_stringn(geo.inner.x, y, line, width, style);
        }
    }
    draw_axis(buf, plot, pal);
    match view.stack {
        Some(slots) if plot.metric == Metric::Cost => draw_stacked(buf, plot, slots, pal),
        _ => draw_bars(buf, plot, view, pal),
    }
    if let Some(y) = geo.weekdays_y {
        let letters = weekday_letters(geo);
        for (i, bar) in plot.bars.iter().enumerate() {
            let label = weekday_label(letters, bar.first);
            let style = mark_style(
                pal,
                view.cursor == Some(i),
                bar.contains(view.today),
                peak == Some(i),
            );
            let x = geo.x_of(i);
            buf.set_stringn(x, y, label, usize::from(geo.cells[i].1), style);
        }
    }
    if let (Some(y), 1) = (geo.dates_y, geo.per) {
        let step = number_step(plot);
        let last = plot.bars.len().saturating_sub(1);
        for (i, bar) in plot.bars.iter().enumerate() {
            if !shows_number(i, last, step) || !number_fits(plot, i, last, step) {
                continue;
            }
            let style = mark_style(
                pal,
                view.cursor == Some(i),
                bar.contains(view.today),
                peak == Some(i),
            );
            let x = geo.x_of(i);
            let room = usize::from(geo.plot.right().saturating_sub(x));
            buf.set_stringn(x, y, day_number(bar.first), room, style);
        }
    } else if let Some(y) = geo.dates_y {
        for (at, label) in date_labels(plot, usize::from(geo.plot.width)) {
            buf.set_stringn(
                geo.plot.x + at as u16,
                y,
                &label,
                usize::from(geo.plot.width).saturating_sub(at),
                dim(pal),
            );
        }
    }
}

/// The y axis: labels at the bottom, middle and top, an axis line, and faint
/// guide lines across the plot at the top and the middle.
fn draw_axis(buf: &mut Buffer, plot: &Plot, pal: &Palette) {
    let geo = &plot.geo;
    let p = geo.plot;
    if p.height == 0 || p.width == 0 {
        return;
    }
    let guide = Style::default().fg(pal.color(pal.div_line));
    let mid = p.height / 2;
    let rows: Vec<(u16, f64)> = match p.height {
        1 => vec![(0, plot.axis.max)],
        2 => vec![(0, plot.axis.max), (1, 0.0)],
        h => vec![(0, plot.axis.max), (mid, plot.axis.max / 2.0), (h - 1, 0.0)],
    };
    if p.height >= MIN_PLOT {
        for row in [0, mid] {
            for x in p.x..p.x + p.width {
                buf[(x, p.y + row)].set_char('\u{2504}').set_style(guide);
            }
        }
    }
    if geo.gutter == 0 {
        return;
    }
    let axis_x = p.x - 1;
    for row in 0..p.height {
        buf[(axis_x, p.y + row)]
            .set_char('\u{2502}')
            .set_style(dim(pal));
    }
    for (row, value) in rows {
        buf[(axis_x, p.y + row)]
            .set_char('\u{2524}')
            .set_style(dim(pal));
        let label = fit(&plot.metric.axis_label(value), usize::from(geo.gutter - 2));
        let width = label.chars().count() as u16;
        buf.set_string(axis_x - width, p.y + row, label, dim(pal));
    }
}

/// The bars, colored along the fill by their height on the axis, with the
/// cursor bar in the bright title color.
fn draw_bars(buf: &mut Buffer, plot: &Plot, view: &View, pal: &Palette) {
    let geo = &plot.geo;
    let p = geo.plot;
    for (i, bar) in plot.bars.iter().enumerate() {
        let value = bar.value(plot.metric);
        let levels = plot.axis.levels(value, p.height);
        let pct = value / plot.axis.max * 100.0;
        let color = if view.cursor == Some(i) && !pal.mono {
            pal.color(pal.title)
        } else {
            pal.graded(view.fill, pct)
        };
        let x0 = geo.x_of(i);
        for col in 0..geo.bar_width(i) {
            let x = x0 + col;
            if x >= p.x + p.width {
                break;
            }
            for r in 0..p.height {
                let eighths = levels.saturating_sub(usize::from(r) * 8).min(8);
                if eighths == 0 {
                    break;
                }
                let y = p.y + p.height - 1 - r;
                buf[(x, y)]
                    .set_char(EIGHTHS[eighths - 1])
                    .set_style(Style::default().fg(color));
            }
        }
    }
}

/// The glyphs that tell stacked models apart without color, by slot.
pub const PATTERNS: [char; 8] = [
    '\u{2588}', '\u{2593}', '\u{2592}', '\u{2591}', '\u{259a}', '\u{259e}', '\u{259b}', '\u{259f}',
];

/// A bar's models as runs of eighths, bottom up: the slot and where the run
/// ends. The biggest model comes first and the last run ends at `levels`.
fn runs(bar: &Bar, slots: &Slots, levels: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut sum = 0.0;
    let mut prev = 0;
    for name in &slots.order {
        let cost = bar
            .models
            .iter()
            .find(|(m, _)| m == name)
            .map_or(0.0, |(_, c)| *c);
        if cost <= 0.0 {
            continue;
        }
        sum += cost;
        let end = ((levels as f64 * sum / bar.cost).round() as usize).min(levels);
        if end > prev {
            out.push((slots.slot(name), end));
            prev = end;
        }
    }
    // Rounding can stop short of the top: the last model takes the rest.
    match out.last_mut() {
        Some(last) => last.1 = levels,
        None if levels > 0 => out.push((0, levels)),
        None => {}
    }
    out
}

/// Bars stacked by model: each run of eighths in its model's color. A cell
/// that holds the edge between two models is one block in the lower color on
/// the upper one, so the edge is exact to an eighth of a cell. Without color
/// the models are told apart by pattern.
fn draw_stacked(buf: &mut Buffer, plot: &Plot, slots: &Slots, pal: &Palette) {
    let geo = &plot.geo;
    let p = geo.plot;
    for (i, bar) in plot.bars.iter().enumerate() {
        let levels = plot.axis.levels(bar.cost, p.height);
        let runs = runs(bar, slots, levels);
        let start_of = |k: usize| if k == 0 { 0 } else { runs[k - 1].1 };
        for r in 0..p.height {
            let lo = usize::from(r) * 8;
            if lo >= levels {
                break;
            }
            let hi = (lo + 8).min(levels);
            // How much of this cell each run fills.
            let overlap = |k: usize| hi.min(runs[k].1).saturating_sub(lo.max(start_of(k)));
            let inside: Vec<usize> = (0..runs.len()).filter(|k| overlap(*k) > 0).collect();
            let main = inside
                .iter()
                .copied()
                .max_by_key(|k| (overlap(*k), std::cmp::Reverse(*k)))
                .unwrap_or(0);
            let eighths = hi - lo;
            let (glyph, style) = match (pal.mono, inside.as_slice()) {
                (true, _) if eighths == 8 => {
                    (PATTERNS[runs[main].0 % PATTERNS.len()], Style::default())
                }
                (true, _) => (EIGHTHS[eighths - 1], Style::default()),
                // An edge inside a full cell: lower run as the block, upper as the ground.
                (false, [low, high]) if eighths == 8 => {
                    let edge = runs[*low].1 - lo;
                    let style = Style::default()
                        .fg(pal.color(pal.series[runs[*low].0 % SERIES]))
                        .bg(pal.color(pal.series[runs[*high].0 % SERIES]));
                    (EIGHTHS[edge - 1], style)
                }
                (false, _) => (
                    EIGHTHS[eighths - 1],
                    Style::default().fg(pal.color(pal.series[runs[main].0 % SERIES])),
                ),
            };
            let y = p.y + p.height - 1 - r;
            for col in 0..geo.bar_width(i) {
                let x = geo.x_of(i) + col;
                if x < p.x + p.width {
                    buf[(x, y)].set_char(glyph).set_style(style);
                }
            }
        }
    }
}

/// The ink of a model's series color, for the legend and the Models table.
pub fn series_ink(pal: &Palette, slot: usize) -> Ink {
    pal.series[slot % SERIES]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{ColorDepth, Theme};

    fn pal() -> Palette {
        Theme::Standard.palette(ColorDepth::True)
    }

    /// 2026-08-05, a Wednesday.
    const FIRST: i64 = 20_670;

    fn series(costs: &[f64]) -> Vec<DayPoint> {
        costs
            .iter()
            .enumerate()
            .map(|(i, c)| DayPoint {
                day: FIRST + i as i64,
                cost_usd: *c,
                tokens: (*c * 1000.0) as u64,
                messages: usize::from(*c > 0.0),
                models: if *c > 0.0 {
                    vec![("claude-opus-5".to_string(), *c)]
                } else {
                    Vec::new()
                },
            })
            .collect()
    }

    fn draw(panel: Rect, series: &[DayPoint], cursor: Option<usize>) -> (Plot, Buffer) {
        draw_with(panel, series, cursor, None, &pal())
    }

