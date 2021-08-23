/// STerm: a wrapper around nitty-gritty terminal details.
use crossterm::execute;
use tui::backend::CrosstermBackend;
use tui::Terminal;

type CTBackend = CrosstermBackend<std::io::Stdout>;

// wrapper around tui and crossterm stuff
pub struct STerm {
    terminal: Terminal<CTBackend>,
}

impl Default for STerm {
    fn default() -> Self {
        let stdout = std::io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let terminal = tui::Terminal::new(backend).expect("couldn't make tui::Terminal");
        init_terminal();
        STerm { terminal }
    }
}

impl STerm {
    // proxying the only tui::Terminal method needed.
    // LEARN: why doesn't this work? (error about sized types, etc)
    // pub fn draw(&mut self, f: FnOnce(&mut tui::Frame<CTBackend>)) -> std::io::Result<()> {
    pub fn draw<F>(&mut self, f: F) -> std::io::Result<()>
    where
        F: FnOnce(&mut tui::Frame<CTBackend>),
    {
        // LEARN: nicer way to throw away the Ok value?
        self.terminal.draw(f).map(|_| ())
    }
}

impl Drop for STerm {
    fn drop(&mut self) {
        restore_terminal();
    }
}

// Note: zenith also does cursor::Hide and Clear in init, and Cursor::MoveTo,
// Clear, cursor::Show in restore, but those don't seem to be necessary with the
// alternative screen?

fn init_terminal() {
    log::debug!("initializing STerm");
    let mut sout = std::io::stdout();
    // using an alternative screen prevents blank gap where the UI was rendering
    execute!(sout, crossterm::terminal::EnterAlternateScreen)
        .expect("Unable to enter alternate screen");
    // needed to process key events as they come
    crossterm::terminal::enable_raw_mode().expect("Unable to enter raw mode.");
}

fn restore_terminal() {
    log::debug!("restoring STerm");
    let mut sout = std::io::stdout();
    execute!(sout, crossterm::terminal::LeaveAlternateScreen)
        .expect("Unable to leave alternate screen.");
    // fixes terminal offset weirdness
    crossterm::terminal::disable_raw_mode().expect("Unable to disable raw mode");
}
