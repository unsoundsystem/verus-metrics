mod analysis;
mod parser;
mod types;

use analysis::{analyze_crate, analyze_source};
use clap::Parser as ClapParser;
use std::collections::HashSet;
use std::path::PathBuf;
use types::Counts;
use walkdir::WalkDir;

// ─── CLI ──────────────────────────────────────────────────────────────────────

#[derive(ClapParser)]
#[command(name = "verus-metrics", about = "Count spec/proof/exec lines in Verus code")]
struct Args {
    path: PathBuf,
    #[arg(short, long)]
    verbose: bool,
    /// Restrict reachability analysis to these root functions (comma-separated).
    /// Only spec/proof fns reachable from the requires/ensures or proof blocks
    /// of these functions are counted as reachable.
    /// If omitted, all functions are used as roots.
    #[arg(long, value_delimiter = ',')]
    roots: Vec<String>,
    /// Merge call graphs across all files for cross-file reachability analysis.
    #[arg(long)]
    whole_crate: bool,
}

// ─── Output ───────────────────────────────────────────────────────────────────

fn print_row(label: &str, c: &Counts) {
    println!(
        "{:<50} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6}",
        label,
        c.spec_total(),
        c.proof_total(),
        c.exec,
        c.comment,
        c.blank,
        c.total()
    );
}

