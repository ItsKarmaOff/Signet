//! `chrome.alarms`: alarm construction rules, the scheduler and the persisted API. Port of `apis/schedule.ts` and
//! `apis/alarms.ts`. Timers are replaced by an explicit clock: the host calls [`AlarmScheduler::tick`] with the current
//! time (and can sleep until [`AlarmScheduler::next_due`]).

use crate::common::{ApiError, ApiResult, Emitter, JsonStore};
use crate::shared::ALARM_EVENT;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// MV3 minimum: Chrome rounds shorter delays and periods up to 30 seconds.
pub const MIN_ALARM_MS: f64 = 30_000.0;

/// `chrome.alarms.Alarm`. Times are milliseconds since the epoch (as JSON numbers they stay exact).
#[derive(Debug, Clone, PartialEq)]
pub struct Alarm {
    pub name: String,
    pub scheduled_time: f64,
    pub period_in_minutes: Option<f64>,
}

impl Alarm {
    pub fn to_json(&self) -> Value {
        let mut v = json!({ "name": self.name, "scheduledTime": self.scheduled_time });
        if let Some(p) = self.period_in_minutes {
            v["periodInMinutes"] = json!(p);
        }
        v
    }

    fn from_json(v: &Value) -> Option<Alarm> {
        Some(Alarm {
            name: v.get("name")?.as_str()?.to_string(),
            scheduled_time: v.get("scheduledTime")?.as_f64().filter(|f| f.is_finite())?,
            period_in_minutes: v.get("periodInMinutes").and_then(|p| p.as_f64()),
        })
    }
}

fn finite(v: Option<&Value>) -> Option<Option<f64>> {
    match v {
        None | Some(Value::Null) => Some(None),
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()).map(Some),
        _ => None,
    }
}

/// Builds the alarm an `alarms.create` call describes; errors like Chrome for what it rejects.
pub fn build_alarm(name: &str, info: &Value, now: f64) -> ApiResult<Alarm> {
    let mut vals = [None; 3];
    for (i, key) in ["when", "delayInMinutes", "periodInMinutes"].iter().enumerate() {
        vals[i] = finite(info.get(key)).ok_or_else(|| ApiError::new(format!("Invalid value for argument. Property '{key}': Expected 'number'.")))?;
    }
    let [when, delay, period_min] = vals;
    if when.is_some() && delay.is_some() {
        return Err(ApiError::new("Cannot set both when and delayInMinutes."));
    }
    if when.is_none() && delay.is_none() && period_min.is_none() {
        return Err(ApiError::new("Must set at least one of when, delayInMinutes, or periodInMinutes."));
    }
    if period_min.is_some_and(|p| p <= 0.0) {
        return Err(ApiError::new("Period must be greater than 0."));
    }
    if delay.is_some_and(|d| d < 0.0) {
        return Err(ApiError::new("Delay cannot be negative."));
    }
    let period = period_min.map(|p| (p * 60_000.0).max(MIN_ALARM_MS));
    let first = match (when, delay) {
        (Some(w), _) => w - now,
        (None, Some(d)) => d * 60_000.0,
        _ => period.unwrap_or(0.0),
    };
    Ok(Alarm { name: name.to_string(), scheduled_time: now + first.max(MIN_ALARM_MS), period_in_minutes: period.map(|p| p / 60_000.0) })
}

/// Next time a repeating alarm fires after having fired (or been missed) at `now`: the next slot of its period that is
/// still in the future, so a long sleep fires once instead of replaying every missed tick.
pub fn next_occurrence(alarm: &Alarm, now: f64) -> f64 {
    let period = alarm.period_in_minutes.unwrap_or(0.0) * 60_000.0;
    if period <= 0.0 {
        return alarm.scheduled_time;
    }
    alarm.scheduled_time + period * (((now - alarm.scheduled_time) / period).floor() + 1.0)
}

/// Alarms of all extensions. Alarms are kept per extension id; they only fire for extensions that are loaded
/// (`activate`). An alarm whose time passed while the app was closed fires once on activation, like Chrome.
pub struct AlarmScheduler {
    alarms: Vec<(String, Vec<Alarm>)>,
    /// (extension id, alarm name) with a running timer.
    armed: HashSet<(String, String)>,
    changed: bool,
}

