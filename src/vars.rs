//! `{{variable}}` resolution.
//!
//! Precedence (highest first), same as Postman: environment > collection > globals.
//! Dynamic variables such as `{{$guid}}` are generated on every substitution.

use std::collections::HashMap;

use crate::model::KeyValue;

pub type VarMap = HashMap<String, String>;

/// Build the variable map. Later layers override earlier ones.
pub fn build_vars(layers: &[&[KeyValue]]) -> VarMap {
    let mut map = VarMap::new();
    for layer in layers {
        for kv in layer.iter().filter(|kv| kv.enabled && !kv.key.is_empty()) {
            map.insert(kv.key.clone(), kv.value.clone());
        }
    }
    map
}

fn dynamic(name: &str) -> Option<String> {
    let now = chrono::Utc::now();
    Some(match name {
        "$guid" | "$randomUUID" => uuid::Uuid::new_v4().to_string(),
        "$timestamp" => now.timestamp().to_string(),
        "$isoTimestamp" => now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "$randomInt" => (uuid::Uuid::new_v4().as_u128() % 1001).to_string(),
        "$randomBoolean" => uuid::Uuid::new_v4().as_u128().is_multiple_of(2).to_string(),
        "$randomEmail" => format!("user{}@example.com", uuid::Uuid::new_v4().as_u128() % 100_000),
        _ => return None,
    })
}

pub const DYNAMIC_VARS: [&str; 7] =
    ["$guid", "$randomUUID", "$timestamp", "$isoTimestamp", "$randomInt", "$randomBoolean", "$randomEmail"];

/// Calls `f(name, start, end)` for every `{{name}}` occurrence (byte offsets include braces).
fn for_each_var(s: &str, mut f: impl FnMut(&str, usize, usize)) {
    let mut i = 0;
    while let Some(open) = s[i..].find("{{") {
        let start = i + open;
        match s[start + 2..].find("}}") {
            Some(close) => {
                let end = start + 2 + close + 2;
                let name = s[start + 2..start + 2 + close].trim();
                f(name, start, end);
                i = end;
            }
            None => break,
        }
    }
}

/// Replace `{{name}}` with values. Unknown variables are left untouched.
/// Values may themselves contain variables; resolved up to a few levels deep.
pub fn substitute(s: &str, vars: &VarMap) -> String {
    let mut current = s.to_string();
    for _ in 0..5 {
        if !current.contains("{{") {
            break;
        }
        let mut out = String::with_capacity(current.len());
        let mut last = 0;
        let mut changed = false;
        for_each_var(&current, |name, start, end| {
            let value = vars.get(name).cloned().or_else(|| dynamic(name));
            if let Some(v) = value {
                out.push_str(&current[last..start]);
                out.push_str(&v);
                last = end;
                changed = true;
            }
        });
        out.push_str(&current[last..]);
        current = out;
        if !changed {
            break;
        }
    }
    current
}

/// Names of the `{{variables}}` used in `s` (dynamic `$` variables excluded), in order, without duplicates.
pub fn var_names(s: &str, out: &mut Vec<String>) {
    for_each_var(s, |name, _, _| {
        if !name.is_empty() && !name.starts_with('$') && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    });
}

/// Names of variables used in `s` that cannot be resolved.
#[cfg_attr(not(test), allow(dead_code))]
pub fn unresolved(s: &str, vars: &VarMap) -> Vec<String> {
    let mut missing = Vec::new();
    for_each_var(s, |name, _, _| {
        if !vars.contains_key(name) && dynamic(name).is_none() && !missing.iter().any(|m| m == name) {
            missing.push(name.to_string());
        }
    });
    missing
}

/// Byte ranges of `{{...}}` tokens with a flag telling whether they resolve. Used for highlighting.
pub fn var_spans(s: &str, vars: &VarMap) -> Vec<(usize, usize, bool)> {
    let mut spans = Vec::new();
    for_each_var(s, |name, start, end| {
        let ok = vars.contains_key(name) || DYNAMIC_VARS.contains(&name);
        spans.push((start, end, ok));
    });
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_nesting() {
        let globals = vec![KeyValue::new("host", "global.io"), KeyValue::new("v", "1")];
        let env = vec![KeyValue::new("host", "env.io"), KeyValue::new("base", "https://{{host}}/v{{v}}")];
        let vars = build_vars(&[&globals, &env]);
        assert_eq!(substitute("{{base}}/users/{{ id }}", &vars), "https://env.io/v1/users/{{ id }}");
        assert_eq!(unresolved("{{base}}/{{id}}/{{$guid}}", &vars), vec!["id".to_string()]);
    }

    #[test]
    fn dynamic_vars() {
        let vars = VarMap::new();
        let out = substitute("{{$guid}}", &vars);
        assert_eq!(out.len(), 36);
    }
}
