use std::fmt::Display;

use chrono::{Datelike, Days, Duration, NaiveDateTime, Weekday};
use itertools::Itertools;

#[derive(Debug, Clone)]
pub struct TimeSlot<M> {
    start: Option<NaiveDateTime>,
    end: Option<NaiveDateTime>,
    meta: M,
}
impl<M> TimeSlot<M> {
    pub fn new(start: Option<NaiveDateTime>, end: Option<NaiveDateTime>, meta: M) -> Self {
        Self { start, end, meta }
    }
    pub fn start(&self) -> &Option<NaiveDateTime> {
        &self.start
    }
    pub fn end(&self) -> &Option<NaiveDateTime> {
        &self.end
    }
    pub fn add_days(&mut self, days: u64) {
        if let Some(dt) = self.start.as_mut() {
            *dt = dt.checked_add_days(Days::new(days)).unwrap()
        }
        if let Some(dt) = self.end.as_mut() {
            *dt = dt.checked_add_days(Days::new(days)).unwrap()
        }
    }
    pub fn sub_days(&mut self, days: u64) {
        if let Some(dt) = self.start.as_mut() {
            *dt = dt.checked_sub_days(Days::new(days)).unwrap()
        }
        if let Some(dt) = self.end.as_mut() {
            *dt = dt.checked_sub_days(Days::new(days)).unwrap()
        }
    }
    pub fn duration(&self) -> Option<Duration> {
        match (self.start, self.end) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        }
    }
    pub fn compare_datetime(&self, datetime: NaiveDateTime) -> RelTime {
        use RelTime::*;
        match (self.start, self.end) {
            (None, None) => Contains,
            (None, Some(end)) => {
                if datetime <= end {
                    Contains
                } else {
                    After
                }
            }
            (Some(start), None) => {
                if start <= datetime {
                    Contains
                } else {
                    Before
                }
            }
            (Some(start), Some(end)) => {
                if start <= datetime && datetime <= end {
                    Contains
                } else if datetime < start {
                    Before
                } else {
                    After
                }
            }
        }
    }
}

pub enum RelTime {
    Before,
    Contains,
    After,
}

