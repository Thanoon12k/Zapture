//! Settings file handling. The format is a small `key = value` subset of TOML
//! kept deliberately dependency-free; see `Config::save` for the emitted shape.

use std::fmt;
use std::path::Path;

pub const APP_NAME: &str = "Zapture";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    F(u8),
    Char(u8), // ASCII uppercase letter or digit, matches virtual-key codes on Windows
    PrintScreen,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    pub key: Key,
}

impl Hotkey {
    pub const fn plain(key: Key) -> Self {
        Hotkey { ctrl: false, shift: false, alt: false, win: false, key }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::F(n) => write!(f, "F{n}"),
            Key::Char(c) => write!(f, "{}", *c as char),
            Key::PrintScreen => write!(f, "PrintScreen"),
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            write!(f, "Ctrl+")?;
        }
        if self.shift {
            write!(f, "Shift+")?;
        }
        if self.alt {
            write!(f, "Alt+")?;
        }
        if self.win {
            write!(f, "Win+")?;
        }
        write!(f, "{}", self.key)
    }
}

pub fn parse_key(s: &str) -> Option<Key> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "printscreen" | "prtsc" | "prtscn" => return Some(Key::PrintScreen),
        _ => {}
    }
    if let Some(num) = lower.strip_prefix('f') {
        if let Ok(n) = num.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Some(Key::F(n));
            }
        }
    }
    let bytes = s.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() {
        return Some(Key::Char(bytes[0].to_ascii_uppercase()));
    }
    None
}

pub fn parse_hotkey(s: &str) -> Option<Hotkey> {
    let mut hk = Hotkey::plain(Key::F(9));
    let mut key = None;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => hk.ctrl = true,
            "shift" => hk.shift = true,
            "alt" => hk.alt = true,
            "win" | "super" | "meta" => hk.win = true,
            _ => {
                if key.is_some() {
                    return None;
                }
                key = Some(parse_key(part)?);
            }
        }
    }
    hk.key = key?;
    Some(hk)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioSource {
    Off,
    System,
    Mic,
    Both,
}

impl AudioSource {
    pub fn code(self) -> &'static str {
        match self {
            AudioSource::Off => "off",
            AudioSource::System => "system",
            AudioSource::Mic => "mic",
            AudioSource::Both => "both",
        }
    }

    pub fn from_code(s: &str) -> Option<AudioSource> {
        match s {
            "off" => Some(AudioSource::Off),
            "system" => Some(AudioSource::System),
            "mic" | "microphone" => Some(AudioSource::Mic),
            "both" => Some(AudioSource::Both),
            _ => None,
        }
    }

    pub fn next(self) -> AudioSource {
        match self {
            AudioSource::Off => AudioSource::System,
            AudioSource::System => AudioSource::Mic,
            AudioSource::Mic => AudioSource::Both,
            AudioSource::Both => AudioSource::Off,
        }
    }
}

/// UI language preference; `Auto` follows the system UI language.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LangPref {
    Auto,
    En,
    Zh,
}

impl LangPref {
    pub fn code(self) -> &'static str {
        match self {
            LangPref::Auto => "auto",
            LangPref::En => "en",
            LangPref::Zh => "zh",
        }
    }

    pub fn from_code(s: &str) -> Option<LangPref> {
        match s {
            "auto" => Some(LangPref::Auto),
            "en" => Some(LangPref::En),
            "zh" | "zh-cn" | "zh-hans" => Some(LangPref::Zh),
            _ => None,
        }
    }
}

/// Optional decorative backdrop for screenshots and exports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BgStyle {
    None,
    Graphite,
    Ocean,
    Sunset,
    Snow,
    Forest,
    Rose,
    Lavender,
    Sky,
}

