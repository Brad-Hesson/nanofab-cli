use anyhow::{anyhow, bail, Context, Result};
use chrono::{format::ParseErrorKind, Datelike, Days, NaiveDateTime, Weekday};
use itertools::Itertools;
use reqwest::Client;
use scraper::Selector;
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Display};

pub struct NanoFab {
    client: Client,
}
impl NanoFab {
    pub fn new() -> Self {
        Self {
            client: reqwest::ClientBuilder::new()
                .cookie_store(true)
                .build()
                .expect("Creating the client should not fail"),
        }
    }
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<()> {
        self.post(
            "https://admin.nanofab.ualberta.ca/ajax.login.php",
            [
                ("uname", username),
                ("password", password),
                ("eaaa42a1464aa2b40a3ecfd68e2105d7", "1"),
            ],
        )
        .await
        .context("Failed to authenticate")?;
        Ok(())
    }
    pub async fn get_tools(&self) -> Result<Vec<Tool>> {
        let resp = self
            .get("https://admin.nanofab.ualberta.ca/ajax.get-tools.php?term=&hide_inactive=1")
            .await
            .context("Failed to get tool list from server")?;
        Ok(resp
            .as_array()
            .expect("Tool list should be an JSON array")
            .iter()
            .map(|value| {
                let name = value
                    .get("label")
                    .expect("Tool list entry should contain a `label` member")
                    .as_str()
                    .expect("Tool label should be a string")
                    .to_string();
                let id = value
                    .get("id")
                    .expect("Tool list entry should contain a `id` member")
                    .as_str()
                    .expect("Tool id should be a string")
                    .to_string();
                Tool { name, id }
            })
            .collect())
    }
    pub async fn get_tool_bookings(&self, tool: &Tool) -> Result<TimeTable> {
        let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut fail_count = 0;
        let json_value = loop {
            let (nonce, nonce_key) = self.get_nonce("modal.search-tool-bookings.php").await?;
            let maybe_value = self
                .post(
                    "https://admin.nanofab.ualberta.ca/ajax.get-bookings.php",
                    [
                        ("tool_id[]", tool.id.as_str()),
                        ("nonce", &nonce),
                        ("nonce_key", &nonce_key),
                        ("start_date", &current_date),
                    ],
                )
                .await;
            match maybe_value {
                Ok(value) => break value,
                Err(e) => {
                    fail_count += 1;
                    if fail_count >= 10 {
                        println!("Failed too many times");
                        bail!(e)
                    }
                }
            }
        };
        let html = scraper::Html::parse_fragment(json_value.get("msg").unwrap().as_str().unwrap());
        let mut bookings = vec![];
        for booking in html.select(&Selector::parse("[id^=booking-]").unwrap()) {
            let selector = Selector::parse("[title]").unwrap();
            let mut values = booking.select(&selector);
            let start = parse_datetime(values.next().unwrap().value().attr("title").unwrap())?;
            let end = parse_datetime(values.next().unwrap().value().attr("title").unwrap())?;
            bookings.push(TimeSlot::new(Some(start), Some(end)));
        }
        Ok(TimeTable::new(bookings))
    }
    pub async fn get_nonce(&self, modal: &str) -> Result<(String, String)> {
        let json_value = self
            .post(
                "https://admin.nanofab.ualberta.ca/ajax.load-modal.php",
                [
                    ("class", "ajax-panel"),
                    ("source", "ajax.load-modal.php"),
                    ("load", modal),
                ],
            )
            .await?;
        let html = scraper::Html::parse_fragment(json_value.get("msg").unwrap().as_str().unwrap());
        let nonce = html
            .select(&Selector::parse("[name=nonce]").unwrap())
            .exactly_one()
            .unwrap()
            .value()
            .attr("value")
            .unwrap()
            .to_string();
        let nonce_key = html
            .select(&Selector::parse("[name=nonce_key]").unwrap())
            .exactly_one()
            .unwrap()
            .value()
            .attr("value")
            .unwrap()
            .to_string();
        Ok((nonce, nonce_key))
    }
    pub async fn get(&self, url: &str) -> Result<Value> {
        let mut resp = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to send get request")?
            .bytes()
            .await
            .context("Failed to recieve bytes of response body")?
            .to_vec();
        serde_json::from_str(
            std::str::from_utf8_mut(&mut resp[..]).context("Response body is not utf-8")?,
        )
        .context("Response body is not valid json")
    }
    pub async fn post(
        &self,
        url: &str,
        body: impl IntoIterator<Item = (&str, &str)>,
    ) -> Result<Value> {
        let mut resp = self
            .client
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.into_iter().map(|(k, v)| format!("{k}={v}")).join("&"))
            .send()
            .await
            .context("Failed to send post request")?
            .bytes()
            .await
            .context("Failed to recieve bytes of response body")?
            .to_vec();
        let json: serde_json::Value = serde_json::from_str(
            std::str::from_utf8_mut(&mut resp[..]).expect("Response body was not utf-8"),
        )
        .expect("Response body was not valid JSON");
        match json
            .get("error")
            .expect("json body did not contain error field")
            .as_bool()
            .expect("json error field was not a bool")
        {
            true => Err(anyhow!(json
                .get("msg")
                .expect("Server error did not contain a `msg` field")
                .as_str()
                .expect("Server error message was not a string")
                .to_string()))
            .context("Server responded with an error"),
            false => Ok(json),
        }
    }
}

