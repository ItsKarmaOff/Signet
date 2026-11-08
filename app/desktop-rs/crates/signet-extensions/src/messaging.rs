//! `runtime.sendMessage`, `tabs.sendMessage` and `runtime.connect` routing between the contexts of an extension.
//! New code (the Electron app used Chromium's own implementation).
//!
//! The web layer registers every extension context (worker, page, content script) with [`Messaging::open_context`].
//! A message becomes a pending request: it is delivered to every other context that can receive it, and the first context
//! that answers `handled` resolves the sender's call. If nobody has a listener the call fails like Chrome's
//! "Receiving end does not exist". Ports are paired by the first receiver that accepts.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const NO_RECEIVER: &str = "Could not establish connection. Receiving end does not exist.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Worker,
    Page,
    ContentScript,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MsgContext {
    pub id: String,
    pub kind: Kind,
    pub extension_id: String,
    pub tab_id: Option<i64>,
    pub frame_id: i64,
    pub url: String,
}

impl MsgContext {
    /// `runtime.MessageSender`.
    pub fn sender(&self) -> Value {
        let mut s = json!({ "id": self.extension_id, "url": self.url, "frameId": self.frame_id });
        if let Some(t) = self.tab_id {
            s["tab"] = json!({ "id": t, "url": self.url });
            s["tabId"] = json!(t);
        }
        if self.kind != Kind::ContentScript {
            s["origin"] = json!(format!("chrome-extension://{}", self.extension_id));
        }
        s
    }
}

/// Where a message goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Every other context of the extension (`runtime.sendMessage`).
    Extension,
    /// The content scripts of a tab, optionally one frame (`tabs.sendMessage`).
    Tab { tab_id: i64, frame_id: Option<i64> },
}

/// What the web layer must do.
#[derive(Debug, Clone, PartialEq)]
pub enum Delivery {
    /// Run this event in one context.
    ToContext { context_id: String, name: String, args: Vec<Value> },
    /// Complete the sender's call.
    Reply { call_id: u64, result: Result<Value, String> },
}

struct Pending {
    call_id: u64,
    remaining: HashSet<String>,
    had_listener: bool,
}

struct Port {
    name: String,
    opener: String,
    peer: Option<String>,
    candidates: HashSet<String>,
}

#[derive(Default)]
pub struct Messaging {
    contexts: HashMap<String, MsgContext>,
    pending: HashMap<u64, Pending>,
    ports: HashMap<u64, Port>,
    next: u64,
}

impl Messaging {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_context(&mut self, c: MsgContext) {
        self.contexts.insert(c.id.clone(), c);
    }

    /// A context went away: pending requests stop waiting for it and its ports close.
    pub fn close_context(&mut self, id: &str) -> Vec<Delivery> {
        self.contexts.remove(id);
        let mut out = Vec::new();
        let tokens: Vec<u64> = self.pending.keys().copied().collect();
        for t in tokens {
            if let Some(p) = self.pending.get_mut(&t) {
                p.remaining.remove(id);
            }
            out.extend(self.settle(t));
        }
        let ports: Vec<u64> = self.ports.iter().filter(|(_, p)| p.opener == id || p.peer.as_deref() == Some(id)).map(|(k, _)| *k).collect();
        for p in ports {
            out.extend(self.close_port(p, id));
        }
        out
    }

    pub fn contexts_of(&self, extension_id: &str) -> Vec<&MsgContext> {
        self.contexts.values().filter(|c| c.extension_id == extension_id).collect()
    }

