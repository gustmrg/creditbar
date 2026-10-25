//! Mainland China holiday calendar, as published by Apple's iCloud holiday subscription.
//!
//! Only events Apple explicitly marks as days off (`WORK-HOLIDAY`) or make-up workdays
//! (`ALTERNATE-WORKDAY`) are kept; solar terms and ordinary observances are ignored.
//! All dates are Beijing calendar days.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Apple's Chinese-language holiday calendar.
pub const CALENDAR_URL: &str = "https://calendars.icloud.com/holidays/cn_zh.ics";

/// How often the app should re-check the calendar.
pub const REFRESH_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    AlternateWorkday,
    Holiday,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub kind: EntryKind,
    /// Inclusive.
    pub start_day: NaiveDate,
    /// Exclusive.
    pub end_day_exclusive: NaiveDate,
    /// Chinese name with the "（休）/（班）" suffix removed. Display only.
    pub name: String,
}

impl Entry {
    pub fn contains(&self, day: NaiveDate) -> bool {
        self.start_day <= day && day < self.end_day_exclusive
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HolidaySchedule {
    pub entries: Vec<Entry>,
    pub fetched_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("the calendar is not valid UTF-8")]
    InvalidEncoding,
    #[error("the calendar contains no holidays")]
    NoHolidayEntries,
}

impl HolidaySchedule {
    /// Years with at least one holiday entry.
    pub fn covered_years(&self) -> BTreeSet<i32> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Holiday)
            .map(|e| e.start_day.year())
            .collect()
    }

    pub fn newest_covered_year(&self) -> Option<i32> {
        self.covered_years().last().copied()
    }

    pub fn is_covered(&self, year: i32) -> bool {
        self.covered_years().contains(&year)
    }

    pub fn holiday_on(&self, day: NaiveDate) -> Option<&Entry> {
        self.entry_on(EntryKind::Holiday, day)
    }

    pub fn alternate_workday_on(&self, day: NaiveDate) -> Option<&Entry> {
        self.entry_on(EntryKind::AlternateWorkday, day)
    }

    fn entry_on(&self, kind: EntryKind, day: NaiveDate) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.kind == kind && e.contains(day))
    }

    /// A freshly downloaded calendar that covers fewer years than the current one is a
    /// temporary or partial response: keep the current data instead.
    pub fn is_regression_from(&self, current: &HolidaySchedule) -> bool {
        matches!((self.newest_covered_year(), current.newest_covered_year()),
                 (Some(received), Some(existing)) if received < existing)
    }

    /// Parses Apple's holiday ICS. `DTEND` is an exclusive end date.
    pub fn parse_ics(data: &[u8], fetched_at: DateTime<Utc>) -> Result<Self, ParseError> {
        let raw = std::str::from_utf8(data).map_err(|_| ParseError::InvalidEncoding)?;

        // RFC 5545: a physical line starting with a space or tab continues the previous one.
        let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
        let mut lines: Vec<String> = Vec::new();
        for line in normalized.split('\n') {
            match (
                line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')),
                lines.last_mut(),
            ) {
                (Some(rest), Some(last)) => last.push_str(rest),
                _ => lines.push(line.to_owned()),
            }
        }

        let mut events: Vec<HashMap<String, String>> = Vec::new();
        let mut event: Option<HashMap<String, String>> = None;
        for line in &lines {
            match line.as_str() {
                "BEGIN:VEVENT" => event = Some(HashMap::new()),
                "END:VEVENT" => events.extend(event.take()),
                _ => {
                    let (Some(event), Some((property, value))) =
                        (event.as_mut(), line.split_once(':'))
                    else {
                        continue;
                    };
                    let name = property.split(';').next().unwrap_or_default();
                    event.insert(name.to_owned(), value.to_owned());
                }
            }
        }

        let mut entries: Vec<Entry> = events
            .iter()
            .filter_map(|event| {
                let kind = match event.get("X-APPLE-SPECIAL-DAY")?.as_str() {
                    "WORK-HOLIDAY" => EntryKind::Holiday,
                    "ALTERNATE-WORKDAY" => EntryKind::AlternateWorkday,
                    _ => return None,
                };
                let start = parse_ics_date(event.get("DTSTART")?)?;
                let end = event
                    .get("DTEND")
                    .and_then(|v| parse_ics_date(v))
                    .or_else(|| start.checked_add_days(Days::new(1)))?;
                (end > start).then(|| Entry {
                    kind,
                    start_day: start,
                    end_day_exclusive: end,
                    name: normalized_name(
                        event.get("SUMMARY").map(String::as_str).unwrap_or_default(),
                    ),
                })
            })
            .collect();

        if !entries.iter().any(|e| e.kind == EntryKind::Holiday) {
            return Err(ParseError::NoHolidayEntries);
        }
        entries.sort_by_key(|e| (e.start_day, e.kind));
        Ok(HolidaySchedule {
            entries,
            fetched_at: Some(fetched_at),
        })
    }

    /// Fallback shipped with the app so the current year is right even offline.
    pub fn bundled() -> Self {
        use EntryKind::{AlternateWorkday as W, Holiday as H};
        let entries = [
            (H, "2026-01-01", "2026-01-04", "元旦"),
            (W, "2026-01-04", "2026-01-05", "元旦"),
            (W, "2026-02-14", "2026-02-15", "春节"),
            (H, "2026-02-15", "2026-02-24", "春节"),
            (W, "2026-02-28", "2026-03-01", "春节"),
            (H, "2026-04-04", "2026-04-07", "清明"),
            (H, "2026-05-01", "2026-05-06", "劳动节"),
            (W, "2026-05-09", "2026-05-10", "劳动节"),
            (H, "2026-06-19", "2026-06-22", "端午节"),
            (W, "2026-09-20", "2026-09-21", "国庆节"),
            (H, "2026-09-25", "2026-09-28", "中秋节"),
            (H, "2026-10-01", "2026-10-08", "国庆节"),
            (W, "2026-10-10", "2026-10-11", "国庆节"),
        ]
        .into_iter()
        .map(|(kind, start, end, name)| Entry {
            kind,
            start_day: start.parse().expect("valid bundled date"),
            end_day_exclusive: end.parse().expect("valid bundled date"),
            name: name.to_owned(),
        })
        .collect();
        HolidaySchedule {
            entries,
            fetched_at: None,
        }
    }
}

