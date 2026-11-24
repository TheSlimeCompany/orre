//! UI tests for the Overview: the shell's Overview tab in a headless
//! window, on [`fixture`]'s sample data, and on a scratch database for the
//! account filter. What the screen shows is read through its test-only
//! `describe()` ([`super::describe`]).

use chrono::NaiveDate;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use orre_core::db::Database;
use orre_core::reporting::Period;
use orre_core::reporting::overview::{OverviewQuery, overview as core_overview};
use tempfile::TempDir;

use super::describe::{Action, OverviewDescription, Shown};
use super::store::OverviewStore;
use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::inbox::Inbox;
use crate::inbox::store::InboxStore;
use crate::shell::{ShellView, View};

fn month(year: i32, month: u32) -> Period {
    Period::new(year, month).unwrap()
}

/// The shell's Overview tab showing `overview`, drawn.
fn open(
    cx: &mut TestAppContext,
    overview: OverviewView,
) -> (
    Entity<ShellView>,
    Entity<OverviewView>,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let mut shell = ShellView::new(View::Overview, window, cx);
        let overview = cx.new(|_| overview);
        shell.install_overview(overview, window, cx);
        shell
    });
    let overview = cx.update(|_, cx| shell.read(cx).overview().clone());
    redraw(cx);
    (shell, overview, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn described(overview: &Entity<OverviewView>, cx: &mut VisualTestContext) -> OverviewDescription {
    cx.update(|_, cx| overview.read(cx).describe(cx))
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn action(description: &OverviewDescription, label: &str) -> Action {
    description
        .actions
        .iter()
        .find(|action| action.label == label)
        .unwrap_or_else(|| panic!("no action {label}: {:?}", description.actions))
        .clone()
}

fn figures(description: &OverviewDescription) -> &describe::Figures {
    match &description.shown {
        Shown::Figures(figures) => figures,
        other => panic!("not the figures: {other:?}"),
    }
}

/// The sample data with October selected, as Paper draws it.
fn october(
    cx: &mut TestAppContext,
) -> (
    Entity<ShellView>,
    Entity<OverviewView>,
    &mut VisualTestContext,
) {
    let (shell, overview, cx) = open(cx, OverviewView::fixture());
    overview.update(cx, |overview, cx| {
        overview.select_month(fixture::october(), cx)
    });
    redraw(cx);
    (shell, overview, cx)
}

#[gpui_kit::test]
fn the_sample_shows_octobers_figures_and_what_changed(cx: &mut TestAppContext) {
    let (_, overview, cx) = october(cx);
    let description = described(&overview, cx);
    assert_eq!(description.month, "October 2026");
    let shown = figures(&description);
    assert_eq!(
        shown.figures,
        [
            ("Spent", "£2,481.90".to_owned(), "+8% vs Sep".to_owned()),
            ("Income", "£3,450.00".to_owned(), "Same as Sep".to_owned()),
            (
                "Net",
                "+£968.10".to_owned(),
                "\u{2212}£183.84 vs Sep".to_owned()
            ),
        ]
    );
    for id in [
        "overview-figure-spent",
        "overview-figure-income",
        "overview-figure-net",
        "chart-month-0",
        "overview-categories",
        "overview-recurring",
    ] {
        assert!(shows(id, cx), "{id}");
    }
}

#[gpui_kit::test]
fn the_chart_and_the_lists_follow_the_designs(cx: &mut TestAppContext) {
    let (_, overview, cx) = october(cx);
    let description = described(&overview, cx);
    let shown = figures(&description);
    assert_eq!(shown.chart_caption, "Last 12 months");
    assert_eq!(shown.months.len(), 12);
    let selected: Vec<_> = shown
        .months
        .iter()
        .filter(|month| month.selected)
        .map(|month| month.title.as_str())
        .collect();
    assert_eq!(selected, ["October 2026"]);
    assert_eq!(shown.months[0].label, "Dec");
    assert_eq!(shown.category_caption, "October");
    assert_eq!(shown.other_currencies, None);
    let labels: Vec<_> = shown
        .categories
        .iter()
        .map(|(label, share, amount)| (label.as_str(), share.as_str(), amount.as_str()))
        .collect();
    assert_eq!(
        labels,
        [
            ("Groceries", "25%", "£612.40"),
            ("Eating out", "18%", "£438.15"),
            ("Bills", "16%", "£402.60"),
            ("Shopping", "14%", "£356.98"),
            ("Transport", "9%", "£214.30"),
            ("Entertainment", "4%", "£98.67"),
            ("Other", "12%", "£300.27"),
            ("Uncategorised", "2%", "£58.53"),
        ]
    );
    // Recurring looks forward from the 14th, whichever month is shown.
    assert_eq!(shown.recurring[0].0, "Spotify");
    assert_eq!(shown.recurring[0].2, "15 Nov");
    assert_eq!(shown.recurring[0].3, "\u{2212}£11.99");
    for row in 0..shown.recurring.len() {
        assert!(shows(&format!("overview-recurring-row-{row}"), cx), "{row}");
    }
}

#[gpui_kit::test]
fn the_switcher_steps_by_month_and_stops_at_both_ends(cx: &mut TestAppContext) {
    let (_, overview, cx) = open(cx, OverviewView::fixture());
    // The sample day is in November: there is no later month.
    let description = described(&overview, cx);
    assert_eq!(description.month, "November 2026");
    assert!(!action(&description, "Next month").enabled);
    assert!(action(&description, "Previous month").enabled);
    click("overview-next-month", cx);
    assert_eq!(described(&overview, cx).month, "November 2026");

    click("overview-previous-month", cx);
    let description = described(&overview, cx);
    assert_eq!(description.month, "October 2026");
    assert!(action(&description, "Next month").enabled);
    assert_eq!(figures(&description).figures[0].1, "£2,481.90");

    click("overview-next-month", cx);
    assert_eq!(described(&overview, cx).month, "November 2026");

    // The sample's earliest month is December 2025.
    for _ in 0..11 {
        click("overview-previous-month", cx);
    }
    let description = described(&overview, cx);
    assert_eq!(description.month, "December 2025");
    assert!(!action(&description, "Previous month").enabled);
    click("overview-previous-month", cx);
    assert_eq!(described(&overview, cx).month, "December 2025");
    assert_eq!(overview.read_with(cx, |o, _| o.month()), month(2025, 12));
}

#[gpui_kit::test]
fn clicking_a_month_on_the_chart_selects_it(cx: &mut TestAppContext) {
    let (_, overview, cx) = october(cx);
    // The window starts in December 2025: the fourth band is March.
    click("chart-month-3", cx);
    let description = described(&overview, cx);
    assert_eq!(description.month, "March 2026");
    let shown = figures(&description);
    assert_eq!(shown.months[3].title, "March 2026");
    assert!(shown.months[3].selected);
    assert_eq!(shown.months.iter().filter(|m| m.selected).count(), 1);
    // March's income was £4,280.
    assert_eq!(shown.figures[1].1, "£4,280.00");
    assert_eq!(shown.category_caption, "March");
}

#[gpui_kit::test]
fn no_transactions_shows_the_empty_state(cx: &mut TestAppContext) {
    let (_, overview, cx) = open(cx, OverviewView::sample(fixture::empty));
    assert_eq!(
        described(&overview, cx).shown,
        Shown::Empty {
            title: "No transactions yet",
            shortcut: ("⌘I", "Import a statement"),
        }
    );
    assert!(!shows("overview-canvas", cx));
    assert!(!shows("overview-previous-month", cx));
}

#[gpui_kit::test]
fn other_currencies_are_said_not_to_be_included(cx: &mut TestAppContext) {
    let (_, overview, cx) = open(cx, OverviewView::sample(fixture::with_euros));
    let description = described(&overview, cx);
    assert_eq!(
        figures(&description).other_currencies.as_deref(),
        Some("Amounts in EUR aren't included")
    );
}

#[gpui_kit::test]
fn a_month_before_the_window_ends_the_chart_there(cx: &mut TestAppContext) {
    let (_, overview, cx) = open(cx, OverviewView::fixture());
    for _ in 0..11 {
        click("overview-previous-month", cx);
    }
    // December 2025 is the window's first month: the caption is unchanged.
    assert_eq!(
        figures(&described(&overview, cx)).chart_caption,
        "Last 12 months"
    );
    // A month before the window: the chart ends at it, says so, and it is
    // the selected month.
    overview.update(cx, |overview, cx| overview.select_month(month(2025, 6), cx));
    let description = described(&overview, cx);
    assert_eq!(figures(&description).chart_caption, "12 months to Jun 2025");
    overview.read_with(cx, |overview, _| {
        let data = overview.data().expect("read");
        assert_eq!(data.months.last().map(|m| m.period), Some(month(2025, 6)));
        assert_eq!(data.selected.period, month(2025, 6));
    });
}

// The account filter and the live reads, on a scratch database -----------------

struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    seeded: crate::rules::fixture::Seeded,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let seeded = crate::rules::fixture::seed(&mut db);
        Self {
            _dir: dir,
            path,
            seeded,
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 14).unwrap()
    }

    /// Not read yet, on [`Scratch::today`] whatever the clock says.
    fn view(&self) -> OverviewView {
        let mut view = OverviewView::unloaded(OverviewStore::new(self.path.clone()), Self::today());
        view.hold_day(Self::today());
        view
    }

    /// What core says for `account`.
    fn expected(
        &self,
        account: Option<orre_core::id::AccountId>,
    ) -> orre_core::reporting::overview::Overview {
        let db = Database::open(&self.path).unwrap();
        core_overview(
            &db.reader().unwrap(),
            &OverviewQuery {
                month: Period::containing(Self::today()),
                account,
                today: Self::today(),
            },
        )
        .unwrap()
    }
}