impl AlarmScheduler {
    pub fn new(initial: Vec<(String, Vec<Alarm>)>) -> Self {
        let mut alarms = Vec::new();
        for (id, list) in initial {
            let mut kept: Vec<Alarm> = Vec::new();
            for a in list {
                kept.retain(|k| k.name != a.name);
                kept.push(a);
            }
            if !kept.is_empty() {
                alarms.push((id, kept));
            }
        }
        AlarmScheduler { alarms, armed: HashSet::new(), changed: false }
    }

    fn list_mut(&mut self, id: &str) -> Option<&mut Vec<Alarm>> {
        self.alarms.iter_mut().find(|(i, _)| i == id).map(|(_, l)| l)
    }

    fn list(&self, id: &str) -> Option<&Vec<Alarm>> {
        self.alarms.iter().find(|(i, _)| i == id).map(|(_, l)| l)
    }

    /// True once after the set of alarms changed: persist it.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    pub fn create(&mut self, id: &str, name: &str, info: &Value, now: f64) -> ApiResult<()> {
        let alarm = build_alarm(name, info, now)?;
        self.armed.remove(&(id.to_string(), name.to_string()));
        match self.list_mut(id) {
            Some(l) => {
                l.retain(|a| a.name != name);
                l.push(alarm);
            }
            None => self.alarms.push((id.to_string(), vec![alarm])),
        }
        self.armed.insert((id.to_string(), name.to_string()));
        self.changed = true;
        Ok(())
    }

    pub fn get(&self, id: &str, name: &str) -> Option<Alarm> {
        self.list(id)?.iter().find(|a| a.name == name).cloned()
    }

    pub fn get_all(&self, id: &str) -> Vec<Alarm> {
        self.list(id).cloned().unwrap_or_default()
    }

    pub fn clear(&mut self, id: &str, name: &str) -> bool {
        self.armed.remove(&(id.to_string(), name.to_string()));
        let removed = self.list_mut(id).is_some_and(|l| {
            let n = l.len();
            l.retain(|a| a.name != name);
            l.len() != n
        });
        if removed {
            self.changed = true;
        }
        removed
    }

    pub fn clear_all(&mut self, id: &str) -> bool {
        let had = self.list(id).is_some_and(|l| !l.is_empty());
        self.remove(id);
        had
    }

    /// The extension is loaded: start its timers, firing the ones that came due while it was away.
    pub fn activate(&mut self, id: &str) {
        let names: Vec<String> = self.list(id).map(|l| l.iter().map(|a| a.name.clone()).collect()).unwrap_or_default();
        for n in names {
            self.armed.insert((id.to_string(), n));
        }
    }

    /// The extension is unloaded (reload, update): its timers stop, the alarms are kept and armed again when it loads.
    pub fn suspend(&mut self, id: &str) {
        self.armed.retain(|(i, _)| i != id);
    }

    /// Ids with alarms.
    pub fn known_ids(&self) -> Vec<String> {
        self.alarms.iter().map(|(i, _)| i.clone()).collect()
    }

    /// The extension is uninstalled: forget its alarms.
    pub fn remove(&mut self, id: &str) {
        let before = self.alarms.len();
        self.alarms.retain(|(i, _)| i != id);
        self.armed.retain(|(i, _)| i != id);
        if self.alarms.len() != before {
            self.changed = true;
        }
    }

    pub fn serialize(&self) -> Value {
        Value::Object(self.alarms.iter().map(|(id, l)| (id.clone(), Value::Array(l.iter().map(Alarm::to_json).collect()))).collect())
    }

    /// Earliest time an armed alarm is due.
    pub fn next_due(&self) -> Option<f64> {
        self.alarms
            .iter()
            .flat_map(|(id, l)| l.iter().filter(move |a| self.armed.contains(&(id.clone(), a.name.clone()))).map(|a| a.scheduled_time))
            .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t))))
    }

    /// Fires what is due at `now`: returns `(extension id, alarm)` in the order of their scheduled time. A repeating alarm
    /// is moved to its next slot, a one-shot alarm is forgotten.
    pub fn tick(&mut self, now: f64) -> Vec<(String, Alarm)> {
        let mut due: Vec<(String, Alarm)> = Vec::new();
        for (id, l) in &self.alarms {
            for a in l {
                if self.armed.contains(&(id.clone(), a.name.clone())) && a.scheduled_time <= now {
                    due.push((id.clone(), a.clone()));
                }
            }
        }
        due.sort_by(|a, b| a.1.scheduled_time.partial_cmp(&b.1.scheduled_time).unwrap_or(std::cmp::Ordering::Equal));
        for (id, alarm) in &due {
            let periodic = alarm.period_in_minutes.is_some_and(|p| p > 0.0);
            let next = periodic.then(|| Alarm { scheduled_time: next_occurrence(alarm, now), ..alarm.clone() });
            if let Some(l) = self.list_mut(id) {
                match next {
                    Some(n) => {
                        if let Some(slot) = l.iter_mut().find(|a| a.name == alarm.name) {
                            *slot = n;
                        }
                    }
                    None => {
                        l.retain(|a| a.name != alarm.name);
                    }
                }
            }
            if !periodic {
                self.armed.remove(&(id.clone(), alarm.name.clone()));
            }
            self.changed = true;
        }
        due
    }
}

