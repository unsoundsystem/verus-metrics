use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

// ─── Public API ───────────────────────────────────────────────────────────────

/// Locate the crate root file (`lib.rs` or `main.rs`) under `dir`.
/// Checks `dir/{lib,main}.rs` and then `dir/src/{lib,main}.rs`.
/// Returns `Err` with a user-facing message if neither is found.
pub fn find_crate_root(dir: &Path) -> Result<PathBuf, String> {
    for candidate in &[
        dir.join("lib.rs"),
        dir.join("main.rs"),
        dir.join("src").join("lib.rs"),
        dir.join("src").join("main.rs"),
    ] {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    Err(format!(
        "{}: not a crate root — no lib.rs or main.rs found \
         (checked {dir}/lib.rs, {dir}/main.rs, {dir}/src/lib.rs, {dir}/src/main.rs)",
        dir.display(),
        dir = dir.display()
    ))
}

/// BFS-collect all `.rs` files reachable from `root` via `mod` declarations.
/// Unresolvable `mod` names (file not on disk) produce a warning on stderr and
/// are skipped, since they may be cfg-gated or come from build scripts.
pub fn collect_crate_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut result: Vec<PathBuf> = Vec::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();

    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {}", root.display(), e))?;
    queue.push_back(root);

    while let Some(file) = queue.pop_front() {
        if visited.contains(&file) {
            continue;
        }
        visited.insert(file.clone());

        let source = std::fs::read_to_string(&file)
            .map_err(|e| format!("{}: {}", file.display(), e))?;
        result.push(file.clone());

        let parent = file.parent().unwrap_or(Path::new("."));
        for mod_name in extract_mod_names(&source) {
            match resolve_mod(parent, &mod_name) {
                Some(p) => {
                    if let Ok(canon) = p.canonicalize() {
                        if !visited.contains(&canon) {
                            queue.push_back(canon);
                        }
                    }
                }
                None => {
                    eprintln!(
                        "warning: mod {} declared in {} — neither {}/{}.rs nor {}/{}/mod.rs found, skipping",
                        mod_name,
                        file.display(),
                        parent.display(),
                        mod_name,
                        parent.display(),
                        mod_name,
                    );
                }
            }
        }
    }

    Ok(result)
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Try the two standard Rust module file locations.
fn resolve_mod(parent: &Path, name: &str) -> Option<PathBuf> {
    let direct = parent.join(format!("{}.rs", name));
    if direct.is_file() {
        return Some(direct);
    }
    let subdir = parent.join(name).join("mod.rs");
    if subdir.is_file() {
        return Some(subdir);
    }
    None
}

/// Strip block comments from `source`, replacing comment content with spaces
/// (newlines are preserved so line numbers stay correct).
fn strip_block_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut depth = 0usize; // Rust doesn't support nested block comments, but handle gracefully

    while i < n {
        if depth > 0 {
            if i + 1 < n && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                depth -= 1;
                out.push(' ');
                out.push(' ');
                i += 2;
            } else {
                // Preserve newlines; replace everything else with spaces.
                out.push(if bytes[i] == b'\n' { '\n' } else { ' ' });
                i += 1;
            }
        } else if i + 1 < n && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            out.push(' ');
            out.push(' ');
            i += 2;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Extract module names from `mod foo;` declarations (not inline `mod foo { }`).
/// Handles `pub`, `pub(...)`, and single-line `#[attr]` prefixes.
/// Commented-out declarations (both `//` and `/* */`) are excluded.
fn extract_mod_names(source: &str) -> Vec<String> {
    let stripped = strip_block_comments(source);
    let mut names = Vec::new();

    for line in stripped.lines() {
        // Strip line comment.
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        }
        .trim();

        if let Some(name) = parse_mod_decl(code) {
            names.push(name);
        }
    }

    names
}

/// Attempt to parse `code` (a single trimmed, comment-stripped line) as a
/// `mod <name>;` declaration.  Returns `None` for inline modules (`{ ... }`),
/// non-mod lines, or malformed input.
fn parse_mod_decl(code: &str) -> Option<String> {
    let code = strip_outer_attrs(code).trim();
    let code = strip_visibility(code).trim();

    let rest = code.strip_prefix("mod")?;
    // Must have whitespace after `mod`.
    let rest = rest.strip_prefix(|c: char| c.is_whitespace())?;
    let rest = rest.trim_start();

    // Collect the identifier.
    let ident_end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = &rest[..ident_end];
    if name.is_empty() {
        return None;
    }

    // After the identifier: must end with `;` (not `{`).
    let after = rest[ident_end..].trim();
    if after == ";" {
        Some(name.to_string())
    } else {
        // Inline module or malformed: skip.
        None
    }
}

/// Strip a leading `#[...]` attribute (single-line only).
fn strip_outer_attrs(code: &str) -> &str {
    let mut s = code.trim();
    while s.starts_with('#') {
        match s.find(']') {
            Some(idx) => s = s[idx + 1..].trim(),
            None => break,
        }
    }
    s
}

/// Strip a leading `pub`, `pub(crate)`, `pub(super)`, `pub(in ...)` prefix.
fn strip_visibility(code: &str) -> &str {
    let s = code.trim();
    let rest = match s.strip_prefix("pub") {
        Some(r) => r,
        None => return s,
    };
    let rest = rest.trim_start();
    if rest.starts_with('(') {
        match rest.find(')') {
            Some(idx) => &rest[idx + 1..],
            None => rest,
        }
    } else {
        rest
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn names(src: &str) -> Vec<String> {
        extract_mod_names(src)
    }

    #[test]
    fn test_simple_mod() {
        assert_eq!(names("mod foo;"), vec!["foo"]);
    }

    #[test]
    fn test_pub_mod() {
        assert_eq!(names("pub mod bar;"), vec!["bar"]);
    }

    #[test]
    fn test_pub_crate_mod() {
        assert_eq!(names("pub(crate) mod baz;"), vec!["baz"]);
    }

    #[test]
    fn test_pub_super_mod() {
        assert_eq!(names("pub(super) mod qux;"), vec!["qux"]);
    }

    #[test]
    fn test_cfg_attr_mod() {
        assert_eq!(names("#[cfg(test)] mod tests;"), vec!["tests"]);
    }

    #[test]
    fn test_cfg_pub_mod() {
        assert_eq!(names("#[cfg(feature = \"x\")] pub mod feat;"), vec!["feat"]);
    }

    #[test]
    fn test_inline_mod_skipped() {
        assert!(names("mod inline { }").is_empty());
        assert!(names("mod inline {").is_empty());
    }

    #[test]
    fn test_line_comment_skipped() {
        assert!(names("// mod foo;").is_empty());
        assert!(names("    // mod foo;").is_empty());
    }

    #[test]
    fn test_block_comment_skipped() {
        assert!(names("/* mod foo; */").is_empty());
        assert!(names("/* mod foo;\n   mod bar; */\nmod baz;") == vec!["baz"]);
    }

    #[test]
    fn test_multiple_mods() {
        let src = "mod a;\npub mod b;\n// mod c;\nmod d;";
        assert_eq!(names(src), vec!["a", "b", "d"]);
    }

    #[test]
    fn test_strip_block_comments_preserves_newlines() {
        let src = "mod a;\n/* mod b; */\nmod c;";
        let stripped = strip_block_comments(src);
        assert_eq!(stripped.lines().count(), src.lines().count());
        assert!(names(src).contains(&"a".to_string()));
        assert!(names(src).contains(&"c".to_string()));
        assert!(!names(src).contains(&"b".to_string()));
    }
}
