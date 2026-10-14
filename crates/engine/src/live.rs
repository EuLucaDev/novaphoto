//! Colaboração em tempo real (Nova Live) no motor do Nova Photo.
//!
//! - Comandos do diário (pinceladas, filtros, camadas…) viram [`LiveChange::Op`] com a camada
//!   ativa e as camadas selecionadas de quem editou: o colega roda o mesmo comando, que redesenha
//!   os mesmos pixels (os comandos guardam no diário tudo o que precisam para repetir igual).
//! - Qualquer outra mudança (desfazer, refazer, trabalhos em segundo plano, gestos) vira
//!   [`LiveChange::Replace`]: o documento inteiro, no formato nativo.
//! - Camada ativa, seleção de camadas e histórico continuam de cada pessoa.

use std::sync::Arc;

use novaphoto_doc::{DocId, Document, LayerId};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

#[derive(Clone, Debug, PartialEq)]
pub enum LiveChange {
    Op { cmd: String, params: Value, ctx: Value },
    Replace,
}

#[derive(Default)]
pub struct LiveState {
    /// Identidade do documento compartilhado, independente da posição da aba.
    pub doc: Option<DocId>,
    pub outbox: Vec<LiveChange>,
    pub(crate) applying: bool,
    /// Revisão já informada à sessão.
    pub(crate) seen: u64,
    /// Comandos rodando agora (comandos que chamam comandos): só o de fora é replicado.
    pub(crate) depth: u32,
}

fn err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("colaboração: {e}"))
}

/// Impressão digital estável (FNV-1a 64).
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// Only audited document edits may execute on a peer. Other local edits travel as data.
fn remote_command_allowed(cmd: &str) -> bool {
    matches!(cmd, "layer.new.layer" | "edit.fill")
}

/// O desfazer guarda fotos da imagem: depois de uma alteração de colega, desfazer o passo
/// anterior apagaria o trabalho dele. O histórico local recomeça daqui (mesmos limites).
fn fresh_history(st: &mut crate::DocState) {
    let mut h = novaphoto_ops::History::new(st.history.max_states);
    h.max_bytes = st.history.max_bytes;
    st.history = h;
}

impl Session {
    fn live_index(&self) -> Result<usize> {
        let id = self.live.doc.ok_or_else(|| err("nenhuma sessão"))?;
        self.documents().iter().position(|d| d.doc.id == id).ok_or_else(|| err("a imagem compartilhada foi fechada"))
    }

    pub fn live_start(&mut self) -> Result<()> {
        let i = self.active_index().ok_or_else(|| err("abra uma imagem"))?;
        let rev = self.documents().get(i).map(|d| d.revision).unwrap_or_default();
        let id = self.documents().get(i).ok_or_else(|| err("sem imagem"))?.doc.id;
        self.live = LiveState { doc: Some(id), seen: rev, ..LiveState::default() };
        Ok(())
    }

    pub fn live_stop(&mut self) {
        self.live = LiveState::default();
    }

    pub fn live_active(&self) -> bool {
        self.live_index().is_ok()
    }

    pub fn live_take(&mut self) -> Vec<LiveChange> {
        if let Ok(i) = self.live_index()
            && let Some(d) = self.documents().get(i)
            && d.revision != self.live.seen
        {
            self.live.seen = d.revision;
            self.live.outbox.push(LiveChange::Replace);
        }
        std::mem::take(&mut self.live.outbox)
    }

    pub fn live_busy(&self) -> bool {
        false
    }

    pub fn live_snapshot(&self) -> Result<Vec<u8>> {
        let d = self.documents().get(self.live_index()?).ok_or_else(|| err("sem imagem"))?;
        novaphoto_format::save_to_bytes(&d.doc, &novaphoto_format::SaveOptions::default()).map_err(err)
    }