/// `chrome.alarms`: persisted per extension so alarms survive a restart.
pub struct Alarms {
    pub scheduler: AlarmScheduler,
    store: JsonStore,
    emitter: Arc<dyn Emitter>,
}

impl Alarms {
    pub fn new(file: impl AsRef<Path>, emitter: Arc<dyn Emitter>) -> Self {
        let store = JsonStore::new(file.as_ref());
        let mut initial = Vec::new();
        if let Some(Value::Object(map)) = store.read().and_then(|d| d.get("alarms").cloned()) {
            for (id, list) in map {
                initial.push((id, list.as_array().into_iter().flatten().filter_map(Alarm::from_json).collect()));
            }
        }
        Alarms { scheduler: AlarmScheduler::new(initial), store, emitter }
    }

    pub fn flush(&mut self) -> bool {
        if self.scheduler.take_changed() {
            self.store.request();
        }
        let v = json!({ "version": 1, "alarms": self.scheduler.serialize() });
        self.store.flush(&v)
    }

    /// Fires what is due: `onAlarm` goes to the extension, waking its worker.
    pub fn tick(&mut self, now: f64) {
        for (id, alarm) in self.scheduler.tick(now) {
            self.emitter.emit(&id, ALARM_EVENT, vec![alarm.to_json()], true, true);
        }
        if self.scheduler.take_changed() {
            self.store.request();
        }
    }

