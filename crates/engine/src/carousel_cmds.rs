//! Continuous social artwork. Cards are slices, never duplicated layers or clipped artboards.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use novaphoto_doc::{Document, slices::Slice};
use novaphoto_geom::Rect;
use serde_json::{Value, json};

const MAX_CARDS: u64 = 100;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
fn bad(message: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "carousel".into(), msg: message.into() }
}
fn count(p: &Value) -> Result<u32> {
    let n = p.get("cards").map_or(Some(5), Value::as_u64).ok_or_else(|| bad("cards must be a whole number"))?;
    if !(1..=MAX_CARDS).contains(&n) {
        return Err(bad("cards must be within 1..=100"));
    }
    Ok(n as u32)
}
fn dimension(p: &Value, key: &str, default: u32) -> Result<u32> {
    let n = p.get(key).map_or(Some(u64::from(default)), Value::as_u64).ok_or_else(|| bad(format!("{key} must be a whole number")))?;
    if !(1..=300_000).contains(&n) {
        return Err(bad(format!("{key} must be within 1..=300000")));
    }
    Ok(n as u32)
}
fn budget(width: u32, height: u32) -> Result<()> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(bad("carousel exceeds the 64 megapixel working budget"));
    }
    Ok(())
}
fn layout(width: u32, height: u32, cards: u32, fit: &str) -> Result<Value> {
    let remaining = width % cards;
    let padding = if remaining == 0 { 0 } else { cards - remaining };
    if !matches!(fit, "exact" | "pad") {
        return Err(bad("fit must be exact or pad"));
    }
    let adjusted = width.checked_add(if fit == "pad" { padding } else { 0 }).ok_or_else(|| bad("canvas width overflow"))?;
    if adjusted > 300_000 || width < cards {
        return Err(bad("canvas is too narrow for this number of cards or exceeds the dimension limit"));
    }
    budget(adjusted, height)?;
    Ok(json!({"width":width,"height":height,"cards":cards,"cardWidth":adjusted/cards,
        "targetWidth":adjusted,"padding":padding,"divisible":remaining==0}))
}
fn install(doc: &mut Document, cards: u32) {
    let width = doc.size.width / cards;
    doc.slices.list.clear();
    doc.slices.group_name = doc.name.clone();
    for n in 0..cards {
        let x = n * width;
        doc.slices.list.push(Slice {
            id: n + 1,
            name: format!("{}-{:02}", doc.name, n + 1),
            rect: Rect::from_xywh(x as i32, 0, width, doc.size.height),
            ..Default::default()
        });
        if n > 0 && !doc.guides.vertical.contains(&f64::from(x)) {
            doc.guides.vertical.push(f64::from(x));
        }
    }
    doc.guides.vertical.sort_by(f64::total_cmp);
}
fn creation_params(p: &Value) -> Result<(Value, u32, u32, u32)> {
    let cards = count(p)?;
    let width = dimension(p, "width", 1080)?.checked_mul(cards).ok_or_else(|| bad("canvas width overflow"))?;
    let height = dimension(p, "height", 1350)?;
    if width > 300_000 {
        return Err(bad("canvas width exceeds 300000 pixels"));
    }
    budget(width, height)?;
    let mut params = p.as_object().cloned().ok_or_else(|| bad("params must be an object"))?;
    params.remove("cards");
    if params.get("artboards").and_then(Value::as_bool) == Some(true) {
        return Err(bad("continuous carousels use slices; disable artboards"));
    }
    params.insert("width".into(), json!(width));
    params.insert("height".into(), json!(height));
    params.entry("name").or_insert(json!("Carousel"));
    Ok((Value::Object(params), cards, width, height))
}
pub(crate) fn validate_creation(s: &Session, p: &Value) -> Result<()> {
    let (params, _, _, _) = creation_params(p)?;
    crate::document_preset_cmds::validate_new_params(s, &params)
}
fn new_carousel(s: &mut Session, p: &Value) -> Result<Value> {
    let (params, cards, width, height) = creation_params(p)?;
    let mut template = Session::new();
    template.tools.background = s.tools.background;
    template.execute("file.new", params)?;
    let mut doc = (*template.active().ok_or(EngineError::NoDocument)?.doc).clone();
    install(&mut doc, cards);
    let index = s.add_document(doc, None);
    Ok(json!({"document":index,"cards":cards,"width":width,"height":height}))
}
fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.active().ok_or(EngineError::NoDocument)?;
    layout(d.doc.size.width, d.doc.size.height, count(p)?, p.get("fit").and_then(Value::as_str).unwrap_or("exact"))
}
fn split(s: &mut Session, p: &Value) -> Result<Value> {
    let cards = count(p)?;
    let d = s.active().ok_or(EngineError::NoDocument)?;
    if d.doc.has_artboards() {
        return Err(bad("continuous carousel requires a regular canvas, not artboards"));
    }
    if !d.doc.slices.is_empty() && p.get("replace").and_then(Value::as_bool) != Some(true) {
        return Err(bad("existing slices are preserved; confirm replace=true to replace them"));
    }
    let fit = p.get("fit").and_then(Value::as_str).unwrap_or("exact");
    let info = layout(d.doc.size.width, d.doc.size.height, cards, fit)?;
    if info["divisible"] == false && fit == "exact" {
        return Err(bad("width is not divisible by cards; inspect carousel.preview and explicitly choose fit=pad"));
    }
    let width = info["targetWidth"].as_u64().ok_or_else(|| bad("missing target width"))? as u32;
    s.edit("Divide into carousel cards", |doc, _| {
        doc.size.width = width;
        install(doc, cards);
        Ok(())
    })?;
    Ok(info)
}
/// Writer injection keeps MCP paths inside its authorized workspace and enables browser export.
pub fn export_with_writer(s: &mut Session, p: &Value, write: &mut dyn FnMut(&str, &[u8]) -> Result<()>) -> Result<Value> {
    let d = s.active().ok_or(EngineError::NoDocument)?;
    let cards = d.doc.slices.list.len();
    if cards == 0 || cards > MAX_CARDS as usize || d.doc.has_artboards() {
        return Err(bad("create or divide a continuous carousel first"));
    }
    budget(d.doc.size.width, d.doc.size.height)?;
    let mut ordered = d.doc.slices.list.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|sl| sl.rect.x0);
    let mut edge = 0;
    for sl in ordered {
        if sl.rect.x0 != edge || sl.rect.y0 != 0 || sl.rect.y1 != d.doc.size.height as i32 || sl.rect.x1 <= edge {
            return Err(bad("cards must partition the complete canvas from left to right"));
        }
        edge = sl.rect.x1;
    }
    if edge != d.doc.size.width as i32 {
        return Err(bad("cards do not cover the full canvas"));
    }
    let format = match p.get("format").and_then(Value::as_str).unwrap_or("png") {
        "png" => "png24",
        "jpg" | "jpeg" => "jpeg",
        _ => return Err(bad("format must be png or jpeg")),
    };
    let dir = p.get("dir").and_then(Value::as_str).ok_or_else(|| bad("choose an export directory"))?;
    let prefix = crate::file_cmds::sanitize(p.get("prefix").and_then(Value::as_str).unwrap_or(&d.doc.name));
    if prefix.is_empty() {
        return Err(bad("filename prefix must not be empty"));
    }
    let ext = if format == "png24" { "png" } else { "jpg" };
    let mut files = Vec::new();
    let mut bytes = 0;
    let mut writer = |_: &str, data: &[u8]| {
        let path = crate::file_cmds::join(dir, &format!("{prefix}-{:02}.{ext}", files.len() + 1));
        write(&path, data)?;
        bytes += data.len();
        files.push(path);
        Ok(())
    };
    crate::web_cmds::save_for_web_with_writer(
        s,
        &json!({"dir":dir,"format":format,"convertToSrgb":true,
        "embedIcc":true,"slices":"user","html":false,"quality":p.get("quality").and_then(Value::as_u64).unwrap_or(90)}),
        &mut writer,
    )?;
    Ok(json!({"files":files,"bytes":bytes}))
}
#[cfg(not(target_arch = "wasm32"))]
fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let doc = s.active().ok_or(EngineError::NoDocument)?.doc.clone();
    let params = p.clone();
    crate::jobs::run(
        s,
        "Export Carousel Cards",
        false,
        move |ctx| {
            use std::io::Write;
            let mut snapshot = Session::new();
            snapshot.add_document((*doc).clone(), None);
            let total = doc.slices.list.len().max(1);
            let dir = params.get("dir").and_then(Value::as_str).ok_or_else(|| bad("choose an export directory"))?;
            let prefix = crate::file_cmds::sanitize(params.get("prefix").and_then(Value::as_str).unwrap_or(&doc.name));
            let ext = if matches!(params.get("format").and_then(Value::as_str), Some("jpg" | "jpeg")) { "jpg" } else { "png" };
            for n in 1..=total {
                let path = std::path::Path::new(dir).join(format!("{prefix}-{n:02}.{ext}"));
                if path.try_exists().map_err(|e| bad(e.to_string()))? {
                    return Err(bad(format!("{} already exists; choose another name or folder", path.display())));
                }
            }
            let mut staged = Vec::new();
            let result = export_with_writer(&mut snapshot, &params, &mut |path, bytes| {
                ctx.check()?;
                let parent = std::path::Path::new(path).parent().ok_or_else(|| bad("invalid export folder"))?;
                let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| bad(e.to_string()))?;
                file.write_all(bytes).and_then(|_| file.as_file().sync_all()).map_err(|e| bad(e.to_string()))?;
                staged.push((path.to_string(), file));
                ctx.progress(staged.len() as f32 / total as f32, "Preparing carousel cards");
                Ok(())
            })?;
            ctx.check()?;
            let mut committed = Vec::new();
            for (path, file) in staged {
                if let Err(error) = ctx.check().and_then(|_| file.persist_noclobber(&path).map(|_| ()).map_err(|e| bad(e.error.to_string()))) {
                    for path in committed {
                        let _ = std::fs::remove_file(path);
                    }
                    return Err(error);
                }
                committed.push(path);
            }
            Ok(result)
        },
        |_, result| Ok(result),
    )
}
#[cfg(target_arch = "wasm32")]
fn export(_: &mut Session, _: &Value) -> Result<Value> {
    Err(bad("use the browser export service"))
}
pub fn specs() -> Vec<CommandSpec> {
    let has_doc = |s: &Session| s.active().map(|_| ()).ok_or_else(|| "open a document first".to_string());
    vec![
        CommandSpec {
            id: "file.new.carousel",
            label: "New Carousel…",
            menu: &["File"],
            shortcut: None,
            params: r#"{"width":1080,"height":1350,"cards":5,"name":"Carousel","background":"white"} plus file.new settings; max 100 cards, 64 MP"#,
            enabled: crate::commands::always,
            run: new_carousel,
            journal: true,
        },
        CommandSpec {
            id: "carousel.preview",
            label: "Preview Carousel Layout",
            menu: &[],
            shortcut: None,
            params: r#"{"cards":5,"fit":"exact|pad"} -> {width,height,cards,cardWidth,targetWidth,padding,divisible}"#,
            enabled: has_doc,
            run: preview,
            journal: false,
        },
        CommandSpec {
            id: "carousel.divide",
            label: "Divide into Carousel Cards…",
            menu: &["Image"],
            shortcut: None,
            params: r#"{"cards":5,"fit":"exact|pad","replace":false}"#,
            enabled: has_doc,
            run: split,
            journal: true,
        },
        CommandSpec {
            id: "file.export.carousel",
            label: "Export Carousel Cards…",
            menu: &["File", "Export"],
            shortcut: None,
            params: r#"{"dir":"folder","format":"png|jpeg","prefix":"name","quality":90}; never overwrite existing files"#,
            enabled: crate::file_cmds::native_doc,
            run: export,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_document_round_trips_without_duplicating_layers() {
        let mut s = Session::new();
        s.execute("file.new.carousel", json!({"width":12,"height":8,"cards":5})).unwrap();
        let doc = &s.active().unwrap().doc;
        assert_eq!(doc.size.width, 60);
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.guides.vertical, vec![12., 24., 36., 48.]);
        let (bytes, _) = novaphoto_format::PcraftWriter::new().save_zip(doc, &Default::default()).unwrap();
        let restored = novaphoto_format::load_from_bytes(&bytes).unwrap();
        assert_eq!(restored.slices, doc.slices);
    }
    #[test]
    fn padding_and_replacement_are_explicit_and_one_undo_restores_document() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":23,"height":8})).unwrap();
        assert!(s.execute("carousel.divide", json!({"cards":5})).is_err());
        assert_eq!(s.active().unwrap().doc.size.width, 23);
        s.execute("carousel.divide", json!({"cards":5,"fit":"pad"})).unwrap();
        assert_eq!(s.active().unwrap().doc.size.width, 25);
        assert!(s.execute("carousel.divide", json!({"cards":5})).is_err());
        assert!(s.undo());
        assert_eq!(s.active().unwrap().doc.size.width, 23);
        assert!(s.active().unwrap().doc.slices.is_empty());
    }
    #[test]
    fn invalid_creation_is_atomic() {
        for p in [
            json!({"cards":0}),
            json!({"cards":101}),
            json!({"cards":2.5}),
            json!({"width":300000}),
            json!({"width":10000,"height":10000}),
            json!({"artboards":true}),
        ] {
            let mut s = Session::new();
            assert!(s.execute("file.new.carousel", p).is_err());
            assert!(s.active().is_none());
        }
    }
    #[test]
    fn exports_reassemble_pixels_and_leave_profile_and_layers_unchanged() {
        for depth in [8, 16, 32] {
            let mut s = Session::new();
            s.execute("file.new.carousel", json!({"width":4,"height":3,"cards":3,"depth":depth,"background":"#cc2244","colorProfile":"display-p3"})).unwrap();
            s.edit("test gradient", |doc, _| {
                if let Some(novaphoto_doc::Layer { content: novaphoto_doc::LayerContent::Raster(surface), .. }) = doc.layers.first_mut() {
                    for x in 0..12 {
                        surface.fill_rect(Rect::new(x, 0, x + 1, 3), &[x as f32 / 12.0, 0.2, 0.5, 1.0]);
                    }
                }
                Ok(())
            })
            .unwrap();
            let before = s.active().unwrap().doc.clone();
            let expected =
                crate::web_cmds::web_document(&before, &json!({}), &crate::web_cmds::WebSettings::from_params(&json!({"convertToSrgb":true}), "test").unwrap())
                    .unwrap()
                    .0;
            let flat = novaphoto_compose::flatten(&expected);
            let mut outputs = Vec::new();
            export_with_writer(&mut s, &json!({"dir":"out","prefix":"campaign","format":"png"}), &mut |name, bytes| {
                outputs.push((name.to_string(), bytes.to_vec()));
                Ok(())
            })
            .unwrap();
            assert_eq!(outputs.len(), 3);
            assert!(outputs[0].0.ends_with("campaign-01.png"));
            let mut whole = Vec::new();
            crate::web_cmds::save_for_web_with_writer(
                &mut s,
                &json!({"path":"whole.png","format":"png24","convertToSrgb":true,"embedIcc":true}),
                &mut |_, bytes| {
                    whole = bytes.to_vec();
                    Ok(())
                },
            )
            .unwrap();
            let whole = novaphoto_codecs::decode(&whole).unwrap();
            let mut joined = vec![0; whole.data().len()];
            let stride = whole.data().len() / 3;
            for (card, (_, bytes)) in outputs.into_iter().enumerate() {
                let img = novaphoto_codecs::decode(&bytes).unwrap();
                assert_eq!((img.width(), img.height()), (4, 3));
                let row = img.data().len() / 3;
                for y in 0..3 {
                    joined[y * stride + card * row..y * stride + (card + 1) * row].copy_from_slice(&img.data()[y * row..(y + 1) * row]);
                }
            }
            assert_eq!(joined, whole.data());
            assert_eq!(flat.rect.width(), 12);
            assert_eq!(&*s.active().unwrap().doc, &*before);
        }
    }
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn existing_output_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-02.png");
        std::fs::write(&path, b"keep").unwrap();
        let mut s = Session::new();
        s.execute("file.new.carousel", json!({"width":2,"height":2,"cards":2})).unwrap();
        assert!(s.execute("file.export.carousel", json!({"dir":dir.path(),"prefix":"test"})).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"keep");
        assert!(!dir.path().join("test-01.png").exists());
    }
}