#[gpui_kit::test]
fn the_database_is_read_in_the_background_and_shown(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, overview, cx) = open(cx, scratch.view());
    // Nothing is shown until the first read lands.
    assert_eq!(described(&overview, cx).shown, Shown::Blank);
    overview.update(cx, |overview, cx| overview.reload(cx));
    redraw(cx);
    let expected = scratch.expected(None);
    assert_eq!(
        overview.read_with(cx, |o, _| o.data().cloned()),
        Some(expected)
    );
    assert!(matches!(described(&overview, cx).shown, Shown::Figures(_)));
}

#[gpui_kit::test]
fn the_accounts_panel_selection_filters_the_overview(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, overview, cx) = open(cx, scratch.view());
    overview.update(cx, |overview, cx| overview.reload(cx));
    redraw(cx);
    let all = overview.read_with(cx, |o, _| o.data().cloned().unwrap());
    assert_eq!(all, scratch.expected(None));

    let joint = scratch.seeded.joint;
    cx.update(|_, cx| {
        let accounts = shell.read(cx).accounts().clone();
        accounts.update(cx, |accounts, cx| accounts.select(Some(joint), cx));
    });
    redraw(cx);
    assert_eq!(overview.read_with(cx, |o, _| o.account()), Some(joint));
    let joint_only = overview.read_with(cx, |o, _| o.data().cloned().unwrap());
    assert_eq!(joint_only, scratch.expected(Some(joint)));
    assert_ne!(
        joint_only.selected, all.selected,
        "the filter changes the figures"
    );

    // Back to all accounts.
    cx.update(|_, cx| {
        let accounts = shell.read(cx).accounts().clone();
        accounts.update(cx, |accounts, cx| accounts.select(None, cx));
    });
    redraw(cx);
    assert_eq!(
        overview.read_with(cx, |o, _| o.data().cloned().unwrap()),
        all
    );
}

