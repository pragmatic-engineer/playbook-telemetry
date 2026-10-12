// SPDX-FileCopyrightText: 2026 Igor Santos
// SPDX-License-Identifier: Apache-2.0

//! The terminal view's state and what each key does. Pure: no terminal, no
//! store, so every key is a unit test.

use super::chart::Plot;
use super::data::Data;
use super::keys::{self, Cmd};
use super::panels;
use super::sort::{
    day_rows, group_rows, pair_rows, session_day_rows, session_rows, Panel, Sort, PANELS,
};
use super::theme::{ColorDepth, Theme};
use super::view::{self, help_height};
use super::worker::DEFAULT_REFRESH_SECONDS;
use crate::aggregate::SECONDS_PER_DAY;
use crate::period::pair_groups;
use crate::query::{Range, WeekStart};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use std::time::Duration;

/// Every range the view can show: the calendar ranges from the shortest to
/// the longest, then the rolling ones.
pub fn ranges(week: WeekStart) -> [Range; 9] {
    [
        Range::Today,
        Range::ThisWeek(week),
        Range::LastWeek(week),
        Range::CurrentMonth,
        Range::LastMonth,
        Range::LastDays(30),
        Range::LastDays(60),
        Range::LastDays(90),
        Range::All,
    ]
}

/// The two ranges `r` steps between. Both are periods, so the view shows each
/// against the one before it. The month is reached with `m`.
pub fn default_ranges(week: WeekStart) -> [Range; 2] {
    [Range::Today, Range::ThisWeek(week)]
}

/// What the spend panel shows. `v` steps through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpendView {
    /// Cost per day, one color along the gradient.
    #[default]
    Total,
    /// Cost per day stacked by model.
    ByModel,
    /// A sortable table with a row for each day.
    Daily,
}

impl SpendView {
    pub fn name(self) -> &'static str {
        match self {
            SpendView::Total => "total",
            SpendView::ByModel => "by model",
            SpendView::Daily => "daily table",
        }
    }

    pub fn next(self) -> SpendView {
        match self {
            SpendView::Total => SpendView::ByModel,
            SpendView::ByModel => SpendView::Daily,
            SpendView::Daily => SpendView::Total,
        }
    }
}

/// What the projects panel shows. `v` switches it while the panel is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectsView {
    #[default]
    Projects,
    /// Cost per agent, from telemetry.
    Agents,
    /// Cost per skill, from telemetry.
    Skills,
    /// Tool latency and error rate per tool, from telemetry.
    Tools,
    /// API errors, retries, cache hit rate and busy time, from telemetry.
    Health,
}

impl ProjectsView {
    pub fn name(self) -> &'static str {
        match self {
            ProjectsView::Projects => "projects",
            ProjectsView::Agents => "agents",
            ProjectsView::Skills => "skills",
            ProjectsView::Tools => "tools",
            ProjectsView::Health => "health",
        }
    }

    pub fn next(self) -> ProjectsView {
        match self {
            ProjectsView::Projects => ProjectsView::Agents,
            ProjectsView::Agents => ProjectsView::Skills,
            ProjectsView::Skills => ProjectsView::Tools,
            ProjectsView::Tools => ProjectsView::Health,
            ProjectsView::Health => ProjectsView::Projects,
        }
    }
}

