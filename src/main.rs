mod nanofab;

use std::io::{stdout, Write};

use crate::nanofab::{Login, NanoFab, Tool};
use anyhow::{Ok, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    style::{self, style, Stylize},
    terminal, ExecutableCommand, QueueableCommand,
};
use itertools::Itertools;

const CONFIG_DIR: &str = ".nanofab-cli";

#[tokio::main]
async fn main() -> Result<()> {
    // Create the config dir of it doesn't exits
    let mut config_dir = dirs::home_dir().unwrap();
    config_dir.push(CONFIG_DIR);
    std::fs::create_dir(config_dir).ok();

    // Create the client struct
    let client = NanoFab::new();

    // Login the user
    if user_login(&client).await?.is_none() {
        return Ok(());
    };

    // Main menu
    let menu = terminal_menu::menu(vec![
        terminal_menu::button("List Tool Openings"),
        terminal_menu::back_button("Exit"),
    ]);
    loop {
        terminal_menu::run(&menu);
        match terminal_menu::mut_menu(&menu).selected_item_name() {
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
    openings.subtract_after_hours();
    terminal::disable_raw_mode()?;
    println!("Openings for `{}`", tool.name);
    println!("{openings}");
    std::io::stdin().read_line(&mut String::new())?;
    Ok(())
}

async fn user_login(client: &NanoFab) -> Result<Option<Login>> {
    let mut login_filepath = dirs::home_dir().unwrap();
    login_filepath.push(CONFIG_DIR);
    login_filepath.push("login.ron");
    match std::fs::read_to_string(&login_filepath) {
        std::io::Result::Ok(login_raw) => {
            let login = ron::from_str::<Login>(&login_raw)?;
            client.authenticate(&login).await?;
            return Ok(Some(login));
        }
        Err(_) => {}
    }
    crossterm::terminal::enable_raw_mode()?;
    stdout().execute(crossterm::terminal::EnterAlternateScreen)?;
    let mut username = String::new();
    loop {
        stdout()
            .queue(cursor::MoveTo(0, 0))?
            .queue(style::Print("Enter username: "))?
            .queue(style::Print(&username))?
            .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?
            .flush()?;
        let Event::Key(key) = event::read()?else{
            continue;
        };
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    username.push(c.to_ascii_uppercase());
                } else {
                    username.push(c);
                }
            }
            KeyCode::Backspace => {
                username.pop();
            }
            KeyCode::Esc => {
                return Ok(None);
            }
            KeyCode::Enter => {
                break;
            }
            _ => continue,
        }
    }
    let mut password = String::new();
    loop {
        stdout()
            .queue(cursor::MoveTo(0, 1))?
            .queue(style::Print("Enter password: "))?;
        for _ in 0..password.len() {
            stdout().queue(style::Print('*'))?;
        }
        stdout().flush()?;
        let Event::Key(key) = event::read()?else{
            continue;
        };
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    password.push(c.to_ascii_uppercase());
                } else {
                    password.push(c);
                }
            }
            KeyCode::Backspace => {
                password.pop();
            }
            KeyCode::Esc => {
                return Ok(None);
            }
            KeyCode::Enter => {
                break;
            }
            _ => continue,
        }
    }
    let login = Login { username, password };
    client.authenticate(&login).await?;
    let mut save_login = false;
    loop {
        stdout()
            .queue(cursor::Hide)?
            .queue(cursor::MoveTo(0, 2))?
            .queue(style::Print("Save login? "))?;
        if save_login {
            stdout()
                .queue(style::PrintStyledContent("Yes".negative()))?
                .queue(style::Print(" No"))?;
        } else {
            stdout()
                .queue(style::Print("Yes "))?
                .queue(style::PrintStyledContent("No".negative()))?;
        }
        stdout().flush()?;
        let Event::Key(key) = event::read()?else{
            continue;
        };
        match key.code {
            KeyCode::Left if save_login == false => {
                save_login = true;
            }
            KeyCode::Right if save_login == true => {
                save_login = false;
            }
            KeyCode::Enter => {
                break;
            }
            _ => continue,
        }
    }
    if save_login {
        std::fs::write(login_filepath, ron::to_string(&login)?)?;
    }
    Ok(Some(login))
}

async fn tool_select(client: &NanoFab) -> Result<Option<Tool>> {
    let bottom_gap = 2;
    let mut max_tools = (terminal::size()?.1 as usize).saturating_sub(bottom_gap);
    let all_tools = client.get_tools().await?;
    let mut search = String::new();
    let mut selection = None;
    let mut disp_tools = all_tools.iter().take(max_tools).collect_vec();

    stdout()
        .execute(crossterm::terminal::EnterAlternateScreen)?
        .execute(cursor::MoveTo(0, 0))?;
    crossterm::terminal::enable_raw_mode()?;
    loop {
        stdout()
            .queue(cursor::MoveTo(0, 0))?
            .queue(style::Print("Search for tool:"))?
            .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?
            .queue(cursor::MoveDown(1))?
            .queue(cursor::MoveToColumn(0))?
            .queue(style::Print(&search))?
            .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?
            .queue(cursor::SavePosition)?;
        for (i, tool) in disp_tools.iter().enumerate() {
            let mut tool_name = style(&tool.name);
            if selection == Some(i) {
                tool_name = tool_name.negative();
            }
            stdout()
                .queue(cursor::MoveDown(1))?
                .queue(cursor::MoveToColumn(0))?
                .queue(style::PrintStyledContent(tool_name))?
                .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?;
        }
        stdout()
            .queue(cursor::MoveDown(1))?
            .queue(cursor::MoveToColumn(0))?
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?
            .queue(cursor::RestorePosition)?;
        stdout().flush()?;
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
                KeyCode::Esc => return Ok(None),
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
                    } else if selection.unwrap() < disp_tools.len().saturating_sub(1) {
                        *selection.as_mut().unwrap() += 1;
                    }
                }
                KeyCode::Up => {
                    if selection.is_some() && selection.unwrap() > 0 {
                        *selection.as_mut().unwrap() -= 1;
                    }
                }
                KeyCode::Enter if selection.is_some() => {
                    return Ok(Some(disp_tools[selection.unwrap()].clone()));
                }
                _ => continue,
            },
            _ => continue,
        }
    }
}
