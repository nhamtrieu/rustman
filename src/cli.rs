//! Command line: `rustman run <collection>` runs a collection without the GUI, like Newman, for
//! terminals and CI. Prints a report, can write JSON and JUnit reports, and exits with 1 when an
//! assertion, a script or a request failed.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use uuid::Uuid;

use crate::exec::Executed;
use crate::http::{human_duration, human_size};
use crate::model::{ClientCert, Collection, Dataset, Environment, KeyValue, NetSettings, Workspace};
use crate::run::{RunEvent, RunPlan};
use crate::script::{LogLevel, Scope, Scopes, ScriptEvent, TestResult, VarOp};

/// Exit codes.
const EXIT_FAILED: i32 = 1;
const EXIT_USAGE: i32 = 2;

const HELP: &str = "\
Rustman — API client. Without arguments, opens the app.

Usage:
  rustman run <collection> [options]   Run a collection (like `newman run`)
  rustman --version                     Print the version
  rustman --help                        Print this help

<collection>: a Postman Collection v2.1 file, or the name of a collection in the Rustman workspace.

Options:
  -e, --environment <file|name>    Postman environment file, or a workspace environment name
  -g, --globals <file>             Postman globals file
  -d, --iteration-data <file|name> CSV or JSON data file (or a workspace dataset name); one row per iteration
  -n, --iteration-count <n>        Number of iterations (default: number of data rows, or 1)
      --folder <name>              Only run this folder or request (repeatable)
      --env-var <key=value>        Set an environment variable (repeatable)
      --global-var <key=value>     Set a global variable (repeatable)
      --delay-request <ms>         Wait between requests
      --timeout-request <ms>       Request timeout
  -k, --insecure                   Do not verify SSL certificates
      --ssl-extra-ca-certs <file>  Also trust the CA certificates in this PEM file
      --ssl-client-cert <file>     Client certificate (PEM) sent to every HTTPS host
      --ssl-client-key <file>      Private key (PEM) of the client certificate
      --bail                       Stop at the first failure
  -r, --reporters <list>           cli,json,junit (default: cli)
      --reporter-json-export <file>   JSON report path (default: rustman-report.json)
      --reporter-junit-export <file>  JUnit XML report path (default: rustman-report.xml)
      --export-environment <file>  Write the environment, with changes made by scripts, to a file
      --export-globals <file>      Write the globals, with changes made by scripts, to a file
      --color <on|off|auto>        Colored output (default: auto); --no-color is off
      --silent                     No output on the terminal
  -x, --suppress-exit-code         Always exit with 0

Exit code: 0 when everything passed, 1 when a test, script or request failed, 2 on a usage error.
Proxy: the HTTP_PROXY, HTTPS_PROXY and NO_PROXY environment variables are used. A collection taken
from the workspace also uses the proxy and certificates set in the app.";

/// Handles the command line. `None`: no command, open the app.
pub fn dispatch(args: &[String]) -> Option<i32> {
    let first = args.get(1)?;
    // macOS passes `-psn_…` when an app is opened from the Finder.
    if first.starts_with("-psn") {
        return None;
    }
    attach_console();
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    Some(main(&args[1..], &mut out, &mut err))
}

/// A release build on Windows is a GUI program: attach to the console it was started from.
#[cfg(windows)]
fn attach_console() {
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}

pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            let _ = writeln!(out, "{HELP}");
            0
        }
        Some("-V" | "--version" | "version") => {
            let _ = writeln!(out, "rustman {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("run") => {
            let opts = match Opts::parse(&args[1..]) {
                Ok(o) => o,
                Err(e) => {
                    let _ = writeln!(err, "error: {e}\n\nRun `rustman --help` for the options.");
                    return EXIT_USAGE;
                }
            };
            match run(&opts, out) {
                Ok(code) => code,
                Err(e) => {
                    let _ = writeln!(err, "error: {e}");
                    EXIT_USAGE
                }
            }
        }
        Some(other) => {
            let _ = writeln!(err, "error: unknown command '{other}'\n\n{HELP}");
            EXIT_USAGE
        }
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Opts {
    collection: String,
    environment: Option<String>,
    globals: Option<String>,
    data: Option<String>,
    iterations: Option<u32>,
    folders: Vec<String>,
    env_vars: Vec<(String, String)>,
    global_vars: Vec<(String, String)>,
    delay_ms: u64,
    timeout_ms: Option<u64>,
    insecure: bool,
    ca: Option<String>,
    client_cert: Option<String>,
    client_key: Option<String>,
    bail: bool,
    reporters: Vec<String>,
    json_export: Option<String>,
    junit_export: Option<String>,
    export_environment: Option<String>,
    export_globals: Option<String>,
    color: Option<bool>,
    silent: bool,
    suppress_exit_code: bool,
}

impl Opts {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut o = Opts { reporters: vec!["cli".into()], ..Default::default() };
        let mut collection = None;
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            // `--name=value` or `--name value`.
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
                _ => (arg.clone(), None),
            };
            let mut value = || -> Result<String, String> {
                if let Some(v) = &inline {
                    return Ok(v.clone());
                }
                i += 1;
                args.get(i).cloned().ok_or_else(|| format!("{name} needs a value"))
            };
            let number = |v: String, name: &str| v.trim().parse::<u64>().map_err(|_| format!("{name} expects a number, got '{v}'"));
            let pair = |v: String, name: &str| match v.split_once('=') {
                Some((k, val)) if !k.trim().is_empty() => Ok((k.trim().to_string(), val.to_string())),
                _ => Err(format!("{name} expects key=value, got '{v}'")),
            };
            match name.as_str() {
                "-e" | "--environment" => o.environment = Some(value()?),
                "-g" | "--globals" => o.globals = Some(value()?),
                "-d" | "--iteration-data" => o.data = Some(value()?),
                "-n" | "--iteration-count" => o.iterations = Some(number(value()?, &name)?.max(1) as u32),
                "--folder" => o.folders.push(value()?),
                "--env-var" => o.env_vars.push(pair(value()?, &name)?),
                "--global-var" => o.global_vars.push(pair(value()?, &name)?),
                "--delay-request" => o.delay_ms = number(value()?, &name)?,
                "--timeout-request" => o.timeout_ms = Some(number(value()?, &name)?),
                "-k" | "--insecure" => o.insecure = true,
                "--ssl-extra-ca-certs" => o.ca = Some(value()?),
                "--ssl-client-cert" => o.client_cert = Some(value()?),
                "--ssl-client-key" => o.client_key = Some(value()?),
                "--bail" => o.bail = true,
                "-r" | "--reporters" => {
                    o.reporters = value()?.split(',').map(|r| r.trim().to_ascii_lowercase()).filter(|r| !r.is_empty()).collect();
                    if let Some(bad) = o.reporters.iter().find(|r| !matches!(r.as_str(), "cli" | "json" | "junit")) {
                        return Err(format!("unknown reporter '{bad}' (use cli, json, junit)"));
                    }
                }
                "--reporter-json-export" => o.json_export = Some(value()?),
                "--reporter-junit-export" => o.junit_export = Some(value()?),
                "--export-environment" => o.export_environment = Some(value()?),
                "--export-globals" => o.export_globals = Some(value()?),
                "--color" => {
                    o.color = match value()?.as_str() {
                        "on" => Some(true),
                        "off" => Some(false),
                        "auto" => None,
                        v => return Err(format!("--color expects on, off or auto, got '{v}'")),
                    }
                }
                "--no-color" => o.color = Some(false),
                "--silent" => o.silent = true,
                "-x" | "--suppress-exit-code" => o.suppress_exit_code = true,
                _ if name.starts_with('-') && name.len() > 1 => return Err(format!("unknown option '{name}'")),
                _ if collection.is_none() => collection = Some(arg.clone()),
                _ => return Err(format!("unexpected argument '{arg}'")),
            }
            i += 1;
        }
        o.collection = collection.ok_or("missing <collection>: a Postman collection file or a workspace collection name")?;
        // An export path enables its reporter, like Newman.
        for (path, r) in [(&o.json_export, "json"), (&o.junit_export, "junit")] {
            if path.is_some() && !o.reporters.iter().any(|x| x == r) {
                o.reporters.push(r.into());
            }
        }
        Ok(o)
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

struct Loaded {
    collection: Collection,
    environment: Option<Environment>,
    globals: Vec<KeyValue>,
    rows: Vec<Vec<KeyValue>>,
    net: NetSettings,
}

fn read(path: &str, what: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {what} '{path}': {e}"))
}