#[derive(Debug)]
pub struct App {
    pub data: Option<Data>,
    pub range: Range,
    /// The day the chart cursor is on, if it was moved or hovered. Both
    /// charts share it, whatever their bar widths.
    pub cursor: Option<i64>,
    /// What the spend panel shows.
    pub spend_view: SpendView,
    /// What the projects panel shows.
    pub projects_view: ProjectsView,
    /// The first day of the week for the week ranges.
    pub week_start: WeekStart,
    pub filter: String,
    /// The last store error, shown in the header until the next good refresh.
    pub error: Option<String>,
    pub quit: bool,
    pub theme: Theme,
    pub depth: ColorDepth,
    /// `NO_COLOR` is set: the theme stays `mono`.
    pub no_color: bool,
    /// True while the filter is being typed; keys then edit `draft`.
    pub editing: bool,
    pub draft: String,
    /// The panel the keys act on.
    pub focus: Panel,
    /// The selected row of each panel, by `Panel::index`.
    pub selected: [usize; 6],
    pub sorts: [Sort; 6],
    /// The screen size, for laying out the charts the cursor keys move over.
    pub area: Rect,
    /// The help window, with the line it is scrolled to, when it is open.
    pub help: Option<usize>,
    /// Files read and files to read while a first read runs.
    pub indexing: Option<(usize, usize)>,
    /// Where transcripts are read from, named in the empty states.
    pub sources: String,
    /// How telemetry is configured, named in the footer.
    pub telemetry: Option<crate::telemetry_status::Mode>,
    /// How often the snapshot is refreshed, shown in the footer.
    pub refresh_every: Duration,
}

/// What the loop must do after a key.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// The range or filter changed: re-query now.
    Requery,
    /// Open the Sessions view.
    Sessions,
}

impl App {
    pub fn new(range: Range) -> App {
        App {
            data: None,
            range,
            cursor: None,
            spend_view: SpendView::default(),
            projects_view: ProjectsView::default(),
            week_start: WeekStart::default(),
            filter: String::new(),
            error: None,
            quit: false,
            theme: Theme::default(),
            depth: ColorDepth::default(),
            no_color: false,
            editing: false,
            draft: String::new(),
            focus: Panel::Models,
            selected: [0; 6],
            sorts: PANELS.map(Panel::default_sort),
            area: Rect::new(0, 0, 100, 30),
            help: None,
            indexing: None,
            sources: String::new(),
            telemetry: None,
            refresh_every: Duration::from_secs(DEFAULT_REFRESH_SECONDS),
        }
    }

    pub fn sort(&self, panel: Panel) -> Sort {
        self.sorts[panel.index()]
    }

    /// Whether the range is a period shown against the one before it.
    pub fn compared(&self) -> bool {
        self.data.as_ref().is_some_and(|d| d.comparison.is_some())
    }

    /// How many rows the panel's table has.
    fn rows(&self, panel: Panel) -> usize {
        let Some(d) = &self.data else { return 0 };
        match panel {
            Panel::Models => pair_groups(&d.models, &d.models_before).len(),
            Panel::Projects if self.projects_view == ProjectsView::Agents => d.agents.rows.len(),
            Panel::Projects if self.projects_view == ProjectsView::Skills => d.skills.rows.len(),
            Panel::Projects if self.projects_view == ProjectsView::Tools => d.health.tools.len(),
            Panel::Projects if self.projects_view == ProjectsView::Health => panels::HEALTH_ROWS,
            Panel::Projects => d.repos.len(),
            Panel::Live => d.live.active.len(),
            Panel::Sessions => d.sessions.len(),
            Panel::Spend if self.spend_view == SpendView::Daily => {
                let today = d.now.div_euclid(SECONDS_PER_DAY);
                d.series.iter().filter(|p| p.day <= today).count()
            }
            Panel::Spend | Panel::Tokens => 0,
        }
    }

    /// Whether the panel is showing a table right now.
    pub fn is_table(&self, panel: Panel) -> bool {
        match panel {
            Panel::Spend => self.spend_view == SpendView::Daily,
            Panel::Tokens => false,
            _ => true,
        }
    }

    /// The selected row of `panel`, kept inside its rows.
    pub fn selection(&self, panel: Panel) -> usize {
        self.selected[panel.index()].min(self.rows(panel).saturating_sub(1))
    }

    fn select(&mut self, panel: Panel, to: usize) {
        self.selected[panel.index()] = to.min(self.rows(panel).saturating_sub(1));
    }

    fn step(&mut self, panel: Panel, by: isize) {
        let at = self.selection(panel) as isize;
        self.select(panel, (at + by).max(0) as usize);
    }