#[gpui_kit::test]
fn a_month_picked_while_a_read_is_in_flight_wins(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, overview, cx) = open(cx, scratch.view());
    overview.update(cx, |overview, cx| overview.reload(cx));
    redraw(cx);
    // Two months in a row without letting the first read land.
    overview.update(cx, |overview, cx| {
        overview.select_month(month(2026, 10), cx);
        overview.select_month(month(2026, 9), cx);
    });
    redraw(cx);
    let data = overview.read_with(cx, |o, _| o.data().cloned().unwrap());
    assert_eq!(data.selected.period, month(2026, 9));
}

#[gpui_kit::test]
fn picking_the_shown_month_while_a_landing_read_is_in_flight_wins(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let store = OverviewStore::new(scratch.path.clone());
    let (_, overview, cx) = open(cx, OverviewView::from_store(store, Scratch::today()));
    // October is what was shown before a new statement (November's) arrived.
    overview.update(cx, |overview, cx| {
        overview.month = month(2026, 10);
        overview.reload(cx);
        overview.select_month(month(2026, 10), cx);
    });
    redraw(cx);
    assert_eq!(overview.read_with(cx, |o, _| o.month()), month(2026, 10));
    let data = overview.read_with(cx, |o, _| o.data().cloned().unwrap());
    assert_eq!(data.selected.period, month(2026, 10));
}

