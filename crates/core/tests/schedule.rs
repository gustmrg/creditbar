//! Ported from the Swift app's self-check (`Tools/Snapshot/main.swift`, `checkSchedule`).

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use status_core::holidays::{EntryKind, HolidaySchedule};
use status_core::pricing::{self, beijing, compact_countdown, DayKind, PricingSnapshot};
use status_core::PricePeriod::{self, OffPeak, Peak};

fn bj(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    beijing()
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .unwrap()
        .with_timezone(&Utc)
}

fn day(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[test]
fn weekday_boundaries_and_next_transition() {
    let schedule = HolidaySchedule::bundled();
    // 2026-09-14 is a Monday.
    let cases: [(&str, DateTime<Utc>, PricePeriod, DateTime<Utc>); 13] = [
        (
            "Mon 08:59",
            bj(2026, 9, 14, 8, 59),
            OffPeak,
            bj(2026, 9, 14, 9, 0),
        ),
        (
            "Mon 09:00",
            bj(2026, 9, 14, 9, 0),
            Peak,
            bj(2026, 9, 14, 12, 0),
        ),
        (
            "Mon 11:59",
            bj(2026, 9, 14, 11, 59),
            Peak,
            bj(2026, 9, 14, 12, 0),
        ),
        (
            "Mon 12:00",
            bj(2026, 9, 14, 12, 0),
            OffPeak,
            bj(2026, 9, 14, 14, 0),
        ),
        (
            "Mon 13:59",
            bj(2026, 9, 14, 13, 59),
            OffPeak,
            bj(2026, 9, 14, 14, 0),
        ),
        (
            "Mon 14:00",
            bj(2026, 9, 14, 14, 0),
            Peak,
            bj(2026, 9, 14, 18, 0),
        ),
        (
            "Mon 17:59",
            bj(2026, 9, 14, 17, 59),
            Peak,
            bj(2026, 9, 14, 18, 0),
        ),
        (
            "Mon 18:00",
            bj(2026, 9, 14, 18, 0),
            OffPeak,
            bj(2026, 9, 15, 9, 0),
        ),
        (
            "Tue 00:30",
            bj(2026, 9, 15, 0, 30),
            OffPeak,
            bj(2026, 9, 15, 9, 0),
        ),
        (
            "Fri 17:59",
            bj(2026, 9, 18, 17, 59),
            Peak,
            bj(2026, 9, 18, 18, 0),
        ),
        (
            "Fri 18:00",
            bj(2026, 9, 18, 18, 0),
            OffPeak,
            bj(2026, 9, 21, 9, 0),
        ),
        (
            "Sat 10:00",
            bj(2026, 9, 19, 10, 0),
            OffPeak,
            bj(2026, 9, 21, 9, 0),
        ),
        (
            "Sun 23:59",
            bj(2026, 9, 20, 23, 59),
            OffPeak,
            bj(2026, 9, 21, 9, 0),
        ),
    ];
    for (label, moment, period, next) in cases {
        let snapshot = PricingSnapshot::new(moment, &schedule);
        assert_eq!(snapshot.period, period, "{label}: period");
        assert_eq!(snapshot.next_transition, next, "{label}: next transition");
        assert_eq!(
            snapshot.next_period,
            period.opposite(),
            "{label}: next period"
        );
    }
}

#[test]
fn same_instant_in_any_time_zone_gives_the_same_period() {
    let schedule = HolidaySchedule::bundled();
    let instant = bj(2026, 9, 14, 10, 30);
    for hours in [-8, 0, 1, 9] {
        let elsewhere = instant.with_timezone(&FixedOffset::east_opt(hours * 3600).unwrap());
        assert_eq!(
            pricing::period(elsewhere.with_timezone(&Utc), &schedule),
            Peak
        );
    }
}

#[test]
fn holidays_and_make_up_workdays() {
    let schedule = HolidaySchedule::bundled();
    let cases = [
        (
            "make-up workday on a Sunday",
            bj(2026, 9, 20, 10, 0),
            OffPeak,
        ),
        ("Mid-Autumn Friday", bj(2026, 9, 25, 10, 0), OffPeak),
        ("National Day Monday", bj(2026, 10, 5, 10, 0), OffPeak),
        (
            "ordinary Thursday after the holiday",
            bj(2026, 10, 8, 10, 0),
            Peak,
        ),
        (
            "make-up workday on a Saturday",
            bj(2026, 10, 10, 10, 0),
            OffPeak,
        ),
    ];
    for (label, moment, period) in cases {
        assert_eq!(pricing::period(moment, &schedule), period, "{label}");
    }

    assert_eq!(
        pricing::day_info_for_date(day(2026, 10, 10), &schedule).kind,
        DayKind::AlternateWorkdayWeekend("国庆节".into())
    );
    assert_eq!(
        pricing::day_info_for_date(day(2026, 10, 5), &schedule).kind,
        DayKind::PublicHoliday("国庆节".into())
    );
}

#[test]
fn spring_festival_jumps_to_first_peak_after_the_holiday() {
    let schedule = HolidaySchedule::bundled();
    let snapshot = PricingSnapshot::new(bj(2026, 2, 16, 10, 0), &schedule);
    assert_eq!(snapshot.next_transition, bj(2026, 2, 24, 9, 0));
    // The holiday started at the end of Friday's peak, 2026-02-13 18:00.
    assert_eq!(snapshot.interval_start, bj(2026, 2, 13, 18, 0));
}

#[test]
fn interval_progress_and_countdown() {
    let schedule = HolidaySchedule::bundled();
    let snapshot = PricingSnapshot::new(bj(2026, 9, 14, 10, 30), &schedule);
    assert_eq!(snapshot.interval_start, bj(2026, 9, 14, 9, 0));
    assert!((snapshot.interval_progress() - 0.5).abs() < 1e-9);
    assert_eq!(snapshot.seconds_until_transition(), 90 * 60);
    assert_eq!(
        compact_countdown(snapshot.seconds_until_transition()),
        "01:30:00"
    );
    assert_eq!(compact_countdown(100 * 3600 + 61), "100:01:01");
}

#[test]
fn parses_ics_end_dates_folded_lines_and_ignores_ordinary_events() {
    let ics = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\n\
DTSTART;VALUE=DATE:20261001\r\n\
DTEND;VALUE=DATE:20261008\r\n\
SUMMARY;LANGUAGE=zh_CN:国庆节（休）\r\n\
X-APPLE-SPECIAL-DAY:WORK-HOLIDAY\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
DTSTART;VALUE=DATE:20261010\r\n\
SUMMARY;LANGUAGE=zh_CN:国庆节（班）\r\n\
X-APPLE-SPECIAL-DAY:ALTERNATE-\r\n \
WORKDAY\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
DTSTART;VALUE=DATE:20261023\r\n\
SUMMARY;LANGUAGE=zh_CN:霜降\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    let parsed = HolidaySchedule::parse_ics(ics.as_bytes(), Utc::now()).unwrap();
    assert_eq!(parsed.entries.len(), 2, "the solar term is not a day off");
    assert!(parsed.holiday_on(day(2026, 10, 7)).is_some());
    assert!(
        parsed.holiday_on(day(2026, 10, 8)).is_none(),
        "DTEND is exclusive"
    );

    let workday = parsed
        .alternate_workday_on(day(2026, 10, 10))
        .expect("folded line joined");
    assert_eq!(workday.kind, EntryKind::AlternateWorkday);
    assert_eq!(workday.name, "国庆节");
    assert_eq!(
        workday.end_day_exclusive,
        day(2026, 10, 11),
        "missing DTEND means one day"
    );
}

#[test]
fn rejects_calendars_without_holidays_and_detects_regressions() {
    let empty = "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";
    assert!(HolidaySchedule::parse_ics(empty.as_bytes(), Utc::now()).is_err());
    assert!(HolidaySchedule::parse_ics(&[0xff, 0xfe], Utc::now()).is_err());

    let current = HolidaySchedule::bundled();
    let mut older = current.clone();
    for entry in &mut older.entries {
        entry.start_day = entry.start_day.with_year_offset(-1);
        entry.end_day_exclusive = entry.end_day_exclusive.with_year_offset(-1);
    }
    assert!(older.is_regression_from(&current));
    assert!(!current.is_regression_from(&older));
}

#[test]
fn schedule_round_trips_through_json_cache() {
    let schedule = HolidaySchedule::bundled();
    let json = serde_json::to_string(&schedule).unwrap();
    assert!(json.contains(r#""startDay":"2026-01-01""#));
    assert_eq!(
        serde_json::from_str::<HolidaySchedule>(&json).unwrap(),
        schedule
    );
}

trait YearOffset {
    fn with_year_offset(self, years: i32) -> Self;
}

impl YearOffset for NaiveDate {
    fn with_year_offset(self, years: i32) -> Self {
        use chrono::Datelike;
        self.with_year(self.year() + years).unwrap()
    }
}

#[test]
fn month_calendar_marks_off_peak_days() {
    let schedule = HolidaySchedule::bundled();
    let october = pricing::month_days(2026, 10, &schedule);
    assert_eq!(october.len(), 31);
    assert_eq!(october[0].date, day(2026, 10, 1));
    assert_eq!(
        october[0].info.kind,
        DayKind::PublicHoliday("国庆节".into())
    );
    assert!(october[0].hours.iter().all(|&p| p == OffPeak));
    assert_eq!(october[7].info.kind, DayKind::RegularWeekday, "Oct 8");
    assert!(pricing::month_days(2026, 13, &schedule).is_empty());
}

#[test]
fn week_grid_runs_monday_to_sunday_by_hour() {
    let schedule = HolidaySchedule::bundled();
    // Wednesday 2026-09-16 → week of Monday 2026-09-14.
    let week = pricing::week_days(day(2026, 9, 16), &schedule);
    assert_eq!(week.len(), 7);
    assert_eq!(week[0].date, day(2026, 9, 14));
    assert_eq!(week[6].date, day(2026, 9, 20));

    let monday = &week[0].hours;
    let peak_hours: Vec<usize> = (0..24).filter(|&h| monday[h] == Peak).collect();
    assert_eq!(peak_hours, vec![9, 10, 11, 14, 15, 16, 17]);
    assert!(week[5].hours.iter().all(|&p| p == OffPeak), "Saturday");

    // National Day week: holidays Mon–Wed, regular Thu–Fri, make-up workday Saturday.
    let golden = pricing::week_days(day(2026, 10, 5), &schedule);
    let has_peak: Vec<bool> = golden.iter().map(|d| d.hours.contains(&Peak)).collect();
    assert_eq!(
        has_peak,
        vec![false, false, false, true, true, false, false]
    );
}
