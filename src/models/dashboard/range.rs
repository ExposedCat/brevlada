use chrono::{DateTime, Duration, Local, Months, NaiveDate, TimeZone};

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub from: i64,
    pub until: i64,
}

impl Bounds {
    pub fn overlaps(self, start: &str, end: Option<&str>, exclusive_date_end: bool) -> bool {
        let Some(start_time) = super::timestamp(start) else {
            return false;
        };
        let end_time = if let Ok(date) = NaiveDate::parse_from_str(end.unwrap_or(start), "%Y-%m-%d")
        {
            Some(midnight(if exclusive_date_end {
                date
            } else {
                date.succ_opt().unwrap()
            }))
        } else {
            super::timestamp(end.unwrap_or(start)).map(|time| time + i64::from(end.is_none()))
        };
        start_time < self.until && end_time.is_some_and(|end| end > self.from)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Range {
    Day,
    Week,
    #[default]
    Month,
    Quarter,
    Year,
    Custom {
        from: NaiveDate,
        to: NaiveDate,
    },
}

impl Range {
    pub const PRESETS: [Self; 5] = [
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::Year,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Quarter => "3m",
            Self::Year => "1y",
            Self::Custom { .. } => "Custom",
        }
    }

    pub fn past(self, now: i64) -> Bounds {
        self.bounds(now, false)
    }

    pub fn future(self, now: i64) -> Bounds {
        self.bounds(now, true)
    }

    fn bounds(self, now: i64, future: bool) -> Bounds {
        let now = DateTime::from_timestamp(now, 0)
            .unwrap()
            .with_timezone(&Local);
        let today = now.date_naive();
        let tomorrow = today.succ_opt().unwrap();
        let (from, until) = match self {
            Self::Custom { from, to } => (from, to.succ_opt().unwrap()),
            Self::Day | Self::Week | Self::Month => {
                let days = match self {
                    Self::Day => 1,
                    Self::Week => 7,
                    _ => 30,
                };
                if future {
                    (today, today + Duration::days(days))
                } else {
                    (today - Duration::days(days - 1), tomorrow)
                }
            }
            Self::Quarter | Self::Year => {
                let months = Months::new(if self == Self::Quarter { 3 } else { 12 });
                if future {
                    (today, today.checked_add_months(months).unwrap())
                } else {
                    (today.checked_sub_months(months).unwrap(), tomorrow)
                }
            }
        };
        Bounds {
            from: midnight(from),
            until: midnight(until),
        }
    }
}

fn midnight(date: NaiveDate) -> i64 {
    Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .unwrap()
        .timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_look_back_for_purchases_and_forward_for_plans() {
        let now = Local.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap();
        let today = now.date_naive();
        for (range, days) in [(Range::Day, 1), (Range::Week, 7), (Range::Month, 30)] {
            let past = range.past(now.timestamp());
            let future = range.future(now.timestamp());
            assert_eq!(past.from, midnight(today - Duration::days(days - 1)));
            assert_eq!(past.until, midnight(today.succ_opt().unwrap()));
            assert_eq!(future.from, midnight(today));
            assert_eq!(future.until, midnight(today + Duration::days(days)));
            assert!(past.from <= now.timestamp() && now.timestamp() < past.until);
            assert!(future.from <= now.timestamp() && now.timestamp() < future.until);
        }
        for (range, months) in [(Range::Quarter, 3), (Range::Year, 12)] {
            assert_eq!(
                range.past(now.timestamp()).from,
                midnight(today.checked_sub_months(Months::new(months)).unwrap())
            );
            assert_eq!(
                range.future(now.timestamp()).until,
                midnight(today.checked_add_months(Months::new(months)).unwrap())
            );
        }
    }

    #[test]
    fn custom_dates_include_both_whole_local_days() {
        let from = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        let range = Range::Custom { from, to };
        let bounds = range.past(0);
        assert_eq!(range.future(0).from, bounds.from);
        assert_eq!(range.future(0).until, bounds.until);
        assert_eq!(bounds.from, midnight(from));
        assert_eq!(bounds.until, midnight(to.succ_opt().unwrap()));
        assert!(bounds.overlaps("2026-10-26T23:59:59", None, false));
        assert!(!bounds.overlaps("2026-10-27", None, false));
    }
}
