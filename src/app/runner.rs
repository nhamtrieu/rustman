//! Collection runner: sends every request of a collection in order, once per iteration,
//! optionally driven by a dataset (one row per iteration, columns become variables).
//! Requests run one after another in a background task with their scripts, so variables set
//! by one request (`pm.environment.set`) are seen by the next, and `pm.execution.setNextRequest`
//! can change the order.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Id, RichText, Sense, Ui, pos2, vec2};

use super::*;
use crate::exec::{self, Executed};
use crate::run::RunEvent;
use crate::http::{human_duration, human_size};
use crate::script::TestResult;
use crate::icons;
use crate::pm::{self, paint_text};
use crate::theme::{icon_font, mono, pal, regular, semibold};

pub(super) enum RunMsg {
    Started { run: Uuid, iteration: u32, name: String, method: Method },
    Step { run: Uuid, iteration: u32, name: String, method: Method, executed: Box<Executed> },
    Finished { run: Uuid },
}

pub(super) struct RunnerState {
    pub collection: Uuid,
    /// Requests unticked in the run order.
    excluded: HashSet<Uuid>,
    iterations: u32,
    pub dataset: Option<Uuid>,
    delay_ms: u64,
    run: Option<Run>,
}

impl RunnerState {
    #[cfg(debug_assertions)]
    pub fn set_iterations(&mut self, n: u32) {
        self.iterations = n.max(1);
    }

    fn new(collection: Uuid) -> Self {
        Self { collection, excluded: HashSet::new(), iterations: 1, dataset: None, delay_ms: 0, run: None }
    }
}

struct Run {
    id: Uuid,
    /// Finished requests, in execution order.
    jobs: Vec<Job>,
    /// Requests expected without `setNextRequest` jumps (iterations × requests).
    planned: usize,
    /// Request being executed.
    current: Option<(u32, String, Method)>,
    handle: Option<tokio::task::JoinHandle<()>>,
    started: Instant,
    elapsed: Option<Duration>,
    filter: u8, // 0 all, 1 passed, 2 failed
    /// Where script variable changes are saved.
    env: Option<Uuid>,
}

struct Job {
    iteration: u32,
    name: String,
    method: Method,
    url: String,
    outcome: Outcome,
    tests: Vec<TestResult>,
    errors: Vec<String>,
}

enum Outcome {
    Done { status: u16, status_text: String, duration: Duration, size: usize },
    Failed(String),
}

impl Job {
    /// Passes when every test passes; a request without tests passes on status < 400.
    fn passed(&self) -> bool {
        if !self.errors.is_empty() {
            return false;
        }
        match &self.outcome {
            Outcome::Failed(_) => false,
            Outcome::Done { status, .. } => {
                let tests: Vec<&TestResult> = self.tests.iter().filter(|t| !t.skipped).collect();
                if tests.is_empty() { *status < 400 } else { tests.iter().all(|t| t.passed) }
            }
        }
    }
}

impl Run {
    fn is_running(&self) -> bool {
        self.elapsed.is_none()
    }
}

impl RustmanApp {
    pub(super) fn open_runner(&mut self, collection: Uuid) {
        if self.runner.as_ref().is_none_or(|r| r.collection != collection) {
            if let Some(Run { handle: Some(h), .. }) = self.runner.as_mut().and_then(|r| r.run.take()) {
                h.abort();
            }
            self.runner = Some(RunnerState::new(collection));
        }
        self.open_side(SideTab::Runner(collection));
    }