    /// Impressão digital do conteúdo: o índice do arquivo sem os ids internos (que são de cada
    /// computador); os pixels entram pelos hashes de conteúdo que o índice lista.
    pub fn live_digest(&self) -> Result<u64> {
        let bytes = self.live_snapshot()?;
        let manifest = novaphoto_format::read_manifest(&bytes).map_err(err)?;
        let mut v = serde_json::to_value(&manifest).map_err(err)?;
        strip_ids(&mut v);
        Ok(fnv64(&serde_json::to_vec(&v).map_err(err)?))
    }

    pub fn live_load(&mut self, bytes: &[u8]) -> Result<()> {
        let i = self.live_index()?;
        let doc = novaphoto_format::load_from_bytes(bytes).map_err(err)?;
        let received_id = doc.id;
        let prev = self.active_index();
        self.set_active(i);
        let st = self.active_mut().ok_or_else(|| err("sem imagem"))?;
        st.doc = Arc::new(doc);
        st.revision += 1;
        fresh_history(st);
        keep_valid(st);
        let rev = st.revision;
        self.live.seen = rev;
        self.live.doc = Some(received_id);
        if let Some(p) = prev {
            self.set_active(p);
        }
        Ok(())
    }

    /// Roda o comando de um colega com a camada dele; desfazer e camadas locais ficam como estavam.
    pub fn live_apply(&mut self, cmd: &str, params: &Value, ctx: &Value) -> Result<()> {
        if !remote_command_allowed(cmd) {
            return Err(err("comando não permitido na colaboração"));
        }
        let i = self.live_index()?;
        let prev = self.active_index();
        self.set_active(i);
        let (local_active, local_selected, doc) = {
            let d = self.active().ok_or_else(|| err("sem imagem"))?;
            (d.active_layer, d.selected_layers.clone(), d.doc.clone())
        };
        // Camadas chegam pela posição na pilha (os ids internos são de cada computador).
        let params = decode(&doc, params);
        if let Some(st) = self.active_mut() {
            st.active_layer = ctx.get("active").and_then(Value::as_u64).and_then(|i| layer_at(&doc, i as usize));
            st.selected_layers = ctx
                .get("selected")
                .and_then(Value::as_array)
                .map(|v| v.iter().filter_map(|i| i.as_u64().and_then(|i| layer_at(&doc, i as usize))).collect())
                .unwrap_or_default();
        }
        self.live.applying = true;
        let r = self.execute(cmd, params);
        self.live.applying = false;
        if let Some(st) = self.active_mut() {
            fresh_history(st);
            st.active_layer = local_active;
            st.selected_layers = local_selected;
            keep_valid(st);
            let rev = st.revision;
            self.live.seen = rev;
        }
        if let Some(p) = prev {
            self.set_active(p);
        }
        r.map(|_| ())
    }

    /// Chamado pelo despacho depois de cada comando de fora.
    pub(crate) fn live_record(&mut self, id: &str, params: &Value, journal: bool, ctx: Value) {
        if self.live.applying {
            return;
        }
        let Ok(i) = self.live_index() else { return };
        let Some(d) = self.documents().get(i) else { return };
        let rev = d.revision;
        if rev == self.live.seen {
            return;
        }
        if journal && remote_command_allowed(id) && !matches!(id, "edit.undo" | "edit.redo") {
            self.live.outbox.push(LiveChange::Op { cmd: id.to_string(), params: params.clone(), ctx });
            self.live.seen = rev;
        }
    }
}

fn strip_ids(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.retain(|k, _| !(k == "id" || k == "ids" || k.ends_with("_id") || k.ends_with("Id") || k.ends_with("_ids") || k.ends_with("Ids")));
            map.values_mut().for_each(strip_ids);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_ids),
        _ => {}
    }
}

/// Camada ativa e selecionadas continuam existindo depois de mudanças de outra pessoa.
fn keep_valid(st: &mut crate::DocState) {
    let doc = st.doc.clone();
    if st.active_layer.is_some_and(|l| doc.layer(l).is_none()) {
        st.active_layer = None;
    }
    st.selected_layers.retain(|l| doc.layer(*l).is_some());
}

