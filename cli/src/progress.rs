pub struct ProgressBarStyle {
    pub width: usize,
}

pub struct ProgressBar {
    style: ProgressBarStyle,
    message: String,
    current_progress: usize,
    max_progress: usize,
}

const BLOCKS: &[&str] = &["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

impl ProgressBar {
    pub fn new(style: ProgressBarStyle, max_progress: usize, message: String) -> Self {
        let bar = Self {
            style,
            message,
            current_progress: 0,
            max_progress,
        };
        bar.redraw();
        bar
    }

    fn redraw(&self) {
        let progress =
            (self.current_progress * self.style.width * BLOCKS.len()) / self.max_progress;

        if !self.message.is_empty() {
            eprint!("\x1b[1m{}\x1b[0m▕", self.message);
        }

        let (full, left) = (progress / 8, progress % 8);
        for _ in 0..full {
            eprint!("{}", BLOCKS.last().unwrap());
        }
        if left > 0 {
            eprint!("{}", BLOCKS[left - 1]);
        }

        for _ in (full + (left > 0) as usize)..self.style.width {
            eprint!(" ");
        }
        eprint!("▏ {}/{}\r", self.current_progress, self.max_progress);
    }

    pub fn paused(&self, func: impl FnOnce()) {
        eprint!("\x1b[2K");
        func();
        self.redraw();
    }

    pub fn inc(&mut self) {
        self.current_progress += 1;
        self.redraw()
    }

    pub fn finish(self) {
        eprintln!();
    }
}
