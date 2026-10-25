#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub at_ms: u64,
    pub text: String,
    pub chunks: Vec<TimedChunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedChunk {
    pub at_ms: u64,
    pub text: String,
}

pub fn parse_lrc(input: &str) -> Result<Vec<LyricLine>, String> {
    let mut offset_ms = 0_i64;
    let mut lyrics = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("[offset:") {
            if let Some(value) = value.strip_suffix(']')
                && let Ok(value) = value.parse::<i64>()
            {
                offset_ms = value;
            }
            continue;
        }

        let mut rest = trimmed;
        let mut timestamps = Vec::new();
        while let Some(after_open) = rest.strip_prefix('[') {
            let Some((tag, after_close)) = after_open.split_once(']') else {
                break;
            };
            match parse_timestamp(tag) {
                Some(at_ms) => {
                    timestamps.push(at_ms);
                    rest = after_close;
                }
                None => break,
            }
        }

        if timestamps.is_empty() {
            continue;
        }

        let text = rest.trim();
        if text.is_empty() {
            continue;
        }
        for at_ms in timestamps {
            let adjusted = at_ms as i64 + offset_ms;
            lyrics.push(LyricLine {
                at_ms: adjusted.max(0) as u64,
                text: text.to_owned(),
                chunks: vec![TimedChunk {
                    at_ms: adjusted.max(0) as u64,
                    text: text.to_owned(),
                }],
            });
        }
    }

    if lyrics.is_empty() {
        return Err("没有识别到歌词时间轴".to_owned());
    }

    lyrics.sort_by_key(|line| line.at_ms);
    Ok(lyrics)
}

pub fn parse_qrc(input: &str) -> Result<Vec<LyricLine>, String> {
    let content = extract_qrc_content(input)?;
    let mut lyrics = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((line_tag, rest)) = rest.split_once(']') else {
            continue;
        };
        let Some((line_start, _line_duration)) = parse_pair(line_tag) else {
            continue;
        };

        let mut chunks = Vec::new();
        let mut remaining = rest;
        while let Some(open) = remaining.find('(') {
            let Some(close_offset) = remaining[open + 1..].find(')') else {
                break;
            };
            let close = open + 1 + close_offset;
            let timestamp = &remaining[open + 1..close];
            let Some((at_ms, _duration)) = parse_pair(timestamp) else {
                break;
            };
            let text = remaining[..open].to_owned();
            if !text.is_empty() {
                chunks.push(TimedChunk { at_ms, text });
            }
            remaining = &remaining[close + 1..];
        }
        if chunks.is_empty() && !rest.is_empty() {
            chunks.push(TimedChunk {
                at_ms: line_start,
                text: rest.to_owned(),
            });
        }
        let text = chunks
            .iter()
            .map(|chunk| chunk.text.as_str())
            .collect::<String>();
        if !text.trim().is_empty() {
            lyrics.push(LyricLine {
                at_ms: line_start,
                text,
                chunks,
            });
        }
    }

    if lyrics.is_empty() {
        return Err("QQ 音乐返回的逐字歌词中没有可识别的时间轴".to_owned());
    }
    lyrics.sort_by_key(|line| line.at_ms);
    for line in &mut lyrics {
        line.chunks.sort_by_key(|chunk| chunk.at_ms);
    }
    Ok(lyrics)
}

fn parse_pair(value: &str) -> Option<(u64, u64)> {
    let (start, duration) = value.split_once(',')?;
    Some((start.parse().ok()?, duration.parse().ok()?))
}

fn extract_qrc_content(input: &str) -> Result<String, String> {
    let Some(marker) = input.find("LyricContent=\"") else {
        if input.lines().any(|line| line.trim_start().starts_with('[')) {
            return Ok(input.to_owned());
        }
        return Err("QQ 音乐返回的数据中没有逐字歌词内容".to_owned());
    };
    let start = marker + "LyricContent=\"".len();
    let end = input[start..]
        .find("\"/>")
        .map(|offset| start + offset)
        .ok_or_else(|| "QQ 音乐逐字歌词 XML 内容不完整".to_owned())?;
    Ok(unescape_xml(&input[start..end]))
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&#9;", "\t")
        .replace("&#xA;", "\n")
        .replace("&#xD;", "\r")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn parse_timestamp(value: &str) -> Option<u64> {
    let (minutes, seconds) = value.split_once(':')?;
    let minutes = minutes.parse::<u64>().ok()?;
    let (whole_seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, "0"));
    let whole_seconds = whole_seconds.parse::<u64>().ok()?;
    if whole_seconds >= 60 {
        return None;
    }
    let fraction_ms = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()? * 100,
        2 => fraction.parse::<u64>().ok()? * 10,
        _ => {
            let digits = &fraction[..3];
            digits.parse::<u64>().ok()?
        }
    };
    Some((minutes * 60 + whole_seconds) * 1000 + fraction_ms)
}

#[cfg(test)]
mod tests {
    use super::{parse_lrc, parse_qrc};

    #[test]
    fn parses_lrc_lines() {
        let lyrics = parse_lrc("[00:00.00]第一行\n[00:02.00]第二行\n[00:05.00]第三行").unwrap();
        assert_eq!(lyrics.len(), 3);
        assert_eq!(lyrics[0].text, "第一行");
        assert_eq!(lyrics[1].at_ms, 2000);
        assert_eq!(lyrics[0].chunks[0].at_ms, 0);
    }

    #[test]
    fn applies_offset() {
        let lyrics = parse_lrc("[offset:-500]\n[00:00.20]过渡").unwrap();
        assert_eq!(lyrics[0].at_ms, 0);
    }

    #[test]
    fn parses_word_timestamps_from_qrc() {
        let lyrics = parse_qrc(
            r#"<Lyric_1 LyricType="1" LyricContent="[1000,1000]你(1000,250)好(1250,750)世界(2000,200)"/>"#,
        )
        .unwrap();
        assert_eq!(lyrics[0].text, "你好世界");
        assert_eq!(lyrics[0].chunks.len(), 3);
        assert_eq!(lyrics[0].chunks[1].at_ms, 1250);
    }
}
