//! Application UI typography: the resolved system UI font.
//!
//! [`crate::sys::ui_font`] reads the desktop's setting; this module turns it
//! into iced fonts and keeps the result in one process-wide store so plain
//! widgets (through iced's default font), explicit bold/italic text and
//! custom-drawn UI text (canvas labels, key tips, measured ribbon labels) all
//! use the same family.
//!
//! Only application chrome reads from here. Drawing text — text styles,
//! dimensions, document fonts, style previews and exports — keeps its own
//! fonts.

use crate::sys::ui_font::SystemUiFont;
use iced::font::{Family, Font, Style, Weight};
use iced::Pixels;
use std::sync::{Mutex, RwLock};

/// iced's default text size, used by every widget without an explicit size.
pub const DEFAULT_TEXT_PX: f32 = 16.0;

/// The resolved UI typography.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typography {
    /// Regular UI font; bold and italic variants derive from it.
    pub font: Font,
}

impl Typography {
    /// iced's built-in default: generic sans-serif.
    pub const FALLBACK: Typography = Typography {
        font: Font::DEFAULT,
    };

    /// Typography for a detected system font. Families are taken as given;
    /// [`Typography::resolve_family`] later swaps in an installed alternate.
    pub fn from_system(system: Option<&SystemUiFont>) -> Self {
        let Some(system) = system else {
            return Self::FALLBACK;
        };
        let family = system
            .family
            .as_deref()
            .or(system.alternates.first().map(String::as_str))
            .map(|name| Family::Name(intern(name)))
            .unwrap_or(Family::SansSerif);
        Self {
            font: Font {
                family,
                weight: weight_from_css(system.weight),
                style: if system.italic {
                    Style::Italic
                } else {
                    Style::Normal
                },
                ..Font::DEFAULT
            },
        }
    }

    /// Pick the first of the system font's candidate families that is
    /// installed (`available` is the renderer's family list). When none is,
    /// fall back to the generic sans-serif family so text never renders in an
    /// arbitrary per-glyph fallback face.
    pub fn resolve_family(self, system: &SystemUiFont, available: &[String]) -> Self {
        // Use the renderer's spelling: its font database matches names exactly.
        let installed = |name: &String| available.iter().find(|a| a.eq_ignore_ascii_case(name));
        let family = system
            .family
            .iter()
            .chain(system.alternates.iter())
            .find_map(installed)
            .map(|name| Family::Name(intern(name)))
            .unwrap_or(Family::SansSerif);
        Self {
            font: Font {
                family,
                ..self.font
            },
        }
    }

    /// Bold variant of the UI font.
    pub fn bold(&self) -> Font {
        self.font.weight(heavier(self.font.weight, Weight::Bold))
    }

    /// Semibold variant of the UI font.
    pub fn semibold(&self) -> Font {
        self.font
            .weight(heavier(self.font.weight, Weight::Semibold))
    }

    /// Italic variant of the UI font.
    pub fn italic(&self) -> Font {
        Font {
            style: Style::Italic,
            ..self.font
        }
    }

    /// Bold italic variant of the UI font.
    pub fn bold_italic(&self) -> Font {
        Font {
            style: Style::Italic,
            ..self.bold()
        }
    }

    /// Write the font into iced's startup settings.
    pub fn apply_to_settings(&self, settings: &mut iced::Settings) {
        settings.default_font = self.font;
    }

    /// Replace iced's default font in the running application and relayout
    /// every window.
    pub fn set_iced_defaults<Message>(&self) -> iced::Task<Message> {
        iced::font::set_defaults(self.font, Pixels(DEFAULT_TEXT_PX))
    }
}

impl Default for Typography {
    fn default() -> Self {
        Self::FALLBACK
    }
}

static CURRENT: RwLock<Typography> = RwLock::new(Typography::FALLBACK);

