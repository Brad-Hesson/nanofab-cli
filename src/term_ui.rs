use anyhow::Result;
use crossterm::{
    cursor,
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    style::{self, style, Stylize},
    terminal,
};

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
    fn is_key(&self, key_code: KeyCode) -> bool {
        match self.event() {
            Event::Key(key) if key.code == key_code => true,
            _ => false,
        }
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
