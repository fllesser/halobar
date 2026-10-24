#![cfg_attr(windows, windows_subsystem = "windows")]

mod display;
mod lyrics;
mod pixelbar;
mod qq_music;
#[cfg(windows)]
mod smtc;
#[cfg(windows)]
mod startup;

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    thread,
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::time::{SystemTime, UNIX_EPOCH};

use display::DisplayMode;

const LYRIC_SYNC_LEAD_MS: u64 = 150;
const MEDIA_POLL_INTERVAL_MS: u64 = 40;
const LYRIC_RETRY_DELAY: Duration = Duration::from_secs(3);

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
        log_message("HaloBar 后台监听已启动；开机启动项已启用")?;
    }

    let device = match pixelbar::PixelBar::connect()? {
        Some(device) => {
            log_message("已连接 Halo PixelBar 字幕屏")?;
            device.show_clock()?;
            Some(device)
        }
        None => {
            log_message("未检测到 Halo PixelBar；仍会监听 QQ 音乐")?;
            None
        }
    };

    monitor_qq_music(mode, device.as_ref())
}

#[cfg(windows)]
fn monitor_qq_music(mode: DisplayMode, device: Option<&pixelbar::PixelBar>) -> Result<(), String> {
    let mut last_initialization_error: Option<String> = None;
    let mut monitor = loop {
        match smtc::MediaMonitor::new() {
            Ok(monitor) => {
                if last_initialization_error.take().is_some() {
                    log_message("系统媒体会话初始化已恢复")?;
                }
                break monitor;
            }
            Err(error) => {
                if last_initialization_error.as_ref() != Some(&error) {
                    log_message(&format!("初始化 Windows 媒体会话失败，将自动重试：{error}"))?;
                    last_initialization_error = Some(error);
                }
                thread::sleep(Duration::from_millis(500));
            }
        }
    };
    // println!("正在后台监听 QQ 音乐媒体会话（Ctrl+C 退出）");

    let mut lyric_cache: Option<(String, Vec<lyrics::LyricLine>, bool)> = None;
    let mut failed_track: Option<(String, Instant, bool)> = None;
    let mut lyrics_screen_active = false;
    let mut last_frame = String::new();
    let mut waiting_reported = false;
    let mut last_media_error: Option<String> = None;
    let mut last_reconnect_error: Option<String> = None;

    loop {
        let current_track = match monitor.current_qq_track() {
            Ok(track) => {
                let media_recovered = last_media_error.take().is_some();
                let reconnect_recovered = last_reconnect_error.take().is_some();
                if media_recovered || reconnect_recovered {
                    log_message("系统媒体会话读取已恢复")?;
                }
                track
            }
            Err(error) => {
                if last_media_error.as_ref() != Some(&error) {
                    log_message(&format!("读取系统媒体状态失败，将自动重试：{error}"))?;
                    last_media_error = Some(error);
                }
                if lyrics_screen_active {
                    if let Some(device) = device {
                        device.show_clock()?;
                    }
                    lyrics_screen_active = false;
                    last_frame.clear();
                }
                match monitor.reconnect() {
                    Ok(()) => {
                        if last_reconnect_error.take().is_some() {
                            log_message("重新连接 Windows 媒体会话成功")?;
                        }
                    }
                    Err(reconnect_error) => {
                        if last_reconnect_error.as_ref() != Some(&reconnect_error) {
                            log_message(&format!(
                                "重新连接 Windows 媒体会话失败，将继续重试：{reconnect_error}"
                            ))?;
                            last_reconnect_error = Some(reconnect_error);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        match current_track {
            Some(track) if track.playing && !track.title.trim().is_empty() => {
                waiting_reported = false;
                let track_key = format!(
                    "{}|{}|{}",
                    track.app_id,
                    track.title.trim(),
                    track.artist.trim()
                );
                let missing_cached_lyrics = lyric_cache
                    .as_ref()
                    .is_none_or(|(cached_key, _, _)| cached_key != &track_key);
                let retry_due =
                    failed_track
                        .as_ref()
                        .is_none_or(|(failed_key, failed_at, retry_used)| {
                            failed_key != &track_key
                                || (!retry_used && failed_at.elapsed() >= LYRIC_RETRY_DELAY)
                        });
                if missing_cached_lyrics && retry_due {
                    let is_new_track = failed_track
                        .as_ref()
                        .is_none_or(|(failed_key, _, _)| failed_key != &track_key);
                    let is_retry =
                        failed_track
                            .as_ref()
                            .is_some_and(|(failed_key, _, retry_used)| {
                                failed_key == &track_key && !retry_used
                            });
                    if is_new_track {
                        log_message(&format!(
                            "检测到正在播放：{} - {}",
                            track.title, track.artist
                        ))?;
                    }
                    let fetch_result = qq_music::fetch_cached_for_track(
                        &track.title,
                        &track.artist,
                        track.duration_ms,
                    )
                    .and_then(|cached| {
                        cached
                            .map(|(lyrics, has_word_timing)| (lyrics, has_word_timing, true))
                            .ok_or_else(|| "QQ 音乐本地缓存中没有这首歌的歌词".to_owned())
                    });
                    match fetch_result {
                        Ok((lyrics, has_word_timing, from_local_cache)) => {
                            if from_local_cache && has_word_timing {
                                log_message("已从 QQ 音乐本地缓存读取逐字 QRC 歌词")?;
                            } else if from_local_cache {
                                log_message("已从 QQ 音乐本地缓存读取行级歌词")?;
                            }
                            if mode == DisplayMode::Typewriter && !has_word_timing {
                                log_message("这首歌没有逐字时间轴；逐字模式将按歌词行同步显示")?;
                            }
                            lyric_cache = Some((track_key.clone(), lyrics, has_word_timing));
                            failed_track = None;
                        }
                        Err(error) => {
                            if is_retry {
                                log_message(&format!(
                                    "等待 2 秒后重试仍无法获取当前歌曲歌词：{error}"
                                ))?;
                            } else if failed_track
                                .as_ref()
                                .is_none_or(|(failed_key, _, _)| failed_key != &track_key)
                            {
                                log_message(&format!(
                                    "无法获取当前歌曲歌词，将在 2 秒后仅重读本地缓存一次：{error}"
                                ))?;
                            }
                            failed_track = Some((track_key, Instant::now(), is_retry));
                            if let Some(device) = device {
                                device.show_clock()?;
                            }
                            lyrics_screen_active = false;
                            last_frame.clear();
                            thread::sleep(Duration::from_millis(500));
                            continue;
                        }
                    }
                }

                if let Some((cached_key, lyrics, _has_word_timing)) = &lyric_cache
                    && cached_key == &track_key
                {
                    if !lyrics_screen_active {
                        if let Some(device) = device {
                            device.prepare_text_layout()?;
                        }
                        lyrics_screen_active = true;
                        last_frame.clear();
                    }

                    let elapsed_ms = track
                        .position_sampled_at
                        .elapsed()
                        .as_millis()
                        .min(u64::MAX as u128) as u64;
                    let position_ms = track
                        .position_ms
                        .saturating_add(elapsed_ms)
                        .saturating_add(LYRIC_SYNC_LEAD_MS);
                    let frame = display::at_position(lyrics, mode, position_ms).unwrap_or_default();
                    if !frame.is_empty() && frame != last_frame {
                        if let Some(device) = device {
                            device.send_text(&frame)?;
                        } else {
                            log_message(&format!("歌词预览：{frame}"))?;
                        }
                        last_frame = frame;
                    }
                }
            }
            _ => {
                if !waiting_reported {
                    if lyrics_screen_active {
                        log_message("QQ 音乐已暂停或停止，切回音响时钟")?;
                    } else {
                        log_message("等待 QQ 音乐开始播放")?;
                    }
                    waiting_reported = true;
                }
                if lyrics_screen_active {
                    if let Some(device) = device {
                        device.show_clock()?;
                    }
                    lyrics_screen_active = false;
                    last_frame.clear();
                }
            }
        }
        thread::sleep(Duration::from_millis(MEDIA_POLL_INTERVAL_MS));
    }
}

#[cfg(not(windows))]
fn monitor_qq_music(
    _mode: DisplayMode,
    _device: Option<&pixelbar::PixelBar>,
) -> Result<(), String> {
    Err("自动读取 QQ 音乐播放状态目前仅支持 Windows".to_owned())
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
