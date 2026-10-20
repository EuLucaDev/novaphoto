//! SVG clipboard interchange keeps the vector source inside an editable Smart Object.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use serde_json::{Value, json};
fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let svg = p
        .get("svg")
        .and_then(Value::as_str)
        .filter(|text| text.len() <= 8 * 1024 * 1024)
        .ok_or_else(|| EngineError::BadParams { cmd: "edit.pasteVector".into(), msg: "provide SVG text up to 8 MiB".into() })?;
    if !novaphoto_io::svg::is_svg(svg.as_bytes()) {
        return Err(EngineError::Other("The clipboard contains no SVG artwork".into()));
    }
    let mut params = json!({"smartObject":true});
    if let Some(center) = p.get("center") {
        params["center"] = center.clone();
    }
    crate::file_cmds::place_bytes(s, "Vector.svg", svg.as_bytes().to_vec(), None, &params)
}
pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "edit.pasteVector",
        label: "Paste Vector as Smart Object",
        menu: &[],
        shortcut: None,
        params: r#"{"svg":"<svg …>","center":[x,y]?}; preserves SVG source in a new Smart Object"#,
        enabled: |s| s.active().map(|_| ()).ok_or_else(|| "Open a document first".into()),
        run: paste,
        journal: true,
    }]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_stays_embedded_editable_and_undoable() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":32,"height":32})).unwrap();
        let before = s.active().unwrap().doc.clone();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><path fill="#13ab54" d="M0 0H20V20H0Z"/></svg>"##;
        s.execute("edit.pasteVector", json!({"svg":svg})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let layer = doc.layer(s.active().unwrap().active_layer.unwrap()).unwrap();
        let novaphoto_doc::LayerContent::Smart(smart) = &layer.content else { panic!("expected Smart Object") };
        let novaphoto_doc::SmartSource::Embedded { bytes, .. } = &smart.source else { panic!("expected embedded SVG") };
        assert_eq!(bytes.as_slice(), svg.as_bytes());
        assert!(smart.cache.is_some());
        assert!(s.undo());
        assert_eq!(*s.active().unwrap().doc, *before);
    }
}
