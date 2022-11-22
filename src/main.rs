mod nanofab;
mod term_ui;

use anyhow::{bail, Result};
use crossterm::{
    cursor,
    event::{self, KeyCode},
    style, terminal, ExecutableCommand, QueueableCommand,
};
use itertools::Itertools;
use std::{
    io::{stdout, Write},
    vec,
};
use term_ui::display_error_msg;

use crate::nanofab::{Login, NanoFab, Tool};
use crate::term_ui::{EventObject, QueueableCommand as _};

const CONFIG_DIR: &str = ".nanofab-cli";
const LOGIN_FILENAME: &str = "login.ron";

#[tokio::main]
async fn main() -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    stdout()
        .execute(crossterm::terminal::EnterAlternateScreen)?
        .execute(event::EnableMouseCapture)?;
    let res = run_ui().await;
    crossterm::terminal::disable_raw_mode()?;
    stdout()
        .execute(crossterm::terminal::LeaveAlternateScreen)?
        .queue(cursor::Show)?;
    res
}

async fn run_ui() -> Result<()> {
    // Create the config dir if it doesn't exist
    let mut config_dir = dirs::home_dir().unwrap();
    config_dir.push(CONFIG_DIR);
    let mut login_filepath = config_dir.clone();
    login_filepath.push(LOGIN_FILENAME);
    std::fs::create_dir(&config_dir).ok();

    // Create the client struct
    let client = NanoFab::new();

    // Login the user
    loop {
        let err = match user_login(&client).await {
            Ok(Some(_)) => break,
            Ok(None) => return Ok(()),
            Err(e) => e,
        };
        display_error_msg(err)?;
    }

    // Main menu
    let mut selector = Some(0);
    loop {
        let mut options = vec![];
        options.push("List Tool Openings");
        options.push("List User Bookings");
        if login_filepath.exists() {
            options.push("Delete Saved Login");
        }
        options.push("Exit");
        stdout()
            .queue(cursor::Hide)?
            .queue(cursor::MoveTo(0, 0))?
            .queue_ver_selector(&options, selector)?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .flush()?;
        let event = event::read()?;
        if event.updown_driver(&mut selector, options.len() - 1) {
        } else if event.is_key(KeyCode::Esc) {
            break;
        } else if event.is_key(KeyCode::Enter) {
            let res = match options[selector.unwrap()] {
                "Exit" => break,
                "List Tool Openings" => list_tool_openings(&client).await,
                "Delete Saved Login" => {
                    if user_confirm()? {
                        std::fs::remove_file(&login_filepath).ok();
                    }
                    Ok(())
                }
                "List User Bookings" => {
                    let bookings = client.get_user_bookings().await?;
                    let mut scroll = Some(0);
                    let bottom_gap = 0;
                    let mut max_lines = (terminal::size()?.1 as usize).saturating_sub(bottom_gap);
                    loop {
                        stdout().queue(cursor::Hide)?.queue(cursor::MoveTo(0, 0))?;
                        for (name, time) in bookings.iter().skip(scroll.unwrap()).take(max_lines) {
                            stdout()
                                .queue(style::Print(name.trim()))?
                                .queue(style::Print(" : "))?
                                .queue(style::Print(time.trim()))?
                                .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?
                                .queue(cursor::MoveDown(1))?
                                .queue(cursor::MoveToColumn(0))?;
                        }
                        stdout()
                            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
                            .flush()?;
                        let event = event::read()?;
                        #[allow(clippy::if_same_then_else)]
                        if event
                            .updown_driver(&mut scroll, bookings.len().saturating_sub(max_lines))
                        {
                        } else if event
                            .scroll_driver(&mut scroll, bookings.len().saturating_sub(max_lines))
                        {
                        } else if event.is_key(KeyCode::Enter) {
                            break;
                        } else if event.is_key(KeyCode::Esc) {
                            break;
                        } else if let Some((_, rows)) = event.is_resize() {
                            max_lines = (rows as usize).saturating_sub(bottom_gap);
                        }
                    }
                    Ok(())
                }
                selection => bail!("`{selection}` is not implemented"),
            };
            if let Err(err) = res {
                display_error_msg(err)?;
            }
        };
    }
    Ok(())
}

async fn list_tool_openings(client: &NanoFab) -> Result<()> {
    let Some(tool) = user_tool_select(client).await?else{
        return Ok(());
    };
    let bookings = client.get_tool_bookings(&tool).await?;
    let mut openings = bookings.inverted();
    openings.subtract_before_now();
    openings.subtract_weekends();
    openings.subtract_after_hours();

    let mut scroll = Some(0);
    let buffer = format!("{openings}");
    let lines = buffer.lines().collect_vec();
    let bottom_gap = 1;
    let mut max_lines = (terminal::size()?.1 as usize).saturating_sub(bottom_gap);
    loop {
        stdout()
            .queue(cursor::Hide)?
            .queue(cursor::MoveTo(0, 0))?
            .queue(style::Print(format!("Openings for `{}`", tool.name)))?;
        for line in lines.iter().skip(scroll.unwrap()).take(max_lines) {
            stdout()
                .queue(cursor::MoveDown(1))?
                .queue(cursor::MoveToColumn(0))?
                .queue(style::Print(line))?
                .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?;
        }
        stdout()
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .flush()?;
        let event = event::read()?;
        #[allow(clippy::if_same_then_else)]
        if event.updown_driver(&mut scroll, lines.len().saturating_sub(max_lines)) {
        } else if event.scroll_driver(&mut scroll, lines.len().saturating_sub(max_lines)) {
        } else if event.is_key(KeyCode::Enter) {
            break;
        } else if event.is_key(KeyCode::Esc) {
            break;
        } else if let Some((_, rows)) = event.is_resize() {
            max_lines = (rows as usize).saturating_sub(bottom_gap);
        }
    }
    Ok(())
}