    /// The row keys of a table in the order it is drawn: what a row is, so a
    /// refresh can find the selected row again after the rows moved.
    fn row_ids(&self, panel: Panel) -> Vec<String> {
        let Some(d) = &self.data else {
            return Vec::new();
        };
        let sort = self.sort(panel);
        match panel {
            Panel::Models => pair_rows(&d.models, &d.models_before, sort)
                .into_iter()
                .map(|p| p.key)
                .collect(),
            Panel::Projects => {
                let groups = match self.projects_view {
                    ProjectsView::Agents => &d.agents.rows,
                    ProjectsView::Skills => &d.skills.rows,
                    ProjectsView::Projects => &d.repos,
                    // Fixed order, one row per tool or figure.
                    ProjectsView::Tools => {
                        return d.health.tools.iter().map(|t| t.name.clone()).collect()
                    }
                    ProjectsView::Health => return Vec::new(),
                };
                group_rows(groups, sort)
                    .iter()
                    .map(|g| g.key.clone())
                    .collect()
            }
            Panel::Live => session_rows(&d.live.active, sort)
                .iter()
                .map(|s| s.id.clone())
                .collect(),
            Panel::Sessions => session_day_rows(&d.sessions, sort)
                .iter()
                .map(|s| format!("{}/{}", s.day, s.id))
                .collect(),
            Panel::Spend if self.spend_view == SpendView::Daily => {
                let today = d.now.div_euclid(SECONDS_PER_DAY);
                day_rows(&d.series, today, sort)
                    .iter()
                    .map(|p| p.day.to_string())
                    .collect()
            }
            Panel::Spend | Panel::Tokens => Vec::new(),
        }
    }

    /// The text that filters the view to the selected row: its model, project
    /// or session id.
    fn selected_key(&self) -> Option<String> {
        let d = self.data.as_ref()?;
        let panel = self.focus;
        let at = self.selection(panel);
        let sort = self.sort(panel);
        match panel {
            Panel::Models => pair_rows(&d.models, &d.models_before, sort)
                .into_iter()
                .nth(at)
                .map(|p| p.key),
            // An agent, skill, tool or figure is not a field of a usage event, so it cannot filter.
            Panel::Projects if self.projects_view != ProjectsView::Projects => None,
            Panel::Projects => group_rows(&d.repos, sort).get(at).map(|g| g.key.clone()),
            Panel::Live => session_rows(&d.live.active, sort)
                .get(at)
                .map(|s| s.id.clone()),
            Panel::Sessions => session_day_rows(&d.sessions, sort)
                .get(at)
                .map(|s| s.id.clone()),
            Panel::Spend | Panel::Tokens => None,
        }
    }

    /// Takes a fresh snapshot without moving anything the user set: each
    /// table's selected row is found again by what it is, so the selection
    /// and the scroll that follows it stay on the same row when the rows
    /// reorder. The cursor, focus, sorts and view are not touched.
    pub fn refresh(&mut self, data: Data) {
        let kept: Vec<Option<String>> = PANELS
            .iter()
            .map(|p| self.row_ids(*p).into_iter().nth(self.selection(*p)))
            .collect();
        self.data = Some(data);
        self.error = None;
        for (panel, id) in PANELS.iter().zip(kept) {
            if let Some(at) = id.and_then(|id| self.row_ids(*panel).iter().position(|r| *r == id)) {
                self.selected[panel.index()] = at;
            }
        }
    }

    fn focus_next(&mut self, by: isize) {
        let at = self.focus.index() as isize;
        self.focus = PANELS[(at + by).rem_euclid(PANELS.len() as isize) as usize];
    }

    /// The laid out chart of a chart panel, once there is data to chart.
    fn plot(&self, panel: Panel) -> Option<Plot> {
        let data = self.data.as_ref()?;
        if data.series.is_empty()
            || (panel == Panel::Spend && self.spend_view == SpendView::Daily)
            || self.area.width < view::MIN_WIDTH
            || self.area.height < view::MIN_HEIGHT
        {
            return None;
        }
        let p = view::layout(self.area);
        let area = if panel == Panel::Tokens {
            p.tokens
        } else {
            p.spend
        };
        Some(panels::chart_plot(area, data, panel))
    }

