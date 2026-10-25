use std::{env, fs, path::Path};

use super::lyrics::{self, LyricLine};

pub fn fetch_cached_for_track(
    title: &str,
    artist: &str,
    duration_ms: u64,
) -> Result<Option<(Vec<LyricLine>, bool)>, String> {
    fetch_cached_qrc(title, artist, duration_ms)
}

fn fetch_cached_qrc(
    title: &str,
    artist: &str,
    duration_ms: u64,
) -> Result<Option<(Vec<LyricLine>, bool)>, String> {
    let normalized_title = normalize_cache_component(title);
    if normalized_title.is_empty() {
        return Ok(None);
    }
    let Some(app_data) = env::var_os("APPDATA") else {
        return Ok(None);
    };
    let directory = Path::new(&app_data)
        .join("Tencent")
        .join("QQMusic")
        .join("QQMusicCache")
        .join("QQMusicLyricNew");
    if !directory.exists() {
        return Ok(None);
    }

    let normalized_artists = artist
        .split('/')
        .map(normalize_cache_component)
        .filter(|artist| !artist.is_empty())
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&directory)
        .map_err(|error| format!("读取 QQ 音乐本地歌词缓存目录失败: {error}"))?
    {
        let entry = entry.map_err(|error| format!("枚举 QQ 音乐本地歌词缓存失败: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("读取 QQ 音乐缓存文件类型失败: {error}"))?
            .is_file()
            || !entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with("_qm.qrc")
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let normalized_name = normalize_cache_component(&name);
        if !normalized_name.contains(&normalized_title) {
            continue;
        }
        if !cache_artist_matches(&normalized_name, &normalized_artists) {
            continue;
        }
        let duration_delta = cache_duration_delta_ms(&name, duration_ms).unwrap_or(u64::MAX);
        candidates.push((duration_delta, entry.path()));
    }
    candidates.sort_by(|(left_delta, left_path), (right_delta, right_path)| {
        left_delta
            .cmp(right_delta)
            .then_with(|| left_path.cmp(right_path))
    });

    for (_, path) in candidates {
        let encrypted =
            fs::read(&path).map_err(|error| format!("读取 QQ 音乐本地 QRC 缓存失败: {error}"))?;
        let Ok(decrypted) = decrypt_local_qrc(&encrypted) else {
            continue;
        };
        if let Ok(parsed) = lyrics::parse_qrc(&decrypted) {
            return Ok(Some((parsed, true)));
        }
        if let Ok(parsed) = lyrics::parse_lrc(&decrypted) {
            return Ok(Some((parsed, false)));
        }
    }
    Ok(None)
}

fn cached_duration_ms(filename: &str) -> Option<u64> {
    filename
        .split(" - ")
        .skip(2)
        .find_map(|part| part.parse::<u64>().ok()?.checked_mul(1000))
}

fn cache_artist_matches(normalized_name: &str, normalized_artists: &[String]) -> bool {
    normalized_artists.is_empty()
        || normalized_artists
            .iter()
            .any(|artist| normalized_name.contains(artist))
}

fn cache_duration_delta_ms(filename: &str, duration_ms: u64) -> Option<u64> {
    if duration_ms == 0 {
        Some(0)
    } else {
        cached_duration_ms(filename).map(|cached_duration| cached_duration.abs_diff(duration_ms))
    }
}

fn decrypt_local_qrc(encrypted: &[u8]) -> Result<String, String> {
    const MAGIC_HEADER: &[u8; 11] = b"\x98%\xB0\xAC\xE3\x02\x83h\xE8\xFCl";
    const QMC1_KEY: [u8; 128] = [
        0xC3, 0x4A, 0xD6, 0xCA, 0x90, 0x67, 0xF7, 0x52, 0xD8, 0xA1, 0x66, 0x62, 0x9F, 0x5B, 0x09,
        0x00, 0xC3, 0x5E, 0x95, 0x23, 0x9F, 0x13, 0x11, 0x7E, 0xD8, 0x92, 0x3F, 0xBC, 0x90, 0xBB,
        0x74, 0x0E, 0xC3, 0x47, 0x74, 0x3D, 0x90, 0xAA, 0x3F, 0x51, 0xD8, 0xF4, 0x11, 0x84, 0x9F,
        0xDE, 0x95, 0x1D, 0xC3, 0xC6, 0x09, 0xD5, 0x9F, 0xFA, 0x66, 0xF9, 0xD8, 0xF0, 0xF7, 0xA0,
        0x90, 0xA1, 0xD6, 0xF3, 0xC3, 0xF3, 0xD6, 0xA1, 0x90, 0xA0, 0xF7, 0xF0, 0xD8, 0xF9, 0x66,
        0xFA, 0x9F, 0xD5, 0x09, 0xC6, 0xC3, 0x1D, 0x95, 0xDE, 0x9F, 0x84, 0x11, 0xF4, 0xD8, 0x51,
        0x3F, 0xAA, 0x90, 0x3D, 0x74, 0x47, 0xC3, 0x0E, 0x74, 0xBB, 0x90, 0xBC, 0x3F, 0x92, 0xD8,
        0x7E, 0x11, 0x13, 0x9F, 0x23, 0x95, 0x5E, 0xC3, 0x00, 0x09, 0x5B, 0x9F, 0x62, 0x66, 0xA1,
        0xD8, 0x52, 0xF7, 0x67, 0x90, 0xCA, 0xD6, 0x4A,
    ];
    if encrypted.len() <= MAGIC_HEADER.len() || !encrypted.starts_with(MAGIC_HEADER) {
        return Err("QQ 音乐本地 QRC 文件头无效".to_owned());
    }

    let mut data = encrypted.to_vec();
    for (index, byte) in data.iter_mut().enumerate() {
        let key_index = if index > 0x7fff {
            (index % 0x7fff) & 0x7f
        } else {
            index & 0x7f
        };
        *byte ^= QMC1_KEY[key_index];
    }
    let encrypted_payload = data
        .get(MAGIC_HEADER.len()..)
        .ok_or_else(|| "QQ 音乐本地 QRC 内容为空".to_owned())?;
    let hex = encrypted_payload
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    decrypt_qrc(&hex)
}

fn decrypt_qrc(encrypted: &str) -> Result<String, String> {
    if !encrypted.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("QQ 音乐返回的 QRC 内容不是十六进制密文".to_owned());
    }
    lyrics_crypto::decrypter::qrc::decrypter::decrypt_lyrics(encrypted)
        .ok_or_else(|| "QQ 音乐 QRC 解密失败".to_owned())
}