    pub(super) fn start_run(&mut self, ctx: &egui::Context) {
        let Some(st) = &self.runner else { return };
        let Some(coll) = self.ws.collection(st.collection) else { return };
        let mut all = Vec::new();
        collect_requests(&coll.items, &coll.name, &mut all);
        let plan: Vec<Request> = all.into_iter().map(|(_, r)| r).filter(|r| !st.excluded.contains(&r.id)).cloned().collect();
        if plan.is_empty() {
            self.notify_err("Select at least one request to run");
            return;
        }
        let iterations = st.iterations.max(1);
        let dataset = st.dataset.and_then(|d| self.ws.datasets.iter().find(|x| x.id == d)).filter(|d| !d.rows.is_empty());
        let rows: Vec<Vec<KeyValue>> = (0..iterations).map(|it| dataset.map(|d| d.row_vars(it as usize % d.rows.len())).unwrap_or_default()).collect();
        let env = self.ws.active_env();
        let scopes = Scopes::new(&self.ws.globals, &coll.variables, env.map(|e| &e.variables[..]), &[]);
        let env_name = env.map(|e| e.name.clone());
        let coll_scripts = exec::ScriptSet { label: "Collection", pre: coll.pre_request_script.clone(), post: coll.post_response_script.clone() };
        let run_plan = crate::run::RunPlan {
            requests: plan,
            collection_scripts: Some(coll_scripts),
            auth: Some(coll.auth.clone()),
            scopes,
            environment_name: env_name,
            rows,
            iterations,
            delay_ms: st.delay_ms,
        };

        let run_id = Uuid::new_v4();
        let planned = run_plan.requests.len() * iterations as usize;
        let (tx, jar, ctx) = (self.run_tx.clone(), self.jar.clone(), ctx.clone());
        let handle = self.runtime.spawn(async move {
            crate::run::run(run_plan, jar, |event| {
                let msg = match event {
                    RunEvent::Started { iteration, request } => RunMsg::Started { run: run_id, iteration, name: request.name.clone(), method: request.method },
                    RunEvent::Step { iteration, request, executed } => RunMsg::Step { run: run_id, iteration, name: request.name.clone(), method: request.method, executed },
                };
                let ok = tx.send(msg).is_ok();
                ctx.request_repaint();
                ok
            })
            .await;
            let _ = tx.send(RunMsg::Finished { run: run_id });
            ctx.request_repaint();
        });
        let env = self.ws.active_environment;
        if let Some(st) = &mut self.runner {
            st.run = Some(Run { id: run_id, jobs: Vec::new(), planned, current: None, handle: Some(handle), started: Instant::now(), elapsed: None, filter: 0, env });
        }
    }

    fn stop_run(&mut self) {
        if let Some(run) = self.runner.as_mut().and_then(|r| r.run.as_mut()) {
            if let Some(h) = run.handle.take() {
                h.abort();
            }
            if let Some((iteration, name, method)) = run.current.take() {
                run.jobs.push(Job { iteration, name, method, url: String::new(), outcome: Outcome::Failed("Run stopped".into()), tests: Vec::new(), errors: Vec::new() });
            }
            run.elapsed = Some(run.started.elapsed());
        }
    }

    /// Applies results coming from the run task (also logs them to the console).
    pub(super) fn receive_runs(&mut self) {
        while let Ok(msg) = self.run_rx.try_recv() {
            let Some(st) = self.runner.as_mut() else { continue };
            let collection = st.collection;
            let Some(run) = st.run.as_mut().filter(|r| r.id == run_msg_id(&msg)) else { continue };
            match msg {
                RunMsg::Started { iteration, name, method, .. } => run.current = Some((iteration, name, method)),
                RunMsg::Finished { .. } => {
                    run.current = None;
                    run.elapsed = Some(run.started.elapsed());
                    run.handle = None;
                }
                RunMsg::Step { iteration, name, method, executed, .. } => {
                    run.current = None;
                    let env = run.env;
                    let Executed { prepared, result, report, .. } = *executed;
                    let url = prepared.as_ref().map(|p| p.url.clone()).unwrap_or_default();
                    let label = name.clone();
                    run.jobs.push(Job {
                        iteration,
                        name,
                        method,
                        url,
                        outcome: match &result {
                            Ok(r) => Outcome::Done { status: r.status, status_text: r.status_text.clone(), duration: r.duration, size: r.body.len() },
                            Err(e) => Outcome::Failed(e.clone()),
                        },
                        tests: report.tests.clone(),
                        errors: report.errors.clone(),
                    });
                    self.log_script_events(&report.pre_events);
                    match &prepared {
                        Some(p) => {
                            let log = self.console.start(p);
                            self.console.finish(log, &result);
                        }
                        None => {
                            if let Err(e) = &result {
                                self.console.failed_to_prepare(method, &label, e);
                            }
                        }
                    }
                    self.log_script_events(&report.post_events);
                    if prepared.is_some() {
                        for e in &report.errors {
                            self.console.log(script::LogLevel::Error, e.clone());
                        }
                    }
                    self.apply_var_ops(report.persistent_ops(), Some(collection), env);
                    if let Some(u) = &report.token_update {
                        self.apply_token_update(u, Some(collection));
                    }
                }
            }
        }
    }

