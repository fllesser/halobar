use std::time::Instant;

use windows::{
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    },
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

pub struct MediaSnapshot {
    pub app_id: String,
    pub title: String,
    pub artist: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub position_sampled_at: Instant,
    pub playing: bool,
}

pub struct MediaMonitor {
    _apartment: RoApartment,
    manager: GlobalSystemMediaTransportControlsSessionManager,
}

impl MediaMonitor {
    pub fn new() -> Result<Self, String> {
        let apartment = RoApartment::initialize()?;
        let manager = request_manager()?;
        Ok(Self {
            _apartment: apartment,
            manager,
        })
    }

    pub fn reconnect(&mut self) -> Result<(), String> {
        self.manager = request_manager()?;
        Ok(())
    }

    pub fn current_qq_track(&self) -> Result<Option<MediaSnapshot>, String> {
        let sessions = self.manager.GetSessions().map_err(api_error)?;
        for index in 0..sessions.Size().map_err(api_error)? {
            let session = sessions.GetAt(index).map_err(api_error)?;
            let app_id = session
                .SourceAppUserModelId()
                .map_err(api_error)?
                .to_string_lossy();
            if !is_qq_music(&app_id) {
                continue;
            }

            return snapshot_session(&session, app_id).map(Some);
        }
        Ok(None)
    }
}

fn request_manager() -> Result<GlobalSystemMediaTransportControlsSessionManager, String> {
    GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(api_error)?
        .join()
        .map_err(api_error)
}

fn snapshot_session(
    session: &GlobalSystemMediaTransportControlsSession,
    app_id: String,
) -> Result<MediaSnapshot, String> {
    let playback_status = session
        .GetPlaybackInfo()
        .map_err(api_error)?
        .PlaybackStatus()
        .map_err(api_error)?;
    let playing =
        playback_status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
    if !playing {
        return Ok(MediaSnapshot {
            app_id,
            title: String::new(),
            artist: String::new(),
            position_ms: 0,
            duration_ms: 0,
            position_sampled_at: Instant::now(),
            playing: false,
        });
    }

    let timeline = session.GetTimelineProperties().map_err(api_error)?;
    let ticks = timeline.Position().map_err(api_error)?.Duration;
    let start_ticks = timeline.StartTime().map_err(api_error)?.Duration;
    let end_ticks = timeline.EndTime().map_err(api_error)?.Duration;
    let position_sampled_at = Instant::now();
    let properties = session
        .TryGetMediaPropertiesAsync()
        .map_err(api_error)?
        .join()
        .map_err(api_error)?;
    Ok(MediaSnapshot {
        app_id,
        title: properties.Title().map_err(api_error)?.to_string_lossy(),
        artist: properties.Artist().map_err(api_error)?.to_string_lossy(),
        position_ms: ticks.max(0) as u64 / 10_000,
        duration_ms: end_ticks.saturating_sub(start_ticks).max(0) as u64 / 10_000,
        position_sampled_at,
        playing: true,
    })
}

fn is_qq_music(app_id: &str) -> bool {
    let app_id = app_id.to_lowercase();
    app_id.contains("qqmusic")
        || app_id.contains("qq music")
        || app_id.contains("qq音乐")
        || app_id.contains("tencent.qqmusic")
}

fn api_error(error: impl std::fmt::Display) -> String {
    format!("读取 Windows 媒体会话失败: {error}")
}

struct RoApartment;

impl RoApartment {
    fn initialize() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|error| format!("初始化 Windows Runtime 失败: {error}"))?;
        Ok(Self)
    }
}

impl Drop for RoApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::is_qq_music;

    #[test]
    fn recognizes_qq_music_session_ids() {
        assert!(is_qq_music("QQMusic.exe"));
        assert!(is_qq_music("Tencent.QQMusic_1"));
        assert!(is_qq_music("QQ音乐"));
        assert!(!is_qq_music("Spotify.exe"));
    }
}
