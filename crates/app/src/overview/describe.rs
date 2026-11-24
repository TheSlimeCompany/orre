//! What the Overview shows, as plain data, for tests and the dev driver
//! (`orre dev describe`): the month, what each figure says, the note about
//! other currencies, the agent card, the chart's months with the shown one
//! marked, the spending by category and the recurring payments, and the
//! buttons.
//!
//! Compiled for tests and the dev driver only (the `dev-driver` feature,
//! debug builds): it isn't in a release build and nothing but the tests and
//! the driver can reach it, `orre mcp` included (`docs/architecture/app.md`
//! → "Decision record: views that describe themselves stay out of the
//! release binary"). Every string is [`super::text`]'s, the same the screen
//! draws, so bank text (merchants) is described as shown: through `plain`.
//! Colours, markers and bars are left out.

use gpui_kit::App;

pub use crate::ui::describe::Action;

use super::OverviewView;
use super::text::{AgentCardText, ScreenText, agent_card_text, screen_text};

/// What the Overview shows ([`OverviewView::describe`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewDescription {
    /// The switcher's title ("October 2026"): the month the shown data is
    /// for, which lags a pick while its read is in flight; before the first
    /// read, the selected month.
    pub month: String,
    pub shown: Shown,
    /// "Previous month", "Next month" and, with the agent card, "Review".
    pub actions: Vec<Action>,
}

/// What the canvas holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Nothing has been read yet.
    Blank,
    /// The read failed: the empty state's title and why.
    Failed(String),
    /// The database has no transactions: the empty state's title and its
    /// shortcut.
    Empty {
        title: &'static str,
        shortcut: (&'static str, &'static str),
    },
    Figures(Box<Figures>),
}

/// The screen with data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Figures {
    /// Spent, Income and Net: label, amount, caption.
    pub figures: Vec<(&'static str, String, String)>,
    pub other_currencies: Option<String>,
    pub agent_card: Option<AgentCardText>,
    pub chart_caption: String,
    pub months: Vec<ChartMonthDescription>,
    pub category_caption: String,
    /// Label, share, amount.
    pub categories: Vec<(String, String, String)>,
    pub recurring_total: String,
    /// Merchant, cadence, next date, amount.
    pub recurring: Vec<(String, &'static str, String, String)>,
}

/// One month on the chart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartMonthDescription {
    /// "Oct".
    pub label: String,
    /// "October 2026".
    pub title: String,
    pub spending: String,
    pub income: String,
    pub net: String,
    /// Whether it is the month the switcher shows.
    pub selected: bool,
}

impl OverviewView {
    /// What the screen shows now (module docs).
    pub fn describe(&self, cx: &App) -> OverviewDescription {
        // As the view: the month the shown data is for, which lags the
        // switcher while a read for another month is in flight.
        let shown_month = self
            .data
            .as_ref()
            .map_or(self.month, |data| data.selected.period);
        let month = super::text::month_title(shown_month);
        let agent = self
            .inbox
            .as_ref()
            .and_then(|inbox| agent_card_text(inbox.read(cx)));
        let mut actions = vec![
            Action::button("Previous month", self.previous_month().is_some()),
            Action::button("Next month", self.next_month().is_some()),
        ];
        let shown = if let Some(error) = &self.load_error {
            Shown::Failed(error.to_string())
        } else {
            match &self.data {
                None => Shown::Blank,
                Some(data) if !data.has_transactions => Shown::Empty {
                    title: "No transactions yet",
                    shortcut: ("⌘I", "Import a statement"),
                },
                Some(data) => {
                    if agent.is_some() {
                        actions.push(Action::button("Review", true));
                    }
                    Shown::Figures(Box::new(figures(
                        screen_text(data, shown_month, self.today),
                        agent,
                    )))
                }
            }
        };
        OverviewDescription {
            month,
            shown,
            actions,
        }
    }
}

fn figures(text: ScreenText, agent: Option<AgentCardText>) -> Figures {
    Figures {
        figures: text
            .figures
            .into_iter()
            .map(|figure| (figure.label, figure.value, figure.caption))
            .collect(),
        other_currencies: text.other_currencies,
        agent_card: agent,
        chart_caption: text.chart_caption,
        months: text
            .months
            .into_iter()
            .enumerate()
            .map(|(at, month)| ChartMonthDescription {
                label: month.label,
                title: month.title,
                spending: month.spending.1,
                income: month.income.1,
                net: month.net.1,
                selected: text.selected_month == Some(at),
            })
            .collect(),
        category_caption: text.category_caption,
        categories: text
            .categories
            .into_iter()
            .map(|entry| (entry.label, entry.share, entry.amount))
            .collect(),
        recurring_total: text.recurring_total,
        recurring: text
            .recurring
            .into_iter()
            .map(|row| (row.merchant, row.cadence, row.next, row.amount))
            .collect(),
    }
}