    pub(super) fn runner_view(&mut self, ui: &mut Ui, collection: Uuid) {
        let p = pal(ui);
        let ctx = ui.ctx().clone();
        if self.runner.as_ref().is_none_or(|r| r.collection != collection) {
            self.runner = Some(RunnerState::new(collection));
        }
        let Some(coll) = self.ws.collection(collection) else {
            self.close_side_tab();
            return;
        };
        let mut all = Vec::new();
        collect_requests(&coll.items, &coll.name, &mut all);
        let requests: Vec<(String, Uuid, Method, String)> = all.iter().map(|(path, r)| (path.clone(), r.id, r.method, r.name.clone())).collect();
        let coll_name = coll.name.clone();
        let datasets: Vec<(Uuid, String, usize)> = self.ws.datasets.iter().map(|d| (d.id, d.name.clone(), d.rows.len())).collect();
        let env_name = self.ws.active_env().map(|e| e.name.clone()).unwrap_or_else(|| "No Environment".into());
        let (mut run_clicked, mut stop_clicked) = (false, false);

        // Header.
        let st = self.runner.as_mut().expect("runner state");
        let running = st.run.as_ref().is_some_and(Run::is_running);
        ui.horizontal(|ui| {
            ui.set_height(46.0);
            pm::icon(ui, icons::PLAY, 16.0, p.text2);
            ui.label(RichText::new(format!("Run \"{coll_name}\"")).font(semibold(15.0)).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if running {
                    if pm::secondary_button(ui, "Stop").clicked() {
                        stop_clicked = true;
                    }
                } else if pm::primary_button(ui, &format!("Run {coll_name}")).clicked() {
                    run_clicked = true;
                }
            });
        });

