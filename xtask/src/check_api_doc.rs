//! `API.md` ↔ `openapi.yaml` operation coverage gate.
//!
//! `API.md` contains exactly one machine-checked table between the
//! `BEGIN CANONICAL OPENAPI OPERATIONS` and
//! `END CANONICAL OPENAPI OPERATIONS` comments. Each row has the shape
//! `Verb | Path | Purpose`. The gate compares normalized `(method, path)`
//! pairs in both directions; parameter names inside braces are ignored so
//! `{project_id}` and `{projectId}` identify the same route.
//!
//! Direct poem routes and future/design-only routes belong outside the
//! marked table. Prose elsewhere in `API.md` is deliberately not parsed.

use std::collections::BTreeMap;
use std::fs;

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde_yaml::Value;

const SPEC_PATH: &str = "openapi.yaml";
const DOC_PATH: &str = "API.md";
const BEGIN_MARKER: &str = "<!-- BEGIN CANONICAL OPENAPI OPERATIONS -->";
const END_MARKER: &str = "<!-- END CANONICAL OPENAPI OPERATIONS -->";
const HTTP_METHODS: &[&str] = &[
    "GET", "PUT", "POST", "DELETE", "OPTIONS", "HEAD", "PATCH", "TRACE",
];

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OperationKey {
    method: String,
    path: String,
}

#[derive(Clone, Debug)]
struct SpecOperation {
    key: OperationKey,
    method: String,
    path: String,
}

#[derive(Clone, Debug)]
struct DocumentedOperation {
    key: OperationKey,
    method: String,
    path: String,
    line: usize,
}

/// Replace every OpenAPI parameter name with `{}` while preserving the
/// surrounding path. Method names are normalized separately.
fn normalize_path(path: &str) -> String {
    let re = Regex::new(r"\{[^}]*\}").expect("parameter regex is valid");
    re.replace_all(path, "{}").into_owned()
}

fn operation_key(method: &str, path: &str) -> OperationKey {
    OperationKey {
        method: method.to_ascii_uppercase(),
        path: normalize_path(path),
    }
}

pub fn run() -> Result<()> {
    let spec_raw = fs::read_to_string(SPEC_PATH).with_context(|| format!("reading {SPEC_PATH}"))?;
    let doc_raw = fs::read_to_string(DOC_PATH).with_context(|| format!("reading {DOC_PATH}"))?;

    let spec = collect_spec_operations(&spec_raw, SPEC_PATH)?;
    let documented = collect_documented_operations(&doc_raw, DOC_PATH)?;
    let errors = compare_operations(&spec, &documented, DOC_PATH);

    if errors.is_empty() {
        println!(
            "xtask check-api-doc: {} OpenAPI operation(s), exact method/path coverage in {DOC_PATH}",
            spec.len()
        );
        Ok(())
    } else {
        for error in &errors {
            eprintln!("{error}");
        }
        Err(anyhow!("{} API documentation drift error(s)", errors.len()))
    }
}

/// Parse every HTTP operation below the OpenAPI `paths` mapping. Path-item
/// metadata such as `parameters`, `summary`, and `servers` is ignored.
fn collect_spec_operations(raw: &str, source: &str) -> Result<Vec<SpecOperation>> {
    let value: Value = serde_yaml::from_str(raw).with_context(|| format!("parsing {source}"))?;
    let paths = value
        .get("paths")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("{source} has no `paths` mapping"))?;

    let mut operations = Vec::new();
    for (raw_path, raw_item) in paths {
        let path = raw_path
            .as_str()
            .ok_or_else(|| anyhow!("{source}: a `paths` key is not a string"))?;
        let item = raw_item
            .as_mapping()
            .ok_or_else(|| anyhow!("{source}: path item `{path}` is not a mapping"))?;

        for (raw_method, _operation) in item {
            let Some(method) = raw_method.as_str() else {
                continue;
            };
            let method = method.to_ascii_uppercase();
            if !HTTP_METHODS.contains(&method.as_str()) {
                continue;
            }
            operations.push(SpecOperation {
                key: operation_key(&method, path),
                method,
                path: path.to_string(),
            });
        }
    }
    operations.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(operations)
}

