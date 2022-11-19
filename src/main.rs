mod nanofab;

use std::io::{stdout, Write};

use crate::nanofab::NanoFab;
use anyhow::{Ok, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    style::{self, Stylize},
    terminal, ExecutableCommand, QueueableCommand,
};
use itertools::Itertools;
use nanofab::Tool;

#[tokio::main]
async fn main() -> Result<()> {
    let client = NanoFab::new();
    let (username, password) = user_login().await?;
    client
        .authenticate(&username.trim(), &password.trim())
        .await?;
    println!("Authentication Successful");
    let menu = terminal_menu::menu(vec![
        terminal_menu::button("List Tools"),
        terminal_menu::button("List Tool Openings"),
        terminal_menu::back_button("Exit"),
    ]);
    loop {
        terminal_menu::run(&menu);
        match terminal_menu::mut_menu(&menu).selected_item_name() {
            "List Tools" => list_tools(&client).await?,
            "List Tool Openings" => list_tool_openings(&client).await?,
            "Exit" => break,
            _ => unreachable!(),
        }
    }
    Ok(())
}

async fn list_tool_openings(client: &NanoFab) -> Result<()> {
    let Some(tool) = tool_select(&client).await?else{
        return Ok(());
    };
    let bookings = client.get_tool_bookings(&tool).await?;
    let mut openings = bookings.inverted();
    openings.subtract_before_now();
    openings.subtract_weekends();
    openings.subract_after_hours();
    println!("Openings for `{}`", tool.name);
    println!("{openings}");
    std::io::stdin().read_line(&mut String::new())?;
    Ok(())
}

async fn list_tools(client: &NanoFab) -> Result<()> {
    let tools = client.get_tools().await?;
    for tool in tools {
        println!("{}", tool.name);
    }
    std::io::stdin().read_line(&mut String::new())?;
    Ok(())
}

async fn user_login() -> Result<(String, String)> {
    println!("Input username:");
    let mut username = String::new();
    std::io::stdin().read_line(&mut username)?;
    println!("Input password:");
    let mut password = String::new();
    std::io::stdin().read_line(&mut password)?;
    Ok((username, password))
}

async fn tool_select(client: &NanoFab) -> Result<Option<Tool>> {
    let update_term = |search: &str, tools: &[&Tool], selection: Option<usize>| {
        stdout()
            .queue(cursor::MoveTo(0, 0))?
            .queue(terminal::Clear(terminal::ClearType::CurrentLine))?
            .queue(style::Print("Search for tool:"))?
            .queue(cursor::MoveDown(1))?
            .queue(terminal::Clear(terminal::ClearType::CurrentLine))?
            .queue(cursor::MoveToColumn(0))?
            .queue(style::Print(&search))?
            .queue(cursor::SavePosition)?;
        for (i, tool) in tools.iter().enumerate() {
            stdout()
                .queue(cursor::MoveDown(1))?
                .queue(cursor::MoveToColumn(0))?
                .queue(terminal::Clear(terminal::ClearType::CurrentLine))?;
            match selection {
                Some(selected_index) if selected_index == i => {
                    stdout().queue(style::PrintStyledContent(tool.name.clone().negative()))?;
                }
                _ => {
                    stdout().queue(style::Print(&tool.name))?;
                }
            };
        }
        stdout()
            .queue(cursor::MoveDown(1))?
            .queue(cursor::MoveToColumn(0))?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .queue(cursor::RestorePosition)?;
        stdout().flush()?;
        Ok(())
    };
    // Logic start
    stdout()
        .execute(crossterm::terminal::EnterAlternateScreen)?
        .execute(cursor::MoveTo(0, 0))?;
    crossterm::terminal::enable_raw_mode()?;
    let bottom_gap = 3;
    let mut max_tools = (terminal::size()?.1 as usize).saturating_sub(bottom_gap);
    let all_tools = client.get_tools().await?;

    let mut search = String::new();
    let mut selection = None;
    let mut disp_tools = all_tools.iter().take(max_tools).collect_vec();
    update_term(&search, &disp_tools, selection)?;

    let result = loop {
        match event::read()? {
            Event::Resize(_, rows) => {
                max_tools = (rows as usize).saturating_sub(bottom_gap);
                disp_tools = all_tools
                    .iter()
                    .filter(|tool| tool.name.to_lowercase().contains(&search.to_lowercase()))
                    .take(max_tools)
                    .collect();
                selection
                    .as_mut()
                    .map(|s| *s = (*s).min(disp_tools.len().saturating_sub(1)));
            }
            Event::Key(key) => match key.code {
                KeyCode::Esc => break None,
                KeyCode::Char(c) => {
                    search.push(c);
                    selection = None;
                    disp_tools = all_tools
                        .iter()
                        .filter(|tool| tool.name.to_lowercase().contains(&search.to_lowercase()))
                        .take(max_tools)
                        .collect();
                }
                KeyCode::Backspace => {
                    search.pop();
                    selection = None;
                    disp_tools = all_tools
                        .iter()
                        .filter(|tool| tool.name.to_lowercase().contains(&search.to_lowercase()))
                        .take(max_tools)
                        .collect();
                }
                KeyCode::Down => {
                    if selection.is_none() {
                        selection = Some(0)
                    } else if selection.unwrap() < disp_tools.len() - 1 {
                        *selection.as_mut().unwrap() += 1;
                    }
                }
                KeyCode::Up => {
                    if selection.is_some() && selection.unwrap() > 0 {
                        *selection.as_mut().unwrap() -= 1;
                    }
                }
                KeyCode::Enter if selection.is_some() => {
                    break Some(disp_tools[selection.unwrap()].clone());
                }
                _ => continue,
            },
            _ => continue,
        }
        update_term(&search, &disp_tools, selection)?;
    };
    crossterm::terminal::disable_raw_mode()?;
    stdout().execute(crossterm::terminal::LeaveAlternateScreen)?;
    Ok(result)
}