fn parse_datetime(datetime_string: &str) -> Result<NaiveDateTime> {
    let mut fixed_str = datetime_string
        .trim_end_matches(|c: char| c.is_alphabetic())
        .to_string();
    if fixed_str.chars().nth(1).unwrap() == ':' {
        fixed_str.insert(0, '0');
    }
    let current_year = chrono::Local::now()
        .format("%Y")
        .to_string()
        .parse::<usize>()
        .expect("Paring current year should never fail");
    let maybe_datetime = chrono::NaiveDateTime::parse_from_str(
        &format!("{fixed_str} {current_year}"),
        "%I:%M%P %a %b %e %Y",
    );
    match maybe_datetime {
        Err(e) if e.kind() == ParseErrorKind::Impossible => chrono::NaiveDateTime::parse_from_str(
            &format!("{fixed_str} {}", current_year + 1),
            "%I:%M%P %a %b %e %Y",
        ),
        Ok(dt) => Ok(dt),
        Err(e) => Err(e),
    }
    .with_context(|| format!("Failed to parse datetime `{fixed_str}`"))
}

#[derive(Debug, Clone)]
pub struct TimeSlot {
    start: Option<NaiveDateTime>,
    end: Option<NaiveDateTime>,
}
impl TimeSlot {
    pub fn new(start: Option<NaiveDateTime>, end: Option<NaiveDateTime>) -> Self {
        Self { start, end }
    }
    pub fn start(&self) -> &Option<NaiveDateTime> {
        &self.start
    }
    pub fn end(&self) -> &Option<NaiveDateTime> {
        &self.end
    }
    pub fn add_days(&mut self, days: u64) {
        self.start
            .as_mut()
            .map(|dt| *dt = dt.checked_add_days(Days::new(days)).unwrap());
        self.end
            .as_mut()
            .map(|dt| *dt = dt.checked_add_days(Days::new(days)).unwrap());
    }
    pub fn sub_days(&mut self, days: u64) {
        self.start
            .as_mut()
            .map(|dt| *dt = dt.checked_sub_days(Days::new(days)).unwrap());
        self.end
            .as_mut()
            .map(|dt| *dt = dt.checked_sub_days(Days::new(days)).unwrap());
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

pub struct TimeTable {
    timeslots: Vec<TimeSlot>,
}
impl TimeTable {
    pub fn new(timeslots: impl IntoIterator<Item = TimeSlot>) -> Self {
        Self {
            timeslots: timeslots.into_iter().collect(),
        }
    }
    pub fn inverted(self) -> Self {
        match &self.timeslots[..] {
            [] => return Self::new([TimeSlot::new(None, None)]),
            [ts] if ts.start().is_none() & ts.end().is_none() => return Self::new([]),
            _ => {}
        }
        let mut new_timeslots = vec![];
        if let Some(dt) = self.timeslots.first().unwrap().start() {
            new_timeslots.push(TimeSlot::new(None, Some(dt.clone())))
        }
        for (a, b) in self.timeslots.iter().tuple_windows() {
            if a.end() != b.start() {
                new_timeslots.push(TimeSlot::new(a.end().clone(), b.start().clone()))
            }
        }
        if let Some(dt) = self.timeslots.last().unwrap().end() {
            new_timeslots.push(TimeSlot::new(Some(dt.clone()), None))
        }
        Self::new(new_timeslots)
    }
    pub fn subtract_before_now(&mut self) {
        let now = chrono::Local::now().naive_local();
        let before_now = TimeSlot::new(None, Some(now));
        self.subtract_timeslot(&before_now);
    }
    pub fn subtract_weekends(&mut self) {
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
        let mut weekend = TimeSlot::new(Some(saturday_morning), Some(monday_morning));
        while weekend.start.unwrap() <= last_time {
            self.subtract_timeslot(&weekend);
            weekend.add_days(7);
        }
    }
    pub fn subract_after_hours(&mut self) {
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
        let mut overnight = TimeSlot::new(Some(day_end), Some(next_day_start));
        overnight.sub_days(2);
        while overnight.start.unwrap() <= last_time {
            self.subtract_timeslot(&overnight);
            overnight.add_days(1);
        }
    }
    pub fn subtract_timeslot(&mut self, timeslot: &TimeSlot) {
        match (timeslot.start, timeslot.end) {
            (None, None) => self.timeslots.clear(),
            (None, Some(end)) => {
                for i in (0..self.timeslots.len()).rev() {
                    match self.timeslots[i].compare_datetime(end) {
                        RelTime::Before => {}
                        RelTime::Contains => {
                            let new_start = Some(end);
                            let new_end = self.timeslots[i].end;
                            self.timeslots.remove(i);
                            if new_end != new_start {
                                self.timeslots.insert(i, TimeSlot::new(new_start, new_end));
                            }
                        }
                        RelTime::After => {
                            self.timeslots.remove(i);
                        }
                    }
                }
            }
            (Some(start), None) => {
                for i in (0..self.timeslots.len()).rev() {
                    match self.timeslots[i].compare_datetime(start) {
                        RelTime::Before => {
                            self.timeslots.remove(i);
                        }
                        RelTime::Contains => {
                            let new_start = self.timeslots[i].start;
                            let new_end = Some(start);
                            self.timeslots.remove(i);
                            if new_end != new_start {
                                self.timeslots.insert(i, TimeSlot::new(new_start, new_end));
                            }
                        }
                        RelTime::After => {}
                    }
                }
            }
            (Some(start), Some(end)) => {
                use RelTime::*;
                for i in (0..self.timeslots.len()).rev() {
                    match (
                        self.timeslots[i].compare_datetime(start),
                        self.timeslots[i].compare_datetime(end),
                    ) {
                        (Before, Before) | (After, After) => {}
                        (Before, After) => {
                            self.timeslots.remove(i);
                        }
                        (Before, Contains) => {
                            let new_start = Some(end);
                            let new_end = self.timeslots[i].end;
                            self.timeslots.remove(i);
                            if new_end != new_start {
                                self.timeslots.insert(i, TimeSlot::new(new_start, new_end));
                            }
                        }
                        (Contains, After) => {
                            let new_start = self.timeslots[i].start;
                            let new_end = Some(start);
                            self.timeslots.remove(i);
                            if new_end != new_start {
                                self.timeslots.insert(i, TimeSlot::new(new_start, new_end));
                            }
                        }
                        (Contains, Contains) => {
                            let new_start_1 = self.timeslots[i].start;
                            let new_end_1 = Some(start);
                            let new_start_2 = Some(end);
                            let new_end_2 = self.timeslots[i].end;
                            self.timeslots.remove(i);
                            if new_end_2 != new_start_2 {
                                self.timeslots
                                    .insert(i, TimeSlot::new(new_start_2, new_end_2));
                            }
                            if new_end_1 != new_start_1 {
                                self.timeslots
                                    .insert(i, TimeSlot::new(new_start_1, new_end_1));
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
}
impl Display for TimeTable {
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

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub id: String,
}
