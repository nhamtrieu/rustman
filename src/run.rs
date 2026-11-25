//! The collection run loop, shared by the runner tab and `rustman run`: sends the requests in
//! order, once per iteration, with their scripts. Variables set by one request are seen by the
//! next, `pm.execution.setNextRequest` changes the order, and an OAuth 2.0 token refreshed by one
//! request is used by the following ones.

use std::sync::Arc;
use std::time::Duration;

use crate::exec::{self, Executed, ScriptSet};
use crate::model::{Auth, Item, KeyValue, Request};
use crate::script::{Info, Scopes};

/// Safety net against `setNextRequest` loops: max requests per iteration = this × request count.
pub const MAX_STEPS_FACTOR: usize = 100;

pub struct RunPlan {
    pub requests: Vec<Request>,
    pub collection_scripts: Option<ScriptSet>,
    /// Collection auth, for requests that inherit it.
    pub auth: Option<Auth>,
    pub scopes: Scopes,
    pub environment_name: Option<String>,
    /// Data rows (column name → value); iteration `n` uses row `n % len`.
    pub rows: Vec<Vec<KeyValue>>,
    pub iterations: u32,
    pub delay_ms: u64,
}

pub enum RunEvent<'a> {
    /// `iteration` is 1-based.
    Started { iteration: u32, request: &'a Request },
    Step { iteration: u32, request: &'a Request, executed: Box<Executed> },
}

/// Runs the plan. `on_event` returns `false` to stop the run. Returns the variables at the end.
pub async fn run(plan: RunPlan, jar: Arc<reqwest::cookie::Jar>, mut on_event: impl FnMut(RunEvent<'_>) -> bool) -> Scopes {
    let RunPlan { mut requests, collection_scripts, mut auth, mut scopes, environment_name, rows, iterations, delay_ms } = plan;
    let iterations = iterations.max(1);
    let mut first = true;
    'run: for it in 0..iterations {
        scopes.data = if rows.is_empty() { Default::default() } else { crate::vars::build_vars(&[&rows[it as usize % rows.len()]]) };
        let mut idx = 0;
        let mut steps = 0;
        while idx < requests.len() && steps < requests.len() * MAX_STEPS_FACTOR {
            steps += 1;
            if !first && delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            first = false;
            if !on_event(RunEvent::Started { iteration: it + 1, request: &requests[idx] }) {
                break 'run;
            }
            scopes.local.clear();
            let r = &requests[idx];
            let mut scripts: Vec<ScriptSet> = collection_scripts.iter().cloned().collect();
            scripts.push(ScriptSet { label: "Request", pre: r.pre_request_script.clone(), post: r.post_response_script.clone() });
            let job = exec::Job {
                request: r.clone(),
                scripts,
                auth: auth.clone(),
                scopes: scopes.clone(),
                environment_name: environment_name.clone(),
                info: Info { request_name: r.name.clone(), request_id: r.id.to_string(), iteration: it, iteration_count: iterations },
            };
            let executed = exec::run(job, jar.clone(), |_, _| {}).await;
            scopes = executed.scopes.clone();
            let next = executed.report.next_request.clone();
            if let Some(u) = &executed.report.token_update {
                if u.inherited {
                    if let Some(a) = &mut auth {
                        a.oauth2.set_token(&u.token);
                    }
                } else if let Some(req) = requests.iter_mut().find(|p| p.id == u.request) {
                    req.auth.oauth2.set_token(&u.token);
                }
            }
            if !on_event(RunEvent::Step { iteration: it + 1, request: &requests[idx], executed: Box::new(executed) }) {
                break 'run;
            }
            idx = match next {
                None => idx + 1,
                // setNextRequest(null) ends this iteration.
                Some(None) => break,
                Some(Some(name)) => match requests.iter().position(|p| p.name == name) {
                    Some(i) => i,
                    None => break,
                },
            };
        }
    }
    scopes
}

/// Every request of a tree in order, with the names of the folders it is in.
pub fn flatten(items: &[Item], folders: &mut Vec<String>, out: &mut Vec<(Vec<String>, Request)>) {
    for i in items {
        match i {
            Item::Request(r) => out.push((folders.clone(), r.clone())),
            Item::Folder(f) => {
                folders.push(f.name.clone());
                flatten(&f.items, folders, out);
                folders.pop();
            }
        }
    }
}