fn load(o: &Opts) -> Result<Loaded, String> {
    // The workspace is only read when a name refers to it.
    let mut ws: Option<Workspace> = None;
    fn workspace(cache: &mut Option<Workspace>) -> &Workspace {
        cache.get_or_insert_with(crate::storage::load)
    }

    let (collection, from_workspace) = if Path::new(&o.collection).is_file() {
        (crate::import::import_postman_collection(&read(&o.collection, "collection")?).map_err(|e| format!("{}: {e}", o.collection))?, false)
    } else {
        let w = workspace(&mut ws);
        let c = w.collections.iter().find(|c| c.name.eq_ignore_ascii_case(&o.collection) || c.id.to_string() == o.collection).cloned().ok_or_else(|| {
            let names: Vec<&str> = w.collections.iter().map(|c| c.name.as_str()).collect();
            let list = if names.is_empty() { "none".to_string() } else { names.join(", ") };
            format!("'{}' is not a file, and the workspace has no collection with that name (workspace collections: {list})", o.collection)
        })?;
        (c, true)
    };

    let environment = match &o.environment {
        Some(e) if Path::new(e).is_file() => Some(crate::import::import_postman_environment(&read(e, "environment")?).map_err(|err| format!("{e}: {err}"))?),
        Some(e) => Some(
            workspace(&mut ws)
                .environments
                .iter()
                .find(|x| x.name.eq_ignore_ascii_case(e) || x.id.to_string() == *e)
                .cloned()
                .ok_or_else(|| format!("'{e}' is not a file, and the workspace has no environment with that name"))?,
        ),
        None => None,
    };

    let globals = match &o.globals {
        Some(g) => crate::import::import_postman_environment(&read(g, "globals")?).map_err(|e| format!("{g}: {e}"))?.variables,
        None if from_workspace => workspace(&mut ws).globals.clone(),
        None => Vec::new(),
    };

    let rows = match &o.data {
        Some(d) => {
            let ds = if Path::new(d).is_file() {
                let (columns, rows) = crate::dataset::import_file(Path::new(d)).map_err(|e| format!("{d}: {e}"))?;
                Dataset { columns, rows, ..Default::default() }
            } else {
                workspace(&mut ws)
                    .datasets
                    .iter()
                    .find(|x| x.name.eq_ignore_ascii_case(d))
                    .cloned()
                    .ok_or_else(|| format!("'{d}' is not a file, and the workspace has no dataset with that name"))?
            };
            (0..ds.rows.len()).map(|i| ds.row_vars(i)).collect()
        }
        None => Vec::new(),
    };

    let mut net = if from_workspace { workspace(&mut ws).network.clone() } else { NetSettings::default() };
    if let Some(ca) = &o.ca {
        net.ca_enabled = true;
        net.ca_path = ca.clone();
    }
    if let Some(cert) = &o.client_cert {
        net.client_certs.insert(0, ClientCert { host: "*".into(), cert_path: cert.clone(), key_path: o.client_key.clone().unwrap_or_default(), ..Default::default() });
    } else if o.client_key.is_some() {
        return Err("--ssl-client-key needs --ssl-client-cert".into());
    }
    Ok(Loaded { collection, environment, globals, rows, net })
}