/// Parse only the marked canonical table. Keeping the boundary structural
/// lets `API.md` describe direct poem routes and unshipped designs without
/// accidentally changing the generated-contract gate.
fn collect_documented_operations(raw: &str, source: &str) -> Result<Vec<DocumentedOperation>> {
    let mut in_table = false;
    let mut saw_begin = false;
    let mut saw_end = false;
    let mut table_line = 0usize;
    let mut operations = Vec::new();

    for (index, line) in raw.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();

        if trimmed == BEGIN_MARKER {
            if saw_begin {
                return Err(anyhow!(
                    "{source}:{line_number}: duplicate canonical-operation begin marker"
                ));
            }
            saw_begin = true;
            in_table = true;
            continue;
        }
        if trimmed == END_MARKER {
            if !in_table {
                return Err(anyhow!(
                    "{source}:{line_number}: canonical-operation end marker without begin marker"
                ));
            }
            saw_end = true;
            in_table = false;
            continue;
        }
        if !in_table || trimmed.is_empty() {
            continue;
        }

        table_line += 1;
        let cells = markdown_cells(trimmed).ok_or_else(|| {
            anyhow!("{source}:{line_number}: canonical operation table rows must have three cells")
        })?;

        match table_line {
            1 => {
                if cells != ["Verb", "Path", "Purpose"] {
                    return Err(anyhow!(
                        "{source}:{line_number}: expected canonical header `| Verb | Path | Purpose |`"
                    ));
                }
            }
            2 => {
                if !cells.iter().all(|cell| {
                    let body = cell.trim_matches(':');
                    body.len() >= 3 && body.chars().all(|ch| ch == '-')
                }) {
                    return Err(anyhow!(
                        "{source}:{line_number}: malformed canonical table separator"
                    ));
                }
            }
            _ => {
                let method = strip_code_span(&cells[0]).to_ascii_uppercase();
                if !HTTP_METHODS.contains(&method.as_str()) {
                    return Err(anyhow!(
                        "{source}:{line_number}: unsupported HTTP method `{method}` in canonical table"
                    ));
                }
                let path = strip_code_span(&cells[1]);
                if !path.starts_with('/') {
                    return Err(anyhow!(
                        "{source}:{line_number}: canonical operation path must start with `/`: `{path}`"
                    ));
                }
                if cells[2].trim().is_empty() {
                    return Err(anyhow!(
                        "{source}:{line_number}: canonical operation purpose must not be empty"
                    ));
                }
                operations.push(DocumentedOperation {
                    key: operation_key(&method, path),
                    method,
                    path: path.to_string(),
                    line: line_number,
                });
            }
        }
    }

    if !saw_begin {
        return Err(anyhow!("{source}: missing `{BEGIN_MARKER}`"));
    }
    if !saw_end {
        return Err(anyhow!("{source}: missing `{END_MARKER}`"));
    }
    if table_line < 3 {
        return Err(anyhow!("{source}: canonical operation table has no rows"));
    }

    Ok(operations)
}

fn markdown_cells(line: &str) -> Option<[String; 3]> {
    let inner = line.strip_prefix('|')?.strip_suffix('|')?;
    let cells: Vec<String> = inner
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();
    if cells.len() != 3 {
        return None;
    }
    Some([cells[0].clone(), cells[1].clone(), cells[2].clone()])
}

fn strip_code_span(cell: &str) -> &str {
    cell.trim()
        .strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
        .unwrap_or_else(|| cell.trim())
}

