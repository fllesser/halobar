#![cfg_attr(windows, windows_subsystem = "windows")]

mod pixelbar;
mod qq_music;
#[cfg(windows)]
mod startup;

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
};

#[cfg(windows)]
use std::{thread, time::Duration};

#[cfg(not(windows))]
use std::time::{SystemTime, UNIX_EPOCH};

use qq_music::DisplayMode;

#[cfg(windows)]
const DEVICE_RETRY_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(windows)]
const CONNECTION_NOTICE_DURATION: Duration = Duration::from_secs(2);

fn main() {
    if let Err(error) = run() {
        let _ = log_message(&format!("错误：{error}"));
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut mode = DisplayMode::Typewriter;
    let mut remove_startup = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--mode" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--mode 缺少参数".to_owned())?;
                mode = DisplayMode::parse(value)?;
                index += 2;
            }
            "--remove-startup" => {
                remove_startup = true;
                index += 1;
            }
            "--help" | "-h" => {
                log_message(usage())?;
                return Ok(());
            }
            other => {
                return Err(format!("未知参数: {other}\n{}", usage()));
            }
        }
    }

    #[cfg(windows)]
    if remove_startup {
        startup::remove()?;
        log_message("已移除 HaloBar 开机启动项")?;
        return Ok(());
    }
    #[cfg(not(windows))]
    if remove_startup {
        return Err("开机启动管理目前仅支持 Windows".to_owned());
    }

    #[cfg(windows)]
    {
        startup::install()?;
        log_message("HaloBar 后台程序已启动；开机启动项已启用")?;

        let mut waiting_reported = false;
        let mut last_connection_error: Option<String> = None;
        let device = loop {
            match pixelbar::PixelBar::connect() {
                Ok(Some(device)) => {
                    log_message("已连接 Halo PixelBar 字幕屏")?;
                    device.prepare_text_layout()?;
                    device.send_text("HaloBar 已连接(Power By FLLess)")?;
                    thread::sleep(CONNECTION_NOTICE_DURATION);
                    device.show_clock()?;
                    break device;
                }
                Ok(None) => {
                    if !waiting_reported {
                        log_message("未检测到 Halo PixelBar，等待设备连接")?;
                        waiting_reported = true;
                    }
                    last_connection_error = None;
                }
                Err(error) => {
                    if last_connection_error.as_ref() != Some(&error) {
                        log_message(&format!("连接 Halo PixelBar 失败，将继续重试：{error}"))?;
                        last_connection_error = Some(error);
                    }
                }
            }
            thread::sleep(DEVICE_RETRY_INTERVAL);
        };

        qq_music::run(mode, &device)
    }
    #[cfg(not(windows))]
    {
        let _ = mode;
        Err("自动读取 QQ 音乐播放状态目前仅支持 Windows".to_owned())
    }
}

fn usage() -> &'static str {
    "用法：halobar [--mode scroll|typewriter] [--remove-startup]\n启动后自动监听 QQ 音乐；默认启用开机自启。使用 --remove-startup 可移除开机启动项。"
}

fn log_message(message: &str) -> Result<(), String> {
    #[cfg(windows)]
    let path = startup::log_path()?;
    #[cfg(not(windows))]
    let path = env::temp_dir().join("halobar.log");

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建 HaloBar 日志目录失败: {error}"))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("打开 HaloBar 日志失败: {error}"))?;
    let timestamp = log_timestamp()?;
    writeln!(file, "[{timestamp}] {message}")
        .map_err(|error| format!("写入 HaloBar 日志失败: {error}"))
}

#[cfg(windows)]
fn log_timestamp() -> Result<String, String> {
    let local_time = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local_time.wYear,
        local_time.wMonth,
        local_time.wDay,
        local_time.wHour,
        local_time.wMinute,
        local_time.wSecond
    ))
}

#[cfg(not(windows))]
fn log_timestamp() -> Result<String, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("读取系统时间失败: {error}"))?
        .as_secs();
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_date_from_days(days);
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        seconds_of_day / 3_600,
        seconds_of_day / 60 % 60,
        seconds_of_day % 60
    ))
}

#[cfg(not(windows))]
fn civil_date_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_parameter = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_parameter + 2) / 5 + 1;
    let month = month_parameter + if month_parameter < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}
