//! DeepSeek's time-of-day pricing.
//!
//! Off-peak is half the peak price. Peak hours are Monday–Friday 09:00–12:00 and 14:00–18:00,
//! **Beijing time**; everything else is off-peak. Weekends and mainland China public holidays
//! are off-peak all day, and so are weekend make-up workdays.
//!
//! All ranges are half-open: 09:00 sharp is peak, 12:00 and 18:00 sharp are off-peak.

use std::ops::{Range, RangeInclusive};

use chrono::{
    DateTime, Datelike, Days, Duration, FixedOffset, NaiveDate, TimeZone, Timelike, Utc, Weekday,
};
use serde::{Deserialize, Serialize};

use crate::holidays::HolidaySchedule;

/// Peak ranges in minutes since Beijing midnight, half-open.
pub const PEAK_MINUTE_RANGES: [Range<u32>; 2] = [9 * 60..12 * 60, 14 * 60..18 * 60];

/// How far to look for the next / previous real transition. Chinese holiday runs are far shorter.
const SEARCH_DAYS: i64 = 32;

/// Beijing time: UTC+8, no daylight saving.
pub fn beijing() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("valid offset")
}

/// The Beijing calendar day an instant falls on.
pub fn beijing_date(at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&beijing()).date_naive()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PricePeriod {
    /// Full price.
    Peak,
    /// Half the peak price.
    OffPeak,
}

impl PricePeriod {
    /// Price relative to peak: 1.0 or 0.5.
    pub fn price_multiplier(self) -> f64 {
        match self {
            PricePeriod::Peak => 1.0,
            PricePeriod::OffPeak => 0.5,
        }
    }

    pub fn opposite(self) -> PricePeriod {
        match self {
            PricePeriod::Peak => PricePeriod::OffPeak,
            PricePeriod::OffPeak => PricePeriod::Peak,
        }
    }
}

/// Why a Beijing calendar day follows the rule it does. Holiday names only feed the UI text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "name")]
pub enum DayKind {
    PublicHoliday(String),
    /// A make-up workday that lands on a weekend: still off-peak all day.
    AlternateWorkdayWeekend(String),
    Weekend,
    RegularWeekday,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayInfo {
    pub kind: DayKind,
    /// Whether the holiday calendar covers this year at all; if not, holidays may be missing.
    pub schedule_is_covered: bool,
}

impl DayInfo {
    pub fn is_all_day_off_peak(&self) -> bool {
        !matches!(self.kind, DayKind::RegularWeekday)
    }
}

/// The pricing rule for a Beijing calendar day (used by the monthly calendar).
pub fn day_info_for_date(day: NaiveDate, schedule: &HolidaySchedule) -> DayInfo {
    let is_weekend = matches!(day.weekday(), Weekday::Sat | Weekday::Sun);
    let kind = if let Some(entry) = schedule.alternate_workday_on(day).filter(|_| is_weekend) {
        DayKind::AlternateWorkdayWeekend(entry.name.clone())
    } else if let Some(entry) = schedule.holiday_on(day) {
        DayKind::PublicHoliday(entry.name.clone())
    } else if is_weekend {
        DayKind::Weekend
    } else {
        DayKind::RegularWeekday
    };
    DayInfo {
        kind,
        schedule_is_covered: schedule.is_covered(day.year()),
    }
}

pub fn day_info(at: DateTime<Utc>, schedule: &HolidaySchedule) -> DayInfo {
    day_info_for_date(beijing_date(at), schedule)
}

/// One Beijing calendar day with its pricing rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDay {
    pub date: NaiveDate,
    pub info: DayInfo,
    /// The period of each Beijing hour, 00:00 to 23:00. Peak ranges start and end on the hour.
    pub hours: Vec<PricePeriod>,
}

pub fn calendar_day(date: NaiveDate, schedule: &HolidaySchedule) -> CalendarDay {
    let hours = (0..24)
        .map(|hour| {
            let local = date.and_hms_opt(hour, 0, 0).expect("valid hour");
            let at = beijing()
                .from_local_datetime(&local)
                .unwrap()
                .with_timezone(&Utc);
            period(at, schedule)
        })
        .collect();
    CalendarDay {
        date,
        info: day_info_for_date(date, schedule),
        hours,
    }
}

/// Every day of a month (for the monthly calendar). Empty for an invalid month.
pub fn month_days(year: i32, month: u32, schedule: &HolidaySchedule) -> Vec<CalendarDay> {
    let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return Vec::new();
    };
    first
        .iter_days()
        .take_while(|day| day.month() == month)
        .map(|day| calendar_day(day, schedule))
        .collect()
}

