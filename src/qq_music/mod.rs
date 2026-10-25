mod cache;
pub mod display;
mod lyrics;

#[cfg(windows)]
mod monitor;
#[cfg(windows)]
mod smtc;

pub use display::DisplayMode;
#[cfg(windows)]
pub use monitor::run;