fn set_var(list: &mut Vec<KeyValue>, key: &str, value: &str) {
    match list.iter_mut().find(|kv| kv.key == key) {
        Some(kv) => {
            kv.value = value.to_string();
            kv.enabled = true;
        }
        None => list.push(KeyValue::new(key, value)),
    }
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/// One executed request.
struct Step {
    iteration: u32,
    /// Folders the request is in.
    path: Vec<String>,
    name: String,
    method: String,
    url: String,
    status: Option<(u16, String)>,
    duration: Duration,
    size: usize,
    /// The request could not be sent or no response came back.
    error: Option<String>,
    tests: Vec<TestResult>,
    script_errors: Vec<String>,
}

impl Step {
    fn label(&self) -> String {
        let mut parts = self.path.clone();
        parts.push(self.name.clone());
        parts.join(" / ")
    }
    fn failed(&self) -> bool {
        self.error.is_some() || !self.script_errors.is_empty() || self.tests.iter().any(|t| !t.passed && !t.skipped)
    }
}

/// A numbered failure in the summary.
struct Failure {
    kind: &'static str,
    title: String,
    detail: String,
    at: String,
}

struct Style {
    on: bool,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
    fn red(&self, s: &str) -> String {
        self.paint("31", s)
    }
    fn cyan(&self, s: &str) -> String {
        self.paint("36", s)
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
}

fn run(o: &Opts, term: &mut dyn Write) -> Result<i32, String> {
    let loaded = load(o)?;
    let Loaded { collection, mut environment, mut globals, rows, net } = loaded;
    for (k, v) in &o.global_vars {
        set_var(&mut globals, k, v);
    }
    if !o.env_vars.is_empty() {
        let env = environment.get_or_insert_with(|| Environment { name: "Environment".into(), ..Default::default() });
        for (k, v) in &o.env_vars {
            set_var(&mut env.variables, k, v);
        }
    }
    crate::http::set_network(&net);

    let mut all = Vec::new();
    crate::run::flatten(&collection.items, &mut Vec::new(), &mut all);
    if !o.folders.is_empty() {
        all.retain(|(path, r)| o.folders.iter().any(|f| path.iter().any(|p| p == f) || r.name == *f));
        if all.is_empty() {
            return Err(format!("no folder or request named {} in '{}'", o.folders.iter().map(|f| format!("'{f}'")).collect::<Vec<_>>().join(", "), collection.name));
        }
    }
    if all.is_empty() {
        return Err(format!("the collection '{}' has no requests", collection.name));
    }
    let paths: HashMap<Uuid, Vec<String>> = all.iter().map(|(p, r)| (r.id, p.clone())).collect();
    let requests: Vec<_> = all
        .into_iter()
        .map(|(_, mut r)| {
            if let Some(t) = o.timeout_ms {
                r.settings.timeout_ms = t;
            }
            if o.insecure {
                r.settings.verify_ssl = false;
            }
            r
        })
        .collect();
    let iterations = o.iterations.unwrap_or(rows.len().max(1) as u32);

    let plan = RunPlan {
        requests,
        collection: collection.clone(),
        scopes: Scopes::new(&globals, &collection.variables, environment.as_ref().map(|e| &e.variables[..]), &[]),
        environment_name: environment.as_ref().map(|e| e.name.clone()),
        rows,
        iterations,
        delay_ms: o.delay_ms,
    };

    let color = o.color.unwrap_or_else(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal());
    let st = Style { on: color && !o.silent };
    let mut sink = std::io::sink();
    let out: &mut dyn Write = if o.silent || !o.reporters.iter().any(|r| r == "cli") { &mut sink } else { term };
    let _ = writeln!(out, "{}\n\n{}", st.dim("rustman"), st.bold(&collection.name));

    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().map_err(|e| format!("cannot start: {e}"))?;
    let jar = Arc::new(reqwest::cookie::Jar::default());
    let started_at = chrono::Local::now();
    let started = Instant::now();
    let mut steps: Vec<Step> = Vec::new();
    let mut failures: Vec<Failure> = Vec::new();
    let mut ops: Vec<VarOp> = Vec::new();
    let mut current_iteration = 0;
    let mut bailed = false;

    runtime.block_on(crate::run::run(plan, jar, |event| {
        let RunEvent::Step { iteration, request, executed } = event else { return true };
        if iteration != current_iteration && iterations > 1 {
            let _ = writeln!(out, "\n{}", st.dim(&format!("Iteration {iteration}/{iterations}")));
        }
        current_iteration = iteration;
        let Executed { prepared, result, report, .. } = *executed;
        let path = paths.get(&request.id).cloned().unwrap_or_default();
        let mut step = Step {
            iteration,
            path,
            name: request.name.clone(),
            method: request.method.as_str().to_string(),
            url: prepared.as_ref().map(|p| p.url.clone()).unwrap_or_else(|| request.url.clone()),
            status: None,
            duration: Duration::ZERO,
            size: 0,
            error: None,
            tests: report.tests.clone(),
            script_errors: Vec::new(),
        };
        match &result {
            Ok(r) => {
                step.status = Some((r.status, r.status_text.clone()));
                step.duration = r.duration;
                step.size = r.body.len();
            }
            // A pre-request script error is reported as a script error, not twice.
            Err(e) if !report.errors.iter().any(|x| x == e) => step.error = Some(e.clone()),
            Err(_) => {}
        }
        step.script_errors = report.errors.clone();
        ops.extend(report.persistent_ops().cloned());

        // Terminal output.
        let _ = writeln!(out, "\n→ {}", step.label());
        print_events(out, &st, &report.pre_events);
        match (&step.status, &step.error) {
            (Some((code, text)), _) => {
                let status = format!("{code} {text}");
                let status = if *code < 400 { st.green(&status) } else { st.red(&status) };
                let _ = writeln!(out, "  {} {} [{status}, {}, {}]", step.method, step.url, human_size(step.size), human_duration(step.duration));
            }
            (None, Some(e)) => {
                failures.push(Failure { kind: "RequestError", title: first_line(e), detail: e.clone(), at: at(&step) });
                let _ = writeln!(out, "  {} {} {}", step.method, step.url, st.red(&format!("[errored] {}", first_line(e))));
            }
            (None, None) => {
                let _ = writeln!(out, "  {} {}", step.method, step.url);
            }
        }
        print_events(out, &st, &report.post_events);
        for t in &step.tests {
            if t.skipped {
                let _ = writeln!(out, "  {}  {}", st.cyan("-"), st.cyan(&t.name));
            } else if t.passed {
                let _ = writeln!(out, "  {}  {}", st.green("✓"), st.dim(&t.name));
            } else {
                failures.push(Failure { kind: "AssertionError", title: t.name.clone(), detail: t.error.clone().unwrap_or_default(), at: at(&step) });
                let _ = writeln!(out, "  {}  {}", st.red(&format!("{:>2}.", failures.len())), st.red(&t.name));
            }
        }
        for e in &step.script_errors {
            failures.push(Failure { kind: "ScriptError", title: first_line(e), detail: e.clone(), at: at(&step) });
            let _ = writeln!(out, "  {}  {}", st.red(&format!("{:>2}.", failures.len())), st.red(&first_line(e)));
        }
        let failed = step.failed();
        steps.push(step);
        if failed && o.bail {
            bailed = true;
            return false;
        }
        true
    }));
    let elapsed = started.elapsed();

    print_summary(out, &st, iterations, &steps, &failures, elapsed, bailed);

    if o.reporters.iter().any(|r| r == "json") {
        let path = o.json_export.clone().unwrap_or_else(|| "rustman-report.json".into());
        let report = json_report(&collection, environment.as_ref(), started_at, elapsed, iterations, &steps, &failures);
        std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap_or_default()).map_err(|e| format!("cannot write '{path}': {e}"))?;
        let _ = writeln!(out, "JSON report: {path}");
    }
    if o.reporters.iter().any(|r| r == "junit") {
        let path = o.junit_export.clone().unwrap_or_else(|| "rustman-report.xml".into());
        std::fs::write(&path, junit_report(&collection, started_at, elapsed, iterations, &steps)).map_err(|e| format!("cannot write '{path}': {e}"))?;
        let _ = writeln!(out, "JUnit report: {path}");
    }
    if let Some(path) = &o.export_environment {
        let mut env = environment.clone().unwrap_or_else(|| Environment { name: "Environment".into(), ..Default::default() });
        ops.iter().filter(|op| op.scope() == Scope::Environment).for_each(|op| op.apply_to(&mut env.variables));
        std::fs::write(path, crate::import::export_postman_environment(&env)).map_err(|e| format!("cannot write '{path}': {e}"))?;
    }
    if let Some(path) = &o.export_globals {
        let mut g = Environment { name: "Globals".into(), variables: globals.clone(), ..Default::default() };
        ops.iter().filter(|op| op.scope() == Scope::Globals).for_each(|op| op.apply_to(&mut g.variables));
        let mut v: serde_json::Value = serde_json::from_str(&crate::import::export_postman_environment(&g)).unwrap_or_default();
        v["_postman_variable_scope"] = json!("globals");
        std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| format!("cannot write '{path}': {e}"))?;
    }
    let _ = out.flush();
    Ok(if failures.is_empty() || o.suppress_exit_code { 0 } else { EXIT_FAILED })
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn at(step: &Step) -> String {
    format!("inside \"{}\" (iteration {})", step.label(), step.iteration)
}

/// `console.log` lines and `pm.sendRequest` calls made by scripts.
fn print_events(out: &mut dyn Write, st: &Style, events: &[ScriptEvent]) {
    for e in events {
        match e {
            ScriptEvent::Log(level, text) => {
                let prefix = match level {
                    LogLevel::Warn => "warn ",
                    LogLevel::Error => "error ",
                    LogLevel::Info => "info ",
                    LogLevel::Log => "",
                };
                for line in text.lines() {
                    let _ = writeln!(out, "  {} {}", st.dim("┊"), st.dim(&format!("{prefix}{line}")));
                }
            }
            ScriptEvent::Http(x) => {
                let (p, r) = (&x.0, &x.1);
                let status = match r {
                    Ok(r) => format!("{} {}", r.status, r.status_text),
                    Err(e) => format!("errored: {}", first_line(e)),
                };
                let _ = writeln!(out, "  {} {}", st.dim("┊"), st.dim(&format!("{} {} [{status}]", p.method.as_str(), p.url)));
            }
        }
    }
}

fn print_summary(out: &mut dyn Write, st: &Style, iterations: u32, steps: &[Step], failures: &[Failure], elapsed: Duration, bailed: bool) {
    let iter_failed = (1..=iterations).filter(|i| steps.iter().any(|s| s.iteration == *i && s.failed())).count();
    let iter_run = steps.iter().map(|s| s.iteration).max().unwrap_or(0);
    let req_failed = steps.iter().filter(|s| s.error.is_some()).count();
    let assertions: Vec<&TestResult> = steps.iter().flat_map(|s| &s.tests).filter(|t| !t.skipped).collect();
    let assert_failed = assertions.iter().filter(|t| !t.passed).count();
    let script_errors: usize = steps.iter().map(|s| s.script_errors.len()).sum();
    let times: Vec<Duration> = steps.iter().filter(|s| s.status.is_some()).map(|s| s.duration).collect();

    let w = [26usize, 12, 12];
    let line = |l: &str, m: &str, r: &str| format!("{l}{}{m}{}{m}{}{r}", "─".repeat(w[0]), "─".repeat(w[1]), "─".repeat(w[2]));
    let row = |name: &str, a: usize, b: usize| {
        let b_txt = format!("{b:>width$}", width = w[2] - 1);
        let b_txt = if b > 0 { st.red(&b_txt) } else { b_txt };
        format!("│{name:>width0$} │{a:>width1$} │{b_txt} │", width0 = w[0] - 1, width1 = w[1] - 1)
    };
    let full = |text: String| {
        let inner = w[0] + w[1] + w[2] + 2;
        let pad = inner.saturating_sub(text.chars().count() + 1);
        format!("│ {text}{}│", " ".repeat(pad))
    };
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", line("┌", "┬", "┐"));
    let _ = writeln!(out, "│{:>w0$} │{:>w1$} │{:>w2$} │", "", "executed", "failed", w0 = w[0] - 1, w1 = w[1] - 1, w2 = w[2] - 1);
    let _ = writeln!(out, "{}", line("├", "┼", "┤"));
    let _ = writeln!(out, "{}", row("iterations", iter_run as usize, iter_failed));
    let _ = writeln!(out, "{}", line("├", "┼", "┤"));
    let _ = writeln!(out, "{}", row("requests", steps.len(), req_failed));
    let _ = writeln!(out, "{}", line("├", "┼", "┤"));
    let _ = writeln!(out, "{}", row("assertions", assertions.len(), assert_failed));
    let _ = writeln!(out, "{}", line("├", "┼", "┤"));
    let _ = writeln!(out, "{}", row("script errors", script_errors, script_errors));
    let _ = writeln!(out, "{}", line("├", "┴", "┤"));
    let _ = writeln!(out, "{}", full(format!("total run duration: {}", human_duration(elapsed))));
    if let (Some(min), Some(max)) = (times.iter().min(), times.iter().max()) {
        let avg = times.iter().sum::<Duration>() / times.len() as u32;
        let _ = writeln!(out, "{}", full(format!("average response time: {} [min: {}, max: {}]", human_duration(avg), human_duration(*min), human_duration(*max))));
    }
    let _ = writeln!(out, "{}", line("└", "─", "┘"));
    if bailed {
        let _ = writeln!(out, "\n{}", st.red("Stopped at the first failure (--bail)."));
    }
    if !failures.is_empty() {
        let _ = writeln!(out, "\n{:>4}  {:<15}  detail\n", "#", "failure");
        for (i, f) in failures.iter().enumerate() {
            let _ = writeln!(out, "{}  {:<15}  {}", st.red(&format!("{:>3}.", i + 1)), f.kind, f.title);
            for l in f.detail.lines().filter(|l| *l != f.title) {
                let _ = writeln!(out, "{:23}{}", "", st.dim(l));
            }
            let _ = writeln!(out, "{:23}{}\n", "", st.dim(&f.at));
        }
    }
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

fn json_report(
    c: &Collection,
    env: Option<&Environment>,
    started_at: chrono::DateTime<chrono::Local>,
    elapsed: Duration,
    iterations: u32,
    steps: &[Step],
    failures: &[Failure],
) -> serde_json::Value {
    let assertions: Vec<&TestResult> = steps.iter().flat_map(|s| &s.tests).collect();
    json!({
        "collection": c.name,
        "environment": env.map(|e| e.name.clone()),
        "startedAt": started_at.to_rfc3339(),
        "durationMs": elapsed.as_millis() as u64,
        "stats": {
            "iterations": { "total": iterations, "failed": (1..=iterations).filter(|i| steps.iter().any(|s| s.iteration == *i && s.failed())).count() },
            "requests": { "total": steps.len(), "failed": steps.iter().filter(|s| s.error.is_some()).count() },
            "assertions": {
                "total": assertions.iter().filter(|t| !t.skipped).count(),
                "failed": assertions.iter().filter(|t| !t.passed && !t.skipped).count(),
                "skipped": assertions.iter().filter(|t| t.skipped).count(),
            },
            "scriptErrors": steps.iter().map(|s| s.script_errors.len()).sum::<usize>(),
        },
        "executions": steps.iter().map(|s| json!({
            "iteration": s.iteration,
            "name": s.name,
            "folder": s.path,
            "method": s.method,
            "url": s.url,
            "status": s.status.as_ref().map(|(c, _)| *c),
            "statusText": s.status.as_ref().map(|(_, t)| t.clone()),
            "responseTimeMs": s.status.as_ref().map(|_| s.duration.as_millis() as u64),
            "responseSize": s.status.as_ref().map(|_| s.size),
            "error": s.error,
            "tests": s.tests.iter().map(|t| json!({ "name": t.name, "passed": t.passed, "skipped": t.skipped, "error": t.error })).collect::<Vec<_>>(),
            "scriptErrors": s.script_errors,
        })).collect::<Vec<_>>(),
        "failures": failures.iter().map(|f| json!({ "type": f.kind, "title": f.title, "detail": f.detail, "at": f.at })).collect::<Vec<_>>(),
    })
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters XML 1.0 does not allow.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// One `<testsuite>` per executed request, one `<testcase>` per test.
fn junit_report(c: &Collection, started_at: chrono::DateTime<chrono::Local>, elapsed: Duration, iterations: u32, steps: &[Step]) -> String {
    let secs = |d: Duration| format!("{:.3}", d.as_secs_f64());
    let mut suites = String::new();
    let (mut total, mut total_failures, mut total_errors) = (0, 0, 0);
    for s in steps {
        let name = if iterations > 1 { format!("{} (iteration {})", s.label(), s.iteration) } else { s.label() };
        let classname = xml_escape(&std::iter::once(c.name.clone()).chain(s.path.iter().cloned()).chain(std::iter::once(s.name.clone())).collect::<Vec<_>>().join("."));
        let mut cases = String::new();
        let (mut tests, mut failures, mut errors, mut skipped) = (0, 0, 0, 0);
        for t in &s.tests {
            tests += 1;
            let mut case = format!("    <testcase name=\"{}\" classname=\"{classname}\" time=\"{}\"", xml_escape(&t.name), secs(s.duration));
            if t.skipped {
                skipped += 1;
                case.push_str(">\n      <skipped/>\n    </testcase>\n");
            } else if !t.passed {
                failures += 1;
                let msg = t.error.clone().unwrap_or_else(|| "Assertion failed".into());
                case.push_str(&format!(">\n      <failure type=\"AssertionError\" message=\"{}\">{}</failure>\n    </testcase>\n", xml_escape(&first_line(&msg)), xml_escape(&msg)));
            } else {
                case.push_str("/>\n");
            }
            cases.push_str(&case);
        }
        let mut problems: Vec<(&str, &str, String)> = Vec::new();
        if let Some(e) = &s.error {
            problems.push(("Request", "RequestError", e.clone()));
        }
        for e in &s.script_errors {
            problems.push(("Script", "ScriptError", e.clone()));
        }
        for (case_name, kind, msg) in problems {
            tests += 1;
            errors += 1;
            cases.push_str(&format!(
                "    <testcase name=\"{case_name}\" classname=\"{classname}\" time=\"{}\">\n      <error type=\"{kind}\" message=\"{}\">{}</error>\n    </testcase>\n",
                secs(s.duration),
                xml_escape(&first_line(&msg)),
                xml_escape(&msg)
            ));
        }
        // A request without tests still shows up, as one passing case.
        if tests == 0 {
            tests = 1;
            cases.push_str(&format!("    <testcase name=\"{} {}\" classname=\"{classname}\" time=\"{}\"/>\n", s.method, xml_escape(&s.url), secs(s.duration)));
        }
        total += tests;
        total_failures += failures;
        total_errors += errors;
        suites.push_str(&format!(
            "  <testsuite name=\"{}\" tests=\"{tests}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"{skipped}\" time=\"{}\" timestamp=\"{}\">\n{cases}  </testsuite>\n",
            xml_escape(&name),
            secs(s.duration),
            started_at.format("%Y-%m-%dT%H:%M:%S")
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"{}\" tests=\"{total}\" failures=\"{total_failures}\" errors=\"{total_errors}\" time=\"{}\">\n{suites}</testsuites>\n",
        xml_escape(&c.name),
        secs(elapsed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_options() {
        let o = Opts::parse(&args(&["c.json", "-e", "env.json", "-n", "3", "--folder", "A", "--folder=B", "--env-var", "k=v=w", "--reporter-junit-export", "r.xml", "-k", "--color=off"])).unwrap();
        assert_eq!(o.collection, "c.json");
        assert_eq!(o.environment.as_deref(), Some("env.json"));
        assert_eq!(o.iterations, Some(3));
        assert_eq!(o.folders, vec!["A", "B"]);
        assert_eq!(o.env_vars, vec![("k".to_string(), "v=w".to_string())]);
        assert_eq!(o.reporters, vec!["cli", "junit"], "an export path turns its reporter on");
        assert!(o.insecure);
        assert_eq!(o.color, Some(false));
        for (bad, msg) in [
            (vec!["c.json", "-n"], "needs a value"),
            (vec!["c.json", "-n", "x"], "expects a number"),
            (vec!["c.json", "--env-var", "novalue"], "key=value"),
            (vec!["c.json", "-r", "html"], "unknown reporter"),
            (vec!["c.json", "--nope"], "unknown option"),
            (vec!["c.json", "d.json"], "unexpected argument"),
            (vec![], "missing <collection>"),
        ] {
            let e = Opts::parse(&args(&bad)).unwrap_err();
            assert!(e.contains(msg), "{bad:?}: {e}");
        }
    }

    /// Echoes method, path and Authorization; `/missing` answers 404.
    async fn echo(listener: tokio::net::TcpListener) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let mut first = req.split_whitespace();
                let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("/").to_string());
                let auth = req.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.trim().to_string())).unwrap_or_default();
                let body = json!({ "method": method, "path": path, "auth": auth }).to_string();
                let status = if path.starts_with("/missing") { "404 Not Found" } else { "200 OK" };
                let resp = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    }

    #[test]
    fn runs_a_collection_file_end_to_end() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        rt.spawn(echo(listener));

        let dir = std::env::temp_dir().join(format!("rustman-cli-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = |name: &str| dir.join(name).display().to_string();
        let collection = json!({
            "info": { "name": "Demo <API>" },
            "variable": [{ "key": "base", "value": base }],
            "item": [
                { "name": "Auth", "item": [{
                    "name": "Login",
                    "request": { "method": "POST", "url": "{{base}}/login" },
                    "event": [{ "listen": "test", "script": { "exec": [
                        "pm.test('ok', () => pm.response.to.have.status(200));",
                        "pm.environment.set('token', 'tok-' + pm.iterationData.get('user'));",
                        "console.log('hello', pm.iterationData.get('user'));"
                    ] } }]
                }] },
                { "name": "Me",
                  "request": { "method": "GET", "url": "{{base}}/me", "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "{{token}}" }] } },
                  "event": [{ "listen": "test", "script": { "exec": [
                      "pm.test('token sent', () => pm.expect(pm.response.json().auth).to.equal('Bearer tok-' + pm.iterationData.get('user')));",
                      "pm.test('is admin', () => pm.expect(pm.iterationData.get('user')).to.equal('admin'));"
                  ] } }] },
                { "name": "Missing", "request": { "method": "GET", "url": "{{base}}/missing" },
                  "event": [{ "listen": "test", "script": { "exec": ["pm.test.skip('later', () => {});"] } }] }
            ]
        });
        std::fs::write(f("c.json"), collection.to_string()).unwrap();
        std::fs::write(f("data.csv"), "user\nadmin\nbob\n").unwrap();
        std::fs::write(f("env.json"), r#"{"name":"Local","values":[{"key":"token","value":"","enabled":true}]}"#).unwrap();

        let run = |extra: &[&str]| {
            let mut a = args(&["run", &f("c.json"), "-e", &f("env.json"), "-d", &f("data.csv"), "--no-color"]);
            a.extend(args(extra));
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = main(&a, &mut out, &mut err);
            (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
        };

        let (code, out, err) = run(&["--reporter-json-export", &f("r.json"), "--reporter-junit-export", &f("r.xml"), "--export-environment", &f("env-out.json")]);
        assert_eq!(code, 1, "one assertion fails\n{out}{err}");
        assert!(out.contains("Iteration 2/2"), "{out}");
        assert!(out.contains("→ Auth / Login"), "{out}");
        assert!(out.contains("┊ hello bob"), "{out}");
        assert!(out.contains("404 Not Found"), "{out}");
        assert!(out.contains("AssertionError   is admin"), "{out}");
        assert!(out.contains("inside \"Me\" (iteration 2)"), "{out}");

        let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(f("r.json")).unwrap()).unwrap();
        assert_eq!(report["stats"]["requests"]["total"], 6);
        assert_eq!(report["stats"]["assertions"], json!({ "total": 6, "failed": 1, "skipped": 2 }));
        assert_eq!(report["executions"][0]["folder"], json!(["Auth"]));

        let xml = std::fs::read_to_string(f("r.xml")).unwrap();
        assert!(xml.contains(r#"<testsuites name="Demo &lt;API&gt;" tests="8" failures="1" errors="0""#), "{xml}");
        assert!(xml.contains(r#"<testsuite name="Me (iteration 2)""#), "{xml}");
        assert!(xml.contains("<failure type=\"AssertionError\""), "{xml}");
        assert!(xml.contains("<skipped/>"), "{xml}");

        let env = crate::import::import_postman_environment(&std::fs::read_to_string(f("env-out.json")).unwrap()).unwrap();
        assert_eq!(env.variables[0].value, "tok-bob", "the environment as left by the scripts");

        // Only one folder, first data row, stop at the first failure.
        let (code, out, _) = run(&["--folder", "Me", "-n", "1", "--suppress-exit-code"]);
        assert_eq!(code, 0, "--suppress-exit-code");
        assert!(!out.contains("Login"), "{out}");
        let (code, out, _) = run(&["--bail", "--silent", "--env-var", "token=wrong"]);
        assert_eq!(code, 1);
        assert!(out.is_empty(), "--silent: {out}");

        let (code, _, err) = run(&["--folder", "Nope"]);
        assert_eq!((code, err.contains("no folder or request named 'Nope'")), (EXIT_USAGE, true), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