        // Configuration (left).
        egui::Panel::left(Id::unique("runner_config"))
            .exact_size(340.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 14, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    egui::ScrollArea::vertical().id_salt("runner_cfg_scroll").auto_shrink(false).show(ui, |ui| {
                        ui.label(RichText::new("Run configuration").font(semibold(13.5)).color(p.text));
                        ui.add_space(8.0);
                        let row = |ui: &mut Ui, label: &str, add: &mut dyn FnMut(&mut Ui)| {
                            ui.horizontal(|ui| {
                                ui.add_sized(vec2(90.0, 24.0), egui::Label::new(RichText::new(label).color(p.text2)));
                                add(ui);
                            });
                            ui.add_space(4.0);
                        };
                        row(ui, "Iterations", &mut |ui| {
                            ui.add(egui::DragValue::new(&mut st.iterations).range(1..=1000));
                        });
                        row(ui, "Delay", &mut |ui| {
                            ui.add(egui::DragValue::new(&mut st.delay_ms).range(0..=60_000).speed(10).suffix(" ms"));
                        });
                        let mut pick_rows = None;
                        row(ui, "Data", &mut |ui| {
                            let label = st.dataset.and_then(|d| datasets.iter().find(|x| x.0 == d)).map(|x| x.1.clone()).unwrap_or_else(|| "None".into());
                            let btn = pm::text_button(ui, Some(icons::TABLE), &label, true);
                            egui::Popup::menu(&btn).width(220.0).show(|ui| {
                                if ui.selectable_label(st.dataset.is_none(), "None").clicked() {
                                    st.dataset = None;
                                    ui.close();
                                }
                                for (id, name, rows) in &datasets {
                                    if ui.selectable_label(st.dataset == Some(*id), format!("{name}  ({rows} rows)")).clicked() {
                                        st.dataset = Some(*id);
                                        pick_rows = Some(*rows);
                                        ui.close();
                                    }
                                }
                                if datasets.is_empty() {
                                    ui.label(RichText::new("No datasets. Create one in the sidebar (DATASETS).").color(p.text3));
                                }
                            });
                        });
                        if let Some(rows) = pick_rows {
                            st.iterations = rows.max(1) as u32;
                        }
                        row(ui, "Environment", &mut |ui| {
                            ui.label(RichText::new(&env_name).color(p.text));
                        });
                        if st.dataset.is_some() {
                            ui.label(RichText::new("Iteration n uses row n of the dataset (wrapping around); its columns override other variables.").font(regular(12.5)).color(p.text3));
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Requests  {}/{}", requests.len() - requests.iter().filter(|r| st.excluded.contains(&r.1)).count(), requests.len())).font(semibold(13.5)).color(p.text));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if pm::text_button(ui, None, "Deselect all", false).clicked() {
                                    st.excluded = requests.iter().map(|r| r.1).collect();
                                }
                                if pm::text_button(ui, None, "Select all", false).clicked() {
                                    st.excluded.clear();
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (path, id, method, name) in &requests {
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 4.0, p.hover);
                            }
                            let mut on = !st.excluded.contains(id);
                            let cb = egui::Rect::from_center_size(pos2(r.left() + 12.0, r.center().y), vec2(18.0, 18.0));
                            let changed = pm::checkbox(ui, cb, Id::unique(("run_req", *id)), &mut on).changed();
                            if resp.clicked() {
                                on = !on;
                            }
                            if changed || resp.clicked() {
                                if on {
                                    st.excluded.remove(id);
                                } else {
                                    st.excluded.insert(*id);
                                }
                            }
                            let (m, f) = pm::method_label_font(*method, 10.0);
                            paint_text(ui, pos2(r.left() + 30.0, r.center().y), &m, f, p.method(*method), 44.0);
                            paint_text(ui, pos2(r.left() + 78.0, r.center().y), name, regular(13.5), p.text, r.width() - 84.0);
                            resp.on_hover_text(path);
                        }
                        if requests.is_empty() {
                            ui.label(RichText::new("This collection has no requests.").color(p.text3));
                        }
                    });
                });
            });

        // Results (center).
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let Some(run) = &mut st.run else {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    pm::icon(ui, icons::PLAY, 40.0, p.text3);
                    ui.add_space(8.0);
                    ui.label(RichText::new("Choose the requests, iterations and data, then click Run.").color(p.text2));
                    ui.label(RichText::new("A request passes when all its tests pass, or, if it has no tests, when its status is below 400.").font(regular(12.5)).color(p.text3));
                    ui.label(RichText::new("Variables set by scripts carry over to the next requests.").font(regular(12.5)).color(p.text3));
                });
                return;
            };
            let passed = run.jobs.iter().filter(|j| j.passed()).count();
            let failed = run.jobs.len() - passed;
            let all_tests: Vec<&TestResult> = run.jobs.iter().flat_map(|j| j.tests.iter()).filter(|t| !t.skipped).collect();
            let tests_passed = all_tests.iter().filter(|t| t.passed).count();
            let times: Vec<Duration> = run.jobs.iter().filter_map(|j| if let Outcome::Done { duration, .. } = j.outcome { Some(duration) } else { None }).collect();
            let avg = if times.is_empty() { Duration::ZERO } else { times.iter().sum::<Duration>() / times.len() as u32 };
            let elapsed = run.elapsed.unwrap_or_else(|| run.started.elapsed());
            if run.is_running() {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let stat = |ui: &mut Ui, label: &str, value: String, color| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(label).font(regular(12.0)).color(p.text3));
                        ui.label(RichText::new(value).font(semibold(16.0)).color(color));
                    });
                };
                let progress = if run.is_running() { format!("{}/{}", run.jobs.len(), run.planned.max(run.jobs.len())) } else { run.jobs.len().to_string() };
                stat(ui, "Requests", progress, p.text);
                stat(ui, "Passed", passed.to_string(), p.green_dot);
                stat(ui, "Failed", failed.to_string(), if failed > 0 { p.danger } else { p.text2 });
                if !all_tests.is_empty() {
                    let tc = if tests_passed == all_tests.len() { p.green_dot } else { p.danger };
                    stat(ui, "Tests", format!("{tests_passed}/{}", all_tests.len()), tc);
                }
                stat(ui, "Avg. response", human_duration(avg), p.text);
                stat(ui, "Duration", human_duration(elapsed), p.text);
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, label) in ["All", "Passed", "Failed"].iter().enumerate() {
                    if pm::pill_tab(ui, None, label, None, false, run.filter == i as u8).clicked() {
                        run.filter = i as u8;
                    }
                }
            });
            ui.add_space(4.0);
            pm::hline(ui);
            let filter = run.filter;
            let mut copy = None;
            egui::ScrollArea::vertical().id_salt("runner_results").auto_shrink(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut last_iter = 0;
                for j in &run.jobs {
                    let show = match filter {
                        1 => j.passed(),
                        2 => !j.passed(),
                        _ => true,
                    };
                    if !show {
                        continue;
                    }
                    if j.iteration != last_iter {
                        last_iter = j.iteration;
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("Iteration {}", j.iteration)).font(semibold(13.0)).color(p.text2));
                        ui.add_space(2.0);
                    }
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 4.0, p.hover);
                    }
                    let cy = r.center().y;
                    let (glyph, color) = if j.passed() { (icons::CHECK_CIRCLE, p.green_dot) } else { (icons::X_CIRCLE, p.danger) };
                    ui.painter().text(pos2(r.left() + 12.0, cy), Align2::CENTER_CENTER, glyph, icon_font(16.0), color);
                    let (m, f) = pm::method_label_font(j.method, 10.5);
                    paint_text(ui, pos2(r.left() + 30.0, cy), &m, f, p.method(j.method), 50.0);
                    let nw = paint_text(ui, pos2(r.left() + 84.0, cy), &j.name, regular(13.5), p.text, 260.0);
                    let right_text = match &j.outcome {
                        Outcome::Done { status, status_text, duration, size } => format!("{status} {status_text}   {}   {}", human_duration(*duration), human_size(*size)),
                        Outcome::Failed(e) => e.lines().next().unwrap_or("Error").to_string(),
                    };
                    let rc = match &j.outcome {
                        Outcome::Done { status, .. } => p.status(*status).1,
                        Outcome::Failed(_) => p.danger,
                    };
                    let rw = pm::text_width(ui, &right_text, regular(12.5)).min(r.width() * 0.4);
                    paint_text(ui, pos2(r.right() - rw - 8.0, cy), &right_text, regular(12.5), rc, rw);
                    let url_x = r.left() + 96.0 + nw;
                    paint_text(ui, pos2(url_x, cy), &j.url, mono(12.0), p.text3, (r.right() - rw - 24.0 - url_x).max(0.0));
                    let resp = if let Outcome::Failed(e) = &j.outcome { resp.on_hover_text(e) } else { resp };
                    resp.context_menu(|ui| {
                        if ui.button("Copy URL").clicked() {
                            copy = Some(j.url.clone());
                            ui.close();
                        }
                    });
                    test_lines(ui, j);
                }
                if let Some((iteration, name, method)) = &run.current {
                    if *iteration != last_iter {
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("Iteration {iteration}")).font(semibold(13.0)).color(p.text2));
                        ui.add_space(2.0);
                    }
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                    let cy = r.center().y;
                    ui.painter().text(pos2(r.left() + 12.0, cy), Align2::CENTER_CENTER, icons::DOTS_THREE, icon_font(16.0), p.text3);
                    let (m, f) = pm::method_label_font(*method, 10.5);
                    paint_text(ui, pos2(r.left() + 30.0, cy), &m, f, p.method(*method), 50.0);
                    let nw = paint_text(ui, pos2(r.left() + 84.0, cy), name, regular(13.5), p.text, 260.0);
                    paint_text(ui, pos2(r.left() + 96.0 + nw, cy), "running…", regular(12.5), p.text3, 120.0);
                }
            });
            if let Some(u) = copy {
                ui.ctx().copy_text(u);
            }
        });

        if run_clicked {
            self.start_run(&ctx);
        }
        if stop_clicked {
            self.stop_run();
        }
    }
}