pub struct TimeTable<M> {
    timeslots: Vec<TimeSlot<M>>,
}
impl<M> TimeTable<M> {
    pub fn new(timeslots: impl IntoIterator<Item = TimeSlot<M>>) -> Self {
        Self {
            timeslots: timeslots.into_iter().collect(),
        }
    }
    pub fn timeslots(&self) -> &Vec<TimeSlot<M>> {
        &self.timeslots
    }
    pub fn subtract_less_duration(&mut self, duration: Duration) {
        self.timeslots = self
            .timeslots
            .drain(..)
            .filter(|ts| match ts.duration() {
                Some(dur) => dur >= duration,
                None => true,
            })
            .collect();
    }
    pub fn inverted(self) -> TimeTable<()> {
        match &self.timeslots[..] {
            [] => return TimeTable::new([TimeSlot::new(None, None, ())]),
            [ts] if ts.start().is_none() & ts.end().is_none() => return TimeTable::new([]),
            _ => {}
        }
        let mut new_timeslots = vec![];
        if let Some(dt) = self.timeslots.first().unwrap().start() {
            new_timeslots.push(TimeSlot::new(None, Some(*dt), ()))
        }
        for (a, b) in self.timeslots.iter().tuple_windows() {
            if a.end() != b.start() {
                new_timeslots.push(TimeSlot::new(*a.end(), *b.start(), ()))
            }
        }
        if let Some(dt) = self.timeslots.last().unwrap().end() {
            new_timeslots.push(TimeSlot::new(Some(*dt), None, ()))
        }
        TimeTable::new(new_timeslots)
    }
    pub fn subtract_before_now(&mut self)
    where
        M: Clone,
    {
        let now = chrono::Local::now().naive_local();
        let before_now = TimeSlot::new(None, Some(now), ());
        self.subtract_timeslot(&before_now);
    }
    pub fn subtract_weekends(&mut self)
    where
        M: Clone,
    {
        let last_time = match self.timeslots.last().unwrap().end {
            Some(dt) => dt,
            None => self
                .timeslots
                .last()
                .unwrap()
                .start
                .expect("Should be no unbounded slots inside timetable"),
        };
        let now = chrono::Local::now().naive_local();
        let mut saturday_morning = now.date().and_hms_opt(0, 0, 0).unwrap();
        while saturday_morning.weekday() != Weekday::Sat {
            saturday_morning = saturday_morning.checked_sub_days(Days::new(1)).unwrap();
        }
        let monday_morning = saturday_morning.checked_add_days(Days::new(2)).unwrap();
        let mut weekend = TimeSlot::new(Some(saturday_morning), Some(monday_morning), ());
        while weekend.start.unwrap() <= last_time {
            self.subtract_timeslot(&weekend);
            weekend.add_days(7);
        }
    }
    pub fn subtract_after_hours(&mut self)
    where
        M: Clone,
    {
        let last_time = match self.timeslots.last().unwrap().end {
            Some(dt) => dt,
            None => self
                .timeslots
                .last()
                .unwrap()
                .start
                .expect("Should be no unbounded slots inside timetable"),
        };
        let now = chrono::Local::now().naive_local();
        let today = now.date();
        let day_end = today
            .and_hms_opt(17, 0, 0)
            .expect("Creating day start should not fail");
        let next_day_start = today
            .and_hms_opt(8, 0, 0)
            .expect("Creating day start should not fail")
            .checked_add_days(Days::new(1))
            .expect("adding days should not fail");
        let mut overnight = TimeSlot::new(Some(day_end), Some(next_day_start), ());
        overnight.sub_days(2);
        while overnight.start.unwrap() <= last_time {
            self.subtract_timeslot(&overnight);
            overnight.add_days(1);
        }
    }
    pub fn subtract_timeslot<MO>(&mut self, timeslot: &TimeSlot<MO>)
    where
        M: Clone,
    {
        match (timeslot.start, timeslot.end) {
            (None, None) => self.timeslots.clear(),
            (None, Some(other_end)) => {
                for i in (0..self.timeslots.len()).rev() {
                    match self.timeslots[i].compare_datetime(other_end) {
                        RelTime::Before => {}
                        RelTime::Contains => {
                            let current = self.timeslots.remove(i);
                            let new_start = Some(other_end);
                            let new_end = current.end;
                            if new_end != new_start {
                                self.timeslots
                                    .insert(i, TimeSlot::new(new_start, new_end, current.meta));
                            }
                        }
                        RelTime::After => {
                            self.timeslots.remove(i);
                        }
                    }
                }
            }
            (Some(other_start), None) => {
                for i in (0..self.timeslots.len()).rev() {
                    match self.timeslots[i].compare_datetime(other_start) {
                        RelTime::Before => {
                            self.timeslots.remove(i);
                        }
                        RelTime::Contains => {
                            let current = self.timeslots.remove(i);
                            let new_start = current.start;
                            let new_end = Some(other_start);
                            self.timeslots.remove(i);
                            if new_end != new_start {
                                self.timeslots
                                    .insert(i, TimeSlot::new(new_start, new_end, current.meta));
                            }
                        }
                        RelTime::After => {}
                    }
                }
            }
            (Some(other_start), Some(other_end)) => {
                use RelTime::*;
                for i in (0..self.timeslots.len()).rev() {
                    match (
                        self.timeslots[i].compare_datetime(other_start),
                        self.timeslots[i].compare_datetime(other_end),
                    ) {
                        (Before, Before) | (After, After) => {}
                        (Before, After) => {
                            self.timeslots.remove(i);
                        }
                        (Before, Contains) => {
                            let current = self.timeslots.remove(i);
                            let new_start = Some(other_end);
                            let new_end = current.end;
                            if new_end != new_start {
                                self.timeslots
                                    .insert(i, TimeSlot::new(new_start, new_end, current.meta));
                            }
                        }
                        (Contains, After) => {
                            let current = self.timeslots.remove(i);
                            let new_start = current.start;
                            let new_end = Some(other_start);
                            if new_end != new_start {
                                self.timeslots
                                    .insert(i, TimeSlot::new(new_start, new_end, current.meta));
                            }
                        }
                        (Contains, Contains) => {
                            let current = self.timeslots.remove(i);
                            let new_start_1 = current.start;
                            let new_end_1 = Some(other_start);
                            let new_start_2 = Some(other_end);
                            let new_end_2 = current.end;
                            if new_end_2 != new_start_2 {
                                self.timeslots.insert(
                                    i,
                                    TimeSlot::new(new_start_2, new_end_2, current.meta.clone()),
                                );
                            }
                            if new_end_1 != new_start_1 {
                                self.timeslots
                                    .insert(i, TimeSlot::new(new_start_1, new_end_1, current.meta));
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
}
impl<M> Display for TimeTable<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut prev_date = match self.timeslots.get(0) {
            Some(ts) => match (ts.start(), ts.end()) {
                (Some(dt), _) | (None, Some(dt)) => dt.date(),
                _ => panic!("Timeslot cannot be endless on the start and end"),
            },
            None => return f.write_str("Empty Timetable"),
        };
        f.write_fmt(format_args!(
            "[ {:^23} ]\n",
            prev_date.format("%A %b %e %Y")
        ))?;
        for (i, mdt) in self
            .timeslots
            .iter()
            .flat_map(|ts| [ts.start(), ts.end()])
            .enumerate()
        {
            if let Some(dt) = mdt {
                if dt.date() != prev_date {
                    prev_date = dt.date();
                    if i % 2 == 1 {
                        f.write_str(" - ")?;
                    }
                    f.write_str("\n")?;
                    f.write_fmt(format_args!(
                        "[ {:^23} ]\n",
                        prev_date.format("%A %b %e %Y")
                    ))?;
                    if i % 2 == 1 {
                        f.write_str("       ")?;
                    }
                }
            }
            if i % 2 == 1 {
                f.write_str(" - ")?;
            }
            match mdt {
                Some(dt) => f.write_str(&dt.format("%l:%M%P").to_string())?,
                None => f.write_str("       ")?,
            }
            if i % 2 == 1 {
                f.write_str("\n")?;
            }
        }
        Ok(())
    }
}