impl BgStyle {
    pub fn code(self) -> &'static str {
        match self {
            BgStyle::None => "none",
            BgStyle::Graphite => "graphite",
            BgStyle::Ocean => "ocean",
            BgStyle::Sunset => "sunset",
            BgStyle::Snow => "snow",
            BgStyle::Forest => "forest",
            BgStyle::Rose => "rose",
            BgStyle::Lavender => "lavender",
            BgStyle::Sky => "sky",
        }
    }

    pub fn from_code(s: &str) -> Option<BgStyle> {
        match s {
            "none" | "off" => Some(BgStyle::None),
            "graphite" => Some(BgStyle::Graphite),
            "ocean" => Some(BgStyle::Ocean),
            "sunset" => Some(BgStyle::Sunset),
            "snow" => Some(BgStyle::Snow),
            "forest" => Some(BgStyle::Forest),
            "rose" => Some(BgStyle::Rose),
            "lavender" => Some(BgStyle::Lavender),
            "sky" => Some(BgStyle::Sky),
            _ => None,
        }
    }

    /// Backdrop gradient as (top, bottom) ARGB. `None` is never drawn.
    pub fn colors(self) -> (u32, u32) {
        match self {
            BgStyle::None => (0xFF00_0000, 0xFF00_0000),
            BgStyle::Graphite => (0xFF2A_2E37, 0xFF15_171C),
            BgStyle::Ocean => (0xFF20_3A5C, 0xFF0B_1A2E),
            BgStyle::Sunset => (0xFFE7_9A6B, 0xFF6E_3A6B),
            BgStyle::Snow => (0xFFF5_F7FB, 0xFFDA_E0EA),
            BgStyle::Forest => (0xFF2F_5D40, 0xFF10_2A1C),
            BgStyle::Rose => (0xFFD8_7A93, 0xFF6B_2740),
            BgStyle::Lavender => (0xFF8E_7CC9, 0xFF3F_3273),
            BgStyle::Sky => (0xFFBF_D9F2, 0xFF8F_B8E0),
        }
    }
}

/// Which edge of a recording carries the caption bar, if any.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptionEdge {
    None,
    Top,
    Bottom,
    Left,
    Right,
}

impl CaptionEdge {
    pub fn code(self) -> &'static str {
        match self {
            CaptionEdge::None => "none",
            CaptionEdge::Top => "top",
            CaptionEdge::Bottom => "bottom",
            CaptionEdge::Left => "left",
            CaptionEdge::Right => "right",
        }
    }

    pub fn from_code(s: &str) -> Option<CaptionEdge> {
        match s {
            "none" | "off" => Some(CaptionEdge::None),
            "top" => Some(CaptionEdge::Top),
            "bottom" => Some(CaptionEdge::Bottom),
            "left" => Some(CaptionEdge::Left),
            "right" => Some(CaptionEdge::Right),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub output_dir: String,
    pub open_shot_folder: bool,
    pub open_record_folder: bool,
    pub bar_scale: u32,
    pub recording_dot: bool,

    pub fps: u32,
    /// Video bitrate in kbit/s; 0 selects an automatic value from region size and fps.
    pub bitrate_kbps: u32,
    pub audio: AudioSource,
    pub cursor: bool,
    /// Glow the cursor and ripple on click, composited into recordings.
    pub highlight: bool,
    /// Decorative backdrop for video exports (off by default).
    pub bg: BgStyle,
    /// Decorative backdrop for screenshots and clipboard snips (on by default).
    pub shot_bg: BgStyle,
    /// Text tiled across a snip when the watermark tool is on; empty uses a
    /// built-in wording in the current language.
    pub watermark: String,
    /// Edge of the recording that carries the caption bar.
    pub caption_edge: CaptionEdge,
    /// Caption bar label; `\n` starts another line.
    pub caption_text: String,
    /// Caption bar thickness in capture pixels; 0 sizes it from the text.
    pub caption_size: i32,
    /// MP4 export: auto-zoom toward recorded clicks.
    pub autozoom: bool,
    pub language: LangPref,
    pub hk_record: Hotkey,
    pub hk_stop: Hotkey,
    pub hk_shot: Hotkey,
    /// Freeform region snip to the clipboard.
    pub hk_snip: Hotkey,
    /// Last capture-frame rectangle (x, y, w, h) in screen pixels.
    pub frame: Option<(i32, i32, i32, i32)>,
    /// Webcam overlay shape: circle (else rounded square).
    pub cam_circle: bool,
    /// Last webcam overlay rectangle (x, y, w, h).
    pub cam_frame: Option<(i32, i32, i32, i32)>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            output_dir: String::new(),
            open_shot_folder: true,
            open_record_folder: true,
            bar_scale: 100,
            recording_dot: true,
            fps: 30,
            bitrate_kbps: 0,
            audio: AudioSource::System,
            cursor: true,
            highlight: false,
            bg: BgStyle::None,
            shot_bg: BgStyle::None,
            watermark: String::new(),
            caption_edge: CaptionEdge::None,
            caption_text: String::new(),
            caption_size: 0,
            autozoom: false,
            language: LangPref::En,
            hk_record: Hotkey::plain(Key::F(9)),
            hk_stop: Hotkey::plain(Key::F(10)),
            hk_shot: Hotkey::plain(Key::F(8)),
            hk_snip: Hotkey { ctrl: false, shift: false, alt: true, win: false, key: Key::Char(b'A') },
            frame: None,
            cam_circle: true,
            cam_frame: None,
        }
    }
}

