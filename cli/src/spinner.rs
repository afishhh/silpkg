#[derive(Clone)]
pub struct SpinnerStyle<'a> {
    pub stages: &'a [&'a str],
}

impl SpinnerStyle<'static> {
    pub const fn const_default() -> Self {
        Self {
            stages: &[
                "⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈", "⠐", "⠠", "⢀", "⠄", "⠂",
            ],
        }
    }
}

impl Default for SpinnerStyle<'static> {
    fn default() -> Self {
        Self::const_default()
    }
}

pub struct Spinner<'a> {
    style: SpinnerStyle<'a>,
    message: String,
    current: usize,
}

impl<'a> Spinner<'a> {
    pub fn new(style: SpinnerStyle<'a>, message: impl Into<String>) -> Self {
        let spinner = Self {
            style,
            message: message.into(),
            current: 0,
        };
        spinner.redraw();
        spinner
    }

    fn redraw(&self) {
        if !self.message.is_empty() {
            eprint!("\x1b[2K\x1b[1m{}\x1b[0m ", self.message);
        }

        eprint!("{}\r", self.style.stages[self.current]);
    }

    pub fn paused(&self, func: impl FnOnce()) {
        eprint!("\x1b[2K");
        func();
        self.redraw();
    }

    pub fn inc(&mut self) {
        self.current += 1;
        self.current %= self.style.stages.len();
        self.redraw()
    }

    pub fn finish_with(self, message: &str) {
        if !self.message.is_empty() {
            eprint!("\x1b[2K\x1b[1m{}\x1b[0m ", self.message);
        }

        eprintln!("{message}");
    }

    pub fn finish(self) {
        eprintln!();
    }
}
