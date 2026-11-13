//! Themes. A theme is a small JSON file in the suite's shared format (the same one Hangar and
//! the other apps read: see Hangar's docs/THEMES.md), plus an optional `marquee` block for the
//! things only Marquee has (corner radii, blur, shadows, fonts, motion intensity).
//!
//! Because the base format is shared, a Hangar theme file loads in Marquee unchanged, and a
//! `.marqueetheme` file loads in Hangar (which ignores the extra block).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "marqueetheme";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Colors {
    pub background: String,
    pub sidebar: String,
    pub surface: String,
    pub surface_hover: String,
    pub surface_strong: String,
    pub text: String,
    pub subtext: String,
    pub faint: String,
    /// A hex color, or "windows" to follow the system accent.
    pub accent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Extras {
    /// Corner radius of cards and panels, px.
    pub radius: f64,
    /// Corner radius of buttons and inputs, px.
    pub control_radius: f64,
    /// Backdrop blur behind glass surfaces, px.
    pub blur: f64,
    /// Shadow strength, 0..1.
    pub shadow: f64,
    pub font: String,
    pub display_font: String,
    /// Motion intensity, 0..1.5 (distance and springiness of animations).
    pub motion: f64,
    /// Glow strength on hovered cards, 0..1.
    pub glow: f64,
}

impl Default for Extras {
    fn default() -> Self {
        Extras { radius: 14.0, control_radius: 10.0, blur: 24.0, shadow: 0.6, font: "Inter Variable".into(), display_font: "Inter Variable".into(), motion: 1.0, glow: 0.5 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    #[serde(default = "one")]
    pub schema: u32,
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// "dark" | "light"
    #[serde(default = "dark")]
    pub mode: String,
    /// "mica" | "acrylic" | "none"
    #[serde(default = "none")]
    pub backdrop: String,
    #[serde(default)]
    pub transparency: f64,
    #[serde(default = "one_f")]
    pub surface_opacity: f64,
    pub colors: Colors,
    #[serde(default)]
    pub marquee: Extras,
    #[serde(default)]
    pub builtin: bool,
}

fn one() -> u32 {
    1
}
fn one_f() -> f64 {
    1.0
}
fn dark() -> String {
    "dark".into()
}
fn none() -> String {
    "none".into()
}

#[allow(clippy::too_many_arguments)]
fn def(id: &str, name: &str, description: &str, mode: &str, backdrop: &str, transparency: f64, c: [&str; 9], extras: Extras) -> Theme {
    Theme {
        schema: 1,
        id: id.into(),
        name: name.into(),
        description: description.into(),
        mode: mode.into(),
        backdrop: backdrop.into(),
        transparency,
        surface_opacity: 1.0,
        colors: Colors { background: c[0].into(), sidebar: c[1].into(), surface: c[2].into(), surface_hover: c[3].into(), surface_strong: c[4].into(), text: c[5].into(), subtext: c[6].into(), faint: c[7].into(), accent: c[8].into() },
        marquee: extras,
        builtin: true,
    }
}

/// The built-in themes. "system" follows Windows (light or dark, and the system accent).
pub fn builtin() -> Vec<Theme> {
    let x = Extras::default;
    //                                                                   background sidebar    surface    hover      strong     text       subtext    faint      accent
    vec![
        def("marquee", "Marquee", "Deep black with warm bulb gold", "dark", "mica", 0.18, ["#0A0A0C", "#070709", "#151518", "#1D1D21", "#28282E", "#F6F2EA", "#B9B4AA", "#898680", "#F2C14E"], x()),
        def("midnight", "Midnight", "Deep navy, cool and calm", "dark", "mica", 0.15, ["#0B1020", "#080C19", "#141B31", "#1B2440", "#253052", "#EAF0FF", "#A9B6D6", "#818CB0", "#7AA2FF"], x()),
        def("cinema-red", "Cinema Red", "Velvet curtains and a red carpet", "dark", "mica", 0.12, ["#120809", "#0D0506", "#1F0E10", "#2A1316", "#381A1E", "#FBEDEC", "#D0AEAC", "#9C7C7A", "#FF4D5E"], Extras { glow: 0.7, ..x() }),
        def("slate-light", "Slate Light", "Soft gray, easy in daylight", "light", "mica", 0.15, ["#EEF1F5", "#E3E8EE", "#FAFBFD", "#F2F5F9", "#DCE2EA", "#161C24", "#4C5866", "#5D6873", "#2F6FDB"], Extras { shadow: 0.35, glow: 0.2, ..x() }),
        def("pure-light", "Pure Light", "Bright white, artwork does the talking", "light", "none", 0.0, ["#FFFFFF", "#F5F5F7", "#FFFFFF", "#F2F2F5", "#E4E4EA", "#111114", "#4E4E58", "#6C6C76", "#B8860B"], Extras { shadow: 0.3, glow: 0.15, ..x() }),
        def("oled-black", "OLED Black", "True black for OLED screens", "dark", "none", 0.0, ["#000000", "#000000", "#0C0C0E", "#141417", "#1E1E22", "#F5F5F7", "#A8A8B2", "#7F7F88", "#F2C14E"], Extras { shadow: 0.0, blur: 0.0, ..x() }),
        def("synthwave", "Synthwave", "Neon magenta over midnight purple", "dark", "acrylic", 0.25, ["#140A24", "#0F071C", "#1F1136", "#2A1848", "#38215E", "#F6EBFF", "#C4A9DE", "#9680B2", "#FF5CD6"], Extras { glow: 1.0, motion: 1.15, ..x() }),
        def("forest", "Forest", "Evergreen and quiet", "dark", "mica", 0.12, ["#0C1410", "#08100C", "#152019", "#1B2A21", "#25382C", "#E9F2EC", "#A9BCAF", "#819286", "#5FD08B"], x()),
        def("system", "Follow Windows", "Light or dark with your Windows accent color", "dark", "mica", 0.18, ["#0A0A0C", "#070709", "#151518", "#1D1D21", "#28282E", "#F6F2EA", "#B9B4AA", "#898680", "windows"], x()),
    ]
}

/// The light half of "Follow Windows".
pub fn system_light() -> Theme {
    let mut t = builtin().into_iter().find(|t| t.id == "slate-light").expect("slate-light exists");
    t.id = "system".into();
    t.name = "Follow Windows".into();
    t.colors.accent = "windows".into();
    t
}

fn parse_hex(hex: &str) -> Option<(f64, f64, f64)> {
    let h = hex.trim().trim_start_matches('#');
    let h = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect::<String>() } else { h.to_string() };
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(&h, 16).ok()?;
    Some((((n >> 16) & 255) as f64, ((n >> 8) & 255) as f64, (n & 255) as f64))
}

fn luminance(hex: &str) -> Option<f64> {
    let (r, g, b) = parse_hex(hex)?;
    let lin = |c: f64| {
        let c = c / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    Some(0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b))
}

/// WCAG contrast ratio between two colors, 1..=21.
pub fn contrast(a: &str, b: &str) -> Option<f64> {
    let (la, lb) = (luminance(a)?, luminance(b)?);
    Some((la.max(lb) + 0.05) / (la.min(lb) + 0.05))
}

/// The text color to put on top of an accent: whichever of near-black and white reads better.
pub fn on_accent(accent: &str) -> &'static str {
    match (contrast(accent, "#0A0A0C"), contrast(accent, "#FFFFFF")) {
        (Some(dark), Some(light)) if light > dark => "#FFFFFF",
        _ => "#0A0A0C",
    }
}

