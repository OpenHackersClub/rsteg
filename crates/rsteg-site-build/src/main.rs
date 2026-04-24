//! Render marked markdown sections from source READMEs into the static site.
//!
//! Design: markdown files own the prose. Site HTML templates declare
//! placeholder regions with `<!-- site:begin NAME -->` / `<!-- site:end NAME -->`.
//! Source READMEs declare matching regions with the same markers around the
//! markdown that should fill them. The builder extracts each source region,
//! renders it to HTML (pulldown-cmark), and splices it into every matching
//! placeholder in the target templates — preserving the `<!-- site:begin -->`
//! and `<!-- site:end -->` markers so subsequent runs are idempotent.
//!
//! Workflow:
//!   cargo run -p rsteg-site-build
//! ...writes `public/index.html` (and any other wired-up target). CI runs
//! the same command and then `git diff --exit-code` to assert README and
//! site are in sync.

#![deny(unsafe_code)]

use pulldown_cmark::{html, Options, Parser};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// One (section-name, source-markdown-path) wiring. A source README may
/// contribute many named sections; each section is looked up by the same
/// name in the target HTML file.
struct SourceBinding {
    name: &'static str,
    source: &'static str,
}

/// All sources the builder knows about. Order-independent — section names
/// must be unique across the set.
const BINDINGS: &[SourceBinding] = &[
    SourceBinding { name: "intro",          source: "README.md" },
    SourceBinding { name: "lsb-basics",     source: "README.md" },
    SourceBinding { name: "install",        source: "README.md" },
    SourceBinding { name: "bench-headline", source: "bench/README.md" },
];

/// Targets to rewrite. Each target is scanned for `<!-- site:begin NAME -->`
/// ... `<!-- site:end NAME -->` pairs and each pair's contents are replaced
/// with the rendered section named NAME.
const TARGETS: &[&str] = &[
    "public/index.html",
    "public/benchmarks/index.html",
];

fn main() -> ExitCode {
    let repo_root = repo_root();

    let mut rendered: HashMap<String, String> = HashMap::new();
    for b in BINDINGS {
        let md_path = repo_root.join(b.source);
        let md = match fs::read_to_string(&md_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: reading {}: {e}", md_path.display());
                return ExitCode::from(1);
            }
        };
        let body = match extract_section(&md, b.name) {
            Some(s) => s,
            None => {
                eprintln!(
                    "error: section `{}` not found in {}",
                    b.name,
                    md_path.display()
                );
                return ExitCode::from(1);
            }
        };
        rendered.insert(b.name.to_string(), render_markdown(body));
    }

    let mut changed = 0usize;
    for rel in TARGETS {
        let path = repo_root.join(rel);
        let original = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: reading {}: {e}", path.display());
                return ExitCode::from(1);
            }
        };
        let updated = match splice_all(&original, &rendered) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: splicing {}: {e}", path.display());
                return ExitCode::from(1);
            }
        };
        if updated != original {
            if let Err(e) = fs::write(&path, &updated) {
                eprintln!("error: writing {}: {e}", path.display());
                return ExitCode::from(1);
            }
            changed += 1;
            println!("updated {}", rel);
        } else {
            println!("unchanged {}", rel);
        }
    }
    println!("{changed} file(s) changed.");
    ExitCode::SUCCESS
}

/// Walk up from CARGO_MANIFEST_DIR to find the workspace root (the dir
/// that contains Cargo.toml with `[workspace]`). Falls back to the
/// manifest dir's grandparent if detection fails.
fn repo_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in start.ancestors() {
        let cargo = ancestor.join("Cargo.toml");
        if let Ok(s) = fs::read_to_string(&cargo) {
            if s.contains("[workspace]") {
                return ancestor.to_path_buf();
            }
        }
    }
    start
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(start)
}

/// Find `<!-- site:begin NAME -->` ... `<!-- site:end NAME -->` in `src`
/// and return the substring between them (trimmed of surrounding newlines).
fn extract_section<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let begin = format!("<!-- site:begin {name} -->");
    let end = format!("<!-- site:end {name} -->");
    let start = src.find(&begin)? + begin.len();
    let stop = src[start..].find(&end)? + start;
    Some(src[start..stop].trim_matches('\n'))
}

/// Replace every `<!-- site:begin NAME --> ... <!-- site:end NAME -->`
/// region in `html_src` with the rendered markdown for that name. Markers
/// are preserved so the file stays idempotent across runs. Unknown
/// section names cause an error — catches typos.
fn splice_all(html_src: &str, rendered: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::with_capacity(html_src.len());
    let mut cursor = 0usize;

    loop {
        let Some(begin_rel) = html_src[cursor..].find("<!-- site:begin ") else {
            out.push_str(&html_src[cursor..]);
            return Ok(out);
        };
        let begin_at = cursor + begin_rel;
        // Copy the prefix up to and including the begin marker.
        let begin_end_rel = html_src[begin_at..]
            .find(" -->")
            .ok_or_else(|| format!("unterminated begin marker at byte {begin_at}"))?;
        let name_start = begin_at + "<!-- site:begin ".len();
        let name_end = begin_at + begin_end_rel;
        let name = html_src[name_start..name_end].trim().to_string();
        let begin_full_end = begin_at + begin_end_rel + " -->".len();

        let end_marker = format!("<!-- site:end {name} -->");
        let end_rel = html_src[begin_full_end..]
            .find(&end_marker)
            .ok_or_else(|| format!("no matching end marker for `{name}`"))?;
        let end_at = begin_full_end + end_rel;

        let rendered_html = rendered
            .get(&name)
            .ok_or_else(|| format!("section `{name}` referenced but not wired in BINDINGS"))?;

        out.push_str(&html_src[cursor..begin_full_end]);
        out.push('\n');
        out.push_str(rendered_html);
        if !rendered_html.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&end_marker);
        cursor = end_at + end_marker.len();
    }
}

fn render_markdown(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_trims_surrounding_newlines() {
        let src = "before\n<!-- site:begin x -->\nbody\n<!-- site:end x -->\nafter\n";
        assert_eq!(extract_section(src, "x"), Some("body"));
    }

    #[test]
    fn extract_returns_none_for_missing() {
        assert!(extract_section("no markers here", "anything").is_none());
    }

    #[test]
    fn splice_replaces_body_preserving_markers() {
        let html = "a\n<!-- site:begin x -->\nOLD\n<!-- site:end x -->\nb\n";
        let mut m = HashMap::new();
        m.insert("x".to_string(), "<p>new</p>".to_string());
        let got = splice_all(html, &m).unwrap();
        assert!(got.contains("<!-- site:begin x -->"));
        assert!(got.contains("<!-- site:end x -->"));
        assert!(got.contains("<p>new</p>"));
        assert!(!got.contains("OLD"));
    }

    #[test]
    fn splice_errors_on_unknown_section() {
        let html = "<!-- site:begin mystery --><!-- site:end mystery -->";
        let err = splice_all(html, &HashMap::new()).unwrap_err();
        assert!(err.contains("mystery"));
    }

    #[test]
    fn splice_idempotent_across_runs() {
        let html = "<!-- site:begin x -->\nOLD\n<!-- site:end x -->\n";
        let mut m = HashMap::new();
        m.insert("x".to_string(), "<p>new</p>".to_string());
        let once = splice_all(html, &m).unwrap();
        let twice = splice_all(&once, &m).unwrap();
        assert_eq!(once, twice);
    }
}