/// Contexto de quem edita: camada ativa e selecionadas pela posição na pilha de camadas.
pub(crate) fn context(doc: &Document, active: Option<LayerId>, selected: &[LayerId]) -> Value {
    json!({
        "active": active.and_then(|l| layer_index(doc, l)),
        "selected": selected.iter().filter_map(|l| layer_index(doc, *l)).collect::<Vec<_>>(),
    })
}

fn layer_index(doc: &Document, id: LayerId) -> Option<usize> {
    doc.walk().iter().position(|(_, _, l)| l.id == id)
}

fn layer_at(doc: &Document, i: usize) -> Option<LayerId> {
    doc.walk().get(i).map(|(_, _, l)| l.id)
}

/// Parâmetros para enviar: todo número que é o id de uma camada vira `{"$layer": posição}`.
pub(crate) fn encode(doc: &Document, params: &Value) -> Value {
    match params {
        Value::Number(n) => match n.as_u64().and_then(|id| layer_index(doc, LayerId(id))) {
            Some(i) => json!({ "$layer": i }),
            None => params.clone(),
        },
        Value::Array(items) => Value::Array(items.iter().map(|v| encode(doc, v)).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), encode(doc, v))).collect()),
        other => other.clone(),
    }
}

/// Parâmetros recebidos: `{"$layer": posição}` volta a ser o id local da camada nessa posição.
fn decode(doc: &Document, params: &Value) -> Value {
    match params {
        Value::Object(map) if map.len() == 1 && map.contains_key("$layer") => {
            match map.get("$layer").and_then(Value::as_u64).and_then(|i| layer_at(doc, i as usize)) {
                Some(id) => json!(id.0),
                None => Value::Null,
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| decode(doc, v)).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), decode(doc, v))).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 64, "height": 48})).unwrap();
        s
    }

    #[test]
    fn remote_commands_cannot_open_export_or_execute_code() {
        let (_, mut b) = pair();
        let before = b.live_digest().unwrap();
        for command in ["file.open", "file.saveAs", "file.export", "session.open", "script.run", "app.quit", "lisp.eval"] {
            assert!(b.live_apply(command, &serde_json::json!({"path": "/tmp/not-authorized"}), &serde_json::json!({})).is_err());
            assert_eq!(before, b.live_digest().unwrap());
        }
    }

    #[test]
    fn closing_a_shared_tab_never_shares_the_next_file() {
        let mut s = session();
        s.live_start().unwrap();
        s.execute("file.new", json!({"width": 32, "height": 32})).unwrap();
        s.close(0);
        assert!(!s.live_active());
        assert!(s.live_snapshot().is_err());
        assert!(s.live_take().is_empty());
    }

    fn pair() -> (Session, Session) {
        let mut a = session();
        a.live_start().unwrap();
        let mut b = session();
        b.live_start().unwrap();
        let snap = a.live_snapshot().unwrap();
        b.live_load(&snap).unwrap();
        (a, b)
    }

    fn sync(a: &mut Session, b: &mut Session) {
        for change in a.live_take() {
            match change {
                LiveChange::Op { cmd, params, ctx } => b.live_apply(&cmd, &params, &ctx).unwrap(),
                LiveChange::Replace => {
                    let snap = a.live_snapshot().unwrap();
                    b.live_load(&snap).unwrap();
                }
            }
        }
        assert!(b.live_take().is_empty(), "o que veio de A não volta para A");
    }

    #[test]
    fn layers_and_paint_replicate_and_undo_travels_as_a_document() {
        let (mut a, mut b) = pair();
        a.execute("layer.new.layer", json!({"name": "Produto"})).unwrap();
        a.execute("edit.fill", json!({"color": "#006dff"})).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());

        a.execute("edit.undo", json!({})).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());

        b.execute("layer.new.layer", json!({"name": "Texto"})).unwrap();
        sync(&mut b, &mut a);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());
    }
}