/// Checks a theme is usable: valid colors, sensible ranges. Returns it cleaned up.
pub fn validate(v: &Value) -> Result<Theme> {
    let mut t: Theme = serde_json::from_value(v.clone()).map_err(|e| Error::msg(format!("That theme file isn't valid: {e}")))?;
    if t.name.trim().is_empty() {
        return Err(Error::msg("The theme needs a name."));
    }
    if t.schema != 1 {
        return Err(Error::msg(format!("This theme uses format {} and this version of Marquee reads format 1.", t.schema)));
    }
    t.mode = if t.mode.eq_ignore_ascii_case("light") { "light".into() } else { "dark".into() };
    t.backdrop = match t.backdrop.to_lowercase().as_str() {
        "mica" => "mica".into(),
        "acrylic" => "acrylic".into(),
        _ => "none".into(),
    };
    t.transparency = t.transparency.clamp(0.0, 0.9);
    t.surface_opacity = t.surface_opacity.clamp(0.02, 1.0);
    for (name, c) in [
        ("background", &t.colors.background), ("sidebar", &t.colors.sidebar), ("surface", &t.colors.surface), ("surfaceHover", &t.colors.surface_hover),
        ("surfaceStrong", &t.colors.surface_strong), ("text", &t.colors.text), ("subtext", &t.colors.subtext), ("faint", &t.colors.faint),
    ] {
        if parse_hex(c).is_none() {
            return Err(Error::msg(format!("\"{c}\" isn't a color Marquee can read (for {name}). Use hex like #1A2B3C.")));
        }
    }
    if t.colors.accent != "windows" && parse_hex(&t.colors.accent).is_none() {
        return Err(Error::msg("The accent must be a hex color or \"windows\"."));
    }
    t.marquee.radius = t.marquee.radius.clamp(0.0, 32.0);
    t.marquee.control_radius = t.marquee.control_radius.clamp(0.0, 24.0);
    t.marquee.blur = t.marquee.blur.clamp(0.0, 60.0);
    t.marquee.shadow = t.marquee.shadow.clamp(0.0, 1.0);
    t.marquee.motion = t.marquee.motion.clamp(0.0, 1.5);
    t.marquee.glow = t.marquee.glow.clamp(0.0, 1.0);
    t.builtin = false;
    if t.id.is_empty() || builtin().iter().any(|b| b.id == t.id) {
        t.id = format!("custom-{}", crate::model::slug(&t.name));
    }
    Ok(t)
}

