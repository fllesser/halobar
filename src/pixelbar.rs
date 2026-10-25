use hidapi::HidApi;

const VENDOR_ID: u16 = 0x2D99;
const PRODUCT_IDS: [u16; 2] = [0xA106, 0xA160];
const INTERFACE_NUMBER: i32 = 4;
const USAGE_PAGE: u16 = 0xFF14;
const PACKET_LENGTH: usize = 64;
const CMD_TEXT_SET: u8 = 0xE8;
const CMD_SCREEN_SET: u8 = 0xEF;

pub struct PixelBar {
    device: hidapi::HidDevice,
}

impl PixelBar {
    pub fn connect() -> Result<Option<Self>, String> {
        let api = HidApi::new().map_err(|error| format!("初始化 USB HID 失败: {error}"))?;
        let Some(info) = api.device_list().find(|info| {
            info.vendor_id() == VENDOR_ID
                && PRODUCT_IDS.contains(&info.product_id())
                && info.interface_number() == INTERFACE_NUMBER
                && info.usage_page() == USAGE_PAGE
        }) else {
            return Ok(None);
        };

        let device = api
            .open_path(info.path())
            .map_err(|error| format!("打开 Halo PixelBar 失败: {error}"))?;

        Ok(Some(Self { device }))
    }

    pub fn prepare_text_layout(&self) -> Result<(), String> {
        let payload = [0x01, 0xFF, 0xFF, 0xFF, 0x00, 0x02, 0x00, 0x00, 0xFF];
        self.write_packet(CMD_SCREEN_SET, &payload)
    }

    pub fn show_clock(&self) -> Result<(), String> {
        let payload = [0x01, 0xFF, 0xFF, 0xFF, 0x00, 0x01, 0x00, 0x02, 0xFF];
        self.write_packet(CMD_SCREEN_SET, &payload)
    }

    pub fn show_spectrum(&self) -> Result<(), String> {
        self.show_spectrum_style(1)
    }

    pub fn show_spectrum_style(&self, style: u8) -> Result<(), String> {
        let packet = spectrum_packet(style)?;
        self.device
            .write(&packet)
            .map_err(|error| format!("发送到 PixelBar 失败: {error}"))?;
        Ok(())
    }

    pub fn send_text(&self, text: &str) -> Result<(), String> {
        let visible = limit_text(text, 32, 48);
        let utf8 = visible.as_bytes();
        let mut payload = vec![0x00, utf8.len() as u8];
        payload.extend_from_slice(utf8);
        self.write_packet(CMD_TEXT_SET, &payload)
    }

    fn write_packet(&self, command: u8, payload: &[u8]) -> Result<(), String> {
        let total_len = 6 + payload.len() + 1;
        if total_len > PACKET_LENGTH {
            return Err("HID 输出超出 64 字节限制".to_owned());
        }

        let mut packet = Vec::with_capacity(PACKET_LENGTH);
        packet.extend_from_slice(&[0x2E, 0xAA, 0xED, command, 0, 0]);
        let len = payload.len() as u16;
        packet[4] = (len >> 8) as u8;
        packet[5] = len as u8;
        packet.extend_from_slice(payload);
        let checksum = packet[1..]
            .iter()
            .fold(0u8, |sum, value| sum.wrapping_add(*value));
        packet.push(checksum);
        packet.resize(PACKET_LENGTH, 0);

        self.device
            .write(&packet)
            .map_err(|error| format!("发送到 PixelBar 失败: {error}"))?;
        Ok(())
    }
}

fn limit_text(input: &str, max_chars: usize, max_bytes: usize) -> String {
    let mut result = String::new();
    for ch in input.chars() {
        if result.chars().count() >= max_chars || result.len() + ch.len_utf8() > max_bytes {
            break;
        }
        result.push(ch);
    }
    result
}

fn spectrum_packet(style: u8) -> Result<[u8; PACKET_LENGTH], String> {
    if !(1..=4).contains(&style) {
        return Err(format!("不支持的频谱样式: {style}"));
    }

    let style_index = style - 1;
    let checksum = 0x40u8.wrapping_add(0x08).wrapping_add(style_index);
    let mut packet = [0; PACKET_LENGTH];
    packet[..17].copy_from_slice(&[
        0x2E,
        0xAA,
        0xEC,
        CMD_SCREEN_SET,
        0x00,
        0x09,
        0x01,
        0xC0,
        0xFF,
        0xF2,
        0x00,
        0x01,
        0x08,
        style_index,
        0xFF,
        checksum,
        0x00,
    ]);
    Ok(packet)
}

#[cfg(test)]
mod tests {
    use super::{limit_text, spectrum_packet};

    #[test]
    fn limits_text_by_character_count() {
        let visible = limit_text("你好世界Hello", 6, 48);
        assert_eq!(visible, "你好世界He");
    }

    #[test]
    fn limits_text_by_utf8_byte_count() {
        let visible = limit_text("😀".repeat(20).as_str(), 16, 48);
        assert_eq!(visible, "😀".repeat(12));
    }

    #[test]
    fn allows_thirty_two_ascii_characters() {
        let visible = limit_text("abcdefghijklmnopqrstuvwxyzABCDEF", 32, 48);
        assert_eq!(visible.chars().count(), 32);
    }

    #[test]
    fn builds_first_spectrum_style_report() {
        let packet = spectrum_packet(1).unwrap();
        assert_eq!(
            &packet[..17],
            &[
                0x2E, 0xAA, 0xEC, 0xEF, 0x00, 0x09, 0x01, 0xC0, 0xFF, 0xF2, 0x00, 0x01, 0x08, 0x00,
                0xFF, 0x48, 0x00
            ]
        );
        assert!(packet[17..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn rejects_unsupported_spectrum_styles() {
        assert!(spectrum_packet(0).is_err());
        assert!(spectrum_packet(5).is_err());
    }
}
