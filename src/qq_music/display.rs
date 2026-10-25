use super::lyrics::LyricLine;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    Scroll,
    Typewriter,
}

impl DisplayMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "scroll" | "滚动" => Ok(Self::Scroll),
            "typewriter" | "逐字" => Ok(Self::Typewriter),
            _ => Err("显示模式应为 scroll 或 typewriter".to_owned()),
        }
    }
}

pub fn at_position(lyrics: &[LyricLine], mode: DisplayMode, position_ms: u64) -> Option<String> {
    let index = lyrics
        .partition_point(|lyric| lyric.at_ms <= position_ms)
        .checked_sub(1)?;
    let lyric = &lyrics[index];
    match mode {
        DisplayMode::Typewriter => Some(render_typewriter(lyric, position_ms, 32, 48)),
        DisplayMode::Scroll => Some(render_scroll(
            &lyric.text,
            position_ms.saturating_sub(lyric.at_ms),
        )),
    }
}

fn render_scroll(text: &str, elapsed_ms: u64) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let cycle = format!("{trimmed}    {trimmed}");
    let chars: Vec<char> = cycle.chars().collect();
    if chars.len() <= 16 {
        return clamp_to_width(trimmed, 16);
    }
    let offset = ((elapsed_ms / 220) as usize) % (chars.len() / 2);
    let visible: String = chars.iter().skip(offset).take(16).collect();
    clamp_to_width(&visible, 16)
}

