//! Colours and the stylesheet. One generated stylesheet, rebuilt when the
//! desktop's colour scheme or accent changes.

use gtk::gdk;
use waystow_core::config::{Theme, UiConfig, parse_hex};

use crate::system::{Appearance, Scheme};

/// Default accent when neither the config nor the desktop gives one.
const DEFAULT_ACCENT: (u8, u8, u8) = (0x7c, 0x6c, 0xf2);

/// Resolved palette inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub accent: (u8, u8, u8),
    pub opacity: f64,
    pub shadow: bool,
    /// GTK ≥ 4.24 blurs behind `backdrop-filter` via ext-background-effect.
    pub blur: bool,
}

impl Look {
    pub fn resolve(ui: &UiConfig, desktop: Appearance) -> Self {
        let dark = match ui.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => desktop.scheme != Scheme::Light,
        };
        let accent = parse_hex(&ui.accent)
            .or_else(|| desktop.accent.map(|(r, g, b)| ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)))
            .unwrap_or(DEFAULT_ACCENT);
        let blur = gtk::major_version() > 4 || (gtk::major_version() == 4 && gtk::minor_version() >= 24);
        Self { dark, accent, opacity: ui.opacity, shadow: ui.client_shadow, blur }
    }
}

fn rgba((r, g, b): (u8, u8, u8), a: f64) -> String {
    format!("rgba({r},{g},{b},{a:.3})")
}

