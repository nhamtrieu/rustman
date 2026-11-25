//! CSV / JSON import and export for datasets.

use serde_json::Value;

use crate::model::Dataset;

/// RFC 4180-ish CSV: quoted fields, `""` escapes, CRLF or LF, delimiter `,` or `;` (auto-detected).
pub fn parse_csv(text: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let text = text.trim_start_matches('\u{feff}');
    let first_line = text.lines().next().unwrap_or("");
    let delim = if first_line.matches(';').count() > first_line.matches(',').count() { ';' } else { ',' };
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_quotes = true,
            c if c == delim => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if in_quotes {
        return Err("Unterminated quoted field".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        records.push(row);
    }
    records.retain(|r| !(r.len() == 1 && r[0].trim().is_empty()));
    let mut it = records.into_iter();
    let columns: Vec<String> = it.next().ok_or("The CSV file is empty")?.into_iter().map(|c| c.trim().to_string()).collect();
    let rows = it
        .map(|mut r| {
            r.resize(columns.len(), String::new());
            r
        })
        .collect();
    Ok((columns, rows))
}

pub fn to_csv(d: &Dataset) -> String {
    let esc = |s: &str| {
        if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    };
    let mut out = d.columns.iter().map(|c| esc(c)).collect::<Vec<_>>().join(",");
    out.push('\n');
    for r in &d.rows {
        out.push_str(&r.iter().map(|c| esc(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// JSON array of objects (Postman data-file format). Column order follows first appearance.
pub fn parse_json(text: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {e}"))?;
    let items = v.as_array().ok_or("Expected a JSON array of objects")?;
    let mut columns: Vec<String> = Vec::new();
    for item in items {
        let obj = item.as_object().ok_or("Every item must be an object")?;
        for k in obj.keys() {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
    }
    let rows = items
        .iter()
        .map(|item| {
            columns
                .iter()
                .map(|c| match item.get(c) {
                    None | Some(Value::Null) => String::new(),
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                })
                .collect()
        })
        .collect();
    Ok((columns, rows))
}

/// Imports a file by extension (`.json` or anything else as CSV).
pub fn import_file(path: &std::path::Path) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let is_json = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) || text.trim_start().starts_with('[');
    if is_json { parse_json(&text) } else { parse_csv(&text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_roundtrip_with_quotes() {
        let text = "name,note\r\nNham,\"hello, \"\"world\"\"\"\nAn,\"multi\nline\"\n";
        let (cols, rows) = parse_csv(text).unwrap();
        assert_eq!(cols, ["name", "note"]);
        assert_eq!(rows, vec![vec!["Nham".to_string(), "hello, \"world\"".into()], vec!["An".into(), "multi\nline".into()]]);
        let d = Dataset { columns: cols, rows, ..Default::default() };
        assert_eq!(parse_csv(&to_csv(&d)).unwrap().1, d.rows);
    }

    #[test]
    fn csv_semicolon_and_short_rows() {
        let (cols, rows) = parse_csv("a;b;c\n1;2\n").unwrap();
        assert_eq!(cols.len(), 3);
        assert_eq!(rows[0], ["1", "2", ""]);
    }

    #[test]
    fn json_data_file() {
        let (cols, rows) = parse_json(r#"[{"id":1,"name":"A"},{"name":"B","ok":true}]"#).unwrap();
        assert_eq!(cols, ["id", "name", "ok"]);
        assert_eq!(rows[1], ["", "B", "true"]);
        assert!(parse_json(r#"{"a":1}"#).is_err());
    }
}
