//! Optional local agent connection. Conversation and provider authentication stay in the client.
use crate::{ControlRequest, NovaphotoApp, Services, widgets};
use std::sync::mpsc::Receiver;

pub struct Connection {
    pub receiver: Receiver<ControlRequest>,
    pub services: Box<Services>,
    pub endpoint: String,
    pub token_file: String,
    pub cli: String,
    pub guard: Box<dyn std::any::Any>,
    pub probe: Box<dyn Fn() -> Receiver<Result<(), String>>>,
}
pub type ConnectFn = Box<dyn Fn(&egui::Context, &str) -> Result<Connection, String>>;
#[derive(Default)]
pub struct AgentUi {
    pub open: bool,
    pub root: String,
    pub activity: String,
    pub error: Option<String>,
    pub connection: Option<Connection>,
    pub probe: Option<Receiver<Result<(), String>>>,
}
pub fn disconnect(app: &mut NovaphotoApp) {
    if app.agent.connection.take().is_none() {
        return;
    }
    app.control_rx = None;
    app.services.automation_read = None;
    app.services.automation_write = None;
    app.services.automation_write_new = None;
    app.services.automation_carousel = None;
    app.services.automation_open_pcraft = None;
    app.services.automation_save = None;
    app.services.automation_command = None;
    app.services.automation_authorize = None;
    app.agent.activity = tl!("Disconnected").into();
    app.agent.probe = None;
}
fn connect(app: &mut NovaphotoApp, ctx: &egui::Context) -> Result<(), String> {
    if app.control_rx.is_some() {
        return Err(tl!("An external control session is already active").into());
    }
    let factory = app.services.agent_connect.as_ref().ok_or(tl!("Agent connection is available in the desktop app"))?;
    let mut connection = factory(ctx, &app.agent.root)?;
    app.services.automation_read = connection.services.automation_read.take();
    app.services.automation_write = connection.services.automation_write.take();
    app.services.automation_write_new = connection.services.automation_write_new.take();
    app.services.automation_carousel = connection.services.automation_carousel.take();
    app.services.automation_open_pcraft = connection.services.automation_open_pcraft.take();
    app.services.automation_save = connection.services.automation_save.take();
    app.services.automation_command = connection.services.automation_command.take();
    app.services.automation_authorize = connection.services.automation_authorize;
    // The receiver is moved into the app. Keep a disconnected placeholder in the owned session.
    let (_, empty) = std::sync::mpsc::channel();
    app.control_rx = Some(std::mem::replace(&mut connection.receiver, empty));
    app.agent.connection = Some(connection);
    app.agent.activity = tl!("Waiting for agent").into();
    Ok(())
}
pub fn window(app: &mut NovaphotoApp, ctx: &egui::Context) {
    if !app.agent.open {
        return;
    }
    if let Some(receiver) = &app.agent.probe {
        if let Ok(result) = receiver.try_recv() {
            match result {
                Ok(()) => {
                    app.agent.error = None;
                    app.agent.activity = tl!("Connection verified").into();
                }
                Err(e) => app.agent.error = Some(e),
            }
            app.agent.probe = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    let mut open = true;
    egui::Window::new(tl!("Connect Agent"))
        .open(&mut open)
        .default_width(480.0)
        .max_height((ctx.content_rect().height() - 48.0).max(160.0))
        .vscroll(true)
        .constrain_to(ctx.content_rect())
        .show(ctx, |ui| {
            ui.label(tl!("Use your agent to operate Nova Photo. Conversation stays in Claude Code or Codex."));
            ui.add_space(8.0);
            if app.agent.connection.is_none() {
                ui.label(tl!("Authorized Workspace"));
                ui.add(egui::TextEdit::singleline(&mut app.agent.root).desired_width(440.0));
                if widgets::secondary_button(ui, tl!("Choose Folder…"), 160.0).clicked()
                    && let Err(e) = app.ask_folder(|app, path| {
                        app.agent.root = path;
                        Ok(serde_json::Value::Null)
                    })
                {
                    app.agent.error = Some(e);
                }
                ui.label(tl!("Only files in this folder are available for reading and export."));
                if widgets::primary_button(ui, tl!("Enable Connection"), 180.0).clicked() {
                    app.agent.error = connect(app, ctx).err();
                }
            } else {
                ui.label(format!("{}: {}", tl!("Activity"), app.agent.activity));
                if let Some(c) = &mut app.agent.connection {
                    ui.label(tl!("CLI executable"));
                    ui.add(egui::TextEdit::singleline(&mut c.cli).desired_width(440.0));
                    let args = vec!["mcp".to_string(), "--bridge".into(), c.endpoint.clone(), "--control-token-file".into(), c.token_file.clone()];
                    let json = serde_json::json!({"mcpServers":{"nova-photo":{"type":"stdio","command":c.cli,"args":args}}});
                    if widgets::secondary_button(ui, tl!("Copy Claude Code Configuration"), 280.0).clicked() {
                        ctx.copy_text(serde_json::to_string_pretty(&json).unwrap_or_default());
                    }
                    let quoted = |s: &str| serde_json::to_string(s).unwrap_or_default();
                    let toml = format!(
                        "[mcp_servers.nova-photo]\ncommand = {}\nargs = [{}]\n",
                        quoted(&c.cli),
                        args.iter().map(|s| quoted(s)).collect::<Vec<_>>().join(", ")
                    );
                    if widgets::secondary_button(ui, tl!("Copy Codex Configuration"), 280.0).clicked() {
                        ctx.copy_text(toml);
                    }
                    if widgets::secondary_button(ui, tl!("Test Connection"), 180.0).clicked() {
                        app.agent.probe = Some((c.probe)());
                    }
                }
                ui.label(tl!("Add the configuration in your agent, then ask it to inspect the open document."));
                if widgets::secondary_button(ui, tl!("Disconnect Agent"), 180.0).clicked() {
                    disconnect(app);
                }
            }
            if let Some(error) = &app.agent.error {
                ui.colored_label(crate::theme::Tokens::get(ui.ctx()).text, error);
            }
        });
    app.agent.open = open;
}