/// A Hangar theme leaves out colors it doesn't change; fill those from Marquee's dark or light base.
pub fn from_suite_json(v: &Value) -> Result<Theme> {
    let mode = if v["mode"].as_str().map(|m| m.eq_ignore_ascii_case("light")).unwrap_or(false) { "pure-light" } else { "marquee" };
    let base = builtin().into_iter().find(|t| t.id == mode).expect("base theme exists");
    let mut merged = serde_json::to_value(&base)?;
    if let (Some(dst), Some(src)) = (merged.as_object_mut(), v.as_object()) {
        for (k, val) in src {
            if k == "colors" {
                if let (Some(dc), Some(sc)) = (dst.get_mut("colors").and_then(|c| c.as_object_mut()), val.as_object()) {
                    for (ck, cv) in sc {
                        dc.insert(ck.clone(), cv.clone());
                    }
                }
            } else {
                dst.insert(k.clone(), val.clone());
            }
        }
        dst.insert("id".into(), json!(""));
    }
    validate(&merged)
}

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("themes")
}

/// Custom themes saved in the themes folder.
pub fn custom(data_dir: &Path) -> Vec<Theme> {
    let mut out = vec![];
    let Ok(entries) = std::fs::read_dir(dir(data_dir)) else { return out };
    for e in entries.flatten() {
        let path = e.path();
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
        if ext != EXTENSION && ext != "json" {
            continue;
        }
        if let Some(t) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| from_suite_json(&v).ok()) {
            if !out.iter().any(|x: &Theme| x.id == t.id) {
                out.push(t);
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn all(data_dir: &Path) -> Vec<Theme> {
    let mut list = builtin();
    list.extend(custom(data_dir));
    list
}

fn file_for(data_dir: &Path, id: &str) -> PathBuf {
    dir(data_dir).join(format!("{}.{EXTENSION}", crate::model::slug(id)))
}

/// Writes the file form of a theme (without Marquee's internal flags).
pub fn to_file_json(t: &Theme) -> Result<String> {
    let mut v = serde_json::to_value(t)?;
    if let Some(o) = v.as_object_mut() {
        o.remove("builtin");
        o.remove("id");
    }
    Ok(serde_json::to_string_pretty(&v)?)
}

pub fn save(data_dir: &Path, v: &Value) -> Result<Theme> {
    let t = validate(v)?;
    std::fs::create_dir_all(dir(data_dir))?;
    std::fs::write(file_for(data_dir, &t.id), to_file_json(&t)?)?;
    Ok(t)
}

pub fn delete(data_dir: &Path, id: &str) -> Result<()> {
    if builtin().iter().any(|t| t.id == id) {
        return Err(Error::msg("Built-in themes can't be deleted. Duplicate one to change it."));
    }
    let path = file_for(data_dir, id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Copies a theme under a new name, ready to edit.
pub fn duplicate(data_dir: &Path, id: &str) -> Result<Theme> {
    let source = all(data_dir).into_iter().find(|t| t.id == id).ok_or_else(|| Error::msg("That theme no longer exists."))?;
    let existing: Vec<String> = all(data_dir).into_iter().map(|t| t.name).collect();
    let mut name = format!("{} copy", source.name);
    let mut n = 2;
    while existing.contains(&name) {
        name = format!("{} copy {n}", source.name);
        n += 1;
    }
    let mut v = serde_json::to_value(&source)?;
    v["name"] = json!(name);
    v["id"] = json!("");
    if source.colors.accent == "windows" {
        v["colors"]["accent"] = json!("#F2C14E");
    }
    save(data_dir, &v)
}

pub fn import(data_dir: &Path, file: &Path) -> Result<Theme> {
    let text = std::fs::read_to_string(file).map_err(|_| Error::msg("That file couldn't be opened."))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| Error::msg("That isn't a theme file. Theme files are JSON (.marqueetheme, or a suite .json theme)."))?;
    let t = from_suite_json(&v)?;
    save(data_dir, &serde_json::to_value(&t)?)
}

pub fn export(data_dir: &Path, id: &str, dest: &Path) -> Result<()> {
    let t = all(data_dir).into_iter().find(|t| t.id == id).ok_or_else(|| Error::msg("That theme no longer exists."))?;
    std::fs::write(dest, to_file_json(&t)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_theme_meets_wcag_aa() {
        let mut themes = builtin();
        themes.push(system_light());
        for t in themes {
            let c = &t.colors;
            for bg in [&c.background, &c.sidebar, &c.surface, &c.surface_hover] {
                let text = contrast(&c.text, bg).unwrap();
                let sub = contrast(&c.subtext, bg).unwrap();
                let faint = contrast(&c.faint, bg).unwrap();
                assert!(text >= 7.0, "{}: text on {bg} is {text:.2}", t.name);
                assert!(sub >= 4.5, "{}: subtext on {bg} is {sub:.2}", t.name);
                assert!(faint >= 4.5, "{}: faint text on {bg} is {faint:.2}", t.name);
            }
            if c.accent != "windows" {
                // Accent is used for focus rings, rings and buttons: 3:1 against the page (non-text UI),
                // and the label on an accent button must reach 4.5:1.
                let ring = contrast(&c.accent, &c.background).unwrap();
                let label = contrast(on_accent(&c.accent), &c.accent).unwrap();
                assert!(ring >= 3.0, "{}: accent on background is {ring:.2}", t.name);
                assert!(label >= 4.5, "{}: label on accent is {label:.2}", t.name);
            }
        }
    }

    #[test]
    fn the_spec_themes_all_exist() {
        let names: Vec<String> = builtin().into_iter().map(|t| t.name).collect();
        for wanted in ["Marquee", "Midnight", "Cinema Red", "Slate Light", "Pure Light", "OLED Black", "Synthwave", "Forest", "Follow Windows"] {
            assert!(names.contains(&wanted.to_string()), "missing {wanted}");
        }
        let marquee = &builtin()[0];
        assert_eq!((marquee.id.as_str(), marquee.colors.accent.as_str()), ("marquee", "#F2C14E"));
    }

    #[test]
    fn contrast_math_matches_known_values() {
        assert!((contrast("#000000", "#FFFFFF").unwrap() - 21.0).abs() < 0.01);
        assert!((contrast("#777777", "#FFFFFF").unwrap() - 4.48).abs() < 0.02);
        assert_eq!(contrast("nope", "#FFF"), None);
        assert_eq!(on_accent("#F2C14E"), "#0A0A0C");
        assert_eq!(on_accent("#2F6FDB"), "#FFFFFF");
    }

    #[test]
    fn a_hangar_theme_file_loads_with_missing_colors_filled_in() {
        // Straight from Hangar's docs/THEMES.md, minus a few colors.
        let suite = json!({ "schema": 1, "name": "My Theme", "mode": "dark", "backdrop": "mica", "transparency": 0.15, "colors": { "background": "#2E3440", "text": "#ECEFF4", "accent": "#88C0D0" } });
        let t = from_suite_json(&suite).unwrap();
        assert_eq!(t.colors.background, "#2E3440");
        assert_eq!(t.colors.accent, "#88C0D0");
        assert_eq!(t.colors.surface, "#151518", "missing colors come from the dark base");
        assert_eq!(t.id, "custom-my-theme");
        assert_eq!(t.marquee, Extras::default());
    }

    #[test]
    fn themes_save_duplicate_export_import_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let copy = duplicate(dir.path(), "synthwave").unwrap();
        assert_eq!(copy.name, "Synthwave copy");
        assert!(!copy.builtin);
        let again = duplicate(dir.path(), "synthwave").unwrap();
        assert_eq!(again.name, "Synthwave copy 2");
        assert_eq!(custom(dir.path()).len(), 2);

        let out = dir.path().join("shared.marqueetheme");
        export(dir.path(), &copy.id, &out).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("\"surfaceHover\"") && !text.contains("builtin"), "file uses the suite's key names");
        delete(dir.path(), &copy.id).unwrap();
        assert_eq!(custom(dir.path()).len(), 1);
        let back = import(dir.path(), &out).unwrap();
        assert_eq!(back.colors, copy.colors);
        assert!(delete(dir.path(), "marquee").is_err());
    }

    #[test]
    fn bad_theme_files_say_what_is_wrong() {
        assert!(validate(&json!({ "name": "", "colors": {} })).is_err());
        let mut v = serde_json::to_value(&builtin()[0]).unwrap();
        v["colors"]["text"] = json!("blue-ish");
        assert!(validate(&v).unwrap_err().to_string().contains("blue-ish"));
        v["colors"]["text"] = json!("#FFF");
        v["schema"] = json!(2);
        assert!(validate(&v).unwrap_err().to_string().contains("format 2"));
        v["schema"] = json!(1);
        v["transparency"] = json!(5.0);
        assert_eq!(validate(&v).unwrap().transparency, 0.9);
    }
}
