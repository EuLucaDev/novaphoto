//! Discoverable carousel workflows; all document mutations remain engine commands.
use crate::{NovaphotoApp, state::DialogKind, widgets};
use serde_json::{Map, Value, json};

pub fn owns(f: &Map<String, Value>) -> bool {
    f.contains_key("__carouselAction")
}
pub fn menu(app: &mut NovaphotoApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if !matches!(id, "carousel.divide" | "file.export.carousel") || p.as_object().is_some_and(|o| !o.is_empty()) {
        return None;
    }
    let Some(d) = app.session.active() else {
        return Some(Err("Open a document first".into()));
    };
    let export = id == "file.export.carousel";
    let fields = json!({"__carouselAction":if export {"export"} else {"divide"},"__command":id,
        "__label":if export {"Export Carousel Cards"} else {"Divide into Carousel Cards"},
        "__documentId":d.doc.id.0,"cards":5,"fit":"exact","replace":false,
        "format":"png","prefix":d.doc.name,"dir":""});
    Some(Ok(json!({"dialog":app.ui.open_dialog(DialogKind::Command,fields.as_object().cloned().unwrap_or_default())})))
}
pub fn body(app: &mut NovaphotoApp, ui: &mut egui::Ui, f: &mut Map<String, Value>, dialog: u64) {
    let export = f.get("__carouselAction").and_then(Value::as_str) == Some("export");
    ui.set_min_width(340.0);
    if export {
        ui.label(tl!("Format"));
        let mut format = f.get("format").and_then(Value::as_str).unwrap_or("png").to_string();
        if widgets::dropdown(ui, "carousel-format", &mut format, &[("png".into(), "PNG"), ("jpeg".into(), "JPEG")], 200.0) {
            f.insert("format".into(), json!(format));
        }
        ui.label(tl!("Filename Prefix"));
        let mut prefix = f.get("prefix").and_then(Value::as_str).unwrap_or("").to_string();
        if ui.add(egui::TextEdit::singleline(&mut prefix).desired_width(320.0)).changed() {
            f.insert("prefix".into(), json!(prefix));
        }
        ui.label(tl!("Export Folder"));
        let mut dir = f.get("dir").and_then(Value::as_str).unwrap_or("").to_string();
        if ui.add(egui::TextEdit::singleline(&mut dir).desired_width(320.0)).changed() {
            f.insert("dir".into(), json!(dir));
        }
        if widgets::secondary_button(ui, tl!("Choose Folder…"), 160.0).clicked()
            && let Err(error) = app.ask_folder(move |app, path| {
                let d = app.ui.dialogs.iter_mut().find(|d| d.id == dialog).ok_or("The export dialog was closed")?;
                d.fields.insert("dir".into(), json!(path));
                Ok(Value::Null)
            })
        {
            app.ui.status = error;
        }
        ui.add_space(8.0);
        ui.label(tl!("Cards are exported in sRGB. Existing files are never replaced."));
    } else {
        ui.label(tl!("Cards"));
        let mut cards = f.get("cards").and_then(Value::as_f64).unwrap_or(5.0) as f32;
        if widgets::value_field(ui, &mut cards, 1.0..=100.0, "", 120.0).changed() {
            f.insert("cards".into(), json!(cards.round() as u32));
        }
        let mut pad = f.get("fit").and_then(Value::as_str) == Some("pad");
        if widgets::checkbox(ui, &mut pad, tl!("Add transparent margin to fit equal cards")).changed() {
            f.insert("fit".into(), json!(if pad { "pad" } else { "exact" }));
        }
        let mut replace = f.get("replace").and_then(Value::as_bool).unwrap_or(false);
        if app.session.active().is_some_and(|d| !d.doc.slices.is_empty()) && widgets::checkbox(ui, &mut replace, tl!("Replace existing slices")).changed() {
            f.insert("replace".into(), json!(replace));
        }
        match app.session.execute(
            "carousel.preview",
            Value::Object(f.iter().filter(|(k, _)| matches!(k.as_str(), "cards" | "fit")).map(|(k, v)| (k.clone(), v.clone())).collect()),
        ) {
            Ok(info) => {
                ui.label(format!("{} × {} px · {} {}", info["cardWidth"], info["height"], info["cards"], tl!("Cards")));
                if info["divisible"] == false {
                    ui.label(format!("{}: {} px", tl!("Required margin"), info["padding"]));
                }
            }
            Err(e) => {
                ui.label(e.to_string());
            }
        }
    }
}
pub fn validate(app: &NovaphotoApp, f: &Map<String, Value>) -> Result<(), String> {
    let id = app.session.active().ok_or("Open a document first")?.doc.id.0;
    if f.get("__documentId").and_then(Value::as_u64) != Some(id) {
        return Err("The active document changed; reopen this dialog".into());
    }
    if f.get("__carouselAction").and_then(Value::as_str) == Some("export") && f.get("dir").and_then(Value::as_str).is_none_or(|s| s.trim().is_empty()) {
        return Err(tl!("Choose an export folder").into());
    }
    Ok(())
}
pub fn confirm(app: &mut NovaphotoApp, f: &Map<String, Value>) -> Result<Value, String> {
    validate(app, f)?;
    let id = f.get("__command").and_then(Value::as_str).ok_or("Missing carousel command")?;
    let p =
        Value::Object(
            f.iter()
                .filter(|(k, _)| {
                    if id == "carousel.divide" { matches!(k.as_str(), "cards" | "fit" | "replace") } else { matches!(k.as_str(), "dir" | "prefix" | "format") }
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
    app.run(id, p)
}
