//! Permanent Photo documents. Network and native serialization run off the drawing thread.
use crate::{NovaphotoApp, widgets};
use nova_live::{
    documents::{Client, Document, QueuedSave},
    identity::{self, Profile, Status},
};
use novaphoto_doc::{DocId, Document as PhotoDocument};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Binding {
    pub remote: Document,
    pub organization: String,
    pub user: String,
    pub pending: Option<QueuedSave>,
    pub conflict: bool,
}
enum Reply {
    List(Vec<Document>, (String, String)),
    Open { remote: Document, bytes: Vec<u8>, revision: u64, copy: bool, profile: Profile },
    Saved { local: DocId, source_revision: u64, remote: Document, queue: QueuedSave, uploaded: Option<u64>, failure: Option<String> },
    Data(String, Value, (String, String)),
    Asset { local: DocId, name: String, bytes: Vec<u8>, actor: (String, String) },
    Workspace { value: Value, revision: u64, actor: (String, String) },
    WorkspaceSaved { revision: u64, actor: (String, String) },
    Ticket { local: DocId, payload: String, signature: String, actor: (String, String) },
    Done((String, String)),
}
#[derive(Default)]
pub struct SharedUi {
    pub open: bool,
    pub selected: Option<Document>,
    pub bindings: HashMap<DocId, Binding>,
    request_selection: Option<(Option<String>, Option<String>)>,
    rows: Vec<Document>,
    pending: Vec<QueuedSave>,
    rx: Option<Receiver<Result<Reply, String>>>,
    ctx: Option<egui::Context>,
    pub error: Option<String>,
    pub notice: Option<String>,
    data: HashMap<String, Value>,
    query: String,
    comment: String,
    role: String,
    library: Option<String>,
    workspace_revision: u64,
    last_ticket: Option<Instant>,
    ticket_document: Option<DocId>,
    last_sync: Option<Instant>,
    actor: Option<(String, String)>,
}
fn client_actor(client: &Client) -> (String, String) {
    (client.profile().organization.id.clone(), client.profile().user.id.clone())
}
fn profile() -> Result<Profile, String> {
    match identity::status("novaphoto") {
        Status::Verified(p) | Status::OfflineVerified(p) if identity::local_authorized("novaphoto") => Ok(p),
        _ => Err(tl!("Reconnect Account").into()),
    }
}
fn start(app: &mut NovaphotoApp, work: impl FnOnce() -> Result<Reply, String> + Send + 'static) -> Result<Value, String> {
    if app.shared.rx.is_some() {
        return Err(tl!("Wait for the current operation to finish").into());
    }
    let (tx, rx) = mpsc::channel();
    let ctx = app.shared.ctx.clone();
    app.shared.error = None;
    app.shared.rx = Some(rx);
    app.shared.request_selection = Some((app.shared.selected.as_ref().map(|d| d.id.clone()), app.shared.library.clone()));
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
            .unwrap_or_else(|_| Err("The document operation failed; your local work is preserved".into()));
        let _ = tx.send(result);
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
    Ok(json!({"pending":true}))
}
fn selected_id(app: &NovaphotoApp, p: &Value) -> Result<String, String> {
    p.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| app.shared.selected.as_ref().map(|d| d.id.clone()))
        .ok_or_else(|| "Choose a shared document".into())
}
pub fn command(app: &mut NovaphotoApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if !id.starts_with("shared.") {
        return None;
    }
    Some((|| match id {
        "shared.panel" => {
            app.shared.open = true;
            refresh(app)
        }
        "shared.refresh" => refresh(app),
        "shared.status" => Ok(json!({"pending":app.shared.rx.is_some(),"error":app.shared.error,"notice":app.shared.notice,
                "documents":app.shared.rows,"active":app.session.active().and_then(|d|app.shared.bindings.get(&d.doc.id)).map(|b|&b.remote)})),
        "shared.library" | "shared.library.open" => {
            let actor = profile()?;
            let (path, key, slot) = if id == "shared.library.open" {
                let client_id = p["clientId"].as_str().ok_or("Choose a brand")?;
                nova_live::documents::valid_id(client_id).map_err(|e| e.to_string())?;
                app.shared.library = Some(client_id.into());
                (format!("/api/native/library/{client_id}"), client_id.to_string(), "assets")
            } else {
                ("/api/native/library/".into(), "brands".into(), "brands")
            };
            let slot = slot.to_string();
            start(app, move || {
                let value = match Client::current() {
                    Ok(client) => {
                        let value = client.request("GET", &path, Value::Null).map_err(|e| e.to_string())?;
                        let _ = nova_live::documents::library_catalog(&actor, &key, Some(&value));
                        value
                    }
                    Err(_) => nova_live::documents::library_catalog(&actor, &key, None).map_err(|e| e.to_string())?,
                };
                Ok(Reply::Data(slot, value, (actor.organization.id, actor.user.id)))
            })
        }
        "shared.library.insert" => {
            let local = app.session.active().ok_or("Open a document first")?.doc.id;
            let actor = profile()?;
            let actor_id = (actor.organization.id.clone(), actor.user.id.clone());
            let client_id = p["clientId"].as_str().ok_or("Choose a brand")?.to_string();
            let asset_id = p["assetId"].as_str().ok_or("Choose an element")?.to_string();
            nova_live::documents::valid_id(&client_id).map_err(|e| e.to_string())?;
            nova_live::documents::valid_id(&asset_id).map_err(|e| e.to_string())?;
            start(app, move || {
                let result = match Client::current() {
                    Ok(client) => client.library_asset(&client_id, &asset_id),
                    Err(_) => nova_live::documents::cached_library_asset(&actor, &asset_id),
                };
                let (name, bytes) = result.map_err(|e| e.to_string())?;
                Ok(Reply::Asset { local, name, bytes, actor: actor_id })
            })
        }
        "shared.workspace.pull" | "shared.workspace.push" => {
            let client = Client::current().map_err(|e| e.to_string())?;
            let actor = (client.profile().organization.id.clone(), client.profile().user.id.clone());
            let push = id.ends_with("push");
            let value = json!({"workspaces":app.session.prefs().workspaces});
            let base = app.shared.workspace_revision;
            start(app, move || {
                let result = client
                    .request(
                        if push { "PUT" } else { "GET" },
                        "/api/native/preferences/novaphoto/",
                        if push { json!({"baseRevision":base,"value":value}) } else { Value::Null },
                    )
                    .map_err(|e| e.to_string())?;
                let revision = result["revision"].as_u64().ok_or("Invalid preferences revision")?;
                Ok(if push { Reply::WorkspaceSaved { revision, actor } } else { Reply::Workspace { value: result["value"].clone(), revision, actor } })
            })
        }
        "shared.publish" => publish(app),
        "shared.save" => save(app),
        "shared.open" => {
            let id = selected_id(app, p)?;
            nova_live::documents::valid_id(&id).map_err(|e| e.to_string())?;
            let version = p.get("versionId").and_then(Value::as_str).map(str::to_string);
            let client = Client::current().map_err(|e| e.to_string())?;
            start(app, move || {
                let remote = client.get(&id).map_err(|e| e.to_string())?;
                let (bytes, revision) = client.download(&id, version.as_deref()).map_err(|e| e.to_string())?;
                Ok(Reply::Open { remote, bytes, revision, copy: version.is_some(), profile: client.profile().clone() })
            })
        }
        "shared.members" | "shared.versions" | "shared.comments" => {
            let doc = selected_id(app, p)?;
            let slot = id.trim_start_matches("shared.").to_string();
            let client = Client::current().map_err(|e| e.to_string())?;
            nova_live::documents::valid_id(&doc).map_err(|e| e.to_string())?;
            start(app, move || {
                Ok(Reply::Data(
                    slot.clone(),
                    client.request("GET", &format!("/api/native/documents/{doc}/{slot}"), Value::Null).map_err(|e| e.to_string())?,
                    client_actor(&client),
                ))
            })
        }
        "shared.people" => {
            let query = p.get("q").and_then(Value::as_str).ok_or("Enter a name")?;
            let query = query
                .chars()
                .take(120)
                .flat_map(|c| {
                    let mut bytes = [0u8; 4];
                    c.encode_utf8(&mut bytes).as_bytes().iter().map(|b| format!("%{b:02X}")).collect::<Vec<_>>()
                })
                .collect::<String>();
            let client = Client::current().map_err(|e| e.to_string())?;
            start(app, move || {
                Ok(Reply::Data(
                    "people".into(),
                    client.request("GET", &format!("/api/native/documents/people?q={query}"), Value::Null).map_err(|e| e.to_string())?,
                    client_actor(&client),
                ))
            })
        }
        "shared.member" | "shared.revoke" | "shared.access" | "shared.comment" | "shared.comment.resolve" | "shared.restore" => {
            let doc = selected_id(app, p)?;
            nova_live::documents::valid_id(&doc).map_err(|e| e.to_string())?;
            let client = Client::current().map_err(|e| e.to_string())?;
            let (method, path, body) = match id {
                "shared.member" | "shared.revoke" => {
                    let user = p.get("userId").and_then(Value::as_str).ok_or("Choose a person")?;
                    nova_live::documents::valid_id(user).map_err(|e| e.to_string())?;
                    (
                        if id == "shared.member" { "PUT" } else { "DELETE" },
                        format!("/api/native/documents/{doc}/members/{user}"),
                        json!({"role":p.get("role").and_then(Value::as_str).unwrap_or("viewer")}),
                    )
                }
                "shared.access" => ("PATCH", format!("/api/native/documents/{doc}"), p.clone()),
                "shared.comment" => ("POST", format!("/api/native/documents/{doc}/comments"), p.clone()),
                "shared.comment.resolve" => {
                    let comment = p["commentId"].as_str().ok_or("Choose a comment")?;
                    nova_live::documents::valid_id(comment).map_err(|e| e.to_string())?;
                    ("PATCH", format!("/api/native/documents/{doc}/comments/{comment}"), json!({"resolved":p["resolved"].as_bool().unwrap_or(true)}))
                }
                _ => {
                    let version = p.get("versionId").and_then(Value::as_str).ok_or("Choose a version")?;
                    nova_live::documents::valid_id(version).map_err(|e| e.to_string())?;
                    (
                        "POST",
                        format!("/api/native/documents/{doc}/versions/{version}/restore"),
                        json!({"baseRevision":p.get("baseRevision").and_then(Value::as_u64).unwrap_or(0)}),
                    )
                }
            };
            start(app, move || {
                client.request(method, &path, body).map_err(|e| e.to_string())?;
                Ok(Reply::Done(client_actor(&client)))
            })
        }
        _ => Err("Unknown shared document command".into()),
    })())
}
fn refresh(app: &mut NovaphotoApp) -> Result<Value, String> {
    let client = Client::current().map_err(|e| e.to_string())?;
    start(app, move || Ok(Reply::List(client.list().map_err(|e| e.to_string())?, client_actor(&client))))
}
fn source(app: &NovaphotoApp) -> Result<(DocId, u64, Arc<PhotoDocument>), String> {
    let d = app.session.active().ok_or("Open a document first")?;
    Ok((d.doc.id, d.revision, d.doc.clone()))
}
fn publish(app: &mut NovaphotoApp) -> Result<Value, String> {
    let (local, source_revision, doc) = source(app)?;
    if app.shared.bindings.contains_key(&local) {
        return save(app);
    }
    let client = Client::current().map_err(|e| e.to_string())?;
    start(app, move || {
        let (bytes, _) = novaphoto_format::PcraftWriter::new().save_zip(&doc, &Default::default()).map_err(|e| e.to_string())?;
        let remote = client.create(&doc.name).map_err(|e| e.to_string())?;
        let queue = QueuedSave::new(&remote.id, client.profile(), 0).map_err(|e| e.to_string())?;
        queue.persist(&bytes).map_err(|e| e.to_string())?;
        let result = queue.synchronize(&client);
        let failure = result.as_ref().err().map(|e| e.to_string());
        let uploaded = result.ok();
        Ok(Reply::Saved { local, source_revision, remote, queue, uploaded, failure })
    })
}
pub fn save(app: &mut NovaphotoApp) -> Result<Value, String> {
    let (local, source_revision, doc) = source(app)?;
    let binding = app.shared.bindings.get(&local).cloned().ok_or("This document is not shared permanently")?;
    let actor = profile()?;
    if binding.user != actor.user.id || binding.organization != actor.organization.id {
        return Err("This document belongs to another account; save a local copy".into());
    }
    if binding.remote.role == "viewer" {
        return Err("This document is read-only; save a local copy".into());
    }
    let queue = QueuedSave::new(&binding.remote.id, &actor, binding.remote.revision).map_err(|e| e.to_string())?;
    start(app, move || {
        let (bytes, _) = novaphoto_format::PcraftWriter::new().save_zip(&doc, &Default::default()).map_err(|e| e.to_string())?;
        queue.persist(&bytes).map_err(|e| e.to_string())?;
        let result = Client::current().and_then(|client| queue.synchronize(&client));
        let failure = result.as_ref().err().map(|e| e.to_string());
        let uploaded = result.ok();
        Ok(Reply::Saved { local, source_revision, remote: binding.remote, queue, uploaded, failure })
    })
}
pub fn tick(app: &mut NovaphotoApp, ctx: &egui::Context) {
    app.shared.ctx = Some(ctx.clone());
    let actor = profile().ok().map(|p| (p.organization.id, p.user.id));
    if app.shared.actor != actor {
        crate::live_ui::stop(app);
        app.shared.ticket_document = None;
        app.shared.last_ticket = None;
        app.shared.workspace_revision = 0;
        app.shared.rows.clear();
        app.shared.selected = None;
        app.shared.data.clear();
        app.shared.pending = profile().ok().and_then(|p| nova_live::documents::pending(&p).ok()).unwrap_or_default();
        app.shared.actor = actor;
    }
    let result = app.shared.rx.as_ref().and_then(|rx| rx.try_recv().ok());
    if let Some(result) = result {
        app.shared.rx = None;
        let scope_matches = app.shared.request_selection.take() == Some((app.shared.selected.as_ref().map(|d| d.id.clone()), app.shared.library.clone()));
        match result {
            Err(error) => app.shared.error = Some(error),
            Ok(Reply::List(rows, actor)) => {
                if app.shared.actor.as_ref() == Some(&actor) {
                    app.shared.rows = rows;
                    app.shared.notice = None;
                }
            }
            Ok(Reply::Data(slot, data, actor)) => {
                if scope_matches && app.shared.actor.as_ref() == Some(&actor) {
                    app.shared.data.insert(slot, data);
                }
            }
            Ok(Reply::Asset { local, name, bytes, actor }) => {
                if app.shared.actor.as_ref() == Some(&actor)
                    && let Err(error) = app.with_document(local, move |app| {
                        novaphoto_engine::file_cmds::place_bytes(&mut app.session, &name, bytes, None, &json!({})).map(|_| ()).map_err(|e| e.to_string())
                    })
                {
                    app.shared.error = Some(error);
                }
            }
            Ok(Reply::Workspace { value, revision, actor }) => {
                if app.shared.actor.as_ref() == Some(&actor) {
                    if let Some(workspaces) = value.get("workspaces") {
                        match serde_json::from_value(workspaces.clone()) {
                            Ok(workspaces) => {
                                app.session.prefs.edit(|p| p.workspaces = workspaces);
                                app.shared.workspace_revision = revision;
                                app.shared.notice = Some(tl!("Workspaces downloaded").into());
                            }
                            Err(_) => app.shared.error = Some("Invalid workspace data".into()),
                        }
                    } else {
                        app.shared.workspace_revision = revision;
                        app.shared.notice = Some(tl!("No synced workspaces yet").into());
                    }
                }
            }
            Ok(Reply::WorkspaceSaved { revision, actor }) => {
                if app.shared.actor.as_ref() == Some(&actor) {
                    app.shared.workspace_revision = revision;
                    app.shared.notice = Some(tl!("Workspaces uploaded").into());
                }
            }
            Ok(Reply::Ticket { local, payload, signature, actor }) => {
                if app.shared.actor.as_ref() == Some(&actor) && app.session.active().is_some_and(|d| d.doc.id == local) {
                    if let Some(claims) = nova_live::document_ticket::decode(&payload) {
                        let role_changed = app.shared.bindings.get(&local).is_some_and(|b| b.remote.role != claims.role);
                        if role_changed {
                            crate::live_ui::stop(app);
                            app.shared.ticket_document = None;
                        }
                        if let Some(binding) = app.shared.bindings.get_mut(&local) {
                            binding.remote.role = claims.role.clone();
                        }
                        if let Some(doc) = app.session.active_mut() {
                            doc.edit_read_only = claims.role == "viewer";
                        }
                    }
                    if app.shared.ticket_document == Some(local) && app.live.active() {
                        if let Some(live) = app.live.live.as_mut() {
                            live.renew_document_ticket(&payload, &signature);
                        }
                    } else {
                        crate::live_ui::stop(app);
                        match nova_live::Live::document(nova_live::DEFAULT_SERVER, &payload, &signature) {
                            Ok(live) => {
                                if app.session.live_start().is_ok() {
                                    app.live.live = Some(live);
                                    app.shared.ticket_document = Some(local);
                                }
                            }
                            Err(error) => app.shared.error = Some(error.to_string()),
                        }
                    }
                }
            }
            Ok(Reply::Done(actor)) => {
                if app.shared.actor.as_ref() == Some(&actor) {
                    app.shared.notice = Some(tl!("Changes saved").into());
                    app.shared.data.clear();
                }
            }
            Ok(Reply::Open { mut remote, bytes, revision, copy, profile: actor }) => {
                if app.shared.actor.as_ref() != Some(&(actor.organization.id.clone(), actor.user.id.clone())) {
                    app.shared.error = Some("The account changed during the operation".into());
                } else {
                    match app.open_bytes(&format!("{}.pcraft", remote.name), &bytes) {
                        Ok(_) => {
                            if let Some(d) = app.session.active_mut() {
                                d.saved_revision = d.revision;
                                if !copy {
                                    remote.revision = revision;
                                    d.edit_read_only = remote.role == "viewer";
                                    app.shared.bindings.insert(
                                        d.doc.id,
                                        Binding {
                                            remote: remote.clone(),
                                            organization: actor.organization.id,
                                            user: actor.user.id,
                                            pending: None,
                                            conflict: false,
                                        },
                                    );
                                    app.shared.selected = Some(remote);
                                }
                            }
                        }
                        Err(error) => app.shared.error = Some(error),
                    }
                }
            }
            Ok(Reply::Saved { local, source_revision, mut remote, queue, uploaded, failure }) => {
                if app.shared.actor.as_ref() == Some(&(queue.organization_id.clone(), queue.user_id.clone())) {
                    if let Some(revision) = uploaded {
                        remote.revision = revision;
                    }
                    app.shared.bindings.insert(
                        local,
                        Binding {
                            remote: remote.clone(),
                            organization: queue.organization_id.clone(),
                            user: queue.user_id.clone(),
                            pending: uploaded.is_none().then_some(queue),
                            conflict: failure.as_ref().is_some_and(|e| e.starts_with("CONFLICT:")),
                        },
                    );
                    app.shared.selected = Some(remote);
                    let _ = app.with_document(local, |app| {
                        if let Some(d) = app.session.active_mut() {
                            d.saved_revision = source_revision;
                        }
                        Ok(())
                    });
                    app.ui.status =
                        if uploaded.is_some() { tl!("Shared version saved").into() } else { tl!("Saved on this computer — upload pending").into() };
                    app.shared.notice = Some(app.ui.status.clone());
                    if let Some(error) = failure {
                        if error.starts_with("CONFLICT:") {
                            app.shared.open = true;
                        }
                        app.shared.error = Some(error);
                    }
                }
            }
        }
    }
    // Short leases are renewed through the account API; a revoked member cannot renew.
    let current = app.session.active().map(|d| d.doc.id);
    if app.shared.ticket_document.is_some() && app.shared.ticket_document != current {
        crate::live_ui::stop(app);
        app.shared.ticket_document = None;
        app.shared.last_ticket = None;
    }
    if let Some(local) = current
        && app.shared.bindings.contains_key(&local)
        && app.shared.rx.is_none()
        && app.shared.last_ticket.is_none_or(|t| t.elapsed() > Duration::from_secs(if app.live.active() { 240 } else { 30 }))
        && let Ok(client) = Client::current()
    {
        let binding = app.shared.bindings[&local].clone();
        if app.shared.actor.as_ref() == Some(&(binding.organization.clone(), binding.user.clone())) {
            app.shared.last_ticket = Some(Instant::now());
            let _ = start(app, move || {
                let value = client.request("POST", &format!("/api/native/documents/{}/ticket", binding.remote.id), Value::Null).map_err(|e| e.to_string())?;
                let payload = value["payload"].as_str().ok_or("Invalid document ticket")?.to_string();
                let signature = value["signature"].as_str().ok_or("Invalid document ticket")?.to_string();
                Ok(Reply::Ticket { local, payload, signature, actor: (binding.organization, binding.user) })
            });
        }
    }
    if app.shared.rx.is_some() {
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    if app.shared.last_sync.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
        app.shared.last_sync = Some(Instant::now());
        if app.shared.rx.is_none()
            && let Some((local, binding)) = app.shared.bindings.iter().find(|(_, b)| b.pending.is_some() && !b.conflict).map(|(id, b)| (*id, b.clone()))
            && let (Some(queue), Ok(client)) = (binding.pending.clone(), Client::current())
        {
            let source_revision = app.session.documents().iter().find(|d| d.doc.id == local).map_or(0, |d| d.saved_revision);
            let _ = start(app, move || {
                let result = queue.synchronize(&client);
                let failure = result.as_ref().err().map(|e| e.to_string());
                let uploaded = result.ok();
                Ok(Reply::Saved { local, source_revision, remote: binding.remote, queue, uploaded, failure })
            });
        }
    }
}
pub fn window(app: &mut NovaphotoApp, ctx: &egui::Context) {
    if !app.shared.open {
        return;
    }
    let mut open = true;
    egui::Window::new(tl!("Shared with Me")).open(&mut open).default_width(720.0)
        .max_height((ctx.content_rect().height()-48.0).max(160.0)).vscroll(true)
        .constrain_to(ctx.content_rect()).show(ctx,|ui| {
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui,tl!("Refresh"),100.0).clicked()&& let Err(e)=refresh(app){app.shared.error=Some(e);}
            if app.session.active().is_some()&&widgets::primary_button(ui,tl!("Share Permanently"),200.0).clicked()&& let Err(e)=publish(app){app.shared.error=Some(e);}
            if app.shared.rx.is_some(){ui.spinner();ui.label(tl!("Loading…"));}
        });
        if let Some(error)=&app.shared.error {ui.label(error);}
        if let Some(notice)=&app.shared.notice {ui.label(notice);}
        if let Some(binding)=app.session.active().and_then(|d|app.shared.bindings.get(&d.doc.id)).filter(|b|b.conflict).cloned() {
            ui.horizontal(|ui| {
                if ui.button(tl!("Open Current Version")).clicked()&& let Some(Err(e))=command(app,"shared.open",&json!({"id":binding.remote.id})) {app.shared.error=Some(e);}
                if ui.button(tl!("Save Local Copy")).clicked()&& let Err(e)=app.save_as(None) {app.shared.error=Some(e);}
            });
        }
        ui.collapsing(tl!("Brand Library"),|ui| {
            if ui.button(tl!("Load Authorized Libraries")).clicked()&& let Some(Err(e))=command(app,"shared.library",&json!({})){app.shared.error=Some(e);}
            if let Some(brands)=app.shared.data.get("brands").and_then(Value::as_array).cloned(){
                ui.horizontal_wrapped(|ui| {for brand in brands {if ui.button(brand["name"].as_str().unwrap_or("")).clicked()&& let Some(Err(e))=command(app,"shared.library.open",&json!({"clientId":brand["id"]})){app.shared.error=Some(e);}}});
            }
            if let Some(assets)=app.shared.data.get("assets").and_then(Value::as_array).cloned(){for asset in assets {
                ui.horizontal_wrapped(|ui| {
                    ui.label(asset["name"].as_str().unwrap_or(""));
                    match asset["kind"].as_str() {
                        Some("color")=>if let Some(hex)=asset["data"]["hex"].as_str()&& ui.button(hex).clicked()&& let Err(e)=app.run("tools.setColors",json!({"foreground":hex})){app.shared.error=Some(e);},
                        Some("font")=>if let Some(family)=asset["data"]["family"].as_str(){ui.label(family);if ui.button(tl!("Copy Font Name")).clicked(){ctx.copy_text(family.into());}},
                        _=>if !asset["file"].is_null()&&ui.button(tl!("Insert as Smart Object")).clicked(){let client=app.shared.library.clone();if let Some(Err(e))=command(app,"shared.library.insert",&json!({"clientId":client,"assetId":asset["id"]})){app.shared.error=Some(e);}},
                    }
                });
            }}
        });
        ui.collapsing(tl!("Sync Workspaces"),|ui| {
            ui.label(tl!("Saved layouts sync across your account. Monitor positions stay on this computer."));
            ui.horizontal_wrapped(|ui| {for (id,label) in [("shared.workspace.pull","Download Workspaces"),("shared.workspace.push","Upload Workspaces")] {
                if ui.button(tl!(label)).clicked()&& let Some(Err(e))=command(app,id,&json!({})){app.shared.error=Some(e);}
            }});
        });
        crate::live_ui::participants(app,ui);
        ui.separator();
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-180.0).max(120.0)).show(ui,|ui| {
            if app.shared.rows.is_empty()&&app.shared.rx.is_none(){ui.label(tl!("No permanent documents yet. Share an open document to get started."));}
            for remote in app.shared.rows.clone() {
                ui.horizontal(|ui| {
                    ui.label(&remote.name);ui.label(if remote.role=="viewer"{tl!("View Only")}else{tl!("Can Edit")});
                    if ui.button(tl!("Open")).clicked()&& let Some(Err(e))=command(app,"shared.open",&json!({"id":remote.id})){app.shared.error=Some(e);}
                    if ui.button(tl!("Manage Access")).clicked() {app.shared.selected=Some(remote.clone());app.shared.data.clear();}
                });
                ui.separator();
            }
            if let Some(remote)=app.shared.selected.clone() {
                ui.heading(&remote.name);
                if ui.button(tl!("Copy Link")).clicked(){ctx.copy_text(remote.link.clone());}
                ui.horizontal(|ui| {for (slot,label) in [("members","Access"),("versions","Versions"),("comments","Comments")] {
                    if ui.button(tl!(label)).clicked()&& let Some(Err(e))=command(app,&format!("shared.{slot}"),&json!({"id":remote.id})){app.shared.error=Some(e);}
                }});
                if remote.role=="owner" {
                    let mut visibility=remote.visibility.clone();
                    widgets::dropdown(ui,"native-visibility",&mut visibility,&[("invited".into(),tl!("Invited People")),("department".into(),tl!("Department")),("organization".into(),tl!("Organization"))],240.0);
                    let mut scope_role=remote.scope_role.clone();
                    widgets::dropdown(ui,"native-scope-role",&mut scope_role,&[("viewer".into(),tl!("View Only")),("editor".into(),tl!("Can Edit"))],180.0);
                    let mut department=remote.department_id.clone().unwrap_or_default();
                    if visibility=="department"
                        && let Ok(profile)=profile() {
                            let options=profile.departments.iter().map(|d|(d.id.clone(),d.name.as_str())).collect::<Vec<_>>();
                            widgets::dropdown(ui,"native-share-department",&mut department,&options,240.0);
                        }
                    if let Some(selected)=app.shared.selected.as_mut() {selected.visibility=visibility.clone();selected.scope_role=scope_role.clone();selected.department_id=(!department.is_empty()).then_some(department.clone());}
                    if ui.button(tl!("Apply Access")).clicked()&& let Some(Err(e))=command(app,"shared.access",&json!({"id":remote.id,"visibility":visibility,"scopeRole":scope_role,"departmentId":if department.is_empty(){Value::Null}else{json!(department)}})){app.shared.error=Some(e);}
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut app.shared.query).hint_text(tl!("Find a person")));
                        if ui.button(tl!("Search")).clicked(){let q=app.shared.query.clone();if let Some(Err(e))=command(app,"shared.people",&json!({"q":q})){app.shared.error=Some(e);}}
                    });
                    if app.shared.role.is_empty(){app.shared.role="viewer".into();}
                    widgets::dropdown(ui,"invite-role",&mut app.shared.role,&[("viewer".into(),tl!("View Only")),("editor".into(),tl!("Can Edit"))],180.0);
                    if let Some(people)=app.shared.data.get("people").and_then(Value::as_array).cloned(){for person in people {
                        ui.horizontal(|ui| {ui.label(person.get("name").and_then(Value::as_str).unwrap_or(""));
                            if ui.button(tl!("Invite")).clicked(){let role=app.shared.role.clone();if let Some(Err(e))=command(app,"shared.member",&json!({"id":remote.id,"userId":person["id"],"role":role})){app.shared.error=Some(e);}}
                        });
                    }}
                }
                if let Some(members)=app.shared.data.get("members").and_then(Value::as_array).cloned(){for member in members {
                    ui.horizontal(|ui| {ui.label(member["name"].as_str().unwrap_or(""));ui.label(match member["role"].as_str(){Some("owner")=>tl!("Owner"),Some("editor")=>tl!("Can Edit"),_=>tl!("View Only")});
                        if remote.role=="owner"&&member["role"]!="owner"&&ui.button(tl!("Remove Access")).clicked()&& let Some(Err(e))=command(app,"shared.revoke",&json!({"id":remote.id,"userId":member["userId"]})){app.shared.error=Some(e);}
                    });
                }}
                if let Some(versions)=app.shared.data.get("versions").and_then(Value::as_array).cloned(){for version in versions {
                    ui.horizontal(|ui| {ui.label(format!("{} {}",tl!("Version"),version["revision"]));
                        if ui.button(tl!("Open a Copy")).clicked()&& let Some(Err(e))=command(app,"shared.open",&json!({"id":remote.id,"versionId":version["id"]})){app.shared.error=Some(e);}
                        if remote.role!="viewer"&&ui.button(tl!("Restore as New Version")).clicked()&& let Some(Err(e))=command(app,"shared.restore",&json!({"id":remote.id,"versionId":version["id"],"baseRevision":remote.revision})){app.shared.error=Some(e);}
                    });
                }}
                if let Some(comments)=app.shared.data.get("comments").and_then(Value::as_array).cloned(){for comment in comments {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(comment["body"].as_str().unwrap_or(""));
                        let can_resolve=remote.role!="viewer"||profile().ok().is_some_and(|p|comment["authorId"].as_str()==Some(p.user.id.as_str()));
                        if can_resolve&&ui.button(if comment["resolvedAt"].is_null(){tl!("Resolve Comment")}else{tl!("Reopen Comment")}).clicked()
                            &&let Some(Err(e))=command(app,"shared.comment.resolve",&json!({"id":remote.id,"commentId":comment["id"],"resolved":comment["resolvedAt"].is_null()})){app.shared.error=Some(e);}
                    });
                }}
                ui.add(egui::TextEdit::multiline(&mut app.shared.comment).hint_text(tl!("Comment at the current view center")));
                let can_anchor=app.session.active().and_then(|d|app.shared.bindings.get(&d.doc.id)).is_some_and(|b|b.remote.id==remote.id)&&!app.shared.comment.trim().is_empty();
                if ui.add_enabled(can_anchor,egui::Button::new(tl!("Add Comment"))).clicked(){
                    let center=app.session.active_index().and_then(|i|app.ui.views.get(i)).map_or([0.0,0.0],|v|v.center);
                    let body=app.shared.comment.clone();if let Some(Err(e))=command(app,"shared.comment",&json!({"id":remote.id,"body":body,"anchor":{"x":center[0].max(0.0),"y":center[1].max(0.0),"revision":remote.revision}})){app.shared.error=Some(e);}
                }
            }
            if !app.shared.pending.is_empty() {
                ui.separator();ui.heading(tl!("Pending Local Copies"));
                for queue in app.shared.pending.clone(){
                    if ui.button(format!("{}: {}",tl!("Open a Copy"),queue.document_id)).clicked(){
                        match queue.bytes().map_err(|e|e.to_string()).and_then(|bytes|app.open_bytes("Recovered.pcraft",&bytes)){Ok(_)=>{},Err(e)=>app.shared.error=Some(e)}
                    }
                }
            }
        });
    });
    app.shared.open = open;
}