    /// Router dispatch. `create(name?, info)`: the name is optional and comes first.
    pub fn call(&mut self, ext: &str, method: &str, args: &[Value], now: f64) -> ApiResult<Value> {
        let name_of = |v: Option<&Value>| v.and_then(|n| n.as_str()).unwrap_or("").to_string();
        let r = match method {
            "alarms.create" => {
                match args.first().and_then(|a| a.as_str()) {
                    Some(name) => self.scheduler.create(ext, name, args.get(1).unwrap_or(&Value::Null), now)?,
                    None => self.scheduler.create(ext, "", args.first().unwrap_or(&Value::Null), now)?,
                }
                Value::Null
            }
            "alarms.get" => self.scheduler.get(ext, &name_of(args.first())).map_or(Value::Null, |a| a.to_json()),
            "alarms.getAll" => Value::Array(self.scheduler.get_all(ext).iter().map(Alarm::to_json).collect()),
            "alarms.clear" => json!(self.scheduler.clear(ext, &name_of(args.first()))),
            "alarms.clearAll" => json!(self.scheduler.clear_all(ext)),
            _ => return Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        };
        if self.scheduler.take_changed() {
            self.store.request();
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;

    const T0: f64 = 1_767_225_600_000.0; // 2026-01-01
    const MIN: f64 = 60_000.0;

    fn alarm(name: &str, t: f64, p: Option<f64>) -> Alarm {
        Alarm { name: name.into(), scheduled_time: t, period_in_minutes: p }
    }

    #[test]
    fn rounds_delays_and_periods_up_to_30_seconds() {
        assert_eq!(build_alarm("a", &json!({ "delayInMinutes": 0.1 }), T0).unwrap().scheduled_time, T0 + MIN_ALARM_MS);
        assert_eq!(build_alarm("a", &json!({ "when": T0 - 5000.0 }), T0).unwrap().scheduled_time, T0 + MIN_ALARM_MS);
        assert_eq!(build_alarm("a", &json!({ "periodInMinutes": 0.1 }), T0).unwrap(), alarm("a", T0 + MIN_ALARM_MS, Some(0.5)));
    }

    #[test]
    fn first_run_is_when_delay_or_one_period() {
        assert_eq!(build_alarm("a", &json!({ "when": T0 + 120_000.0 }), T0).unwrap().scheduled_time, T0 + 120_000.0);
        assert_eq!(build_alarm("a", &json!({ "delayInMinutes": 2, "periodInMinutes": 5 }), T0).unwrap(), alarm("a", T0 + 120_000.0, Some(5.0)));
        assert_eq!(build_alarm("a", &json!({ "periodInMinutes": 3 }), T0).unwrap().scheduled_time, T0 + 180_000.0);
    }

    #[test]
    fn rejects_what_chrome_rejects() {
        assert!(build_alarm("a", &json!({}), T0).unwrap_err().0.contains("at least one"));
        assert!(build_alarm("a", &json!({ "when": 1, "delayInMinutes": 1 }), T0).unwrap_err().0.contains("both"));
        assert!(build_alarm("a", &json!({ "periodInMinutes": 0 }), T0).is_err());
        assert!(build_alarm("a", &json!({ "delayInMinutes": "x" }), T0).is_err());
    }

    #[test]
    fn next_occurrence_skips_missed_ticks() {
        let a = alarm("a", T0, Some(1.0));
        assert_eq!(next_occurrence(&a, T0), T0 + MIN);
        assert_eq!(next_occurrence(&a, T0 + 10.0 * MIN + 5.0), T0 + 11.0 * MIN);
    }

    #[test]
    fn one_shot_fires_once_then_is_forgotten() {
        let mut s = AlarmScheduler::new(vec![]);
        s.create("ext", "once", &json!({ "delayInMinutes": 1 }), T0).unwrap();
        assert!(s.get("ext", "once").is_some());
        assert!(s.tick(T0 + 59_999.0).is_empty());
        let fired = s.tick(T0 + 60_000.0);
        assert_eq!(fired, vec![("ext".to_string(), alarm("once", T0 + 60_000.0, None))]);
        assert!(s.get("ext", "once").is_none());
        assert!(s.tick(T0 + 10.0 * MIN).is_empty());
    }

    #[test]
    fn periodic_alarm_repeats_until_cleared() {
        let mut s = AlarmScheduler::new(vec![]);
        s.create("ext", "tick", &json!({ "periodInMinutes": 1 }), T0).unwrap();
        let mut n = 0;
        for i in 1..=3 {
            n += s.tick(T0 + i as f64 * MIN).len();
        }
        assert_eq!(n, 3);
        assert!(s.clear("ext", "tick"));
        assert!(!s.clear("ext", "tick"));
        assert!(s.tick(T0 + 8.0 * MIN).is_empty());
    }

    #[test]
    fn same_name_replaces_and_extensions_are_apart() {
        let mut s = AlarmScheduler::new(vec![]);
        s.create("a", "x", &json!({ "delayInMinutes": 1 }), T0).unwrap();
        s.create("a", "x", &json!({ "delayInMinutes": 2 }), T0).unwrap();
        s.create("b", "x", &json!({ "delayInMinutes": 1 }), T0).unwrap();
        let fired = s.tick(T0 + MIN);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].0, "b");
        assert_eq!(s.get_all("a").len(), 1);
        assert!(s.clear_all("a"));
        assert!(s.get_all("a").is_empty());
    }

