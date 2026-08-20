//! Validate that every top-level integration-test target is discoverable by an
//! intentional CI selector.
//!
//! This is a classification gate, not proof that ignored tests execute or that
//! any test meaningfully exercises tenancy, acceptance, or chaos behavior.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const API_SELECTOR: &str = "binary(/^api_/)";
const INTEGRATION_SELECTOR: &str = "binary(/^integration_/)";
const ACCEPTANCE_SELECTOR: &str = "binary(acceptance)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Class {
    Api,
    Integration,
    Acceptance,
    DeferredChaos,
}

impl Class {
    fn selector(self) -> Option<&'static str> {
        match self {
            Self::Api => Some(API_SELECTOR),
            Self::Integration => Some(INTEGRATION_SELECTOR),
            Self::Acceptance => Some(ACCEPTANCE_SELECTOR),
            Self::DeferredChaos => None,
        }
    }
}

#[derive(Debug)]
struct Target {
    name: String,
    class: Class,
    tests: usize,
    ignored: usize,
}

pub fn run() -> Result<()> {
    let root = Path::new(".");
    let files = test_files(&root.join("tests"))?;
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .context("reading .github/workflows/ci.yml")?;
    let nightly = fs::read_to_string(root.join(".github/workflows/nightly.yml"))
        .context("reading .github/workflows/nightly.yml")?;
    let targets = validate_shape(&files, &ci, &nightly)?;

    let acceptance = targets
        .iter()
        .find(|target| target.class == Class::Acceptance)
        .context("tests/acceptance.rs is required")?;
    println!(
        "xtask test-shape: {} classified targets; acceptance has {} active / {} ignored tests; {} chaos targets are explicitly deferred",
        targets.len(),
        acceptance.tests - acceptance.ignored,
        acceptance.ignored,
        targets
            .iter()
            .filter(|target| target.class == Class::DeferredChaos)
            .count()
    );
    Ok(())
}

