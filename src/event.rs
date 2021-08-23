/// Event: event stream (keys, ticks, etc).
use std::{sync::mpsc, thread, time};

use crossterm::event::Event as CTEvent;
use crossterm::event::KeyEvent as CTKeyEvent;

pub enum Event {
    Tick,   // time to update internal state and redraw
    Resize, // terminal resized
    Key(CTKeyEvent),
}

// an instruction to the main loop
#[derive(PartialEq)]
pub enum Next {
    Continue,
    Quit,
}

pub struct EventStream {
    stream: mpsc::Receiver<Event>,
}

// LEARN: why is move needed for the thread closure?
impl EventStream {
    pub fn new(tick_every: time::Duration) -> Self {
        let (tx, rx) = mpsc::channel();

        let tick_tx = tx.clone();
        thread::spawn(move || loop {
            tick_tx.send(Event::Tick).expect("failed to tick");
            thread::sleep(tick_every);
        });

        let event_tx = tx;
        thread::spawn(move || {
            // TODO: limit Resize frequency.
            loop {
                match crossterm::event::read().expect("read term event") {
                    CTEvent::Key(ke) => event_tx.send(Event::Key(ke)),
                    CTEvent::Resize(_, _) => event_tx.send(Event::Resize),
                    CTEvent::Mouse(_) => Ok(()),
                }
                .expect("send term event")
            }
        });

        Self { stream: rx }
    }
}

impl Iterator for EventStream {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        Some(self.stream.recv().expect("get next event"))
    }
}