/// Comments stay in document coordinates, clipped to the active canvas rather than to a tab index.
pub fn paint_comments(app: &mut NovaphotoApp, ctx: &egui::Context) {
    if app.shared.open || app.agent.open || app.live.open || !app.ui.dialogs.is_empty() {
        return;
    }
    let Some(remote) = app.session.active().and_then(|d| app.shared.bindings.get(&d.doc.id)).map(|b| &b.remote) else {
        return;
    };
    if app.shared.selected.as_ref().is_none_or(|d| d.id != remote.id) {
        return;
    }
    let Some(xf) = crate::canvas::ViewXform::active(app) else {
        return;
    };
    let Some(comments) = app.shared.data.get("comments").and_then(Value::as_array).cloned() else {
        return;
    };
    for (number, comment) in comments.into_iter().filter(|c| c["resolvedAt"].is_null()).enumerate() {
        let (Some(x), Some(y)) = (comment["anchor"]["x"].as_f64(), comment["anchor"]["y"].as_f64()) else {
            continue;
        };
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let at = xf.to_screen(x as f32, y as f32);
        if !xf.rect.contains(at) {
            continue;
        }
        let id = comment["id"].as_str().unwrap_or("");
        egui::Area::new(egui::Id::new(("shared-comment-pin", id))).fixed_pos(at).movable(false).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.set_clip_rect(xf.rect);
            if ui.add_sized(egui::vec2(24.0, 24.0), egui::Button::new((number + 1).to_string())).on_hover_text(comment["body"].as_str().unwrap_or("")).clicked()
            {
                app.shared.open = true;
            }
        });
    }
}