/// Sunday to Saturday of the week containing `date` (for the weekly grid).
pub fn week_days(date: NaiveDate, schedule: &HolidaySchedule) -> Vec<CalendarDay> {
    let sunday = date.week(Weekday::Sun).first_day();
    sunday
        .iter_days()
        .take(7)
        .map(|day| calendar_day(day, schedule))
        .collect()
}

/// The period in effect at an instant.
pub fn period(at: DateTime<Utc>, schedule: &HolidaySchedule) -> PricePeriod {
    if day_info(at, schedule).is_all_day_off_peak() {
        return PricePeriod::OffPeak;
    }
    let local = at.with_timezone(&beijing());
    let minutes = local.hour() * 60 + local.minute();
    if PEAK_MINUTE_RANGES
        .iter()
        .any(|range| range.contains(&minutes))
    {
        PricePeriod::Peak
    } else {
        PricePeriod::OffPeak
    }
}

/// Nominal peak boundaries of one Beijing day, whether or not the price really changes there.
fn boundaries_on(day: NaiveDate) -> impl Iterator<Item = DateTime<Utc>> {
    let midnight = day.and_hms_opt(0, 0, 0).expect("valid time");
    PEAK_MINUTE_RANGES
        .into_iter()
        .flat_map(|range| [range.start, range.end])
        .filter_map(move |minute| {
            beijing()
                .from_local_datetime(&(midnight + Duration::minutes(i64::from(minute))))
                .single()
                .map(|local| local.with_timezone(&Utc))
        })
}

fn boundaries_around(at: DateTime<Utc>, day_offsets: RangeInclusive<i64>) -> Vec<DateTime<Utc>> {
    let today = beijing_date(at);
    let mut all: Vec<_> = day_offsets
        .filter_map(|offset| match offset {
            o if o >= 0 => today.checked_add_days(Days::new(o.unsigned_abs())),
            o => today.checked_sub_days(Days::new(o.unsigned_abs())),
        })
        .flat_map(boundaries_on)
        .collect();
    all.sort();
    all
}

fn is_real_transition(boundary: DateTime<Utc>, schedule: &HolidaySchedule) -> bool {
    period(boundary - Duration::seconds(1), schedule) != period(boundary, schedule)
}

/// The next moment the price actually changes (skips boundaries inside weekends and holidays).
pub fn next_transition(after: DateTime<Utc>, schedule: &HolidaySchedule) -> DateTime<Utc> {
    boundaries_around(after, 0..=SEARCH_DAYS)
        .into_iter()
        .find(|&b| b > after && is_real_transition(b, schedule))
        .unwrap_or(after + Duration::days(SEARCH_DAYS))
}

/// The start of the current period: the most recent real transition.
pub fn current_interval_start(before: DateTime<Utc>, schedule: &HolidaySchedule) -> DateTime<Utc> {
    boundaries_around(before, -SEARCH_DAYS..=0)
        .into_iter()
        .rev()
        .find(|&b| b <= before && is_real_transition(b, schedule))
        .unwrap_or_else(|| {
            let midnight = beijing_date(before)
                .and_hms_opt(0, 0, 0)
                .expect("valid time");
            beijing()
                .from_local_datetime(&midnight)
                .unwrap()
                .with_timezone(&Utc)
        })
}

/// Everything the UI shows about "now".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingSnapshot {
    pub now: DateTime<Utc>,
    pub period: PricePeriod,
    pub interval_start: DateTime<Utc>,
    pub next_transition: DateTime<Utc>,
    /// The period after `next_transition`.
    pub next_period: PricePeriod,
    pub day_info: DayInfo,
}

impl PricingSnapshot {
    pub fn new(now: DateTime<Utc>, schedule: &HolidaySchedule) -> Self {
        let next_transition = next_transition(now, schedule);
        PricingSnapshot {
            now,
            period: period(now, schedule),
            interval_start: current_interval_start(now, schedule),
            next_transition,
            next_period: period(next_transition, schedule),
            day_info: day_info(now, schedule),
        }
    }

    pub fn seconds_until_transition(&self) -> i64 {
        (self.next_transition - self.now).num_seconds().max(0)
    }

    /// How much of the current period has elapsed, 0.0 ..= 1.0.
    pub fn interval_progress(&self) -> f64 {
        let total = (self.next_transition - self.interval_start).num_milliseconds();
        if total <= 0 {
            return 0.0;
        }
        let elapsed = (self.now - self.interval_start).num_milliseconds();
        (elapsed as f64 / total as f64).clamp(0.0, 1.0)
    }
}

/// `HH:MM:SS` countdown for the tray. Hours are not capped: long holidays exceed 99 hours.
pub fn compact_countdown(seconds: i64) -> String {
    let total = seconds.max(0);
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}
