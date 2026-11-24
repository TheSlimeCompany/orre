//! The plain arithmetic behind the income and spending chart
//! ([`income_chart`](super::income_chart)): the value axis, bar extents, the
//! compact axis labels, and which side a tooltip or label opens on.
//!
//! No GPUI here. Money stays in integer minor units until it is turned into a
//! fraction of the plot's height; only that fraction and the pixel positions
//! are `f32` (`AGENTS.md`: floats only for pixels, in the app crate).

use orre_core::money::Currency;

use crate::format::currency_prefix;

/// The plot's height (`docs/DESIGN.md` → "Income vs spending": 240px).
pub const PLOT_HEIGHT: f32 = 240.;

/// A chart's value axis: whole steps of one round size, five above zero and
/// as many more below as the lowest value needs.
///
/// The step is the smallest of 1, 2 or 5 × 10ⁿ whole currency units (so £1,
/// £2, £5, £10 … £500, £1k, £2k, £5k …) whose five intervals reach the
/// largest value. A lowest value below zero, the net line when a month
/// overspends, adds steps of the same size below zero. The steps are chosen
/// from the larger of the two magnitudes, so at most five are added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axis {
    step: i64,
    below: u32,
}

impl Axis {
    /// The intervals from zero up to the top of the axis.
    pub const INTERVALS_ABOVE: u32 = 5;

    /// The axis for values from `lowest` to `highest` minor units of
    /// `currency`. Values on one side of zero only are fine: the other side
    /// gets no steps.
    pub fn new(lowest: i64, highest: i64, currency: Currency) -> Axis {
        let unit = 10i64.pow(u32::from(currency.minor_units()));
        let highest = highest.max(0).unsigned_abs();
        let reach = highest.max(lowest.min(0).unsigned_abs());
        let step = Self::step_for(reach, unit.unsigned_abs());
        let below = lowest.min(0).unsigned_abs().div_ceil(step);
        Axis {
            // `step_for` never goes past `i64::MAX`.
            step: i64::try_from(step).unwrap_or(i64::MAX),
            // At most `INTERVALS_ABOVE`, since `reach` bounds `lowest`.
            below: u32::try_from(below).unwrap_or(Self::INTERVALS_ABOVE),
        }
    }

    /// The smallest 1, 2 or 5 × 10ⁿ whole units, at least one unit, whose
    /// five intervals reach `reach`.
    fn step_for(reach: u64, unit: u64) -> u64 {
        let mut power = 1u64;
        loop {
            for base in [1, 2, 5] {
                let step = unit.saturating_mul(base).saturating_mul(power);
                if step.saturating_mul(u64::from(Self::INTERVALS_ABOVE)) >= reach {
                    return step.min(i64::MAX as u64);
                }
            }
            match power.checked_mul(10) {
                Some(next) => power = next,
                None => return i64::MAX as u64,
            }
        }
    }

    /// The step between two gridlines, in minor units.
    pub fn step(&self) -> i64 {
        self.step
    }

    /// The intervals, above and below zero.
    pub fn intervals(&self) -> u32 {
        Self::INTERVALS_ABOVE + self.below
    }

    /// The value at the top gridline.
    pub fn top(&self) -> i64 {
        self.step.saturating_mul(i64::from(Self::INTERVALS_ABOVE))
    }

    /// The gridline values, top to bottom, zero among them.
    pub fn ticks(&self) -> Vec<i64> {
        (0..=self.intervals())
            .map(|at| self.top() - self.step.saturating_mul(i64::from(at)))
            .collect()
    }

    /// How far down the plot `value` is, 0 (top) to 1 (bottom), clamped.
    pub fn fraction(&self, value: i64) -> f32 {
        let span = f64::from(self.intervals()) * self.step as f64;
        (((self.top() as f64 - value as f64) / span) as f32).clamp(0., 1.)
    }

    /// The gridline's distance from the top of a plot `height` tall, for the
    /// `at`th gridline from the top.
    pub fn gridline_y(&self, at: u32, height: f32) -> f32 {
        height * at as f32 / self.intervals() as f32
    }
}

/// Where a bar sits in a plot, in pixels from its top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarExtent {
    /// The bar's top edge.
    pub top: f32,
    pub height: f32,
    /// The bar's value end is its top: the value is above zero.
    pub rises: bool,
}

impl BarExtent {
    /// The bar for `value` on `axis` in a plot `plot` tall, from the zero
    /// line to the value. `None` for zero: no bar is drawn.
    pub fn new(value: i64, axis: &Axis, plot: f32) -> Option<BarExtent> {
        if value == 0 {
            return None;
        }
        let zero = plot * axis.fraction(0);
        let end = plot * axis.fraction(value);
        Some(if value > 0 {
            BarExtent {
                top: end,
                height: zero - end,
                rises: true,
            }
        } else {
            BarExtent {
                top: zero,
                height: end - zero,
                rises: false,
            }
        })
    }

    /// The bar's bottom edge.
    pub fn bottom(&self) -> f32 {
        self.top + self.height
    }
}

