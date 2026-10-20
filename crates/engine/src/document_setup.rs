//! Advanced New Document settings, shared by GUI, presets and automation.
use crate::{EngineError, Result};
use novaphoto_cms::{Builtin, ColorSpace};
use novaphoto_color::ColorMode;
use novaphoto_doc::{Artboard, ArtboardBackground, Document, Layer, LayerContent};
use serde_json::Value;

pub const PIXEL_ASPECT_KEY: &str = "novaphoto:pixelAspectRatio";
pub fn pixel_aspect(doc: &Document) -> f32 {
    doc.metadata
        .text
        .iter()
        .find(|(k, _)| k == PIXEL_ASPECT_KEY)
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .filter(|v| v.is_finite() && (0.1..=10.0).contains(v))
        .unwrap_or(1.0)
}
pub(crate) fn validate(p: &Value, mode: ColorMode) -> Result<()> {
    let bad = |msg: &str| EngineError::BadParams { cmd: "file.new".into(), msg: msg.into() };
    if let Some(v) = p.get("artboards")
        && !v.is_boolean()
    {
        return Err(bad("artboards must be a boolean"));
    }
    if let Some(v) = p.get("pixelAspectRatio")
        && !v.as_f64().is_some_and(|n| n.is_finite() && (0.1..=10.0).contains(&n))
    {
        return Err(bad("pixelAspectRatio must be within 0.1..=10"));
    }
    if let Some(v) = p.get("colorProfile") {
        let profile = v.as_str().and_then(Builtin::from_id).ok_or_else(|| bad("unknown colorProfile"))?;
        let expected = match mode {
            ColorMode::Rgb => ColorSpace::Rgb,
            ColorMode::Grayscale => ColorSpace::Gray,
            ColorMode::Cmyk => ColorSpace::Cmyk,
            ColorMode::Lab => ColorSpace::Lab,
            _ => return Err(bad("unsupported color mode")),
        };
        if profile.profile().color_space != expected {
            return Err(bad("colorProfile does not match the document color mode"));
        }
    }
    Ok(())
}
pub(crate) fn apply(doc: &mut Document, p: &Value) {
    if let Some(profile) = p.get("colorProfile").and_then(Value::as_str).and_then(Builtin::from_id) {
        doc.icc_profile = Some(profile.profile().to_bytes());
    }
    let ratio = p.get("pixelAspectRatio").and_then(Value::as_f64).unwrap_or(1.0);
    if ratio != 1.0 {
        doc.metadata.text.push((PIXEL_ASPECT_KEY.into(), ratio.to_string()));
    }
    if p.get("artboards").and_then(Value::as_bool) == Some(true) {
        let background = match p.get("background").and_then(Value::as_str).unwrap_or("white") {
            "transparent" => ArtboardBackground::Transparent,
            "black" => ArtboardBackground::Black,
            "white" => ArtboardBackground::White,
            color => crate::commands::parse_hex(color)
                .map(|c| ArtboardBackground::Custom(novaphoto_color::Color::rgba(c[0], c[1], c[2], c[3])))
                .unwrap_or(ArtboardBackground::White),
        };
        // The board owns its background; its first child is immediately paintable.
        doc.layers.clear();
        let children = vec![Layer::raster("Layer 1", doc.pixel_format())];
        let mut board = Layer::group("Artboard 1", children);
        if let LayerContent::Group(g) = &mut board.content {
            g.artboard = Some(Artboard { rect: doc.bounds(), background, preset: String::new() });
        }
        doc.layers.push(board);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;
    #[test]
    fn advanced_creation_and_presets_round_trip() {
        let mut s = Session::new();
        let settings = json!({"width":20,"height":15,"colorProfile":"display-p3","pixelAspectRatio":1.5,"artboards":true});
        s.execute("document.presets.save", json!({"name":"Advanced","settings":settings})).unwrap();
        let mut restored = Session::new();
        restored.load_prefs_json(&s.prefs_to_json()).unwrap();
        let params = restored.presets.documents[0].settings.command_params();
        restored.execute("file.new", params).unwrap();
        let d = &restored.active().unwrap().doc;
        assert_eq!(d.artboards().len(), 1);
        assert_eq!(pixel_aspect(d), 1.5);
        assert_eq!(d.icc_profile.as_ref().unwrap().as_slice(), Builtin::DisplayP3.profile().to_bytes().as_slice());
        let (bytes, _) = novaphoto_format::PcraftWriter::new().save_zip(d, &Default::default()).unwrap();
        assert_eq!(pixel_aspect(&novaphoto_format::load_from_bytes(&bytes).unwrap()), 1.5);
    }
    #[test]
    fn a_new_artboard_has_an_editable_child_and_the_requested_background() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":8,"height":8,"artboards":true,"background":"white"})).unwrap();
        assert!(s.is_enabled("edit.fill"));
        let d = s.active().unwrap();
        assert!(matches!(d.doc.layer(d.active_layer.unwrap()).unwrap().content, LayerContent::Raster(_)));
        s.execute("edit.fill", json!({"color":"#ff0000"})).unwrap();
    }
    #[test]
    fn invalid_advanced_settings_never_create_a_document() {
        for p in [
            json!({"pixelAspectRatio":0}),
            json!({"pixelAspectRatio":"wide"}),
            json!({"artboards":1}),
            json!({"colorProfile":"missing"}),
            json!({"mode":"cmyk","colorProfile":"srgb"}),
        ] {
            let mut s = Session::new();
            assert!(s.execute("file.new", p).is_err());
            assert!(s.active().is_none());
        }
    }
}