    /// Moves the chart cursor by `by` bars, or to an end of the chart with
    /// `isize::MIN` or `isize::MAX` steps. With no cursor yet it starts from today's bar.
    fn move_cursor(&mut self, by: isize) {
        let Some(plot) = self.plot(Panel::Spend) else {
            return;
        };
        let last = plot.bars.len().saturating_sub(1);
        let today = self
            .data
            .as_ref()
            .map_or(0, |d| d.now.div_euclid(SECONDS_PER_DAY));
        let at = self
            .cursor
            .and_then(|d| plot.index_of(d))
            .or_else(|| plot.default_index(today))
            .unwrap_or(last);
        let to = at.saturating_add_signed(by).min(last);
        self.cursor = plot.bars.get(to).map(|b| b.first);
    }

    /// Switches the range, re-querying only when it changed.
    fn set_range(&mut self, range: Range) -> Effect {
        if range == self.range {
            return Effect::None;
        }
        self.range = range;
        self.cursor = None;
        Effect::Requery
    }

    pub fn key(&mut self, key: KeyEvent) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.editing {
            return self.edit(key.code, ctrl);
        }
        if self.help.is_some() {
            return self.help_key(key.code, ctrl);
        }
        match keys::lookup(key.code, ctrl) {
            Some(binding) => self.run(binding.cmd, key.code),
            None => Effect::None,
        }
    }

    /// Keys while the help window is open: the arrows scroll it, Ctrl-C quits
    /// and any other key closes it. The data keeps refreshing underneath.
    fn help_key(&mut self, code: KeyCode, ctrl: bool) -> Effect {
        let at = self.help.unwrap_or(0);
        let page = self.help_page();
        let to = match code {
            KeyCode::Down => Some(at + 1),
            KeyCode::Up => Some(at.saturating_sub(1)),
            KeyCode::PageDown => Some(at + page),
            KeyCode::PageUp => Some(at.saturating_sub(page)),
            KeyCode::Home => Some(0),
            KeyCode::End => Some(usize::MAX),
            _ => None,
        };
        match to {
            Some(to) => self.help = Some(to.min(self.help_scroll_max())),
            None if ctrl && code == KeyCode::Char('c') => self.quit = true,
            None => self.help = None,
        }
        Effect::None
    }

    /// Help lines that fit in the window.
    pub fn help_page(&self) -> usize {
        usize::from(help_height(self.area.height).saturating_sub(2)).max(1)
    }

    fn help_scroll_max(&self) -> usize {
        keys::help_lines().len().saturating_sub(self.help_page())
    }

    /// Runs one command.
    fn run(&mut self, cmd: Cmd, code: KeyCode) -> Effect {
        let focus = self.focus;
        match cmd {
            Cmd::Quit => self.quit = true,
            Cmd::Theme if !self.no_color => self.theme = self.theme.next(),
            Cmd::Theme => {}
            Cmd::Today => return self.set_range(Range::Today),
            Cmd::Week => return self.set_range(Range::ThisWeek(self.week_start)),
            Cmd::LastWeek => return self.set_range(Range::LastWeek(self.week_start)),
            Cmd::Month => return self.set_range(Range::CurrentMonth),
            Cmd::LastMonth => return self.set_range(Range::LastMonth),
            Cmd::All => return self.set_range(Range::All),
            Cmd::CycleRange => {
                let all = default_ranges(self.week_start);
                // From any other range, `r` goes to the first of the two.
                let next = match all.iter().position(|r| *r == self.range) {
                    Some(at) => all[(at + 1) % all.len()],
                    None => all[0],
                };
                return self.set_range(next);
            }
            Cmd::Filter => {
                self.editing = true;
                self.draft = self.filter.clone();
            }
            Cmd::ClearFilter if !self.filter.is_empty() => {
                self.filter.clear();
                return Effect::Requery;
            }
            Cmd::ClearFilter => {}
            Cmd::FocusPanel => {
                if let KeyCode::Char(c @ '1'..='6') = code {
                    self.focus = PANELS[usize::from(c as u8 - b'1')];
                }
            }
            Cmd::NextPanel => self.focus_next(1),
            Cmd::PrevPanel => self.focus_next(-1),
            Cmd::CursorLeft => self.move_cursor(-1),
            Cmd::CursorRight => self.move_cursor(1),
            Cmd::First if focus.is_chart() && !self.is_table(focus) => {
                self.move_cursor(isize::MIN);
            }
            Cmd::Last if focus.is_chart() && !self.is_table(focus) => {
                self.move_cursor(isize::MAX);
            }
            Cmd::First => self.select(focus, 0),
            Cmd::Last => self.select(focus, usize::MAX),
            Cmd::Down => self.step(focus, 1),
            Cmd::Up => self.step(focus, -1),
            Cmd::PageDown => self.step(focus, 5),
            Cmd::PageUp => self.step(focus, -5),
            Cmd::NextView => match focus {
                Panel::Projects => self.projects_view = self.projects_view.next(),
                Panel::Spend => self.spend_view = self.spend_view.next(),
                _ => {}
            },
            Cmd::Sort if self.is_table(focus) => {
                self.sorts[focus.index()] = self.sort(focus).next(focus, self.compared());
            }
            Cmd::SortFlip if self.is_table(focus) => {
                self.sorts[focus.index()] = self.sort(focus).flip();
            }
            Cmd::Sort | Cmd::SortFlip => {}
            Cmd::Pick => {
                // Enter on the row already used as the filter lifts it.
                if let Some(key) = self.selected_key() {
                    self.filter = if self.filter == key {
                        String::new()
                    } else {
                        key
                    };
                    return Effect::Requery;
                }
            }
            Cmd::Sessions => return Effect::Sessions,
            Cmd::Help => self.help = Some(0),
            Cmd::Refresh => return Effect::Requery,
        }
        Effect::None
    }

    /// Keys while the filter is being typed. Enter applies, Esc drops it.
    fn edit(&mut self, code: KeyCode, ctrl: bool) -> Effect {
        match code {
            KeyCode::Enter => {
                self.editing = false;
                if self.draft != self.filter {
                    self.filter = std::mem::take(&mut self.draft);
                    return Effect::Requery;
                }
            }
            KeyCode::Esc => self.editing = false,
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Backspace => {
                self.draft.pop();
            }
            KeyCode::Char(c) if !ctrl => self.draft.push(c),
            _ => {}
        }
        Effect::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn q_esc_and_ctrl_c_quit() {
        for key in [
            press(KeyCode::Char('q')),
            press(KeyCode::Esc),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let mut app = App::new(Range::All);
            app.key(key);
            assert!(app.quit);
        }
    }

    #[test]
    fn r_steps_between_today_and_the_week_and_starts_over_from_any_other_range() {
        let mut app = App::new(Range::Today);
        assert_eq!(app.key(press(KeyCode::Char('r'))), Effect::Requery);
        assert_eq!(app.range, Range::ThisWeek(WeekStart::Monday));
        assert_eq!(app.key(press(KeyCode::Char('r'))), Effect::Requery);
        assert_eq!(app.range, Range::Today);
        for other in [Range::CurrentMonth, Range::LastDays(30), Range::All] {
            app.range = other;
            app.key(press(KeyCode::Char('r')));
            assert_eq!(app.range, Range::Today, "{other:?}");
        }
    }

    #[test]
    fn direct_keys_jump_to_a_range_and_requery_only_on_a_change() {
        let mut app = App::new(Range::LastDays(30));
        app.week_start = WeekStart::Sunday;
        for (key, want) in [
            ('d', Range::Today),
            ('w', Range::ThisWeek(WeekStart::Sunday)),
            ('W', Range::LastWeek(WeekStart::Sunday)),
            ('m', Range::CurrentMonth),
            ('M', Range::LastMonth),
            ('a', Range::All),
        ] {
            assert_eq!(app.key(press(KeyCode::Char(key))), Effect::Requery, "{key}");
            assert_eq!(app.range, want, "{key}");
            assert_eq!(app.key(press(KeyCode::Char(key))), Effect::None, "{key}");
        }
    }

    fn type_filter(app: &mut App, text: &str) {
        app.key(press(KeyCode::Char('/')));
        for c in text.chars() {
            app.key(press(KeyCode::Char(c)));
        }
    }

    #[test]
    fn slash_types_a_filter_and_enter_applies_it() {
        let mut app = App::new(Range::All);
        type_filter(&mut app, "opus");
        assert!(app.editing);
        assert_eq!(app.filter, "");
        assert_eq!(app.key(press(KeyCode::Enter)), Effect::Requery);
        assert_eq!((app.editing, app.filter.as_str()), (false, "opus"));
    }

    #[test]
    fn typing_q_or_r_while_editing_is_text_not_a_command() {
        let mut app = App::new(Range::All);
        type_filter(&mut app, "qr");
        assert!(!app.quit);
        assert_eq!(app.range, Range::All);
        assert_eq!(app.draft, "qr");
    }

    #[test]
    fn backspace_edits_and_esc_cancels_without_a_requery() {
        let mut app = App::new(Range::All);
        type_filter(&mut app, "abc");
        app.key(press(KeyCode::Backspace));
        assert_eq!(app.draft, "ab");
        assert_eq!(app.key(press(KeyCode::Esc)), Effect::None);
        assert!(!app.editing && !app.quit);
        assert_eq!(app.filter, "");
    }

    #[test]
    fn enter_on_an_unchanged_filter_does_not_requery() {
        let mut app = App::new(Range::All);
        app.key(press(KeyCode::Char('/')));
        assert_eq!(app.key(press(KeyCode::Enter)), Effect::None);
    }

    #[test]
    fn editing_an_existing_filter_starts_from_it() {
        let mut app = App::new(Range::All);
        app.filter = "sonnet".into();
        app.key(press(KeyCode::Char('/')));
        assert_eq!(app.draft, "sonnet");
    }

    #[test]
    fn c_clears_a_filter_and_requeries_only_when_one_is_set() {
        let mut app = App::new(Range::All);
        assert_eq!(app.key(press(KeyCode::Char('c'))), Effect::None);
        app.filter = "x".into();
        assert_eq!(app.key(press(KeyCode::Char('c'))), Effect::Requery);
        assert_eq!(app.filter, "");
    }

    #[test]
    fn t_cycles_the_theme() {
        let mut app = App::new(Range::All);
        assert_eq!(app.theme, Theme::Standard);
        app.key(press(KeyCode::Char('t')));
        assert_eq!(app.theme, Theme::Dark);
    }

    #[test]
    fn no_color_keeps_the_theme_locked() {
        let mut app = App::new(Range::All);
        app.no_color = true;
        app.theme = Theme::Mono;
        app.key(press(KeyCode::Char('t')));
        assert_eq!(app.theme, Theme::Mono);
    }

    /// A snapshot of three models, one session each, with these costs.
    fn data_with(costs: [f64; 3]) -> Data {
        use crate::UsageEvent;
        let ev = |id: &str, model: &str, repo: &str, cost: f64| UsageEvent {
            event_id: id.into(),
            timestamp: 1_788_400_000 - 30,
            session_id: format!("sess-{id}"),
            model: model.into(),
            repo: repo.into(),
            cost_usd: cost,
            ..UsageEvent::default()
        };
        let events = [
            ev("a", "opus", "alpha", costs[0]),
            ev("b", "sonnet", "beta", costs[1]),
            ev("c", "haiku", "gamma", costs[2]),
        ];
        let live = crate::query::live_view(&events, &events, &events, 1_788_400_000);
        Data::build(&events, live, 1_788_400_000, Range::All, "")
    }

    fn loaded() -> App {
        let mut app = App::new(Range::All);
        app.data = Some(data_with([5.0, 1.0, 3.0]));
        app
    }

    #[test]
    fn a_refresh_keeps_the_selected_row_when_the_rows_reorder() {
        let mut app = loaded();
        app.key(press(KeyCode::Char('3')));
        app.key(press(KeyCode::Down));
        // Models by cost: opus 5, haiku 3, sonnet 1, so haiku is selected.
        assert_eq!(app.selected_key().as_deref(), Some("haiku"));
        app.cursor = Some(NOW.div_euclid(86_400));
        app.spend_view = SpendView::ByModel;
        app.error = Some("old error".into());

        // Haiku now costs the most, so it moves to the top of the table.
        app.refresh(data_with([5.0, 1.0, 9.0]));

        assert_eq!(app.selected_key().as_deref(), Some("haiku"));
        assert_eq!(app.selection(Panel::Models), 0);
        assert_eq!(app.focus, Panel::Models);
        assert_eq!(app.cursor, Some(NOW.div_euclid(86_400)));
        assert_eq!(app.spend_view, SpendView::ByModel);
        assert_eq!(app.sort(Panel::Models), Panel::Models.default_sort());
        assert_eq!(app.error, None);
    }

    #[test]
    fn a_refresh_keeps_the_selection_of_every_table_and_the_filter_draft() {
        let mut app = loaded();
        for panel in ['4', '5', '6'] {
            app.key(press(KeyCode::Char(panel)));
            app.key(press(KeyCode::Down));
        }
        let before: Vec<usize> = PANELS.iter().map(|p| app.selection(*p)).collect();
        app.key(press(KeyCode::Char('/')));
        app.key(press(KeyCode::Char('x')));

        app.refresh(data_with([5.0, 1.0, 3.0]));

        let after: Vec<usize> = PANELS.iter().map(|p| app.selection(*p)).collect();
        assert_eq!(before, after);
        assert!(app.editing);
        assert_eq!(app.draft, "x");
    }

    #[test]
    fn a_refresh_whose_selected_row_is_gone_keeps_the_index_inside_the_rows() {
        let mut app = loaded();
        app.key(press(KeyCode::Char('3')));
        app.key(press(KeyCode::End));
        assert_eq!(app.selection(Panel::Models), 2);
        // A snapshot with no models at all.
        app.refresh(Data::build(
            &[],
            crate::query::LiveView::default(),
            1_788_400_000,
            Range::All,
            "",
        ));
        assert_eq!(app.selection(Panel::Models), 0);
        assert_eq!(app.focus, Panel::Models);
    }

    #[test]
    fn the_models_table_lists_the_models_of_both_periods_and_sorts_by_the_change() {
        use crate::query::LiveView;
        use crate::UsageEvent;
        let ev = |ts: i64, model: &str, cost: f64| UsageEvent {
            event_id: format!("{model}{ts}"),
            timestamp: ts,
            session_id: "s".into(),
            model: model.into(),
            cost_usd: cost,
            ..UsageEvent::default()
        };
        let events = [
            ev(NOW - 60, "opus", 5.0),
            ev(NOW - 86_400 - 60, "opus", 4.0),
            ev(NOW - 86_400 - 30, "haiku", 3.0),
        ];
        let mut app = App::new(Range::Today);
        app.data = Some(Data::build(
            &events,
            LiveView::default(),
            NOW,
            Range::Today,
            "",
        ));
        // Haiku had no cost now but still has a row, and the table is compared.
        assert!(app.compared());
        assert_eq!(app.rows(Panel::Models), 2);
        assert_eq!(app.row_ids(Panel::Models), ["opus", "haiku"]);
        // `s` reaches the previous cost and the change.
        app.key(press(KeyCode::Char('3')));
        app.key(press(KeyCode::Char('s')));
        assert_eq!(app.sort(Panel::Models).col, 4);
        app.key(press(KeyCode::Char('s')));
        assert_eq!(app.sort(Panel::Models).col, 5);
        // Biggest rise first: opus +1, haiku -3.
        assert_eq!(app.row_ids(Panel::Models), ["opus", "haiku"]);
        app.key(press(KeyCode::Char('S')));
        assert_eq!(app.row_ids(Panel::Models), ["haiku", "opus"]);
        // Another range has no comparison and only the first four columns.
        app.range = Range::All;
        app.data = Some(Data::build(
            &events,
            LiveView::default(),
            NOW,
            Range::All,
            "",
        ));
        assert!(!app.compared());
        app.sorts[Panel::Models.index()] = Sort::new(3, true);
        app.key(press(KeyCode::Char('s')));
        assert_eq!(app.sort(Panel::Models).col, 0);
    }

    #[test]
    fn number_keys_and_tab_move_the_focus() {
        let mut app = App::new(Range::All);
        app.key(press(KeyCode::Char('5')));
        assert_eq!(app.focus, Panel::Live);
        app.key(press(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Sessions);
        app.key(press(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Spend);
        app.key(press(KeyCode::BackTab));
        assert_eq!(app.focus, Panel::Sessions);
        app.key(press(KeyCode::Char('7')));
        assert_eq!(app.focus, Panel::Sessions);
    }

    #[test]
    fn arrows_move_the_selection_and_stay_inside_the_rows() {
        let mut app = loaded();
        app.key(press(KeyCode::Char('3')));
        app.key(press(KeyCode::Down));
        app.key(press(KeyCode::Char('j')));
        assert_eq!(app.selection(Panel::Models), 2);
        app.key(press(KeyCode::Down));
        assert_eq!(app.selection(Panel::Models), 2);
        app.key(press(KeyCode::Up));
        assert_eq!(app.selection(Panel::Models), 1);
        app.key(press(KeyCode::Home));
        assert_eq!(app.selection(Panel::Models), 0);
        app.key(press(KeyCode::End));
        assert_eq!(app.selection(Panel::Models), 2);
    }

    #[test]
    fn s_sorts_the_focused_table_and_capital_s_reverses_it() {
        let mut app = loaded();
        app.key(press(KeyCode::Char('3')));
        assert_eq!(app.sort(Panel::Models), Sort::new(3, true));
        app.key(press(KeyCode::Char('s')));
        assert_eq!(app.sort(Panel::Models), Sort::new(0, false));
        app.key(press(KeyCode::Char('S')));
        assert_eq!(app.sort(Panel::Models), Sort::new(0, true));
        // Other panels keep their own sort.
        assert_eq!(app.sort(Panel::Projects), Sort::new(3, true));
        // A graph has nothing to sort.
        app.key(press(KeyCode::Char('1')));
        app.key(press(KeyCode::Char('S')));
        assert_eq!(app.sort(Panel::Spend), Panel::Spend.default_sort());
    }

    #[test]
    fn enter_filters_to_the_selected_row_and_again_lifts_it() {
        let mut app = loaded();
        app.key(press(KeyCode::Char('3')));
        app.key(press(KeyCode::Down));
        // Models by cost: opus 5, haiku 3, sonnet 1.
        assert_eq!(app.key(press(KeyCode::Enter)), Effect::Requery);
        assert_eq!(app.filter, "haiku");
        assert_eq!(app.key(press(KeyCode::Enter)), Effect::Requery);
        assert_eq!(app.filter, "");
    }

    #[test]
    fn enter_on_a_session_filters_by_its_id() {
        let mut app = loaded();
        app.key(press(KeyCode::Char('5')));
        assert_eq!(app.key(press(KeyCode::Enter)), Effect::Requery);
        assert!(app.filter.starts_with("sess-"), "{}", app.filter);
        // A graph has no row to pick.
        app.filter.clear();
        app.key(press(KeyCode::Char('1')));
        assert_eq!(app.key(press(KeyCode::Enter)), Effect::None);
    }

    const NOW: i64 = 1_788_400_000;
    const DAY: i64 = 86_400;

