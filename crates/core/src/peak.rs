//! Weekly peak hours for providers whose rule is a plain clock, with no holiday calendar
//! (DeepSeek's holiday-aware schedule is [`crate::pricing`]).
//!
//! Peak is one or more daily ranges, Monday to Friday, in a fixed UTC offset; weekends are
//! off-peak all day. Ranges are half-open minutes since local midnight, as in `pricing`.

use std::ops::Range;

use chrono::{DateTime, Datelike, Days, Duration, FixedOffset, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::pricing::PricePeriod;

/// Days to search for the next or previous change; a weekend is the longest gap.
const SEARCH_DAYS: u64 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakHours {
    /// The clock the ranges are written in, e.g. 8 for UTC+8. No daylight saving.
    pub utc_offset_hours: i32,
    /// Peak ranges on weekdays, minutes since local midnight, half-open.
    pub ranges: &'static [Range<u32>],
}

/// Where "now" falls in a [`PeakHours`] schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakStatus {
    pub period: PricePeriod,
    pub interval_start: DateTime<Utc>,
    pub next_transition: DateTime<Utc>,
    /// The period after `next_transition`.
    pub next_period: PricePeriod,
}

impl PeakHours {
    fn offset(&self) -> FixedOffset {
        FixedOffset::east_opt(self.utc_offset_hours * 3600).expect("valid offset")
    }

    pub fn period(&self, at: DateTime<Utc>) -> PricePeriod {
        let local = at.with_timezone(&self.offset());
        let minutes = local.hour() * 60 + local.minute();
        let weekday = local.weekday().number_from_monday() <= 5;
        if weekday && self.ranges.iter().any(|range| range.contains(&minutes)) {
            PricePeriod::Peak
        } else {
            PricePeriod::OffPeak
        }
    }

    /// Range edges on weekdays from `first` on, for `days` days, in order.
    fn boundaries(&self, first: NaiveDate, days: u64) -> Vec<DateTime<Utc>> {
        let offset = self.offset();
        let mut all: Vec<_> = (0..days)
            .filter_map(|i| first.checked_add_days(Days::new(i)))
            .filter(|day| day.weekday().number_from_monday() <= 5)
            .flat_map(|day| {
                let midnight = day.and_hms_opt(0, 0, 0).expect("valid time");
                self.ranges
                    .iter()
                    .flat_map(|range| [range.start, range.end])
                    .filter_map(move |minute| {
                        offset
                            .from_local_datetime(&(midnight + Duration::minutes(i64::from(minute))))
                            .single()
                            .map(|local| local.with_timezone(&Utc))
                    })
            })
            .collect();
        all.sort();
        all
    }

    pub fn status(&self, now: DateTime<Utc>) -> PeakStatus {
        let today = now.with_timezone(&self.offset()).date_naive();
        let changes =
            |at: &DateTime<Utc>| self.period(*at - Duration::seconds(1)) != self.period(*at);
        let next_transition = self
            .boundaries(today, SEARCH_DAYS)
            .into_iter()
            .find(|b| *b > now && changes(b))
            .unwrap_or(now + Duration::days(SEARCH_DAYS as i64));
        let interval_start = self
            .boundaries(today - Days::new(SEARCH_DAYS), SEARCH_DAYS + 1)
            .into_iter()
            .rev()
            .find(|b| *b <= now && changes(b))
            .unwrap_or(now);
        PeakStatus {
            period: self.period(now),
            interval_start,
            next_transition,
            next_period: self.period(next_transition),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PricePeriod::{OffPeak, Peak};

    /// Z.AI's: weekdays 14:00–18:00 UTC+8.
    #[allow(clippy::single_range_in_vec_init)]
    const HOURS: PeakHours = PeakHours {
        utc_offset_hours: 8,
        ranges: &[14 * 60..18 * 60],
    };

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        // 2026-10-26 is a Monday.
        FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn weekday_ranges_are_half_open() {
        assert_eq!(HOURS.period(at(26, 13, 59)), OffPeak);
        assert_eq!(HOURS.period(at(26, 14, 0)), Peak);
        assert_eq!(HOURS.period(at(26, 17, 59)), Peak);
        assert_eq!(HOURS.period(at(26, 18, 0)), OffPeak);
    }

    #[test]
    fn weekends_are_off_peak() {
        assert_eq!(HOURS.period(at(31, 15, 0)), OffPeak);
        assert_eq!(HOURS.period(at(25, 15, 0)), OffPeak);
    }

    #[test]
    fn status_finds_the_surrounding_changes() {
        let peak = HOURS.status(at(26, 15, 0));
        assert_eq!(peak.period, Peak);
        assert_eq!(peak.interval_start, at(26, 14, 0));
        assert_eq!(peak.next_transition, at(26, 18, 0));
        assert_eq!(peak.next_period, OffPeak);

        let evening = HOURS.status(at(26, 20, 0));
        assert_eq!(evening.interval_start, at(26, 18, 0));
        assert_eq!(evening.next_transition, at(27, 14, 0));
    }

    #[test]
    fn friday_evening_runs_to_monday() {
        let weekend = HOURS.status(at(31, 9, 0));
        assert_eq!(weekend.period, OffPeak);
        assert_eq!(weekend.interval_start, at(30, 18, 0));
        // 2026-11-02 is the next Monday.
        let monday = FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 2, 14, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(weekend.next_transition, monday);
    }
}
