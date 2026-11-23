use chrono::{DateTime, Local, TimeZone as _};
use objc2::rc::Retained;
use objc2_foundation::{NSDate, NSDateFormatter, NSLocale, NSString};

const TIME: &str = "jmsSSS";
const DAY: &str = "EEEEMMMMd";

thread_local! {
    static TIMES: Clock = Clock::new(&NSLocale::autoupdatingCurrentLocale());
}

struct Clock {
    time: Retained<NSDateFormatter>,
    day: Retained<NSDateFormatter>,
    width: usize,
}

impl Clock {
    fn new(locale: &NSLocale) -> Self {
        let formatter = |template| {
            let formatter = NSDateFormatter::new();
            formatter.setLocale(Some(locale));
            formatter.setLocalizedDateFormatFromTemplate(&NSString::from_str(template));
            formatter
        };
        let mut clock = Self {
            time: formatter(TIME),
            day: formatter(DAY),
            width: 0,
        };
        let today = Local::now().date_naive();
        clock.width = [10, 22]
            .into_iter()
            .filter_map(|hour| today.and_hms_milli_opt(hour, 59, 59, 999))
            .filter_map(|at| Local.from_local_datetime(&at).earliest())
            .map(|at| clock.time(at).chars().count())
            .max()
            .unwrap_or(0);
        clock
    }

    fn time(&self, at: DateTime<Local>) -> String {
        format(&self.time, at)
    }

    fn aligned(&self, at: DateTime<Local>) -> String {
        format!("{:>width$}", self.time(at), width = self.width)
    }

    fn day(&self, at: DateTime<Local>) -> String {
        format(&self.day, at)
    }
}

fn format(formatter: &NSDateFormatter, at: DateTime<Local>) -> String {
    let date = NSDate::dateWithTimeIntervalSince1970(at.timestamp_millis() as f64 / 1000.0);
    formatter
        .stringFromDate(&date)
        .to_string()
        .replace('\u{202f}', " ")
}

pub(crate) fn time(at: DateTime<Local>) -> String {
    TIMES.with(|clock| clock.time(at))
}

pub(crate) fn aligned_time(at: DateTime<Local>) -> String {
    TIMES.with(|clock| clock.aligned(at))
}

pub(crate) fn day(at: DateTime<Local>) -> String {
    TIMES.with(|clock| clock.day(at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(locale: &str) -> Clock {
        Clock::new(&NSLocale::localeWithLocaleIdentifier(&NSString::from_str(
            locale,
        )))
    }

    #[test]
    fn times_and_days_read_the_way_the_locale_writes_them() {
        let at = Local
            .with_ymd_and_hms(2026, 11, 24, 23, 59, 59)
            .unwrap()
            .checked_add_signed(chrono::TimeDelta::milliseconds(903))
            .unwrap();
        let us = clock("en_US");
        assert_eq!(us.time(at), "11:59:59.903 PM");
        assert_eq!(us.day(at), "Tuesday, November 24");
        let uk = clock("en_GB");
        assert_eq!(uk.time(at), "23:59:59.903");
        assert_eq!(uk.day(at), "Tuesday 24 November");
    }

    #[test]
    fn times_pad_to_the_widest_hour_so_the_column_lines_up() {
        let us = clock("en_US");
        let morning = Local.with_ymd_and_hms(2026, 11, 24, 9, 5, 1).unwrap();
        assert_eq!(us.aligned(morning), " 9:05:01.000 AM");
        assert_eq!(clock("en_GB").aligned(morning), "09:05:01.000");
    }
}
