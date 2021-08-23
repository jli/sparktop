/// scratch space for figuring out how to build things in tui reasonably
//
// ok, structure that seems reasonable:
// - State struct
// - transition method that updates state based on events
// - build layout based on state, call helper functions to render each part
//
// still need to work out:
// - bind hotkey with various enums to make selections and help rendering easier
use std::fmt::Display;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use sparktop::sterm;

#[derive(Debug)]
struct DrawState {
    title: String,
    activity: ActivityMode,
    displayed_columns: ColumnDisplay,
    column_sort: Column,
    error: Option<String>,
    debug: Option<String>,
    should_quit: bool,
}

impl Display for DrawState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "title: {}\nactivity: {:?}\ncolumns:{:?}\nsort:{:?}\nerror:{:?}\ndebug:{:?}\nquit:{:?}",
            self.title,
            self.activity,
            self.displayed_columns,
            self.column_sort,
            self.error,
            self.debug,
            self.should_quit,
        )
    }
}

// "activities" that need further input
#[derive(Debug)]
enum ActivityMode {
    Top,
    SortSelect,
    ColumnSelect,
    // ViewSelect -> Flat, Pidtree
}

#[derive(Debug)]
enum Column {
    Pid,
    ProcessName,
    Cpu,
}

// TODO: nice way to combine hotkey char with these enums, so can render help
// hint and programmatically select?
impl Column {
    fn from_char(c: char) -> Result<Self, String> {
        match c {
            'p' => Ok(Column::Pid),
            'n' => Ok(Column::ProcessName),
            'c' => Ok(Column::Cpu),
            _ => Err(format!("invalid column char: {}", c)),
        }
    }
}

#[derive(Debug)]
struct ColumnDisplay {
    pid: bool,
    process: bool,
    cpu: bool,
}

impl ColumnDisplay {
    fn toggle(&mut self, col: Column) {
        match col {
            Column::Pid => self.pid = !self.pid,
            Column::ProcessName => self.process = !self.process,
            Column::Cpu => self.cpu = !self.cpu,
        }
    }
}

impl Default for ColumnDisplay {
    fn default() -> Self {
        ColumnDisplay {
            pid: true,
            process: true,
            cpu: true,
        }
    }
}

impl DrawState {
    // update DrawState based on event
    fn transition(&mut self, event: &Event) {
        self.debug = Some(format!("event: {:?}", event));
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
            }) => self.should_quit = true,
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                ..
            }) => self.transition_char(*c),
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => {
                self.activity = ActivityMode::Top;
                self.error = None;
            }
            _ => self.error = Some(format!("unhandled event: {:?}", event)),
        };
    }

    fn transition_char(&mut self, c: char) {
        let mut err = None;
        match self.activity {
            ActivityMode::Top => match c {
                'q' => self.should_quit = true,
                's' => self.activity = ActivityMode::SortSelect,
                'c' => self.activity = ActivityMode::ColumnSelect,
                _ => err = Some(format!("invalid command: {}", c)),
            },
            ActivityMode::SortSelect => match Column::from_char(c) {
                Err(s) => err = Some(s),
                Ok(c) => {
                    self.column_sort = c;
                    self.activity = ActivityMode::Top
                }
            },
            ActivityMode::ColumnSelect => match Column::from_char(c) {
                Err(s) => err = Some(s),
                Ok(c) => {
                    self.displayed_columns.toggle(c);
                    self.activity = ActivityMode::Top
                }
            },
        };
        self.error = err;
    }
}

fn draw_main<B: Backend>(f: &mut Frame<B>, area: Rect, state: &DrawState) {
    let main = Block::default()
        .title(&state.title[..])
        .borders(Borders::ALL);
    let main = Paragraph::new(format!("state:\n\n{}", state)).block(main);
    f.render_widget(main, area);
}

fn draw_help<B: Backend>(f: &mut Frame<B>, area: Rect, activity: &ActivityMode) {
    let text = match activity {
        ActivityMode::Top => "s:sort c:columns",
        ActivityMode::SortSelect => "sort: pid proc cpu",
        ActivityMode::ColumnSelect => "toggle columns: pid proc cpu",
    };
    let para = Paragraph::new(text);
    f.render_widget(para, area);
}

fn draw_error<B: Backend>(f: &mut Frame<B>, area: Rect, err: &String) {
    let msg = Paragraph::new(&err[..]);
    f.render_widget(msg, area);
}

fn draw<B: Backend>(f: &mut Frame<B>, draw_input: &DrawState) {
    let full = f.size();
    let has_error = draw_input.error.is_some();
    let constraints = if has_error {
        vec![
            Constraint::Length(full.height - 2),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Length(full.height - 1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(full);
    let (main_area, help_area) = (chunks[0], chunks[chunks.len() - 1]);

    draw_main(f, main_area, &draw_input);
    draw_help(f, help_area, &draw_input.activity);
    if has_error {
        draw_error(f, chunks[1], draw_input.error.as_ref().unwrap());
    }
}

fn main() -> Result<(), std::io::Error> {
    let mut sterm = sterm::STerm::new();
    let mut draw_state = DrawState {
        title: String::from("initial title"),
        activity: ActivityMode::Top,
        should_quit: false,
        column_sort: Column::Pid,
        displayed_columns: ColumnDisplay::default(),
        debug: None,
        error: None,
    };
    loop {
        // render
        sterm.draw(|f| draw(f, &draw_state))?;

        // read input
        let event = crossterm::event::read()?;
        draw_state.transition(&event);
        if draw_state.should_quit {
            break;
        }
    }
    Ok(())
}
