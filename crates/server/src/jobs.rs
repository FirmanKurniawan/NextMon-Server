//! Async job registry with progress (GET /api/jobs/{id}, SSE /api/jobs/{id}/stream).
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnap {
    pub id: String,
    pub kind: String,
    /// running | done | error
    pub state: &'static str,
    pub progress: f64,
    pub step: String,
    pub message: String,
    pub error: Option<Value>,
    pub result: Option<Value>,
}

#[derive(Clone)]
pub struct JobHandle {
    tx: Arc<watch::Sender<JobSnap>>,
}

impl JobHandle {
    pub fn id(&self) -> String {
        self.tx.borrow().id.clone()
    }
    pub fn set(&self, progress: f64, step: &str, message: &str) {
        self.tx.send_modify(|s| {
            if s.state == "running" {
                s.progress = progress.clamp(0.0, 1.0);
                s.step = step.to_string();
                s.message = message.to_string();
            }
        });
    }
    pub fn done(&self, result: Value) {
        self.tx.send_modify(|s| {
            s.state = "done";
            s.progress = 1.0;
            s.result = Some(result);
        });
    }
    pub fn fail(&self, code: &str, message: &str, extra: Option<Value>) {
        self.tx.send_modify(|s| {
            s.state = "error";
            let mut e = serde_json::json!({"code": code, "message": message});
            if let (Some(Value::Object(x)), Some(m)) = (extra, e.as_object_mut()) {
                m.extend(x);
            }
            s.error = Some(e);
            s.message = message.to_string();
        });
    }
}

#[derive(Default)]
pub struct Jobs {
    map: Mutex<HashMap<String, Arc<watch::Sender<JobSnap>>>>,
    order: Mutex<VecDeque<String>>,
}

impl Jobs {
    pub fn new() -> Jobs {
        Jobs::default()
    }

    pub fn create(&self, kind: &str) -> JobHandle {
        let id = crate::util::random_token()[..12].to_string();
        let snap = JobSnap {
            id: id.clone(),
            kind: kind.to_string(),
            state: "running",
            progress: 0.0,
            step: "queued".into(),
            message: String::new(),
            error: None,
            result: None,
        };
        let (tx, _rx) = watch::channel(snap);
        let tx = Arc::new(tx);
        let mut map = self.map.lock().unwrap();
        let mut order = self.order.lock().unwrap();
        map.insert(id.clone(), tx.clone());
        order.push_back(id);
        // keep the registry bounded: drop the oldest finished jobs beyond 60
        while order.len() > 60 {
            let Some(old) = order.pop_front() else { break };
            if map.get(&old).map(|t| t.borrow().state != "running").unwrap_or(true) {
                map.remove(&old);
            } else {
                order.push_back(old);
                break;
            }
        }
        JobHandle { tx }
    }

    pub fn get(&self, id: &str) -> Option<JobSnap> {
        self.map.lock().unwrap().get(id).map(|t| t.borrow().clone())
    }

    pub fn subscribe(&self, id: &str) -> Option<watch::Receiver<JobSnap>> {
        self.map.lock().unwrap().get(id).map(|t| t.subscribe())
    }

    /// True if a job of this kind is still running.
    pub fn is_running(&self, kind: &str) -> bool {
        self.map.lock().unwrap().values().any(|t| {
            let s = t.borrow();
            s.kind == kind && s.state == "running"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle() {
        let j = Jobs::new();
        let h = j.create("install");
        assert!(j.is_running("install"));
        h.set(0.5, "download", "50%");
        assert_eq!(j.get(&h.id()).unwrap().progress, 0.5);
        h.fail("network", "boom", Some(serde_json::json!({"hint": "x"})));
        let s = j.get(&h.id()).unwrap();
        assert_eq!(s.state, "error");
        assert_eq!(s.error.unwrap()["hint"], "x");
        assert!(!j.is_running("install"));
        h.set(0.9, "x", "y"); // ignored after finish
        assert_eq!(j.get(&h.id()).unwrap().progress, 0.5);
    }
}