fn normalize_cache_component(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        cache_artist_matches, cache_duration_delta_ms, cached_duration_ms, decrypt_local_qrc,
        decrypt_qrc, normalize_cache_component,
    };

    #[test]
    fn matches_sanitized_cached_title_with_feature_artist() {
        let filename = "Blake Shelton_Ashley Monroe - Lonely Tonight (feat_ Ashley Monroe) - 218 - Reloaded_ 20 #1 Hits_qm.qrc";
        let normalized_name = normalize_cache_component(filename);
        let normalized_title = normalize_cache_component("Lonely Tonight (feat. Ashley Monroe)");
        let normalized_artists = ["Blake Shelton", "Ashley Monroe"].map(normalize_cache_component);

        assert!(normalized_name.contains(&normalized_title));
        assert!(
            normalized_artists
                .iter()
                .any(|artist| normalized_name.contains(artist))
        );
    }

    #[test]
    fn parses_duration_from_qq_cache_filename() {
        assert_eq!(
            cached_duration_ms("韩磊 - 味道 (Live) - 154 - 精彩音乐汇_qm.qrc"),
            Some(154_000)
        );
        assert_eq!(
            cached_duration_ms("韩磊 - 味道 (Live) - 242 - 我是歌手_qm.qrc"),
            Some(242_000)
        );
    }

    #[test]
    fn prefers_matching_artist_and_closest_track_duration() {
        let normalized_artists = ["韩磊"].map(normalize_cache_component);
        let unrelated_artist =
            normalize_cache_component("张学友 - 味道 (Live) - 242 - 我是歌手_qm.qrc");
        let short_version = "韩磊 - 味道 (Live) - 154 - 精彩音乐汇_qm.qrc";
        let long_version = "韩磊 - 味道 (Live) - 242 - 我是歌手_qm.qrc";

        assert!(!cache_artist_matches(
            &unrelated_artist,
            &normalized_artists
        ));
        assert_eq!(cache_duration_delta_ms(short_version, 155_000), Some(1_000));
        assert_eq!(cache_duration_delta_ms(long_version, 155_000), Some(87_000));
    }

    #[test]
    fn rejects_non_hex_qrc_content() {
        assert!(decrypt_qrc("not hex").is_err());
    }

    #[test]
    #[ignore = "requires HALOBAR_LOCAL_QRC_TEST_FILE to point to a local QQ Music cache file"]
    fn decrypts_local_qrc_cache_when_test_file_is_configured() {
        let path = std::env::var_os("HALOBAR_LOCAL_QRC_TEST_FILE").unwrap();
        let encrypted = std::fs::read(path).unwrap();
        let decrypted = decrypt_local_qrc(&encrypted).unwrap();
        let parsed = super::super::lyrics::parse_qrc(&decrypted)
            .or_else(|_| super::super::lyrics::parse_lrc(&decrypted))
            .unwrap();
        assert!(!parsed.is_empty());
        assert!(parsed.iter().any(|line| !line.chunks.is_empty()));
    }
}