pub fn auto_bitrate_kbps(w: u32, h: u32, fps: u32) -> u32 {
    // Roughly 0.1 bit per pixel per frame; 1080p30 lands around 6.2 Mbit/s.
    ((w as u64 * h as u64 * fps as u64) / 10_000).clamp(1_500, 40_000) as u32
}

fn strip_comment(line: &str) -> &str {
    // Keep '#' inside quoted values; cut everything after an unquoted one.
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn unquote(v: &str) -> &str {
    v.strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(v)
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        let (mut fx, mut fy, mut fw, mut fh) = (None, None, None, None);
        let (mut cx, mut cy, mut cw, mut ch) = (None, None, None, None);
        let mut saw_shot_bg = false;
        for raw in text.lines() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "output_dir" => c.output_dir = unquote(v).to_string(),
                "open_shot_folder" => if let Some(b)=parse_bool(v) { c.open_shot_folder=b; },
                "open_record_folder" => if let Some(b)=parse_bool(v) { c.open_record_folder=b; },
                "recording_dot" => if let Some(b)=parse_bool(v) { c.recording_dot=b; },
                "bar_scale" => if let Ok(n)=v.parse::<u32>() { c.bar_scale=n.clamp(75,150); },
                "fps" => {
                    if let Ok(n) = v.parse() {
                        c.fps = n;
                    }
                }
                "bitrate_kbps" => {
                    if let Ok(n) = v.parse() {
                        c.bitrate_kbps = n;
                    }
                }
                "audio_source" => {
                    if let Some(s) = AudioSource::from_code(unquote(v)) {
                        c.audio = s;
                    }
                }
                // Pre-0.1 key: audio = true/false.
                "audio" => {
                    if let Some(b) = parse_bool(v) {
                        c.audio = if b { AudioSource::System } else { AudioSource::Off };
                    }
                }
                "cursor" => {
                    if let Some(b) = parse_bool(v) {
                        c.cursor = b;
                    }
                }
                "highlight" => {
                    if let Some(b) = parse_bool(v) {
                        c.highlight = b;
                    }
                }
                "background" => {
                    if let Some(b) = BgStyle::from_code(unquote(v)) {
                        c.bg = b;
                    }
                }
                "watermark" => c.watermark = unquote(v).to_string(),
                "caption_edge" => {
                    if let Some(e) = CaptionEdge::from_code(unquote(v)) {
                        c.caption_edge = e;
                    }
                }
                "caption_text" => c.caption_text = unquote(v).replace("\\n", "\n"),
                "caption_size" => {
                    if let Ok(n) = v.parse() {
                        c.caption_size = n;
                    }
                }
                "screenshot_background" | "shot_background" => {
                    if let Some(b) = BgStyle::from_code(unquote(v)) {
                        c.shot_bg = b;
                        saw_shot_bg = true;
                    }
                }
                "autozoom" | "auto_zoom" => {
                    if let Some(b) = parse_bool(v) {
                        c.autozoom = b;
                    }
                }
                "language" => {
                    if let Some(l) = LangPref::from_code(unquote(v)) {
                        c.language = l;
                    }
                }
                "hotkey_record" => {
                    if let Some(h) = parse_hotkey(unquote(v)) {
                        c.hk_record = h;
                    }
                }
                "hotkey_stop" => {
                    if let Some(h) = parse_hotkey(unquote(v)) {
                        c.hk_stop = h;
                    }
                }
                "hotkey_shot" => {
                    if let Some(h) = parse_hotkey(unquote(v)) {
                        c.hk_shot = h;
                    }
                }
                "hotkey_snip" => {
                    if let Some(h) = parse_hotkey(unquote(v)) {
                        c.hk_snip = h;
                    }
                }
                "frame_x" => fx = v.parse().ok(),
                "frame_y" => fy = v.parse().ok(),
                "frame_w" => fw = v.parse().ok(),
                "frame_h" => fh = v.parse().ok(),
                "cam_circle" => {
                    if let Some(b) = parse_bool(v) {
                        c.cam_circle = b;
                    }
                }
                "cam_x" => cx = v.parse().ok(),
                "cam_y" => cy = v.parse().ok(),
                "cam_w" => cw = v.parse().ok(),
                "cam_h" => ch = v.parse().ok(),
                _ => {}
            }
        }
        if let (Some(x), Some(y), Some(w), Some(h)) = (fx, fy, fw, fh) {
            if w > 0 && h > 0 {
                c.frame = Some((x, y, w, h));
            }
        }
        if let (Some(x), Some(y), Some(w), Some(h)) = (cx, cy, cw, ch) {
            if w > 0 && h > 0 {
                c.cam_frame = Some((x, y, w, h));
            }
        }
        c.fps = c.fps.clamp(1, 60);
        // Migration: configs written before the screenshot/export backdrop
        // split carry the screenshot choice in `background`. Inherit it so an
        // upgrade never loses the user's picked style.
        if !saw_shot_bg && c.bg != BgStyle::None {
            c.shot_bg = c.bg;
        }
        c
    }

    pub fn load(path: &Path) -> Config {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::parse(&text),
            Err(_) => Config::default(),
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::with_capacity(1536);
        out.push_str(&format!("output_dir = \"{}\"\nopen_shot_folder = {}\nopen_record_folder = {}\nbar_scale = {}\nrecording_dot = {}\n", self.output_dir, self.open_shot_folder, self.open_record_folder, self.bar_scale, self.recording_dot));
        out.push_str("# Cadre settings. Edited by hand or rewritten by the app on exit.\n");
        out.push_str("#\n");
        out.push_str("# Hotkeys combine Ctrl / Shift / Alt / Win with '+', ending in a key:\n");
        out.push_str("#   F1..F24, letters, digits, or PrintScreen. Example: \"Ctrl+Shift+R\".\n\n");
        out.push_str(&format!("fps = {}\n", self.fps));
        out.push_str(&format!(
            "bitrate_kbps = {}  # 0 = automatic, from region size and fps\n",
            self.bitrate_kbps
        ));
        out.push_str(&format!(
            "audio_source = \"{}\"  # off | system | mic | both\n",
            self.audio.code()
        ));
        out.push_str(&format!("cursor = {}  # draw the mouse cursor into captures\n", self.cursor));
        out.push_str(&format!(
            "highlight = {}  # glow the cursor and ripple on click, in recordings\n",
            self.highlight
        ));
        out.push_str(&format!(
            "screenshot_background = \"{}\"  # screenshots & snips: none | graphite | ocean | sunset | snow | forest | rose | lavender | sky\n",
            self.shot_bg.code()
        ));
        out.push_str(&format!(
            "background = \"{}\"  # video export backdrop: none | graphite | ocean | sunset | snow | forest | rose | lavender | sky\n",
            self.bg.code()
        ));
        out.push_str(&format!(
            "watermark = \"{}\"  # snip watermark text; empty = built-in wording\n",
            self.watermark
        ));
        out.push_str(&format!(
            "caption_edge = \"{}\"  # recording caption bar: none | top | bottom | left | right\n",
            self.caption_edge.code()
        ));
        out.push_str(&format!(
            "caption_text = \"{}\"  # caption bar label; \\n starts another line\n",
            self.caption_text.replace('\n', "\\n")
        ));
        out.push_str(&format!(
            "caption_size = {}  # caption bar thickness in pixels; 0 = automatic\n",
            self.caption_size
        ));
        out.push_str(&format!(
            "autozoom = {}  # MP4 export: zoom toward recorded clicks\n",
            self.autozoom
        ));
        out.push_str(&format!(
            "language = \"{}\"  # auto | en | zh\n",
            self.language.code()
        ));
        out.push('\n');
        out.push_str(&format!("hotkey_record = \"{}\"  # start / pause / resume\n", self.hk_record));
        out.push_str(&format!("hotkey_stop = \"{}\"  # stop and save\n", self.hk_stop));
        out.push_str(&format!("hotkey_shot = \"{}\"  # screenshot of the region\n", self.hk_shot));
        out.push_str(&format!("hotkey_snip = \"{}\"  # freeform snip to the clipboard\n", self.hk_snip));
        if let Some((x, y, w, h)) = self.frame {
            out.push('\n');
            out.push_str("# Last position of the capture frame.\n");
            out.push_str(&format!("frame_x = {x}\nframe_y = {y}\nframe_w = {w}\nframe_h = {h}\n"));
        }
        out.push('\n');
        out.push_str(&format!(
            "cam_circle = {}  # webcam overlay: circle, else rounded square\n",
            self.cam_circle
        ));
        if let Some((x, y, w, h)) = self.cam_frame {
            out.push_str(&format!("cam_x = {x}\ncam_y = {y}\ncam_w = {w}\ncam_h = {h}\n"));
        }
        out
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let temp=path.with_extension(format!("{}.tmp",std::process::id()));
        let result=(|| {std::fs::write(&temp,self.render())?;std::fs::rename(&temp,path)})();
        if result.is_err(){let _=std::fs::remove_file(&temp);}
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_settings_roundtrip_unicode_path() {
        let mut cfg=Config::default();
        cfg.output_dir=r"D:\Captures #1\تصوير".into();
        cfg.bar_scale=125;cfg.recording_dot=false;
        cfg.open_shot_folder=false;cfg.open_record_folder=false;
        cfg.hk_record=parse_hotkey("Ctrl+Shift+R").unwrap();
        cfg.bitrate_kbps=12000;
        assert_eq!(Config::parse(&cfg.render()),cfg);
    }
    #[test]
    fn legacy_settings_gain_new_defaults_and_scale_is_bounded() {
        let cfg=Config::parse("fps = 24\nbar_scale = 999\n");
        assert_eq!(cfg.fps,24);assert_eq!(cfg.bar_scale,150);
        assert!(cfg.output_dir.is_empty());assert!(cfg.recording_dot);
        assert!(cfg.open_shot_folder && cfg.open_record_folder);
    }

    #[test]
    fn defaults_roundtrip() {
        let c = Config::default();
        let text = c.render();
        assert_eq!(Config::parse(&text), c);
    }

    #[test]
    fn full_roundtrip() {
        let mut c = Config::default();
        c.frame = Some((-120, 40, 1280, 720));
        c.cam_circle = false;
        c.cam_frame = Some((10, 20, 240, 240));
        c.highlight = true;
        c.bg = BgStyle::Ocean;
        c.shot_bg = BgStyle::Sunset;
        c.watermark = "Team Cadre".to_string();
        c.caption_edge = CaptionEdge::Bottom;
        c.caption_text = "line one\nline two".to_string();
        c.caption_size = 72;
        c.autozoom = true;
        c.audio = AudioSource::Both;
        c.language = LangPref::Zh;
        c.bitrate_kbps = 12_000;
        c.hk_snip = Hotkey { ctrl: true, shift: true, alt: false, win: false, key: Key::Char(b'S') };
        assert_eq!(Config::parse(&c.render()), c);
    }

    #[test]
    fn legacy_background_migrates_to_shot_bg() {
        // Pre-split configs carry the screenshot choice in `background`; an
        // upgrade must not lose the user's picked style.
        let c = Config::parse("background = \"sunset\"\n");
        assert_eq!(c.shot_bg, BgStyle::Sunset); // inherited
        assert_eq!(c.bg, BgStyle::Sunset); // still the export value
        // An explicit screenshot key always wins over inheritance.
        let c = Config::parse("background = \"sunset\"\nscreenshot_background = \"none\"\n");
        assert_eq!(c.shot_bg, BgStyle::None);
        assert_eq!(c.bg, BgStyle::Sunset);
    }

    #[test]
    fn audio_source_codes() {
        for s in [AudioSource::Off, AudioSource::System, AudioSource::Mic, AudioSource::Both] {
            assert_eq!(AudioSource::from_code(s.code()), Some(s));
        }
        let mut s = AudioSource::Off;
        for _ in 0..4 {
            s = s.next();
        }
        assert_eq!(s, AudioSource::Off); // the cycle closes
    }

    #[test]
    fn legacy_audio_key() {
        assert_eq!(Config::parse("audio = false\n").audio, AudioSource::Off);
        assert_eq!(Config::parse("audio = true\n").audio, AudioSource::System);
        // The new key wins regardless of order (legacy writers never emit both).
        let c = Config::parse("audio_source = \"mic\"\n");
        assert_eq!(c.audio, AudioSource::Mic);
    }

    #[test]
    fn language_codes() {
        assert_eq!(Config::parse("language = \"zh\"\n").language, LangPref::Zh);
        assert_eq!(Config::parse("language = \"en\"\n").language, LangPref::En);
        assert_eq!(Config::parse("language = \"nope\"\n").language, LangPref::En);
    }

    #[test]
    fn hotkey_parsing() {
        assert_eq!(parse_hotkey("F9"), Some(Hotkey::plain(Key::F(9))));
        assert_eq!(
            parse_hotkey("ctrl+shift+r"),
            Some(Hotkey { ctrl: true, shift: true, alt: false, win: false, key: Key::Char(b'R') })
        );
        assert_eq!(parse_hotkey("Alt+PrintScreen").map(|h| h.key), Some(Key::PrintScreen));
        assert_eq!(parse_hotkey("F25"), None);
        assert_eq!(parse_hotkey("Ctrl+"), None);
        assert_eq!(parse_hotkey("Ctrl+A+B"), None);
        assert_eq!(parse_hotkey("F9").unwrap().to_string(), "F9");
        assert_eq!(parse_hotkey("ctrl+alt+5").unwrap().to_string(), "Ctrl+Alt+5");
    }

    #[test]
    fn tolerant_parse() {
        let c = Config::parse("garbage\nfps = 999\naudio = maybe\n# audio = false\nbitrate_kbps = 8000 # inline\n");
        assert_eq!(c.fps, 60); // clamped
        assert_eq!(c.audio, AudioSource::System); // unchanged default
        assert_eq!(c.bitrate_kbps, 8000);
    }

    #[test]
    fn bitrate_bounds() {
        assert_eq!(auto_bitrate_kbps(1920, 1080, 30), 6_220);
        assert!(auto_bitrate_kbps(320, 200, 10) >= 1_500);
        assert!(auto_bitrate_kbps(3840, 2160, 60) <= 40_000);
    }
}
