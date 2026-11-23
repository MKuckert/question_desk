//! Times as the user reads them: local clock time, with the day when it is not today.

use std::time::SystemTime;

use jiff::{Timestamp, ZonedDateTime as Zoned, tz::TimeZone};

fn zoned(time: SystemTime) -> Option<Zoned> {
    Some(
        Timestamp::try_from(time)
            .ok()?
            .to_zoned_datetime(TimeZone::system()),
    )
}

/// "14:50", "yesterday 14:50", "Nov 21, 14:50" or "Nov 21 2025, 14:50".
pub fn clock(time: SystemTime) -> String {
    let Some(at) = zoned(time) else {
        return "at an unknown time".to_owned();
    };
    let today = Zoned::now().with_time_zone(TimeZone::system()).date();
    let date = at.date();
    let clock = at.strftime("%H:%M");
    if date == today {
        clock.to_string()
    } else if today.yesterday().is_ok_and(|y| y == date) {
        format!("yesterday {clock}")
    } else if date.year() == today.year() {
        format!("{}, {clock}", at.strftime("%b %-d"))
    } else {
        format!("{}, {clock}", at.strftime("%b %-d %Y"))
    }
}