fn user_confirm() -> Result<bool> {
    let mut selector = Some(1);
    loop {
        stdout()
            .queue(cursor::Hide)?
            .queue(cursor::MoveTo(0, 0))?
            .queue(style::Print("Are you sure? "))?
            .queue_hor_selector(&["[Yes]", "[No]"], selector)?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .flush()?;
        let event = event::read()?;
        if event.leftright_driver(&mut selector, 1) {
        } else if event.is_key(KeyCode::Enter) {
            break;
        }
    }
    Ok(selector.unwrap() == 0)
}

async fn user_login(client: &NanoFab) -> Result<Option<Login>> {
    let mut login_filepath = dirs::home_dir().unwrap();
    login_filepath.push(CONFIG_DIR);
    login_filepath.push(LOGIN_FILENAME);
    if let Ok(login_raw) = std::fs::read_to_string(&login_filepath) {
        let login = ron::from_str::<Login>(&login_raw)?;
        client.authenticate(&login).await?;
        return Ok(Some(login));
    }
    let mut username = String::new();
    loop {
        stdout()
            .queue(cursor::Show)?
            .queue(cursor::MoveTo(0, 0))?
            .queue(style::Print("Enter username: "))?
            .queue(style::Print(&username))?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .flush()?;
        let event = event::read()?;
        if event.string_driver(&mut username) {
        } else if event.is_key(KeyCode::Esc) {
            return Ok(None);
        } else if event.is_key(KeyCode::Enter) {
            break;
        }
    }
    let mut password = String::new();
    loop {
        let stars = (0..password.len()).map(|_| '*').collect::<String>();
        stdout()
            .queue(cursor::MoveTo(0, 1))?
            .queue(style::Print("Enter password: "))?
            .queue(style::Print(stars))?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .flush()?;
        let event = event::read()?;
        if event.string_driver(&mut password) {
        } else if event.is_key(KeyCode::Esc) {
            return Ok(None);
        } else if event.is_key(KeyCode::Enter) {
            break;
        }
    }
    let login = Login { username, password };
    client.authenticate(&login).await?;
    let mut save_login = Some(1);
    loop {
        stdout()
            .queue(cursor::Hide)?
            .queue(cursor::MoveTo(0, 2))?
            .queue(style::Print("Save login? "))?
            .queue_hor_selector(&["[Yes]", "[No]"], save_login)?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .flush()?;
        let event = event::read()?;
        if event.leftright_driver(&mut save_login, 1) {
        } else if event.is_key(KeyCode::Enter) {
            break;
        }
    }
    if save_login == Some(0) {
        std::fs::write(login_filepath, ron::to_string(&login)?)?;
    }
    Ok(Some(login))
}

async fn user_tool_select(client: &NanoFab) -> Result<Option<Tool>> {
    let bottom_gap = 2;
    let mut max_tools = (terminal::size()?.1 as usize).saturating_sub(bottom_gap);
    let all_tools = client.get_tools().await?;
    let mut search_str = String::new();
    let mut selection = None;
    let mut displayed_tools = all_tools.iter().take(max_tools).collect_vec();

    loop {
        let tool_names = displayed_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect_vec();
        stdout()
            .queue(cursor::Show)?
            .queue(cursor::MoveTo(0, 0))?
            .queue(style::Print("Search for tool:"))?
            .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?
            .queue(cursor::MoveDown(1))?
            .queue(cursor::MoveToColumn(0))?
            .queue(style::Print(&search_str))?
            .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?
            .queue(cursor::SavePosition)?
            .queue(cursor::MoveDown(1))?
            .queue(cursor::MoveToColumn(0))?
            .queue_ver_selector(&tool_names, selection)?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .queue(cursor::RestorePosition)?
            .flush()?;
        let event = event::read()?;
        #[allow(clippy::if_same_then_else)]
        if event.string_driver(&mut search_str) {
            selection = None;
            displayed_tools = all_tools
                .iter()
                .filter(|tool| {
                    tool.name
                        .to_lowercase()
                        .contains(&search_str.to_lowercase())
                })
                .take(max_tools)
                .collect();
        } else if event.updown_driver(&mut selection, displayed_tools.len().saturating_sub(1)) {
        } else if event.scroll_driver(&mut selection, displayed_tools.len().saturating_sub(1)) {
        } else if event.is_key(KeyCode::Esc) {
            return Ok(None);
        } else if event.is_key(KeyCode::Enter) & selection.is_some() {
            return Ok(Some(displayed_tools[selection.unwrap()].clone()));
        } else if let Some((_, rows)) = event.is_resize() {
            max_tools = (rows as usize).saturating_sub(bottom_gap);
            displayed_tools = all_tools
                .iter()
                .filter(|tool| {
                    tool.name
                        .to_lowercase()
                        .contains(&search_str.to_lowercase())
                })
                .take(max_tools)
                .collect();
            if let Some(s) = selection.as_mut() {
                *s = (*s).min(displayed_tools.len().saturating_sub(1))
            }
        }
    }
}