fn print_detail(c: &Counts) {
    let code = c.spec_total() + c.proof_total() + c.exec;
    if code == 0 {
        return;
    }
    println!();
    println!(
        "spec:  {:>6} lines ({:.1}%)",
        c.spec_total(),
        c.spec_total() as f64 / code as f64 * 100.0
    );
    println!("  requires/ensures:       {:>6}", c.spec_req_ens);
    println!("  spec fn bodies:");
    println!("    reachable:            {:>6}", c.spec_fn_reachable);
    println!("    unreferenced:         {:>6}", c.spec_fn_unreferenced);
    println!();
    println!(
        "proof: {:>6} lines ({:.1}%)",
        c.proof_total(),
        c.proof_total() as f64 / code as f64 * 100.0
    );
    println!("  proof blocks:           {:>6}", c.proof_block);
    println!("  proof fn bodies:");
    println!("    reachable:            {:>6}", c.proof_fn_reachable);
    println!("    unreferenced:         {:>6}", c.proof_fn_unreferenced);
    println!();
    println!(
        "exec:  {:>6} lines ({:.1}%)",
        c.exec,
        c.exec as f64 / code as f64 * 100.0
    );
    println!();
    println!("assert calls:           {:>6}", c.assert_count);
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();
    let roots: HashSet<String> = args.roots.into_iter().collect();

    let files: Vec<PathBuf> = if args.path.is_dir() {
        WalkDir::new(&args.path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false))
            .map(|e| e.path().to_path_buf())
            .collect()
    } else {
        vec![args.path.clone()]
    };

    if files.is_empty() {
        eprintln!("No .rs files found.");
        std::process::exit(1);
    }

    // Read all sources up front (needed for both modes).
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    for path in &files {
        match std::fs::read_to_string(path) {
            Ok(s) => sources.push((path.clone(), s)),
            Err(e) => eprintln!("Error reading {}: {}", path.display(), e),
        }
    }
    if sources.is_empty() {
        eprintln!("No readable files.");
        std::process::exit(1);
    }

    let header = || {
        println!(
            "{:<50} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6}",
            "File", "spec", "proof", "exec", "comment", "blank", "total"
        );
        println!("{}", "-".repeat(95));
    };

    let short_label = |path: &PathBuf| {
        let d = path.display().to_string();
        if d.len() > 50 { format!("...{}", &d[d.len() - 47..]) } else { d }
    };

    if args.whole_crate {
        // Cross-file analysis: single merged call graph.
        let src_strs: Vec<&str> = sources.iter().map(|(_, s)| s.as_str()).collect();
        let result = analyze_crate(&src_strs, &roots);

        if args.verbose {
            header();
            for (i, (path, _)) in sources.iter().enumerate() {
                print_row(&short_label(path), &result.per_file[i]);
            }
            if sources.len() > 1 {
                println!("{}", "-".repeat(95));
                print_row("TOTAL", &result.total);
            }
        } else {
            header();
            let label = if sources.len() == 1 {
                short_label(&sources[0].0)
            } else {
                "TOTAL".to_string()
            };
            print_row(&label, &result.total);
        }
        print_detail(&result.total);
    } else {
        // Per-file analysis (original behaviour).
        let mut total = Counts::default();
        if args.verbose {
            header();
        }
        for (path, source) in &sources {
            let counts = analyze_source(source, &roots);
            if args.verbose {
                print_row(&short_label(path), &counts);
            }
            total.add(&counts);
        }
        if args.verbose && sources.len() > 1 {
            println!("{}", "-".repeat(95));
            print_row("TOTAL", &total);
        } else if !args.verbose {
            header();
            let label = if sources.len() == 1 {
                short_label(&sources[0].0)
            } else {
                "TOTAL".to_string()
            };
            print_row(&label, &total);
        }
        print_detail(&total);
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_outside_verus() {
        let c = analyze_source("fn add(a: u32, b: u32) -> u32 { a + b }\n", &HashSet::new());
        assert!(c.exec > 0);
        assert_eq!(c.spec_total(), 0);
        assert_eq!(c.proof_total(), 0);
    }

    #[test]
    fn test_req_ens_always_spec() {
        let src = r#"
verus! {
    exec fn foo(n: u32)
        requires n < 100
        ensures n < 200
    {
        let x = n;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.spec_req_ens >= 2, "requires+ensures should be spec: {:?}", c);
        assert_eq!(c.spec_fn_reachable, 0);
        assert_eq!(c.spec_fn_unreferenced, 0);
    }

    #[test]
    fn test_spec_fn_reachable() {
        let src = r#"
verus! {
    spec fn helper(n: int) -> bool {
        n > 0
    }

    exec fn foo(n: u32)
        requires helper(n as int)
    {
        let x = n;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.spec_req_ens >= 1, "requires line should be spec");
        assert!(c.spec_fn_reachable > 0, "helper should be reachable: {:?}", c);
        assert_eq!(c.spec_fn_unreferenced, 0);
    }

    #[test]
    fn test_spec_fn_unreferenced() {
        let src = r#"
verus! {
    spec fn never_called(n: int) -> bool {
        n > 0
    }

    exec fn foo() {
        let x = 1u32;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.spec_fn_unreferenced > 0, "never_called should be unreferenced: {:?}", c);
        assert_eq!(c.spec_fn_reachable, 0);
    }

    #[test]
    fn test_proof_block_calls() {
        let src = r#"
verus! {
    proof fn lemma_pos(n: int)
        ensures n * n >= 0
    {
    }

    exec fn run() {
        proof {
            lemma_pos(5);
        }
        let x = 1u32;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.proof_block > 0, "proof block lines should be proof: {:?}", c);
        assert!(c.proof_fn_reachable > 0, "lemma_pos should be reachable: {:?}", c);
    }

    #[test]
    fn test_transitive_spec() {
        let src = r#"
verus! {
    spec fn base(n: int) -> bool { n >= 0 }

    spec fn derived(n: int) -> bool { base(n) && n < 100 }

    exec fn foo(n: u32)
        requires derived(n as int)
    {
        let x = n;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.spec_fn_reachable >= 2, "both spec fns reachable: {:?}", c);
        assert_eq!(c.spec_fn_unreferenced, 0);
    }

    #[test]
    fn test_comments_blanks() {
        let c = analyze_source("// comment\n\n/* block */\n", &HashSet::new());
        assert!(c.comment >= 2);
        assert!(c.blank >= 1);
    }

    #[test]
    fn test_assert_in_exec_reaches_spec_fn() {
        let src = r#"
verus! {
    spec fn valid(n: int) -> bool {
        n >= 0
    }

    exec fn foo(n: u32) {
        assert(valid(n as int));
        let x = n;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(
            c.spec_fn_reachable > 0,
            "valid should be reachable via assert: {:?}", c
        );
        assert_eq!(c.spec_fn_unreferenced, 0);
    }

    #[test]
    fn test_assert_transitive_spec_reachable() {
        // assert references derived, which calls base — both should be reachable.
        let src = r#"
verus! {
    spec fn base(n: int) -> bool { n >= 0 }

    spec fn derived(n: int) -> bool { base(n) && n < 100 }

    exec fn foo(n: u32) {
        assert(derived(n as int));
        let x = n;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(
            c.spec_fn_reachable >= 2,
            "both base and derived should be reachable via assert: {:?}", c
        );
        assert_eq!(c.spec_fn_unreferenced, 0);
    }

    #[test]
    fn test_exec_call_outside_assert_not_spec_seed() {
        // exec_helper is called in exec body but not inside assert — should not be
        // incorrectly pulled into spec reachability.
        let src = r#"
verus! {
    spec fn spec_helper(n: int) -> bool { n > 0 }

    exec fn exec_helper(n: u32) -> u32 { n + 1 }

    exec fn foo(n: u32) {
        let m = exec_helper(n);
        let x = m;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert_eq!(c.spec_fn_reachable, 0, "no assert means spec_helper unreachable: {:?}", c);
        assert!(c.spec_fn_unreferenced > 0);
    }

    // ── classify.rs path coverage ────────────────────────────────────────────

    #[test]
    fn test_multiline_block_comment() {
        // Block comment spanning multiple lines: continuation lines must be classified
        // as Comment via the `state.in_block_comment` early-return path.
        let src = "/* line one\n   line two\n   line three */\n";
        let c = analyze_source(src, &HashSet::new());
        assert!(c.comment >= 3, "all lines of multiline block comment should be Comment: {:?}", c);
    }

    #[test]
    fn test_raw_string_inside_verus() {
        // r#"..."# inside an exec fn body — the raw-string handling path in classify_line.
        let src = r###"
verus! {
    exec fn foo() {
        let x = r#"hello world"#;
    }
}
"###;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.exec > 0, "raw string line should count as exec: {:?}", c);
    }

    #[test]
    fn test_normal_string_inside_verus() {
        // "..." inside an exec fn body — the normal-string handling path.
        let src = r#"
verus! {
    exec fn foo() {
        let x = "hello";
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.exec > 0, "string literal line should count as exec: {:?}", c);
    }

    #[test]
    fn test_code_then_line_comment() {
        // Code followed by // on the same line: has_code = true when // is reached,
        // so the loop breaks with has_comment = true but still returns a code annotation.
        let src = r#"
verus! {
    exec fn foo() {
        let x = 1u32; // inline comment
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.exec > 0, "line with code + trailing comment should be exec: {:?}", c);
    }

    #[test]
    fn test_plain_fn_in_verus() {
        // `fn` without mode keyword inside verus! → treated as exec.
        let src = r#"
verus! {
    fn plain(n: u32) -> u32 {
        n
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.exec > 0, "plain fn inside verus should be exec: {:?}", c);
    }

    #[test]
    fn test_spec_override_block() {
        // `spec { }` override block inside exec fn — covers the spec{} path in classify.
        let src = r#"
verus! {
    exec fn foo(n: u32) -> bool {
        spec { true }
    }
}
"#;
        // We just verify it parses without panic and produces some counts.
        let c = analyze_source(src, &HashSet::new());
        assert!(c.total() > 0, "spec override block should produce counts: {:?}", c);
    }

    #[test]
    fn test_nested_braces_in_fn_body() {
        // `{` without a fn pending (nested block) → covers the else branch in brace handling.
        let src = r#"
verus! {
    exec fn foo() {
        {
            let x = 1u32;
        }
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.exec > 0, "nested braces should still count as exec: {:?}", c);
    }

    #[test]
    fn test_req_ens_with_line_comment() {
        // requires line followed by // → scan_comment_state line-comment path.
        let src = r#"
verus! {
    exec fn foo(n: u32)
        requires n > 0 // must be positive
    {
        let x = n;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.spec_req_ens >= 1, "requires line should be spec: {:?}", c);
    }

    #[test]
    fn test_req_ens_with_block_comment() {
        // requires line with /* ... */ → scan_comment_state block-comment path.
        let src = r#"
verus! {
    exec fn foo(n: u32)
        requires /* pre */ n > 0
    {
        let x = n;
    }
}
"#;
        let c = analyze_source(src, &HashSet::new());
        assert!(c.spec_req_ens >= 1, "requires line with block comment should be spec: {:?}", c);
    }

    // ── print_row / print_detail / Counts::total ─────────────────────────────

    #[test]
    fn test_print_row_no_panic() {
        let mut c = Counts::default();
        c.exec = 10;
        c.spec_req_ens = 3;
        c.proof_fn_reachable = 2;
        c.comment = 1;
        c.blank = 1;
        // Verify total() is consistent before printing.
        assert_eq!(c.total(), 17);
        // Should not panic.
        print_row("test_label", &c);
    }

    #[test]
    fn test_print_detail_zero_code() {
        // When code == 0, print_detail should return early without panic.
        let c = Counts::default();
        print_detail(&c); // should not panic
    }

    #[test]
    fn test_print_detail_with_counts() {
        let mut c = Counts::default();
        c.exec = 5;
        c.spec_req_ens = 2;
        c.spec_fn_reachable = 3;
        c.proof_block = 1;
        c.proof_fn_reachable = 4;
        c.assert_count = 2;
        c.comment = 1;
        c.blank = 1;
        assert_eq!(c.total(), 17);
        print_detail(&c); // should not panic
    }
}