fn test_files(tests_dir: &Path) -> Result<Vec<(String, String)>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(tests_dir)
        .with_context(|| format!("reading {}", tests_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();

    paths
        .into_iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .map(|path| {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .with_context(|| format!("non-UTF-8 test target path {}", path.display()))?
                .to_owned();
            let source =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            Ok((name, source))
        })
        .collect()
}

fn validate_shape(
    files: &[(String, String)],
    ci_workflow: &str,
    nightly_workflow: &str,
) -> Result<Vec<Target>> {
    let ci_code = workflow_code(ci_workflow);
    let nightly_code = workflow_code(nightly_workflow);
    if nightly_code.contains("chaos_") || nightly_code.contains("--run-ignored") {
        bail!("nightly.yml must not claim or execute deferred chaos coverage");
    }
    if ci_code.contains("chaos_") {
        bail!("ci.yml must not execute deferred chaos skeletons");
    }

    let mut targets = Vec::with_capacity(files.len());
    for (name, source) in files {
        let class = classify(name)?;
        if let Some(selector) = class.selector() {
            if !ci_code.contains(selector) {
                bail!("test target `{name}` has no matching `{selector}` selector in ci.yml");
            }
        }

        let syntax = syn::parse_file(source)
            .with_context(|| format!("parsing tests/{name}.rs while validating test shape"))?;
        let (tests, ignored) = count_tests(&syntax.items);
        if class == Class::DeferredChaos {
            if tests == 0 {
                bail!("deferred chaos target `{name}` must contain at least one test skeleton");
            }
            if ignored != tests {
                bail!(
                    "deferred chaos target `{name}` has {} active test(s); every scenario must remain #[ignore] until a real harness is wired",
                    tests - ignored
                );
            }
        }
        targets.push(Target {
            name: name.clone(),
            class,
            tests,
            ignored,
        });
    }

    for required in [Class::Api, Class::Integration, Class::Acceptance] {
        if !targets.iter().any(|target| target.class == required) {
            bail!("no active {required:?} target found under tests/");
        }
    }
    // Read the field in production code as well as tests; target names are
    // useful context in debugger output and should never silently be empty.
    if targets.iter().any(|target| target.name.is_empty()) {
        bail!("empty test target name");
    }
    Ok(targets)
}

fn workflow_code(workflow: &str) -> String {
    workflow
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn classify(name: &str) -> Result<Class> {
    if name == "acceptance" {
        Ok(Class::Acceptance)
    } else if name.starts_with("api_") {
        Ok(Class::Api)
    } else if name.starts_with("integration_") {
        Ok(Class::Integration)
    } else if name.starts_with("chaos_") {
        Ok(Class::DeferredChaos)
    } else {
        bail!(
            "unknown top-level test target `{name}`; use api_*, integration_*, acceptance.rs, or an explicitly deferred chaos_* skeleton"
        )
    }
}

fn count_tests(items: &[syn::Item]) -> (usize, usize) {
    let mut tests = 0;
    let mut ignored = 0;
    for item in items {
        match item {
            syn::Item::Fn(function) if has_attr(&function.attrs, "test") => {
                tests += 1;
                if has_attr(&function.attrs, "ignore") {
                    ignored += 1;
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    let (nested_tests, nested_ignored) = count_tests(nested);
                    tests += nested_tests;
                    ignored += nested_ignored;
                }
            }
            _ => {}
        }
    }
    (tests, ignored)
}

fn has_attr(attrs: &[syn::Attribute], expected: &str) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == expected)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflows() -> (String, String) {
        (
            format!(
                "run: cargo nextest run -E '{API_SELECTOR}'\nrun: cargo nextest run -E '{INTEGRATION_SELECTOR}'\nrun: cargo nextest run -E '{ACCEPTANCE_SELECTOR}'\n"
            ),
            "# UI-only nightly; chaos has no executable harness.\n".to_owned(),
        )
    }

    fn fixtures() -> Vec<(String, String)> {
        vec![
            ("api_users".into(), "#[test]\nfn api() {}\n".into()),
            (
                "integration_db".into(),
                "#[tokio::test]\nasync fn db() {}\n".into(),
            ),
            (
                "acceptance".into(),
                "#[test]\nfn active() {}\n#[test]\n#[ignore]\nfn deferred() {}\n".into(),
            ),
            (
                "chaos_network".into(),
                "#[tokio::test]\n#[ignore = \"no harness\"]\nasync fn network() {}\n".into(),
            ),
        ]
    }

    #[test]
    fn test_shape_fixture_maps_active_targets_and_defers_ignored_chaos() {
        let (ci, nightly) = workflows();
        let targets = validate_shape(&fixtures(), &ci, &nightly).unwrap();
        let acceptance = targets
            .iter()
            .find(|target| target.class == Class::Acceptance)
            .unwrap();
        assert_eq!((acceptance.tests, acceptance.ignored), (2, 1));
    }

    #[test]
    fn test_shape_fixture_rejects_unknown_top_level_target() {
        let (ci, nightly) = workflows();
        let mut files = fixtures();
        files.push(("smoke".into(), "#[test]\nfn smoke() {}\n".into()));
        assert!(validate_shape(&files, &ci, &nightly)
            .unwrap_err()
            .to_string()
            .contains("unknown top-level test target"));
    }

    #[test]
    fn test_shape_fixture_rejects_active_chaos_scenario() {
        let (ci, nightly) = workflows();
        let mut files = fixtures();
        files.last_mut().unwrap().1 = "#[test]\nfn network() {}\n".into();
        assert!(validate_shape(&files, &ci, &nightly)
            .unwrap_err()
            .to_string()
            .contains("every scenario must remain #[ignore]"));
    }

    #[test]
    fn test_shape_fixture_rejects_missing_ci_selector() {
        let (_, nightly) = workflows();
        assert!(validate_shape(&fixtures(), API_SELECTOR, &nightly)
            .unwrap_err()
            .to_string()
            .contains("has no matching"));
    }
}