/// The typography in effect.
pub fn current() -> Typography {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

/// Make `typography` the one in effect. Returns whether it changed; when it
/// did, the caller pushes it to iced with [`Typography::set_iced_defaults`].
pub fn install(typography: Typography) -> bool {
    let mut current = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    let changed = *current != typography;
    *current = typography;
    changed
}

/// The detected system font behind [`CURRENT`], kept so its candidate
/// families can be re-checked against the installed fonts.
static SYSTEM: Mutex<Option<SystemUiFont>> = Mutex::new(None);

/// Read the desktop UI font and make it current. Called once before iced
/// starts; a failed read leaves iced's default font in place.
pub fn init_from_system() -> Typography {
    let system = crate::sys::ui_font::detect();
    match &system {
        Some(font) => log::info!(
            "UI font: {:?} {} px weight {} from {:?}",
            font.family,
            font.size_px
                .map_or("default".into(), |px| format!("{px:.2}")),
            font.weight,
            font.source,
        ),
        None => log::info!("UI font: desktop setting unavailable, using the default font"),
    }
    let typography = Typography::from_system(system.as_ref());
    *SYSTEM.lock().unwrap_or_else(|e| e.into_inner()) = system;
    install(typography);
    typography
}

/// Record a re-read system font. Returns whether it differs from the one in
/// effect, in which case the caller re-resolves it against the installed
/// families. A failed re-read (`None`) keeps the current font: a settings
/// daemon that is briefly unreachable must not flip the UI to the fallback.
pub fn update_system(system: Option<SystemUiFont>) -> bool {
    let Some(system) = system else {
        return false;
    };
    let mut stored = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
    if stored.as_ref() == Some(&system) {
        return false;
    }
    log::info!(
        "UI font changed: {:?} {:?} px from {:?}",
        system.family,
        system.size_px,
        system.source
    );
    *stored = Some(system);
    true
}

/// Resolve the system font against the renderer's installed `families` and
/// make the result current. Returns the typography when it changed, so the
/// caller can push it to iced.
pub fn resolve_installed(families: &[String]) -> Option<Typography> {
    let system = SYSTEM.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut typography = Typography::from_system(system.as_ref());
    if let Some(system) = &system {
        typography = typography.resolve_family(system, families);
        if typography.font.family == Family::SansSerif {
            log::warn!(
                "UI font {:?} is not installed; using the default sans-serif font",
                system.family
            );
        }
    }
    install(typography).then_some(typography)
}

/// Make `family` the UI font. The web build uses this once the script font
/// for the UI language has been fetched.
pub fn set_family(family: Family) -> Typography {
    let typography = Typography {
        font: Font::with_family(family),
    };
    install(typography);
    typography
}

/// Re-read the desktop UI font off the UI thread: settings reads may run a
/// helper process or wait on D-Bus.
#[cfg(not(target_arch = "wasm32"))]
pub async fn redetect() -> Option<SystemUiFont> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("ocs-ui-font".into())
        .spawn(move || {
            let _ = tx.send(crate::sys::ui_font::detect());
        })
        .ok()?;
    rx.await.ok().flatten()
}

/// Family names reported by `iced::font::list`.
pub fn family_names(families: Vec<Family>) -> Vec<String> {
    families
        .into_iter()
        .filter_map(|family| match family {
            Family::Name(name) => Some(name.to_string()),
            _ => None,
        })
        .collect()
}

/// `iced::font::Family::Name` needs a `&'static str`. Family names are
/// leaked once each and reused, so repeated font changes do not grow memory.
fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&known) = names.iter().find(|&&n| n == name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}

fn weight_from_css(css: u16) -> Weight {
    match css {
        0..=149 => Weight::Thin,
        150..=249 => Weight::ExtraLight,
        250..=349 => Weight::Light,
        350..=449 => Weight::Normal,
        450..=549 => Weight::Medium,
        550..=649 => Weight::Semibold,
        650..=749 => Weight::Bold,
        750..=849 => Weight::ExtraBold,
        _ => Weight::Black,
    }
}

fn css_from_weight(weight: Weight) -> u16 {
    match weight {
        Weight::Thin => 100,
        Weight::ExtraLight => 200,
        Weight::Light => 300,
        Weight::Normal => 400,
        Weight::Medium => 500,
        Weight::Semibold => 600,
        Weight::Bold => 700,
        Weight::ExtraBold => 800,
        Weight::Black => 900,
    }
}

