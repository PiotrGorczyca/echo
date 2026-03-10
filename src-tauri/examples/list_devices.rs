use cpal::traits::{DeviceTrait, HostTrait};
use std::collections::HashMap;

fn get_alsa_card_descriptions() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(content) = std::fs::read_to_string("/proc/asound/cards") {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(bracket_start) = trimmed.find('[') {
                if let Some(bracket_end) = trimmed.find(']') {
                    let card_id = trimmed[bracket_start + 1..bracket_end].trim().to_string();
                    if let Some(dash_pos) = trimmed.find(" - ") {
                        let friendly = trimmed[dash_pos + 3..].trim().to_string();
                        map.insert(card_id, friendly);
                    }
                }
            }
        }
    }
    map
}

fn friendly_device_name(alsa_name: &str, cards: &HashMap<String, String>) -> Option<String> {
    match alsa_name {
        "pipewire" | "pulse" | "default" => {
            return Some(format!("System Default ({})", alsa_name));
        }
        _ => {}
    }
    if let Some(card_pos) = alsa_name.find("CARD=") {
        let after_card = &alsa_name[card_pos + 5..];
        let card_id = after_card.split(&[',', ' ', ':'][..]).next().unwrap_or(after_card);
        if let Some(friendly) = cards.get(card_id) {
            let prefix = alsa_name.split(':').next().unwrap_or("");
            if prefix.starts_with("surround") { return None; }
            let qualifier = match prefix {
                "sysdefault" | "hw" => "",
                "front" => " (Front)",
                _ => "",
            };
            return Some(format!("{}{}", friendly, qualifier));
        }
    }
    Some(alsa_name.to_string())
}

fn main() {
    let host = cpal::default_host();
    let cards = get_alsa_card_descriptions();

    println!("ALSA cards: {:?}", cards);
    println!("\n--- Input devices (friendly) ---");
    match host.input_devices() {
        Ok(devices) => {
            for (i, device) in devices.enumerate() {
                let raw = device.name().unwrap_or_else(|_| "???".into());
                let friendly = friendly_device_name(&raw, &cards);
                let config = device.default_input_config();
                match (friendly, config) {
                    (Some(name), Ok(c)) => println!("  [{}] {} — {}Hz, {}ch, {:?}  (raw: {})",
                        i, name, c.sample_rate().0, c.channels(), c.sample_format(), raw),
                    (Some(name), Err(e)) => println!("  [{}] {} — config error: {}  (raw: {})",
                        i, name, e, raw),
                    (None, _) => println!("  [{}] (filtered out: {})", i, raw),
                }
            }
        }
        Err(e) => println!("  Error: {}", e),
    }
}
