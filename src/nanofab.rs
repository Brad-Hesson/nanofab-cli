use crate::{
    html::Element,
    schedule::{TimeSlot, TimeTable},
};

use anyhow::{anyhow, Context, Result};
use chrono::{format::ParseErrorKind, NaiveDate, NaiveDateTime};
use itertools::Itertools;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use urlencoding::encode;

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
    pub async fn get_user_projects(&self) -> Result<Vec<Project>> {
        let body = [("load", "modal.tool-booking.php")];
        let root = self
            .post("https://admin.nanofab.ualberta.ca/ajax.load-modal.php", body)
            .await?
            .parse::<Element>()?;
        let projects = root
            .iter_decendents()
            .find(|elem| elem.has_attr("id", Some("sel_project_id")))
            .unwrap()
            .iter_children()
            .filter(|elem| elem.has_attr("class", None))
            .map(|elem| {
                let name =
                    elem.iter_contents().cloned().exactly_one().ok().unwrap().as_text().unwrap();
                let id = elem.get_attr("value").unwrap().to_string();
                Project { name, id }
            })
            .collect_vec();
        Ok(projects)
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
    pub async fn get_user_bookings(&self) -> Result<TimeTable<(String, String)>> {
        let root = self
            .post(
                "https://admin.nanofab.ualberta.ca/ajax.load-modal.php",
                [("load", "modal.user.bookings.php")],
            )
            .await?
            .parse::<Element>()?;
        let mut bookings = vec![];
        for booking_elem in
            root.iter_decendents().filter(|elem| elem.has_attr("id", Some("booking-")))
        {
            let (name_str, time_str) = booking_elem
                .iter_decendents()
                .filter(|elem| elem.has_attr("class", Some("columns")))
                .map(|elem| elem.iter_contents().cloned().find_map(|c| c.as_text()).unwrap())
                .collect_tuple()
                .unwrap();
            let name = name_str.trim().to_string();
            let time =
                parse_yearless(time_str.trim(), "%b %-d @ %-I:%M %P").expect("Time did not parse");
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
    ) -> Result<TimeSlot<(String, String)>> {
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
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<TimeTable<(String, String)>> {
        let mut body = vec![("tool_id[]", tool.id.clone())];
        if let Some(start) = start_date {
            body.push(("start_date", start.format("%Y-%m-%d").to_string()));
        }
        if let Some(end) = end_date {
            body.push(("end_date", end.format("%Y-%m-%d").to_string()));
        }
        let (nonce, nonce_key) = self.get_nonce("modal.search-tool-bookings.php").await?;
        body.push(("nonce", nonce));
        body.push(("nonce_key", nonce_key));
        let root = self
            .post("https://admin.nanofab.ualberta.ca/ajax.get-bookings.php", body)
            .await?
            .parse::<Element>()?;
        let mut bookings = vec![];
        for booking_elem in
            root.iter_decendents().filter(|elem| elem.has_attr("id", Some("booking-")))
        {
            let (start_str, end_str, name_str) = booking_elem
                .iter_decendents()
                .filter_map(|elem| elem.get_attr("title"))
                .collect_tuple()
                .unwrap();
            let time_fmt = "%-I:%M%P %a %b %-d";
            let trim_ordinals = |c: char| "stndrh".contains(c);
            let start = parse_yearless(start_str.trim_end_matches(trim_ordinals), time_fmt)?;
            let end = parse_yearless(end_str.trim_end_matches(trim_ordinals), time_fmt)?;
            let (name, email) = name_str.split_once(" <br/> ").unwrap();
            bookings.push(TimeSlot::new(
                Some(start),
                Some(end),
                (name.to_string(), email.to_string()),
            ));
        }
        Ok(TimeTable::new(bookings))
    }
    pub async fn get_nonce(&self, modal: &str) -> Result<(String, String)> {
        let url = "https://admin.nanofab.ualberta.ca/ajax.load-modal.php";
        let root = self.post(url, [("load", modal)]).await?.parse::<Element>()?;
        let nonce = root
            .iter_decendents()
            .find(|elem| elem.has_attr("name", Some("nonce")))
            .unwrap()
            .get_attr("value")
            .unwrap();
        let nonce_key = root
            .iter_decendents()
            .find(|elem| elem.has_attr("name", Some("nonce_key")))
            .unwrap()
            .get_attr("value")
            .unwrap();
        Ok((encode(nonce).to_string(), nonce_key.to_string()))
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
            .body(body.into_iter().map(|(k, v)| format!("{}={}", k.as_ref(), v.as_ref())).join("&"))
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

fn parse_yearless(datetime_string: &str, fmt: &str) -> Result<NaiveDateTime> {
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

#[derive(Debug)]
pub struct Project {
    name: String,
    id: String,
}