/// `minor` as a compact axis label: `£0`, `£500`, `£1k`, `£1.5k`, `£12k`,
/// `£1.2m`, `−£2k`.
///
/// Thousands and millions are chosen per label, so one axis can read `£800`
/// then `£1k`. Up to two decimals, truncated, trailing zeros dropped. A
/// currency without a symbol is written with its code and a space.
pub fn compact_label(minor: i64, currency: Currency) -> String {
    let unit = 10u64.pow(u32::from(currency.minor_units()));
    let magnitude = minor.unsigned_abs();
    let (scale, suffix) = if magnitude >= 1_000_000 * unit {
        (1_000_000 * unit, "m")
    } else if magnitude >= 1_000 * unit {
        (1_000 * unit, "k")
    } else {
        (unit, "")
    };
    let whole = magnitude / scale;
    let hundredths = magnitude % scale * 100 / scale;
    let number = if hundredths == 0 {
        whole.to_string()
    } else if hundredths.is_multiple_of(10) {
        format!("{whole}.{}", hundredths / 10)
    } else {
        format!("{whole}.{hundredths:02}")
    };
    let prefix = currency_prefix(currency);
    let prefix = if prefix.chars().count() > 1 {
        format!("{prefix} ")
    } else {
        prefix
    };
    let sign = if minor < 0 { "\u{2212}" } else { "" };
    format!("{sign}{prefix}{number}{suffix}")
}

/// A side of a month.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// How many months must be free on a side for a tooltip to open there.
const TOOLTIP_ROOM: usize = 3;

/// The side the hover tooltip for month `hovered` of `count` opens on: away
/// from the selected month's label, and never into the chart's edge.
///
/// A month after the selected one opens to its right, one before it to its
/// left. The selected month itself opens opposite its own label
/// ([`label_edge`]). With no selection a month in the left half opens right.
/// A side with fewer than three months of room is given up for the other
/// when that has more.
pub fn tooltip_side(hovered: usize, selected: Option<usize>, count: usize) -> Side {
    let wanted = match selected {
        Some(selected) if selected != hovered => {
            if hovered > selected {
                Side::Right
            } else {
                Side::Left
            }
        }
        // The label runs from the pair toward the middle of the plot, so
        // away from it is the pair's own outer side.
        Some(selected) => label_edge(selected, count),
        None => {
            if hovered * 2 < count {
                Side::Right
            } else {
                Side::Left
            }
        }
    };
    let left_room = hovered;
    let right_room = count.saturating_sub(hovered + 1);
    let (room, other_room, other) = match wanted {
        Side::Left => (left_room, right_room, Side::Right),
        Side::Right => (right_room, left_room, Side::Left),
    };
    if room < TOOLTIP_ROOM && other_room > room {
        other
    } else {
        wanted
    }
}

