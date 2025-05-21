use std::io::{stdout, Write};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    style::{self, style, Stylize},
    terminal, QueueableCommand as _,
};
use itertools::Itertools;

pub trait QueueableCommand: crossterm::QueueableCommand {
    fn queue_maybe_highlighted(&mut self, text: &str, highlight: bool) -> Result<&mut Self> {
        let mut styled_text = style(text);
        if highlight {
            styled_text = styled_text.negative();
        }
        self.queue(style::PrintStyledContent(styled_text))?;
        Ok(self)
    }
    fn queue_hor_selector(
        &mut self,
        options: &[&str],
        selected: Option<usize>,
    ) -> Result<&mut Self> {
        for (i, opt) in options.iter().enumerate() {
            if i > 0 {
                self.queue(style::Print(" "))?;
            }
            self.queue_maybe_highlighted(opt, Some(i) == selected)?;
        }
        Ok(self)
    }
    fn queue_ver_selector(
        &mut self,
        options: &[&str],
        selected: Option<usize>,
    ) -> Result<&mut Self> {
        for (i, opt) in options.iter().enumerate() {
            if i > 0 {
                self.queue(cursor::MoveDown(1))?
                    .queue(cursor::MoveToColumn(0))?;
            }
            self.queue_maybe_highlighted(opt, Some(i) == selected)?
                .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?;
        }
        Ok(self)
    }
}
impl<T> QueueableCommand for T where T: crossterm::QueueableCommand {}

pub trait EventObject {
    fn event(&self) -> &Event;
    fn is_mouse_scroll_up(&self) -> bool {
        matches!(
            self.event(),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            })
        )
    }
    fn is_mouse_scroll_down(&self) -> bool {
        matches!(
            self.event(),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            })
        )
    }
    fn is_key(&self, key_code: KeyCode) -> bool {
        matches!(self.event(), Event::Key(key) if key.code == key_code)
    }
    fn is_resize(&self) -> Option<(usize, usize)> {
        match self.event() {
            Event::Resize(c, r) => Some((*c as usize, *r as usize)),
            _ => None,
        }
    }
    fn is_char(&self) -> Option<char> {
        match self.event() {
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::SHIFT) => Some(c.to_ascii_uppercase()),
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                ..
            }) => Some(*c),
            _ => None,
        }
    }
    fn leftright_driver(&self, selector: &mut Option<usize>, max_val: usize) -> bool {
        if self.is_key(KeyCode::Left) {
            if selector.is_none() {
                *selector = Some(0)
            }
            *selector = Some(selector.unwrap().saturating_sub(1));
            true
        } else if self.is_key(KeyCode::Right) {
            if selector.is_none() {
                *selector = Some(0)
            }
            *selector = Some(selector.unwrap().saturating_add(1));
            if selector.unwrap() > max_val {
                *selector = Some(max_val);
            }
            true
        } else {
            false
        }
    }
    fn updown_driver(&self, selector: &mut Option<usize>, max_val: usize) -> bool {
        if self.is_key(KeyCode::Up) {
            if selector.is_none() {
                *selector = Some(0)
            } else {
                *selector = Some(selector.unwrap().saturating_sub(1));
            }
            true
        } else if self.is_key(KeyCode::Down) {
            if selector.is_none() {
                *selector = Some(0)
            } else {
                *selector = Some(selector.unwrap().saturating_add(1));
                if selector.unwrap() > max_val {
                    *selector = Some(max_val);
                }
            }
            true
        } else {
            false
        }
    }
    fn scroll_driver(&self, selector: &mut Option<usize>, max_val: usize) -> bool {
        if self.is_mouse_scroll_up() {
            if selector.is_none() {
                *selector = Some(0)
            } else {
                *selector = Some(selector.unwrap().saturating_sub(1));
            }
            true
        } else if self.is_mouse_scroll_down() {
            if selector.is_none() {
                *selector = Some(0)
            } else {
                *selector = Some(selector.unwrap().saturating_add(1));
                if selector.unwrap() > max_val {
                    *selector = Some(max_val);
                }
            }
            true
        } else {
            false
        }
    }
    fn string_driver(&self, string: &mut String) -> bool {
        if let Some(c) = self.is_char() {
            string.push(c);
            true
        } else if self.is_key(KeyCode::Backspace) {
            string.pop();
            true
        } else {
            false
        }
    }
}
impl EventObject for Event {
    fn event(&self) -> &Event {
        self
    }
}

pub fn display_error_msg(error: anyhow::Error) -> Result<()> {
    let buffer = format!("{error:?}");
    let mut lines = buffer.lines().collect_vec();
    lines.push("");
    lines.push("Press [Enter] to retry");
    let start_row = terminal::size()?.1 / 2 - lines.len() as u16 / 2;
    stdout()
        .queue(cursor::Hide)?
        .queue(cursor::MoveTo(0, start_row))?
        .queue(style::SetBackgroundColor(style::Color::Red))?;
    for line in &lines {
        stdout()
            .queue(style::Print(line))?
            .queue(terminal::Clear(terminal::ClearType::UntilNewLine))?
            .queue(cursor::MoveDown(1))?
            .queue(cursor::MoveToColumn(0))?;
    }
    stdout().queue(style::ResetColor)?.flush()?;
    'event_loop: loop {
        let event = event::read()?;
        if event
            .as_key_press_event()
            .is_some_and(|k| [KeyCode::Enter, KeyCode::Esc].contains(&k.code))
        {
            break 'event_loop;
        }
    }
    Ok(())
}
