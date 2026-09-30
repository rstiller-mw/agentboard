use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub text: Rgb,
    pub soft: Rgb,
    pub dim: Rgb,
    pub accent: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub red: Rgb,
    pub blue: Rgb,
    pub cyan: Rgb,
}

impl Palette {
    const FALLBACK: Palette = Palette {
        text: Rgb(0xd0, 0xd0, 0xd0),
        soft: Rgb(0x90, 0x90, 0x90),
        dim: Rgb(0x55, 0x55, 0x55),
        accent: Rgb(0x7a, 0xa2, 0xf7),
        green: Rgb(0x9e, 0xce, 0x6a),
        yellow: Rgb(0xe0, 0xaf, 0x68),
        red: Rgb(0xf7, 0x76, 0x8e),
        blue: Rgb(0x7a, 0xa2, 0xf7),
        cyan: Rgb(0x7d, 0xcf, 0xff),
    };

    /// Follows the active Omarchy theme, so a theme switch shows up on the next refresh.
    pub fn load() -> Palette {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        let path = state.join("omarchy/current/theme/colors.toml");
        std::fs::read_to_string(path).map(|toml| Palette::from_toml(&toml)).unwrap_or(Palette::FALLBACK)
    }

    fn from_toml(toml: &str) -> Palette {
        let colors: HashMap<&str, Rgb> = toml
            .lines()
            .filter_map(|line| {
                let (key, value) = line.split_once('=')?;
                Some((key.trim(), parse_hex(value.trim().trim_matches('"'))?))
            })
            .collect();
        let pick = |key: &str, fallback: Rgb| colors.get(key).copied().unwrap_or(fallback);
        let d = Palette::FALLBACK;
        Palette {
            text: pick("foreground", d.text),
            soft: pick("light_foreground", d.soft),
            dim: pick("dark_foreground", d.dim),
            accent: pick("accent", d.accent),
            green: pick("green", d.green),
            yellow: pick("yellow", d.yellow),
            red: pick("red", d.red),
            blue: pick("blue", d.blue),
            cyan: pick("cyan", d.cyan),
        }
    }
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    (hex.len() == 6).then_some(())?;
    Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_omarchy_colors_and_falls_back_for_missing_keys() {
        let p = Palette::from_toml("accent = \"#78824b\"\nmode = \"dark\"\nforeground = \"#c2c2b0\"\n");
        assert_eq!(p.accent, Rgb(0x78, 0x82, 0x4b));
        assert_eq!(p.text, Rgb(0xc2, 0xc2, 0xb0));
        assert_eq!(p.green, Palette::FALLBACK.green);
    }
}