#[gpui_kit::test]
fn a_month_with_no_data_yet_opens_on_the_latest_month_with_some(cx: &mut TestAppContext) {
    // The seeded rows stop in November 2026; two months later, January's
    // statement isn't in yet.
    let scratch = Scratch::new();
    let today = NaiveDate::from_ymd_opt(2027, 1, 20).unwrap();
    let store = OverviewStore::new(scratch.path.clone());
    // Read on this thread, and on this day rather than the clock's.
    let (_, overview, cx) = open(cx, OverviewView::from_store(store, today));
    assert_eq!(overview.read_with(cx, |o, _| o.month()), month(2026, 11));
    assert_eq!(described(&overview, cx).month, "November 2026");
    let data = overview.read_with(cx, |o, _| o.data().cloned().unwrap());
    assert_eq!(data.selected.period, month(2026, 11));
    assert!(!data.selected.spent.is_zero());
    // The chart's net line starts at the first month with data.
    let earliest = data.earliest_month.expect("seeded rows");
    let chart = text::screen_text(&data, data.selected.period, today).months;
    for (figures, shown) in data.months.iter().zip(&chart) {
        assert_eq!(
            shown.before_data,
            figures.period < earliest,
            "{}",
            shown.title
        );
    }
    assert!(chart.first().unwrap().before_data);
    assert!(!chart.last().unwrap().before_data);
    // The switcher still reaches today's month, and a month picked stays
    // picked when the screen reads again.
    let next = action(&described(&overview, cx), "Next month");
    assert!(next.enabled);
    overview.update(cx, |overview, cx| overview.select_month(month(2027, 1), cx));
    redraw(cx);
    overview.update(cx, |overview, cx| overview.reload(cx));
    redraw(cx);
    assert_eq!(overview.read_with(cx, |o, _| o.month()), month(2027, 1));
    let data = overview.read_with(cx, |o, _| o.data().cloned().unwrap());
    assert_eq!(data.selected.period, month(2027, 1));
}

#[gpui_kit::test]
fn the_agent_card_counts_pending_suggestions_and_review_opens_the_inbox(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orre.db");
    crate::inbox::fixture::seed(&mut Database::open(&path).unwrap());
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::from_store(InboxStore::new(path.clone())));
        ShellView::with_data(View::Overview, accounts, inbox, window, cx)
    });
    redraw(cx);
    let overview = cx.update(|_, cx| shell.read(cx).overview().clone());
    let description = described(&overview, cx);
    let card = figures(&description)
        .agent_card
        .clone()
        .expect("suggestions are pending");
    assert!(!card.summary.is_empty());
    assert_eq!(card.pending, shell_badge(&shell, cx).to_string());
    assert!(action(&description, "Review").enabled);

    click("overview-review", cx);
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
}

fn shell_badge(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| shell.read(cx).inbox().read(cx).badge())
}

#[gpui_kit::test]
fn without_pending_suggestions_there_is_no_agent_card(cx: &mut TestAppContext) {
    let (_, overview, cx) = open(cx, OverviewView::fixture());
    assert_eq!(figures(&described(&overview, cx)).agent_card, None);
    assert!(!shows("overview-review", cx));
}

// The day moving on (a window left open across midnight) ------------------------

#[test]
fn the_current_month_follows_the_day_and_a_browsed_one_stays() {
    // The sample day is in November 2026, which is selected.
    let mut view = OverviewView::fixture();
    let november = view.month();
    let december = november.next().expect("a month after");
    let day = |d: u32| NaiveDate::from_ymd_opt(2026, 12, d).expect("a date");
    view.follow(day(1));
    assert_eq!(view.today(), day(1));
    assert_eq!(view.month(), december, "the current month follows");

    let mut browsing = OverviewView::fixture();
    let october = november.previous().expect("a month before");
    browsing.month = october;
    browsing.follow(day(1));
    assert_eq!(browsing.today(), day(1));
    assert_eq!(browsing.month(), october, "a browsed month stays");
    // Once the day has moved, last month's "current" month is a browsed one
    // too. (That `select_month` follows the clock before it clamps, so the
    // pick isn't moved on, reads the real clock and isn't tested here.)
    browsing.month = november;
    browsing.follow(day(2));
    assert_eq!(browsing.month(), november);
}