    fn recipients(&self, from: &MsgContext, target: &Target) -> Vec<String> {
        let mut ids: Vec<String> = self
            .contexts
            .values()
            .filter(|c| c.extension_id == from.extension_id && c.id != from.id)
            .filter(|c| match target {
                Target::Extension => c.kind != Kind::ContentScript,
                Target::Tab { tab_id, frame_id } => c.kind == Kind::ContentScript && c.tab_id == Some(*tab_id) && frame_id.is_none_or(|f| f == c.frame_id),
            })
            .map(|c| c.id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// `sendMessage` from `from`: returns the deliveries (or an immediate failure reply). `call_id` is the pending call.
    pub fn send(&mut self, from: &str, call_id: u64, message: Value, target: Target) -> Vec<Delivery> {
        let Some(sender) = self.contexts.get(from).cloned() else {
            return vec![Delivery::Reply { call_id, result: Err(NO_RECEIVER.into()) }];
        };
        let to = self.recipients(&sender, &target);
        if to.is_empty() {
            return vec![Delivery::Reply { call_id, result: Err(NO_RECEIVER.into()) }];
        }
        self.next += 1;
        let token = self.next;
        self.pending.insert(token, Pending { call_id, remaining: to.iter().cloned().collect(), had_listener: false });
        to.into_iter().map(|c| Delivery::ToContext { context_id: c, name: "runtime.onMessage".into(), args: vec![message.clone(), sender.sender(), json!(token)] }).collect()
    }

    /// A context answers a delivered message: `handled` is true when a listener took it (`response` is its answer,
    /// possibly delayed), false when it had no listener or ignored it.
    pub fn reply(&mut self, context_id: &str, token: u64, handled: bool, has_listener: bool, response: Value) -> Vec<Delivery> {
        let Some(p) = self.pending.get_mut(&token) else { return vec![] };
        p.had_listener |= has_listener;
        if handled {
            let call_id = p.call_id;
            self.pending.remove(&token);
            return vec![Delivery::Reply { call_id, result: Ok(response) }];
        }
        p.remaining.remove(context_id);
        self.settle(token)
    }

    fn settle(&mut self, token: u64) -> Vec<Delivery> {
        let Some(p) = self.pending.get(&token) else { return vec![] };
        if !p.remaining.is_empty() {
            return vec![];
        }
        let call_id = p.call_id;
        let result = if p.had_listener { Ok(Value::Null) } else { Err(NO_RECEIVER.to_string()) };
        self.pending.remove(&token);
        vec![Delivery::Reply { call_id, result }]
    }

    // ---- ports ----

    /// `runtime.connect`: returns the port id and the `runtime.onConnect` deliveries. The caller's call completes with the id.
    pub fn connect(&mut self, from: &str, name: &str, target: Target) -> Result<(u64, Vec<Delivery>), String> {
        let sender = self.contexts.get(from).cloned().ok_or_else(|| NO_RECEIVER.to_string())?;
        let to = self.recipients(&sender, &target);
        if to.is_empty() {
            return Err(NO_RECEIVER.into());
        }
        self.next += 1;
        let id = self.next;
        self.ports.insert(id, Port { name: name.into(), opener: from.into(), peer: None, candidates: to.iter().cloned().collect() });
        let info = json!({ "portId": id, "name": name, "sender": sender.sender() });
        Ok((id, to.into_iter().map(|c| Delivery::ToContext { context_id: c, name: "runtime.onConnect".into(), args: vec![info.clone()] }).collect()))
    }

    /// A context with an `onConnect` listener takes the port (the first one wins).
    pub fn accept_port(&mut self, context_id: &str, port: u64) -> Vec<Delivery> {
        let Some(p) = self.ports.get_mut(&port) else { return vec![] };
        if p.peer.is_some() || !p.candidates.contains(context_id) {
            return vec![];
        }
        p.peer = Some(context_id.to_string());
        vec![Delivery::ToContext { context_id: p.opener.clone(), name: format!("port.accepted:{port}"), args: vec![] }]
    }

    /// A candidate without a listener declines; the opener learns it when none is left.
    pub fn decline_port(&mut self, context_id: &str, port: u64) -> Vec<Delivery> {
        let Some(p) = self.ports.get_mut(&port) else { return vec![] };
        p.candidates.remove(context_id);
        if p.candidates.is_empty() && p.peer.is_none() {
            let opener = p.opener.clone();
            self.ports.remove(&port);
            return vec![Delivery::ToContext { context_id: opener, name: format!("port.onDisconnect:{port}"), args: vec![json!({ "message": NO_RECEIVER })] }];
        }
        vec![]
    }

    /// `port.postMessage` from `from`: delivered to the other end.
    pub fn post(&mut self, from: &str, port: u64, message: Value) -> Vec<Delivery> {
        let Some(p) = self.ports.get(&port) else { return vec![] };
        let other = if p.opener == from { p.peer.clone() } else { Some(p.opener.clone()) };
        match other {
            Some(c) => vec![Delivery::ToContext { context_id: c, name: format!("port.onMessage:{port}"), args: vec![message] }],
            None => vec![],
        }
    }

    /// `port.disconnect()` from `from`, or the end went away.
    pub fn close_port(&mut self, port: u64, from: &str) -> Vec<Delivery> {
        let Some(p) = self.ports.remove(&port) else { return vec![] };
        let other = if p.opener == from { p.peer } else { Some(p.opener) };
        other.map(|c| vec![Delivery::ToContext { context_id: c, name: format!("port.onDisconnect:{port}"), args: vec![] }]).unwrap_or_default()
    }

    pub fn port_name(&self, port: u64) -> Option<&str> {
        self.ports.get(&port).map(|p| p.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn ctx(id: &str, kind: Kind, tab: Option<i64>, frame: i64) -> MsgContext {
        MsgContext { id: id.into(), kind, extension_id: E.into(), tab_id: tab, frame_id: frame, url: format!("chrome-extension://{E}/{id}") }
    }
    fn setup() -> Messaging {
        let mut m = Messaging::new();
        m.open_context(ctx("worker", Kind::Worker, None, 0));
        m.open_context(ctx("popup", Kind::Page, None, 0));
        m.open_context(ctx("cs1", Kind::ContentScript, Some(7), 0));
        m.open_context(ctx("cs2", Kind::ContentScript, Some(7), 3));
        m
    }
    fn token_of(d: &Delivery) -> u64 {
        match d {
            Delivery::ToContext { args, .. } => args[2].as_u64().unwrap(),
            _ => panic!(),
        }
    }

    #[test]
    fn runtime_send_message_goes_to_the_other_extension_contexts_not_content_scripts() {
        let mut m = setup();
        let d = m.send("popup", 1, json!("hi"), Target::Extension);
        assert_eq!(d.len(), 1);
        assert!(matches!(&d[0], Delivery::ToContext { context_id, name, .. } if context_id == "worker" && name == "runtime.onMessage"));
        let Delivery::ToContext { args, .. } = &d[0] else { panic!() };
        assert_eq!(args[0], json!("hi"));
        assert_eq!(args[1]["id"], E);
    }

    #[test]
    fn the_first_handled_answer_completes_the_call() {
        let mut m = setup();
        let d = m.send("worker", 5, json!("q"), Target::Extension);
        let t = token_of(&d[0]);
        let out = m.reply("popup", t, true, true, json!({ "answer": 42 }));
        assert_eq!(out, vec![Delivery::Reply { call_id: 5, result: Ok(json!({ "answer": 42 })) }]);
        assert!(m.reply("popup", t, true, true, json!(1)).is_empty(), "later answers are ignored");
    }

    #[test]
    fn nobody_with_a_listener_means_receiving_end_does_not_exist() {
        let mut m = setup();
        let d = m.send("popup", 1, json!("x"), Target::Extension);
        let out = m.reply("worker", token_of(&d[0]), false, false, Value::Null);
        assert_eq!(out, vec![Delivery::Reply { call_id: 1, result: Err(NO_RECEIVER.into()) }]);
        // Alone in the extension: nothing to deliver to.
        let mut lone = Messaging::new();
        lone.open_context(ctx("only", Kind::Page, None, 0));
        assert_eq!(lone.send("only", 2, json!(1), Target::Extension), vec![Delivery::Reply { call_id: 2, result: Err(NO_RECEIVER.into()) }]);
    }

    #[test]
    fn a_listener_that_never_answers_gives_undefined() {
        let mut m = setup();
        let d = m.send("popup", 1, json!("x"), Target::Extension);
        let out = m.reply("worker", token_of(&d[0]), false, true, Value::Null);
        assert_eq!(out, vec![Delivery::Reply { call_id: 1, result: Ok(Value::Null) }]);
    }

    #[test]
    fn tabs_send_message_reaches_the_content_scripts_of_the_tab_and_frame() {
        let mut m = setup();
        let all = m.send("worker", 1, json!("x"), Target::Tab { tab_id: 7, frame_id: None });
        assert_eq!(all.len(), 2);
        let one = m.send("worker", 2, json!("x"), Target::Tab { tab_id: 7, frame_id: Some(3) });
        assert_eq!(one.len(), 1);
        assert!(matches!(&one[0], Delivery::ToContext { context_id, .. } if context_id == "cs2"));
        let none = m.send("worker", 3, json!("x"), Target::Tab { tab_id: 99, frame_id: None });
        assert!(matches!(&none[0], Delivery::Reply { result: Err(_), .. }));
    }

    #[test]
    fn a_closing_context_stops_being_waited_for() {
        let mut m = setup();
        let d = m.send("popup", 1, json!("x"), Target::Extension);
        let _ = token_of(&d[0]);
        let out = m.close_context("worker");
        assert_eq!(out, vec![Delivery::Reply { call_id: 1, result: Err(NO_RECEIVER.into()) }]);
    }

    #[test]
    fn sender_has_tab_info_for_content_scripts() {
        let m = setup();
        let s = m.contexts_of(E).into_iter().find(|c| c.id == "cs1").unwrap().sender();
        assert_eq!(s["tab"]["id"], 7);
        assert!(s.get("origin").is_none());
    }

    #[test]
    fn ports_pair_with_the_first_acceptor_and_relay_messages() {
        let mut m = setup();
        let (port, d) = m.connect("popup", "chat", Target::Extension).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(m.port_name(port), Some("chat"));
        assert!(m.accept_port("nobody", port).is_empty());
        assert_eq!(m.accept_port("worker", port).len(), 1);
        assert_eq!(m.post("popup", port, json!("a")), vec![Delivery::ToContext { context_id: "worker".into(), name: format!("port.onMessage:{port}"), args: vec![json!("a")] }]);
        assert_eq!(m.post("worker", port, json!("b")), vec![Delivery::ToContext { context_id: "popup".into(), name: format!("port.onMessage:{port}"), args: vec![json!("b")] }]);
        let out = m.close_port(port, "popup");
        assert_eq!(out, vec![Delivery::ToContext { context_id: "worker".into(), name: format!("port.onDisconnect:{port}"), args: vec![] }]);
        assert!(m.post("popup", port, json!("c")).is_empty());
    }

    #[test]
    fn a_port_nobody_accepts_disconnects_the_opener() {
        let mut m = setup();
        let (port, _) = m.connect("popup", "x", Target::Extension).unwrap();
        let out = m.decline_port("worker", port);
        assert!(matches!(&out[0], Delivery::ToContext { context_id, name, .. } if context_id == "popup" && name == &format!("port.onDisconnect:{port}")));
        assert!(m.connect("cs1", "x", Target::Extension).is_ok());
        assert!(Messaging::new().connect("ghost", "x", Target::Extension).is_err());
    }

    #[test]
    fn closing_a_context_closes_its_ports() {
        let mut m = setup();
        let (port, _) = m.connect("popup", "x", Target::Extension).unwrap();
        m.accept_port("worker", port);
        let out = m.close_context("popup");
        assert!(out.iter().any(|d| matches!(d, Delivery::ToContext { context_id, name, .. } if context_id == "worker" && name.starts_with("port.onDisconnect"))));
    }
}