/// The full stylesheet for a look.
pub fn css(look: &Look, font: &str) -> String {
    let (bg, ink): ((u8, u8, u8), (u8, u8, u8)) =
        if look.dark { ((30, 30, 34), (237, 237, 242)) } else { ((250, 250, 252), (28, 28, 34)) };
    let a = look.accent;
    let ink_a = |x: f64| rgba(ink, x);
    let shadow = if look.shadow {
        if look.dark {
            "0 10px 28px rgba(0,0,0,0.42), 0 2px 8px rgba(0,0,0,0.30)"
        } else {
            "0 10px 28px rgba(20,20,40,0.20), 0 2px 8px rgba(20,20,40,0.12)"
        }
    } else {
        "none"
    };
    let highlight = if look.dark { "rgba(255,255,255,0.07)" } else { "rgba(255,255,255,0.9)" };
    let panel_shadow =
        if look.shadow { format!("inset 0 1px {highlight}, {shadow}") } else { format!("inset 0 1px {highlight}") };
    let font = if font.is_empty() { String::new() } else { format!("font-family: \"{font}\";") };
    let accent_ink = if luminance(a) > 0.6 { "rgba(20,20,24,1)" } else { "white" };
    format!(
        r#"
window.waystow {{ background: none; {font} }}
.ws-root {{ background-color: rgba(0,0,0,0.004); }}
window.waystow button, window.waystow menubutton > button {{ background-image: none; text-shadow: none; -gtk-icon-shadow: none; }}
window.waystow *:drop(active) {{ box-shadow: none; outline: none; }}
menubutton.ws-icon > button {{ background: none; border: none; box-shadow: none; border-radius: 9px; padding: 4px; min-width: 26px; min-height: 26px; color: {i58}; }}
menubutton.ws-icon > button:hover {{ background-color: {i08}; color: {ink}; }}
menubutton.ws-action > button {{ background-color: {i07}; border: none; box-shadow: none; border-radius: 10px; padding: 4px 10px; min-height: 28px; font-size: 12.5px; font-weight: 600; color: {ink}; }}
menubutton.ws-action > button:hover {{ background-color: {i12}; }}
window.waystow * {{ outline-color: {acc50}; }}
.ws-panel {{
  background-color: {bg};
  border-radius: 24px;
  border: 1px solid {hair};
  box-shadow: {panel_shadow};
  color: {ink};
  padding: 10px;
  {blur}
}}
.ws-island {{
  background-color: {bg};
  border-radius: 22px;
  border: 1px solid {hair};
  box-shadow: {panel_shadow};
  color: {ink};
  padding: 7px 14px 7px 9px;
}}
.ws-handle {{ background-color: {i35}; border-radius: 3px; min-width: 4px; min-height: 52px; transition: all 180ms ease-out; }}
.ws-handle:hover {{ background-color: {acc}; min-height: 72px; }}

.ws-dim {{ color: {i58}; }}
.ws-faint {{ color: {i36}; }}
.ws-title {{ font-weight: 600; font-size: 13px; }}
.ws-sub {{ font-size: 11.5px; color: {i58}; font-feature-settings: "tnum"; }}
.ws-heading {{ font-weight: 700; font-size: 15px; }}
.ws-caption {{ font-size: 11px; color: {i50}; font-weight: 600; letter-spacing: 0.4px; }}

.ws-tabs {{ background-color: {i06}; border-radius: 11px; padding: 3px; }}
.ws-tabs button {{ background: none; border: none; box-shadow: none; border-radius: 8px; padding: 3px 12px; min-height: 22px; font-size: 12.5px; font-weight: 600; color: {i58}; transition: all 160ms ease-out; }}
.ws-tabs button:hover {{ color: {ink}; }}
.ws-tabs button.active {{ background-color: {i12}; color: {ink}; box-shadow: 0 1px 2px rgba(0,0,0,0.15); }}

button.ws-icon {{ background: none; border: none; box-shadow: none; border-radius: 9px; padding: 4px; min-width: 26px; min-height: 26px; color: {i58}; transition: all 140ms ease-out; }}
button.ws-icon:hover {{ background-color: {i08}; color: {ink}; }}
button.ws-icon.on {{ color: {acc}; }}

flowboxchild {{ padding: 0; min-height: 0; min-width: 0; background: none; }}
.ws-chip {{ background-color: {i06}; border: none; box-shadow: none; border-radius: 999px; padding: 2px 11px; min-height: 24px; font-size: 12px; font-weight: 600; color: {i70}; transition: all 160ms ease-out; }}
.ws-chip:hover {{ background-color: {i10}; color: {ink}; }}
.ws-chip.active {{ background-color: {acc18}; color: {acc}; }}
.ws-chip.drop-hover {{ background-color: {acc}; color: {accent_ink}; }}
entry.ws-entry {{ background-color: {i06}; border: 1px solid {hair}; border-radius: 10px; min-height: 28px; color: {ink}; box-shadow: none; }}

.ws-stack {{ background-color: {i04}; border-radius: 18px; border: 1px solid {hair}; padding: 12px; transition: all 200ms ease-out; }}
.ws-stack:hover {{ background-color: {i07}; }}
.ws-empty {{ border: 1.5px dashed {i20}; border-radius: 18px; padding: 26px 12px; }}
.ws-empty .ws-glyph {{ color: {i36}; }}
.ws-thumb .ws-glyph {{ color: {i50}; }}
.ws-thumb {{ border-radius: 9px; background-color: {i06}; }}
.ws-thumb-frame {{ border-radius: 10px; border: 2px solid {bg}; box-shadow: 0 4px 14px rgba(0,0,0,0.28); background-color: {bg}; }}
.ws-badge {{ background-color: {acc}; color: {accent_ink}; border-radius: 999px; padding: 0 7px; min-height: 18px; font-size: 11px; font-weight: 700; }}

.ws-row {{ border-radius: 12px; padding: 5px 6px; transition: background-color 140ms ease-out; }}
.ws-row:hover {{ background-color: {i06}; }}
.ws-row.selected {{ background-color: {acc16}; box-shadow: inset 0 0 0 1px {acc40}; }}
.ws-row.missing {{ opacity: 0.45; }}
.ws-row.new {{ animation: ws-pop 380ms cubic-bezier(0.2, 1.25, 0.35, 1); }}
.ws-row .ws-row-tools {{ opacity: 0; transition: opacity 120ms ease-out; }}
.ws-row:hover .ws-row-tools, .ws-row .ws-row-tools.pinned {{ opacity: 1; }}
@keyframes ws-pop {{
  from {{ opacity: 0; transform: translateY(-10px) scale(0.96); }}
  to {{ opacity: 1; transform: none; }}
}}

.ws-actions {{ padding-top: 6px; }}
button.ws-action {{ background-color: {i07}; border: none; box-shadow: none; border-radius: 10px; padding: 4px 10px; min-height: 28px; font-size: 12.5px; font-weight: 600; color: {ink}; transition: all 140ms ease-out; }}
button.ws-action:hover {{ background-color: {i12}; }}
button.ws-action.primary {{ background-color: {acc}; color: {accent_ink}; }}
button.ws-action.primary:hover {{ background-color: {acc85}; }}
button.ws-action:disabled {{ opacity: 0.4; }}
button.ws-action.flat {{ background-color: transparent; }}
button.ws-action.flat:hover {{ background-color: {i08}; }}

.ws-zone {{ border-radius: 18px; border: 2px dashed {i22}; padding: 14px; transition: all 160ms cubic-bezier(0.2, 1.2, 0.4, 1); }}
.ws-zone.drop-hover {{ border: 2px solid {acc}; background-color: {acc14}; transform: scale(1.02); }}
.ws-zone .ws-glyph {{ color: {i58}; }}
.ws-zone.drop-hover .ws-glyph {{ color: {acc}; }}
.ws-tile {{ border-radius: 16px; background-color: {i05}; padding: 12px 6px; transition: all 160ms cubic-bezier(0.2, 1.2, 0.4, 1); }}
.ws-tile.drop-hover {{ background-color: {acc}; color: {accent_ink}; transform: scale(1.06); }}
.ws-tile.drop-hover .ws-sub {{ color: {accent_ink}; }}
.ws-reveal {{ animation: ws-rise 320ms cubic-bezier(0.2, 1.2, 0.35, 1); }}
@keyframes ws-rise {{
  from {{ opacity: 0; transform: translateY(8px); }}
  to {{ opacity: 1; transform: none; }}
}}

.ws-job {{ border-radius: 12px; padding: 6px 8px; background-color: {i04}; }}
.ws-job.failed .ws-sub {{ color: {err}; }}
progressbar.ws-progress trough {{ min-height: 4px; border-radius: 4px; background-color: {i10}; }}
progressbar.ws-progress progress {{ min-height: 4px; border-radius: 4px; background-color: {acc}; }}

.ws-toast {{ background-color: {ink}; color: {bg}; border-radius: 12px; padding: 7px 12px; font-size: 12.5px; font-weight: 600; box-shadow: 0 6px 20px rgba(0,0,0,0.25); }}
.ws-ring {{ color: {acc}; }}
.ws-shot {{ border-radius: 12px; }}
popover.ws-pop > contents {{ background-color: {bg}; color: {ink}; border-radius: 16px; border: 1px solid {hair}; padding: 10px; }}
scrolledwindow.ws-scroll undershoot, scrolledwindow.ws-scroll overshoot {{ background: none; }}
scrollbar {{ background: none; border: none; }}
scrollbar slider {{ background-color: {i22}; min-width: 4px; border-radius: 4px; }}
.ws-quicklook {{ background-color: {bg_solid}; border-radius: 26px; border: 1px solid {hair}; box-shadow: {panel_shadow}; color: {ink}; padding: 14px; }}
textview.ws-text, textview.ws-text text {{ background: none; color: {ink}; font-family: monospace; font-size: 12px; }}
"#,
        bg = rgba(bg, look.opacity),
        bg_solid = rgba(bg, 1.0),
        blur = if look.blur { "backdrop-filter: blur(24px);" } else { "" },
        hair = ink_a(if look.dark { 0.09 } else { 0.10 }),
        i04 = ink_a(0.04),
        i05 = ink_a(0.05),
        i06 = ink_a(0.06),
        i07 = ink_a(0.07),
        i08 = ink_a(0.08),
        i10 = ink_a(0.10),
        i12 = ink_a(0.12),
        i20 = ink_a(0.20),
        i22 = ink_a(0.22),
        i35 = ink_a(0.35),
        i36 = ink_a(0.36),
        i50 = ink_a(0.50),
        i58 = ink_a(0.58),
        i70 = ink_a(0.70),
        ink = rgba(ink, 1.0),
        acc = rgba(a, 1.0),
        acc85 = rgba(a, 0.85),
        acc50 = rgba(a, 0.5),
        acc40 = rgba(a, 0.4),
        acc18 = rgba(a, 0.18),
        acc16 = rgba(a, 0.16),
        acc14 = rgba(a, 0.14),
        err = if look.dark { "rgba(255,138,138,1)" } else { "rgba(200,40,50,1)" },
    )
}

fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    (0.0722f64).mul_add(f64::from(b), 0.2126f64.mul_add(f64::from(r), 0.7152 * f64::from(g))) / 255.0
}

/// The installed stylesheet.
pub struct Styles {
    provider: gtk::CssProvider,
}

impl Styles {
    pub fn install() -> Self {
        let provider = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 10,
            );
        }
        Self { provider }
    }

    pub fn apply(&self, look: &Look, font: &str) {
        self.provider.load_from_string(&css(look, font));
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(look.dark);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_parses() {
        gtk::init().ok();
        for dark in [true, false] {
            let look = Look { dark, accent: (10, 200, 120), opacity: 0.9, shadow: dark, blur: false };
            let text = css(&look, "Inter");
            assert!(text.contains("rgba(10,200,120,1.000)"));
            if gtk::is_initialized() {
                let p = gtk::CssProvider::new();
                let errors = std::rc::Rc::new(std::cell::Cell::new(0));
                let e = errors.clone();
                p.connect_parsing_error(move |_, _, err| {
                    eprintln!("css: {err}");
                    e.set(e.get() + 1);
                });
                p.load_from_string(&text);
                assert_eq!(errors.get(), 0);
            }
        }
    }

    #[test]
    fn config_accent_wins_then_desktop_then_default() {
        let ui = UiConfig { accent: "#112233".into(), ..UiConfig::default() };
        let desk = Appearance { scheme: Scheme::Light, accent: Some((1.0, 0.0, 0.0)) };
        assert_eq!(Look::resolve(&ui, desk).accent, (0x11, 0x22, 0x33));
        let ui = UiConfig::default();
        assert_eq!(Look::resolve(&ui, desk).accent, (255, 0, 0));
        assert!(!Look::resolve(&ui, desk).dark);
        let none = Appearance { scheme: Scheme::Default, accent: None };
        assert_eq!(Look::resolve(&ui, none).accent, DEFAULT_ACCENT);
        assert!(Look::resolve(&ui, none).dark, "dark unless the desktop prefers light");
    }
}