    #[test]
    fn fires_once_on_activation_what_came_due_while_closed() {
        let missed = alarm("late", T0 - 3_600_000.0, None);
        let periodic = alarm("p", T0 - 3_600_000.0, Some(10.0));
        let future = alarm("soon", T0 + MIN, None);
        let mut s = AlarmScheduler::new(vec![("ext".into(), vec![missed, periodic, future])]);
        assert!(s.tick(T0 + 10_000.0).is_empty(), "dormant until the extension loads");
        s.activate("ext");
        let mut names: Vec<String> = s.tick(T0 + 10_000.0).into_iter().map(|(_, a)| a.name).collect();
        names.sort();
        assert_eq!(names, ["late", "p"]);
        assert!(s.get("ext", "late").is_none());
        assert_eq!(s.get("ext", "p").unwrap().scheduled_time, T0 + 10_000.0 + (10.0 * MIN - 10_000.0));
        let soon: Vec<_> = s.tick(T0 + MIN + 10_000.0).into_iter().filter(|(_, a)| a.name == "soon").collect();
        assert_eq!(soon.len(), 1);
    }

    #[test]
    fn removal_stops_everything() {
        let mut s = AlarmScheduler::new(vec![]);
        s.create("ext", "x", &json!({ "periodInMinutes": 1 }), T0).unwrap();
        s.remove("ext");
        assert!(s.tick(T0 + 10.0 * MIN).is_empty());
        assert_eq!(s.serialize(), json!({}));
    }

    #[test]
    fn far_alarms_wait_and_suspend_keeps_alarms() {
        let mut s = AlarmScheduler::new(vec![]);
        let day = 24.0 * 60.0 * MIN;
        s.create("ext", "far", &json!({ "when": T0 + 40.0 * day }), T0).unwrap();
        assert!(s.tick(T0 + 39.0 * day).is_empty());
        assert_eq!(s.next_due(), Some(T0 + 40.0 * day));
        s.suspend("ext");
        assert!(s.tick(T0 + 41.0 * day).is_empty());
        assert!(s.get("ext", "far").is_some());
        s.activate("ext");
        assert_eq!(s.tick(T0 + 41.0 * day).len(), 1);
    }

    #[test]
    fn reports_changes_for_persistence() {
        let mut s = AlarmScheduler::new(vec![]);
        s.create("ext", "x", &json!({ "delayInMinutes": 1 }), T0).unwrap();
        assert!(s.take_changed());
        assert_eq!(s.serialize(), json!({ "ext": [{ "name": "x", "scheduledTime": T0 + 60_000.0 }] }));
    }

    #[test]
    fn alarms_api_emits_waking_events_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("extension-alarms.json");
        let out = Arc::new(Outbox::new());
        let mut a = Alarms::new(&file, out.clone());
        a.call("ext", "alarms.create", &[json!("n"), json!({ "delayInMinutes": 1 })], T0).unwrap();
        a.call("ext", "alarms.create", &[json!({ "delayInMinutes": 2 })], T0).unwrap();
        assert_eq!(a.call("ext", "alarms.getAll", &[], T0).unwrap().as_array().unwrap().len(), 2);
        assert!(a.flush());
        let again = Alarms::new(&file, out.clone());
        assert_eq!(again.scheduler.get_all("ext").len(), 2);
        a.tick(T0 + MIN);
        let ev = out.drain();
        assert_eq!((ev[0].extension_id.as_str(), ev[0].name.as_str(), ev[0].wake), ("ext", "alarms.onAlarm", true));
        assert_eq!(ev[0].args[0]["name"], "n");
        assert_eq!(a.call("ext", "alarms.clear", &[json!("n")], T0).unwrap(), json!(false));
        assert_eq!(a.call("ext", "alarms.get", &[json!("none")], T0).unwrap(), Value::Null);
        assert_eq!(a.call("ext", "alarms.clearAll", &[], T0).unwrap(), json!(true));
    }
}