fn compare_operations(
    spec: &[SpecOperation],
    documented: &[DocumentedOperation],
    doc_source: &str,
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut spec_by_key: BTreeMap<&OperationKey, &SpecOperation> = BTreeMap::new();
    let mut doc_by_key: BTreeMap<&OperationKey, &DocumentedOperation> = BTreeMap::new();

    for operation in spec {
        if let Some(first) = spec_by_key.insert(&operation.key, operation) {
            errors.push(format!(
                "{SPEC_PATH}: duplicate normalized operation: {} {} (also {} {})",
                operation.method, operation.path, first.method, first.path
            ));
        }
    }
    for operation in documented {
        if let Some(first) = doc_by_key.insert(&operation.key, operation) {
            errors.push(format!(
                "{doc_source}:{}: duplicate canonical operation {} {}; first declared at line {}",
                operation.line, operation.method, operation.path, first.line
            ));
        }
    }

    for (key, operation) in &spec_by_key {
        if !doc_by_key.contains_key(key) {
            errors.push(format!(
                "{SPEC_PATH} operation missing from {doc_source} canonical table: {} {}",
                operation.method, operation.path
            ));
        }
    }
    for (key, operation) in &doc_by_key {
        if !spec_by_key.contains_key(key) {
            errors.push(format!(
                "{doc_source}:{}: canonical operation not present in {SPEC_PATH}: {} {}",
                operation.line, operation.method, operation.path
            ));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(raw_paths: &str) -> String {
        format!("openapi: 3.0.0\npaths:\n{raw_paths}")
    }

    fn doc(rows: &str, trailing: &str) -> String {
        format!(
            "# API\n{BEGIN_MARKER}\n| Verb | Path | Purpose |\n|---|---|---|\n{rows}{END_MARKER}\n{trailing}"
        )
    }

    #[test]
    fn parameter_names_normalize_deterministically() {
        assert_eq!(
            normalize_path("/v1/orgs/{org_id}/projects/{projectId}"),
            "/v1/orgs/{}/projects/{}"
        );
    }

    #[test]
    fn wrong_method_is_bidirectional_drift() {
        let parsed_spec = collect_spec_operations(
            &spec("  /widgets/{widget_id}:\n    post: {}\n"),
            "fixture.yaml",
        )
        .unwrap();
        let parsed_doc = collect_documented_operations(
            &doc("| `GET` | `/widgets/{widgetId}` | Read. |\n", ""),
            "API.md",
        )
        .unwrap();
        assert_eq!(
            compare_operations(&parsed_spec, &parsed_doc, "API.md"),
            vec![
                "openapi.yaml operation missing from API.md canonical table: POST /widgets/{widget_id}",
                "API.md:5: canonical operation not present in openapi.yaml: GET /widgets/{widgetId}",
            ]
        );
    }

    #[test]
    fn reports_spec_only_and_doc_only_operations() {
        let parsed_spec =
            collect_spec_operations(&spec("  /spec-only:\n    get: {}\n"), "fixture.yaml").unwrap();
        let parsed_doc = collect_documented_operations(
            &doc("| `DELETE` | `/doc-only` | Delete. |\n", ""),
            "API.md",
        )
        .unwrap();
        let errors = compare_operations(&parsed_spec, &parsed_doc, "API.md");
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("GET /spec-only"));
        assert!(errors[1].contains("API.md:5"));
        assert!(errors[1].contains("DELETE /doc-only"));
    }

    #[test]
    fn ignores_path_level_openapi_metadata() {
        let parsed = collect_spec_operations(
            &spec(
                "  /widgets/{id}:\n    summary: Widget path\n    parameters:\n      - name: id\n        in: path\n        required: true\n        schema: { type: string }\n    servers: []\n    get: {}\n",
            ),
            "fixture.yaml",
        )
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].method, "GET");
        assert_eq!(parsed[0].path, "/widgets/{id}");
    }

    #[test]
    fn ignores_future_and_direct_route_prose_outside_markers() {
        let input = doc(
            "| `GET` | `/healthz` | Health. |\n",
            "Direct route: `GET /openapi.json`.\n\n| Verb | Path | Purpose |\n|---|---|---|\n| `POST` | `/future` | Design only. |\n| `GET` | `/e/i/{signed}` | Direct route. |\n",
        );
        let parsed = collect_documented_operations(&input, "API.md").unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "/healthz");
    }

    #[test]
    fn doc_only_diagnostic_uses_api_line_number() {
        let input = format!(
            "# API\n\nintro\n\n{BEGIN_MARKER}\n| Verb | Path | Purpose |\n|---|---|---|\n| `GET` | `/extra` | Extra. |\n{END_MARKER}\n"
        );
        let parsed_doc = collect_documented_operations(&input, "API.md").unwrap();
        let errors = compare_operations(&[], &parsed_doc, "API.md");
        assert_eq!(
            errors,
            vec!["API.md:8: canonical operation not present in openapi.yaml: GET /extra"]
        );
    }
}
