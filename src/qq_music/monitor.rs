use std::{
    thread,
    time::{Duration, Instant},
};

use super::{cache, display, lyrics, smtc};
use crate::{log_message, pixelbar};

const LYRIC_SYNC_LEAD_MS: u64 = 200;
const MEDIA_POLL_INTERVAL_MS: u64 = 40;
const LYRIC_RETRY_DELAY: Duration = Duration::from_secs(3);

pub fn run(mode: super::DisplayMode, device: &pixelbar::PixelBar) -> Result<(), String> {
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
                    device.show_clock()?;
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
                    let fetch_result = cache::fetch_cached_for_track(
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
                            if mode == super::DisplayMode::Typewriter && !has_word_timing {
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
                            device.show_clock()?;
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
                        device.prepare_text_layout()?;
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
                        device.send_text(&frame)?;
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
                    device.show_clock()?;
                    lyrics_screen_active = false;
                    last_frame.clear();
                }
            }
        }
        thread::sleep(Duration::from_millis(MEDIA_POLL_INTERVAL_MS));
    }
}