/// `target`, or a step heavier than `base` when the UI font is already at
/// least that heavy, so emphasis stays visible on a bold desktop font.
fn heavier(base: Weight, target: Weight) -> Weight {
    let base = css_from_weight(base);
    let target = css_from_weight(target);
    if base < target {
        weight_from_css(target)
    } else {
        weight_from_css((base + 200).min(900))
    }
}

// ── Fonts ────────────────────────────────────────────────────────────────

/// The regular UI font.
pub fn font() -> Font {
    current().font
}

/// Bold UI text.
pub fn bold() -> Font {
    current().bold()
}

/// Semibold UI text.
pub fn semibold() -> Font {
    current().semibold()
}

/// Italic UI text.
pub fn italic() -> Font {
    current().italic()
}

/// Bold italic UI text.
pub fn bold_italic() -> Font {
    current().bold_italic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::ui_font::Source;

    fn system(family: &str) -> SystemUiFont {
        SystemUiFont {
            family: Some(family.into()),
            alternates: Vec::new(),
            weight: 400,
            italic: false,
            size_px: Some(12.0),
            source: Source::GnomeSettings,
        }
    }

    #[test]
    fn fallback_is_iced_default() {
        let t = Typography::from_system(None);
        assert_eq!(t, Typography::FALLBACK);
        assert_eq!(t.font, Font::DEFAULT);
    }

    #[test]
    fn detected_family_becomes_the_ui_font() {
        let t = Typography::from_system(Some(&system("Segoe UI")));
        assert_eq!(t.font.family, Family::Name("Segoe UI"));
        assert_eq!(t.font.weight, Weight::Normal);
        assert_eq!(t.font.style, Style::Normal);
    }

    #[test]
    fn variants_derive_from_the_ui_family() {
        let t = Typography::from_system(Some(&system("Inter")));
        assert_eq!(t.bold().family, Family::Name("Inter"));
        assert_eq!(t.bold().weight, Weight::Bold);
        assert_eq!(t.italic().style, Style::Italic);
        assert_eq!(t.italic().weight, Weight::Normal);
        let bi = t.bold_italic();
        assert_eq!(
            (bi.family, bi.weight, bi.style),
            (Family::Name("Inter"), Weight::Bold, Style::Italic)
        );
        assert_eq!(t.semibold().weight, Weight::Semibold);
    }

    #[test]
    fn emphasis_stays_visible_on_heavy_ui_fonts() {
        let mut s = system("Inter");
        s.weight = 700;
        let t = Typography::from_system(Some(&s));
        assert_eq!(t.font.weight, Weight::Bold);
        assert_eq!(t.bold().weight, Weight::Black);
    }

    #[test]
    fn resolve_family_prefers_installed_candidates() {
        let mut s = system(".AppleSystemUIFont");
        s.alternates = vec![".SF NS".into(), "Helvetica Neue".into()];
        let base = Typography::from_system(Some(&s));
        let installed = vec!["Helvetica Neue".to_string(), ".SF NS".to_string()];
        assert_eq!(
            base.resolve_family(&s, &installed).font.family,
            Family::Name(".SF NS")
        );
        let none: Vec<String> = vec!["Other".into()];
        assert_eq!(
            base.resolve_family(&s, &none).font.family,
            Family::SansSerif
        );
        let case = vec!["Segoe UI".to_string()];
        let w = Typography::from_system(Some(&system("segoe ui")));
        assert_eq!(
            w.resolve_family(&system("segoe ui"), &case)
                .font
                .family,
            Family::Name("Segoe UI")
        );
    }

    #[test]
    fn interned_names_are_reused() {
        assert!(std::ptr::eq(intern("Same Family"), intern("Same Family")));
    }

    #[test]
    fn settings_receive_the_font_only() {
        let t = Typography::from_system(Some(&system("Inter")));
        let mut settings = iced::Settings::default();
        t.apply_to_settings(&mut settings);
        assert_eq!(settings.default_font, t.font);
        assert_eq!(settings.default_text_size, Pixels(DEFAULT_TEXT_PX));
    }
}
