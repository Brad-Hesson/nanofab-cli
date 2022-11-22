use anyhow::{anyhow, Context, Result};
use chrono::{format::ParseErrorKind, NaiveDate, NaiveDateTime};
use futures_util::TryFutureExt;
use itertools::Itertools;
use reqwest::Client;
use scraper::Selector;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::future::Future;

use crate::schedule::{TimeSlot, TimeTable};

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
    pub async fn authenticate(&self, login: &Login) -> Result<()> {
        self.post(
            "https://admin.nanofab.ualberta.ca/ajax.login.php",
            [
                ("uname", login.username.as_str()),
                ("password", login.password.as_str()),
                ("eaaa42a1464aa2b40a3ecfd68e2105d7", "1"),
            ],
        )
        .await
        .context("Failed to authenticate")?;
        Ok(())
    }
    pub async fn get_tools(&self) -> Result<Vec<Tool>> {
        self.get::<Vec<Tool>>(
            "https://admin.nanofab.ualberta.ca/ajax.get-tools.php?term=&hide_inactive=1",
        )
        .await
        .context("Failed to get tool list from server")
    }
    pub async fn get_tool_from_label(&self, label: &str) -> Result<Tool> {
        self.get::<Vec<Tool>>(
            format!(
                "https://admin.nanofab.ualberta.ca/ajax.get-tools.php?term={label}&hide_inactive=1"
            )
            .as_str(),
        )
        .await
        .context("Failed to get tool from server")?
        .into_iter()
        .find(|tool| &tool.label == label)
        .context("No tools match label")
    }
    pub async fn get_user_bookings(&self) -> Result<TimeTable<String>> {
        let msg = self
            .post(
                "https://admin.nanofab.ualberta.ca/ajax.load-modal.php",
                [("noclass", "1"), ("load", "modal.user.bookings.php")],
            )
            .await?;
        let html = scraper::Html::parse_fragment(&msg);
        let mut bookings = vec![];
        for booking in html.select(&Selector::parse("[id^=booking-]").unwrap()) {
            let selector = Selector::parse("[class^=columns]").unwrap();
            let mut values = booking.select(&selector);
            let name = values
                .next()
                .unwrap()
                .text()
                .collect::<String>()
                .trim()
                .to_string();
            let time = parse_inject_year(
                &values.next().unwrap().text().collect::<String>().trim(),
                "%b %-d @ %-I:%M %P",
            )
            .expect("Time did not parse");
            let tool = self.get_tool_from_label(&name).await?;
            let timeslot = self.get_tool_booking_at_time(&tool, time).await?;
            bookings.push(timeslot);
        }
        Ok(TimeTable::new(bookings))
    }
    pub async fn get_tool_booking_at_time(
        &self,
        tool: &Tool,
        time: NaiveDateTime,
    ) -> Result<TimeSlot<String>> {
        self.get_tool_bookings(tool, Some(time.date()), Some(time.date()))
            .await?
            .timeslots()
            .into_iter()
            .cloned()
            .find(|timeslot| timeslot.start() == &Some(time))
            .ok_or(anyhow!("Booking not found"))
    }
    pub async fn get_tool_bookings(
        &self,
        tool: &Tool,
        start: Option<NaiveDate>,
        end: Option<NaiveDate>,
    ) -> Result<TimeTable<String>> {
        let mut body = vec![("tool_id[]", tool.id.clone())];
        if let Some(start) = start {
            body.push(("start_date", start.format("%Y-%m-%d").to_string()));
        }
        if let Some(end) = end {
            body.push(("end_date", end.format("%Y-%m-%d").to_string()));
        }
        let msg = retry(
            || {
                self.get_nonce("modal.search-tool-bookings.php")
                    .and_then(|(nonce, nonce_key)| {
                        self.post(
                            "https://admin.nanofab.ualberta.ca/ajax.get-bookings.php",
                            body.iter()
                                .cloned()
                                .chain([("nonce", nonce), ("nonce_key", nonce_key)].into_iter())
                                .collect_vec(),
                        )
                    })
            },
            10,
        )
        .await
        .context("Failed to get bookings from server")?;
        let html = scraper::Html::parse_fragment(&msg);
        let mut bookings = vec![];
        for booking in html.select(&Selector::parse("[id^=booking-]").unwrap()) {
            let selector = Selector::parse("[title]").unwrap();
            let mut values = booking
                .select(&selector)
                .map(|v| v.value().attr("title").unwrap());
            let time_fmt = "%-I:%M%P %a %b %-d";
            let trim_ordinals = |c: char| "stndrdth".contains(c);
            let start = parse_inject_year(
                values.next().unwrap().trim_end_matches(trim_ordinals),
                time_fmt,
            )?;
            let end = parse_inject_year(
                values.next().unwrap().trim_end_matches(trim_ordinals),
                time_fmt,
            )?;
            let name = values.next().unwrap().to_string();
            bookings.push(TimeSlot::new(Some(start), Some(end), name));
        }
        Ok(TimeTable::new(bookings))
    }
    pub async fn get_nonce(&self, modal: &str) -> Result<(String, String)> {
        let msg = self
            .post(
                "https://admin.nanofab.ualberta.ca/ajax.load-modal.php",
                [
                    ("class", "ajax-panel"),
                    ("source", "ajax.load-modal.php"),
                    ("load", modal),
                ],
            )
            .await?;
        let html = scraper::Html::parse_fragment(&msg);
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
    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to send get request")?
            .bytes()
            .await
            .context("Failed to recieve bytes of response body")?;
        serde_json::from_slice(&resp).context("Server response could not be parsed")
    }
    pub async fn post(
        &self,
        url: &str,
        body: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
    ) -> Result<String> {
        let resp = self
            .client
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(
                body.into_iter()
                    .map(|(k, v)| format!("{}={}", k.as_ref(), v.as_ref()))
                    .join("&"),
            )
            .send()
            .await
            .context("Failed to send post request")?
            .bytes()
            .await
            .context("Failed to recieve bytes of response body")?;
        let json = serde_json::from_slice::<PostResponse>(&resp)
            .expect("Response body was not valid JSON");
        if json.error {
            Err(anyhow!(json.msg))
        } else {
            Ok(json.msg)
        }
    }
}

async fn retry<F, T>(mut f: impl FnMut() -> F, retries: usize) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    let mut retry_count = 0;
    loop {
        match f().await {
            ok @ Ok(_) => break ok,
            err @ Err(_) if retry_count >= retries => {
                break err.with_context(|| format!("Failed {retries} times"))
            }
            _ => {}
        }
        retry_count += 1;
    }
}
fn parse_inject_year(datetime_string: &str, fmt: &str) -> Result<NaiveDateTime> {
    let fmt_with_year = fmt.to_string() + " %Y";
    let current_year = chrono::Local::now()
        .format("%Y")
        .to_string()
        .parse::<isize>()
        .expect("Paring current year should never fail");
    for n in (0..10).flat_map(|n| [n, -n]) {
        let maybe_datetime = chrono::NaiveDateTime::parse_from_str(
            &format!("{datetime_string} {}", current_year + n),
            &fmt_with_year,
        );
        match maybe_datetime {
            Err(e) if e.kind() == ParseErrorKind::Impossible => {}
            Ok(dt) => return Ok(dt),
            ok @ Err(_) => {
                return ok.with_context(|| format!("Failed to parse `{datetime_string}`"))
            }
        }
    }
    Err(anyhow!("Could not find year for `{datetime_string}`"))
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    pub label: String,
    pub value: String,
    pub text: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Login {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct PostResponse {
    error: bool,
    #[serde(alias = "location")]
    msg: String,
}