/// Stable key for a holiday name, for the UI to localize. Unknown names map to `"public"`.
pub fn holiday_name_key(name: &str) -> &'static str {
    HOLIDAY_NAME_KEYS
        .iter()
        .find(|(chinese, _)| *chinese == name)
        .map_or("public", |(_, key)| key)
}

/// Apple's Chinese holiday names and their localization keys.
pub const HOLIDAY_NAME_KEYS: [(&str, &str); 7] = [
    ("元旦", "newYear"),
    ("春节", "springFestival"),
    ("清明", "qingming"),
    ("劳动节", "labourDay"),
    ("端午节", "dragonBoat"),
    ("中秋节", "midAutumn"),
    ("国庆节", "nationalDay"),
];

/// `YYYYMMDD` (all-day `VALUE=DATE`). Date-times are not used by the holiday calendar.
fn parse_ics_date(value: &str) -> Option<NaiveDate> {
    if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    NaiveDate::from_ymd_opt(
        value[..4].parse().ok()?,
        value[4..6].parse().ok()?,
        value[6..].parse().ok()?,
    )
}

fn normalized_name(raw: &str) -> String {
    let mut name = raw
        .replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\");
    for suffix in ["（休）", "（班）", "(休)", "(班)"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_owned();
        }
    }
    if name.is_empty() {
        "Public holiday".to_owned()
    } else {
        name
    }
}
