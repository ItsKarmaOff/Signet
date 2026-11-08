//! Switch rules and displayed state of a link (port of packages/shared/src/link/status.ts).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkStatus {
    Connected,
    Connecting,
    PausedHere,
    PausedThere,
    Offline,
    Invalid,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LinkStatusInput {
    pub general: bool,
    pub device: bool,
    pub connected: bool,
    pub connecting: bool,
    pub peer_paused: bool,
    pub invalid: bool,
}

pub fn link_status(s: LinkStatusInput) -> LinkStatus {
    if s.invalid {
        return LinkStatus::Invalid;
    }
    if !(s.general && s.device) {
        return LinkStatus::PausedHere;
    }
    if s.peer_paused {
        return LinkStatus::PausedThere;
    }
    if s.connected {
        return LinkStatus::Connected;
    }
    if s.connecting { LinkStatus::Connecting } else { LinkStatus::Offline }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalhostRoute {
    None,
    Link(String),
    Ask,
}

pub fn pick_localhost_route(active: &[String], chosen: Option<&str>) -> LocalhostRoute {
    if let Some(c) = chosen {
        if active.iter().any(|a| a == c) {
            return LocalhostRoute::Link(c.to_string());
        }
    }
    match active.len() {
        0 => LocalhostRoute::None,
        1 => LocalhostRoute::Link(active[0].clone()),
        _ => LocalhostRoute::Ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> LinkStatusInput {
        LinkStatusInput { general: true, device: true, connected: true, peer_paused: false, invalid: false, connecting: false }
    }

    #[test]
    fn only_works_if_everything_is_on() {
        assert_eq!(link_status(base()), LinkStatus::Connected);
        assert_eq!(link_status(LinkStatusInput { general: false, ..base() }), LinkStatus::PausedHere);
        assert_eq!(link_status(LinkStatusInput { device: false, ..base() }), LinkStatus::PausedHere);
        assert_eq!(link_status(LinkStatusInput { peer_paused: true, ..base() }), LinkStatus::PausedThere);
        assert_eq!(link_status(LinkStatusInput { connected: false, ..base() }), LinkStatus::Offline);
        assert_eq!(link_status(LinkStatusInput { connected: false, connecting: true, ..base() }), LinkStatus::Connecting);
        assert_eq!(link_status(LinkStatusInput { invalid: true, general: false, ..base() }), LinkStatus::Invalid);
    }

    #[test]
    fn picks_the_computer_for_localhost() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_localhost_route(&[], None), LocalhostRoute::None);
        assert_eq!(pick_localhost_route(&s(&["a"]), None), LocalhostRoute::Link("a".into()));
        assert_eq!(pick_localhost_route(&s(&["a", "b"]), None), LocalhostRoute::Ask);
        assert_eq!(pick_localhost_route(&s(&["a", "b"]), Some("b")), LocalhostRoute::Link("b".into()));
        assert_eq!(pick_localhost_route(&s(&["a", "b"]), Some("c")), LocalhostRoute::Ask);
    }
}