fn run_msg_id(m: &RunMsg) -> Uuid {
    match m {
        RunMsg::Started { run, .. } | RunMsg::Step { run, .. } | RunMsg::Finished { run } => *run,
    }
}

/// Tests and script errors under a request row of the results.
fn test_lines(ui: &mut Ui, j: &Job) {
    let p = pal(ui);
    let line = |ui: &mut Ui, glyph: &str, color: egui::Color32, text: &str, detail: Option<&str>| {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        let cy = r.center().y;
        ui.painter().text(pos2(r.left() + 40.0, cy), Align2::CENTER_CENTER, glyph, icon_font(13.0), color);
        let w = paint_text(ui, pos2(r.left() + 54.0, cy), text, regular(12.5), p.text2, (r.width() * 0.5).max(120.0));
        if let Some(d) = detail {
            paint_text(ui, pos2(r.left() + 66.0 + w, cy), d, mono(12.0), p.text3, (r.width() - 80.0 - w).max(0.0));
            resp.on_hover_text(d);
        }
    };
    for e in &j.errors {
        line(ui, icons::WARNING, p.danger, "Script error", Some(e));
    }
    for t in &j.tests {
        let (glyph, color) = if t.skipped {
            (icons::MINUS, p.text3)
        } else if t.passed {
            (icons::CHECK, p.green_dot)
        } else {
            (icons::X, p.danger)
        };
        line(ui, glyph, color, &t.name, t.error.as_deref());
    }
}