fn clamp_to_width(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn render_typewriter(
    lyric: &LyricLine,
    position_ms: u64,
    page_width: usize,
    max_bytes: usize,
) -> String {
    let chunks = if lyric.chunks.is_empty() {
        vec![super::lyrics::TimedChunk {
            at_ms: lyric.at_ms,
            text: lyric.text.clone(),
        }]
    } else {
        lyric.chunks.clone()
    };
    let graphemes = chunks
        .iter()
        .flat_map(|chunk| {
            chunk.text.graphemes(true).map(move |grapheme| {
                (
                    chunk.at_ms,
                    grapheme,
                    UnicodeWidthStr::width(grapheme),
                    grapheme.len(),
                )
            })
        })
        .collect::<Vec<_>>();
    let revealed_count = graphemes
        .iter()
        .filter(|(at_ms, _, _, _)| *at_ms <= position_ms)
        .count();

    let mut pages = vec![(0, 0, 0)];
    let mut current_width = 0;
    let mut current_bytes = 0;
    for (index, (_, _, width, bytes)) in graphemes.iter().enumerate() {
        if current_width > 0
            && (current_width + width > page_width || current_bytes + bytes > max_bytes)
        {
            pages.push((index, 0, 0));
            current_width = 0;
            current_bytes = 0;
        }
        if *width > page_width || *bytes > max_bytes {
            continue;
        }
        current_width += *width;
        current_bytes += *bytes;
        let page = pages.last_mut().expect("first page exists");
        page.1 = current_width;
        page.2 = current_bytes;
    }

    let page = if chunks.len() > 1 {
        pages
            .partition_point(|(start, _, _)| *start < revealed_count)
            .saturating_sub(1)
    } else {
        0
    };
    let (start, content_width, content_bytes) = pages[page];
    let end = pages
        .get(page + 1)
        .map_or(graphemes.len(), |(next_start, _, _)| *next_start);
    let padding = (page_width - content_width) / 2;
    let left_padding = padding.min(max_bytes.saturating_sub(content_bytes));
    let right_padding = page_width
        .saturating_sub(content_width + left_padding)
        .min(max_bytes.saturating_sub(content_bytes + left_padding));

    std::iter::repeat_n(" ", left_padding)
        .chain(graphemes.iter().enumerate().take(end).skip(start).flat_map(
            |(_, (at_ms, grapheme, width, _))| {
                if *at_ms <= position_ms {
                    std::iter::once(*grapheme).collect::<Vec<_>>()
                } else if *width == 2 {
                    vec!["　"]
                } else {
                    vec![" "]
                }
            },
        ))
        .chain(std::iter::repeat_n(" ", right_padding))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::lyrics::LyricLine;
    use super::{DisplayMode, at_position, render_scroll};

    #[test]
    fn scroll_mode_wraps_the_text() {
        let first = render_scroll("一二三四五六七八九十", 0);
        let second = render_scroll("一二三四五六七八九十", 220);
        assert_ne!(first, second);
    }

    #[test]
    fn returns_no_line_before_first_timestamp() {
        let lyrics = [LyricLine {
            at_ms: 1_000,
            text: "歌词".to_owned(),
            chunks: vec![],
        }];
        assert_eq!(at_position(&lyrics, DisplayMode::Scroll, 0), None);
    }

    #[test]
    fn typewriter_uses_placeholders_instead_of_scrolling_revealed_text() {
        let lyrics = [LyricLine {
            at_ms: 1_000,
            text: "你好世界".to_owned(),
            chunks: vec![
                super::super::lyrics::TimedChunk {
                    at_ms: 1_000,
                    text: "你".to_owned(),
                },
                super::super::lyrics::TimedChunk {
                    at_ms: 1_600,
                    text: "好".to_owned(),
                },
                super::super::lyrics::TimedChunk {
                    at_ms: 2_800,
                    text: "世界".to_owned(),
                },
            ],
        }];
        let first_character = format!("{}你{}{}", " ".repeat(12), "　".repeat(3), " ".repeat(12));
        let second_character =
            format!("{}你好{}{}", " ".repeat(12), "　".repeat(2), " ".repeat(12));
        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 1_599).as_deref(),
            Some(first_character.as_str())
        );
        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 1_600).as_deref(),
            Some(second_character.as_str())
        );
    }

    #[test]
    fn typewriter_switches_pages_only_after_current_page_is_revealed() {
        let text = "一二三四五六七八九十一二三四五六七八九十".to_owned();
        let lyrics = [LyricLine {
            at_ms: 0,
            text: text.clone(),
            chunks: text
                .chars()
                .enumerate()
                .map(|(index, character)| super::super::lyrics::TimedChunk {
                    at_ms: index as u64,
                    text: character.to_string(),
                })
                .collect(),
        }];
        let second_page = format!("{}七{}{}", " ".repeat(12), "　".repeat(3), " ".repeat(12));
        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 15).as_deref(),
            Some("一二三四五六七八九十一二三四五六")
        );
        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 16).as_deref(),
            Some(second_page.as_str())
        );
    }

    #[test]
    fn typewriter_fallback_keeps_the_start_of_untimed_lines() {
        let text = "一二三四五六七八九十一二三四五六七八九十";
        let lyrics = [LyricLine {
            at_ms: 0,
            text: text.to_owned(),
            chunks: vec![super::super::lyrics::TimedChunk {
                at_ms: 0,
                text: text.to_owned(),
            }],
        }];
        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 0).as_deref(),
            Some("一二三四五六七八九十一二三四五六")
        );
    }

    #[test]
    fn typewriter_centers_short_lyrics_by_total_line_length() {
        let lyrics = [LyricLine {
            at_ms: 0,
            text: "你好世界".to_owned(),
            chunks: vec![
                super::super::lyrics::TimedChunk {
                    at_ms: 0,
                    text: "你".to_owned(),
                },
                super::super::lyrics::TimedChunk {
                    at_ms: 10,
                    text: "好".to_owned(),
                },
                super::super::lyrics::TimedChunk {
                    at_ms: 20,
                    text: "世界".to_owned(),
                },
            ],
        }];
        let first_character = format!("{}你{}{}", " ".repeat(12), "　".repeat(3), " ".repeat(12));
        let whole_line = format!("{}你好世界{}", " ".repeat(12), " ".repeat(12));

        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 0).as_deref(),
            Some(first_character.as_str())
        );
        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 20).as_deref(),
            Some(whole_line.as_str())
        );
    }

    #[test]
    fn one_page_can_show_sixteen_cjk_characters() {
        let text = "一二三四五六七八九十一二三四五六";
        let lyrics = [LyricLine {
            at_ms: 0,
            text: text.to_owned(),
            chunks: text
                .chars()
                .enumerate()
                .map(|(index, character)| super::super::lyrics::TimedChunk {
                    at_ms: index as u64,
                    text: character.to_string(),
                })
                .collect(),
        }];

        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 15).as_deref(),
            Some(text)
        );
    }

    #[test]
    fn typewriter_supports_thirty_two_ascii_letters() {
        let text = "abcdefghijklmnopqrstuvwxyzABCDEF";
        let lyrics = [LyricLine {
            at_ms: 0,
            text: text.to_owned(),
            chunks: text
                .chars()
                .enumerate()
                .map(|(index, character)| super::super::lyrics::TimedChunk {
                    at_ms: index as u64,
                    text: character.to_string(),
                })
                .collect(),
        }];

        assert_eq!(
            at_position(&lyrics, DisplayMode::Typewriter, 31).as_deref(),
            Some(text)
        );
    }

    #[test]
    fn typewriter_uses_half_and_full_width_placeholders_for_mixed_text() {
        let lyrics = [LyricLine {
            at_ms: 0,
            text: "A中B".to_owned(),
            chunks: vec![
                super::super::lyrics::TimedChunk {
                    at_ms: 0,
                    text: "A".to_owned(),
                },
                super::super::lyrics::TimedChunk {
                    at_ms: 10,
                    text: "中".to_owned(),
                },
                super::super::lyrics::TimedChunk {
                    at_ms: 20,
                    text: "B".to_owned(),
                },
            ],
        }];

        let output = at_position(&lyrics, DisplayMode::Typewriter, 0).unwrap();
        assert_eq!(output, format!("{}A　 {}", " ".repeat(14), " ".repeat(14)));
        assert!(output.len() <= 48);
    }
}
