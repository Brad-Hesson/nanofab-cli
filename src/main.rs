mod nanofab;

use crate::nanofab::NanoFab;
use anyhow::{Ok, Result};
use chrono::Duration;
use itertools::Itertools;

#[tokio::main]
async fn main() -> Result<()> {
    let client = NanoFab::new();
    println!("Input username:");
    let mut username = String::new();
    std::io::stdin().read_line(&mut username)?;
    println!("Input password:");
    let mut password = String::new();
    std::io::stdin().read_line(&mut password)?;
    client
        .authenticate(&username.trim(), &password.trim())
        .await?;
    println!("Authentication Successful");
    let (tool_name, tool_id) = tool_select(&client).await?;
    println!("Enter time required (hh:mm):");
    let mut time_str = String::new();
    std::io::stdin().read_line(&mut time_str)?;
    let (hours, minutes) = time_str.trim().split_once(':').unwrap();
    let duration =
        Duration::hours(hours.parse().unwrap()) + Duration::minutes(minutes.parse().unwrap());
    let bookings = client.get_tool_bookings(&tool_id).await?;
    let mut openings = bookings.inverted();
    openings.subtract_before_now();
    openings.subtract_weekends();
    openings.subtract_after_hours();
    openings.subtract_less_duration(duration);
    println!("Openings for `{tool_name}`");
    println!("{openings}");
    Ok(())
}

async fn tool_select(client: &NanoFab) -> Result<(String, String)> {
    let tools = client.get_tools().await?;
    loop {
        println!("Enter search string:");
        let mut search = String::new();
        std::io::stdin().read_line(&mut search)?;
        let res = tools
            .iter()
            .filter(|(name, _)| name.to_lowercase().contains(search.to_lowercase().trim()))
            .collect_vec();
        for (i, (name, _)) in res.iter().enumerate() {
            println!("{}: {name}", i + 1);
        }
        println!("Enter a number to select a tool:");
        let mut num_str = String::new();
        std::io::stdin().read_line(&mut num_str)?;
        let Some(num) = num_str.trim().parse::<usize>().ok() else{
            println!("Not a number");
            continue;
        };
        if let Some(vals) = res.get(num - 1) {
            break Ok((vals.0.clone(), vals.1.clone()));
        }
        println!("Invalid selection");
    }
}