/// Which edge of the selected month's bar pair its label lines up with, so
/// the label stays inside the plot: [`Side::Left`] puts the label's left edge
/// on the pair's left edge (it runs right) for a month in the left half,
/// [`Side::Right`] the mirror.
pub fn label_edge(selected: usize, count: usize) -> Side {
    if selected * 2 < count {
        Side::Left
    } else {
        Side::Right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GBP: Currency = Currency::GBP;

    fn axis(lowest: i64, highest: i64) -> Axis {
        Axis::new(lowest, highest, GBP)
    }

    fn pounds(axis: &Axis) -> Vec<i64> {
        axis.ticks().iter().map(|minor| minor / 100).collect()
    }

    #[test]
    fn the_step_is_the_smallest_round_one_that_reaches_the_largest_value() {
        // £3,120.75 → five steps of £1k reach £5k.
        assert_eq!(pounds(&axis(0, 312_075)), [5000, 4000, 3000, 2000, 1000, 0]);
        // Exactly five steps of £1k fits.
        assert_eq!(axis(0, 500_000).step(), 100_000);
        // A penny more needs £2k.
        assert_eq!(axis(0, 500_001).step(), 200_000);
        // £12,000 needs steps of at least £2,400: the next round one is £5k.
        assert_eq!(axis(0, 1_200_000).step(), 500_000);
        // £340 needs £68: £100. £240 needs £48: £50.
        assert_eq!(axis(0, 34_000).step(), 10_000);
        assert_eq!(axis(0, 24_000).step(), 5_000);
    }

    #[test]
    fn nothing_to_draw_still_has_a_whole_axis() {
        let empty = axis(0, 0);
        assert_eq!(empty.step(), 100, "one pound");
        assert_eq!(empty.intervals(), 5);
        assert_eq!(pounds(&empty), [5, 4, 3, 2, 1, 0]);
        // Negative highest values count as zero.
        assert_eq!(axis(0, -500), empty);
    }

    #[test]
    fn a_negative_net_extends_the_axis_by_whole_steps() {
        // Largest value £3,120.75 (step £1k); net down to −£1,200 → two more.
        let below = axis(-120_000, 312_075);
        assert_eq!(below.step(), 100_000);
        assert_eq!(below.intervals(), 7);
        assert_eq!(
            pounds(&below),
            [5000, 4000, 3000, 2000, 1000, 0, -1000, -2000]
        );
        // Exactly one step needs one step.
        assert_eq!(axis(-100_000, 312_075).intervals(), 6);
        // A deficit larger than any positive value sets the step.
        let deficit = axis(-1_000_000, 100_000);
        assert_eq!(deficit.step(), 200_000);
        assert_eq!(deficit.intervals(), 10);
    }

    #[test]
    fn fractions_run_from_the_top_gridline_down() {
        let plain = axis(0, 500_000);
        assert_eq!(plain.fraction(500_000), 0.);
        assert_eq!(plain.fraction(0), 1.);
        assert_eq!(plain.fraction(250_000), 0.5);
        // Outside the axis clamps.
        assert_eq!(plain.fraction(900_000), 0.);
        assert_eq!(plain.fraction(-1), 1.);

        let below = axis(-100_000, 500_000);
        assert!((below.fraction(0) - 5. / 6.).abs() < 1e-6);
        assert_eq!(below.fraction(-100_000), 1.);
        assert_eq!(below.gridline_y(5, 240.), 200.);
    }

    #[test]
    fn bars_run_from_the_zero_line_to_the_value() {
        let plain = axis(0, 500_000);
        let half = BarExtent::new(250_000, &plain, 240.).unwrap();
        assert_eq!((half.top, half.height, half.rises), (120., 120., true));
        assert_eq!(half.bottom(), 240.);
        assert!(BarExtent::new(0, &plain, 240.).is_none());

        let below = axis(-100_000, 500_000);
        let zero = 200.;
        let down = BarExtent::new(-50_000, &below, 240.).unwrap();
        assert!(!down.rises);
        assert!((down.top - zero).abs() < 1e-3);
        assert!((down.height - 20.).abs() < 1e-3);
        let up = BarExtent::new(500_000, &below, 240.).unwrap();
        assert_eq!(up.top, 0.);
        assert!((up.bottom() - zero).abs() < 1e-3);
    }

    #[test]
    fn labels_are_compact() {
        let label = |minor| compact_label(minor, GBP);
        assert_eq!(label(0), "£0");
        assert_eq!(label(50_000), "£500");
        assert_eq!(label(100_000), "£1k");
        assert_eq!(label(150_000), "£1.5k");
        assert_eq!(label(1_200_000), "£12k");
        assert_eq!(label(125_000), "£1.25k");
        assert_eq!(label(120_000_000), "£1.2m");
        assert_eq!(label(-200_000), "\u{2212}£2k");
        assert_eq!(label(99_900), "£999");
    }

    #[test]
    fn labels_follow_the_currency() {
        assert_eq!(compact_label(300_000, Currency::EUR), "€3k");
        // No decimals in yen.
        let yen = Currency::new("JPY").unwrap();
        assert_eq!(compact_label(1_500, yen), "JPY 1.5k");
        assert_eq!(compact_label(0, yen), "JPY 0");
    }

    #[test]
    fn a_tooltip_opens_away_from_the_selected_month() {
        // Twelve months, October (index 9) selected.
        assert_eq!(tooltip_side(6, Some(9), 12), Side::Left);
        assert_eq!(tooltip_side(11, Some(9), 12), Side::Left, "no room right");
        // March selected: later months open right.
        assert_eq!(tooltip_side(5, Some(2), 12), Side::Right);
        // The selected month opens opposite its label: 9 is in the right
        // half, so its label runs left and the tooltip would go right, but
        // there is no room there. 3 is in the left half: its label runs
        // right, the tooltip goes left.
        assert_eq!(label_edge(9, 12), Side::Right);
        assert_eq!(tooltip_side(9, Some(9), 12), Side::Left);
        assert_eq!(tooltip_side(3, Some(3), 12), Side::Left);
    }

    #[test]
    fn a_tooltip_stays_off_the_edge() {
        assert_eq!(tooltip_side(0, Some(5), 12), Side::Right);
        assert_eq!(tooltip_side(1, Some(5), 12), Side::Right);
        assert_eq!(tooltip_side(10, Some(5), 12), Side::Left);
        // No selection: the left half opens right, the right half left.
        assert_eq!(tooltip_side(2, None, 12), Side::Right);
        assert_eq!(tooltip_side(8, None, 12), Side::Left);
        // A short chart picks the side with more room.
        assert_eq!(tooltip_side(0, None, 2), Side::Right);
        assert_eq!(tooltip_side(1, None, 2), Side::Left);
    }

    #[test]
    fn the_label_runs_toward_the_middle() {
        assert_eq!(label_edge(0, 12), Side::Left);
        assert_eq!(label_edge(5, 12), Side::Left);
        assert_eq!(label_edge(6, 12), Side::Right);
        assert_eq!(label_edge(11, 12), Side::Right);
    }
}
