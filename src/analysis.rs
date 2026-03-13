use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::{Counts, FnInfo, LineAnno, Mode};
use crate::parser::parse_file;

// ─── Cross-file helpers ───────────────────────────────────────────────────────

fn shift_anno(anno: LineAnno, offset: usize) -> LineAnno {
    match anno {
        LineAnno::FnLine(idx) => LineAnno::FnLine(idx + offset),
        LineAnno::ReqEns(idx) => LineAnno::ReqEns(idx + offset),
        LineAnno::ProofBlk(Some(idx)) => LineAnno::ProofBlk(Some(idx + offset)),
        other => other,
    }
}

#[derive(Debug)]
pub struct CrateAnalysis {
    pub per_file: Vec<Counts>,
    pub total: Counts,
}

pub fn analyze_crate(sources: &[&str], roots: &HashSet<String>) -> CrateAnalysis {
    // Parse each file independently, then merge FnInfo tables with index offsets.
    let mut all_annos: Vec<Vec<LineAnno>> = Vec::new();
    let mut all_fns: Vec<FnInfo> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();

    for source in sources {
        let (annos, fns) = parse_file(source);
        let offset = all_fns.len();
        offsets.push(offset);
        let shifted: Vec<LineAnno> = annos.into_iter().map(|a| shift_anno(a, offset)).collect();
        all_annos.push(shifted);
        all_fns.extend(fns);
    }

    // Single global reachability over the merged FnInfo table.
    let (spec_reach, proof_reach) = compute_reachability(&all_fns, roots);

    // Tally per file, then sum total.
    let mut per_file: Vec<Counts> = Vec::new();
    let mut total = Counts::default();

    for (file_idx, annos) in all_annos.iter().enumerate() {
        let mut c = tally(annos, &all_fns, &spec_reach, &proof_reach);
        c.assert_count = sources[file_idx]
            .lines()
            .zip(annos.iter())
            .filter(|(_, anno)| !matches!(anno, LineAnno::Blank | LineAnno::Comment))
            .map(|(line, _)| count_asserts_in_line(line))
            .sum();
        total.add(&c);
        per_file.push(c);
    }

    CrateAnalysis { per_file, total }
}

// ─── Phase 2: reachability ────────────────────────────────────────────────────

pub fn compute_reachability(
    fns: &[FnInfo],
    roots: &HashSet<String>,
) -> (HashSet<usize>, HashSet<usize>) {
    let name_to_idx: HashMap<&str, usize> = fns
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.as_str(), i))
        .collect();

    let use_fn = |f: &FnInfo| roots.is_empty() || roots.contains(&f.name);

    // Spec: seeded from req_ens_calls and exec assert expressions of root fns;
    // follows body_calls of spec fns.
    let spec_seeds: Vec<&str> = fns
        .iter()
        .filter(|f| use_fn(f))
        .flat_map(|f| {
            f.req_ens_calls.iter().chain(f.exec_assert_calls.iter()).map(|s| s.as_str())
        })
        .collect();
    let spec_reach = bfs_reach(&spec_seeds, &name_to_idx, fns, Mode::Spec);

    // Proof: seeded from proof_blk_calls of root exec fns; follows body_calls of proof fns
    let proof_seeds: Vec<&str> = fns
        .iter()
        .filter(|f| f.mode == Mode::Exec && use_fn(f))
        .flat_map(|f| f.proof_blk_calls.iter().map(|s| s.as_str()))
        .collect();
    let proof_reach = bfs_reach(&proof_seeds, &name_to_idx, fns, Mode::Proof);

    (spec_reach, proof_reach)
}

fn bfs_reach(
    seeds: &[&str],
    name_to_idx: &HashMap<&str, usize>,
    fns: &[FnInfo],
    target_mode: Mode,
) -> HashSet<usize> {
    let mut visited: HashSet<usize> = HashSet::new();
    let mut queue: VecDeque<usize> = VecDeque::new();

    for &name in seeds {
        if let Some(&idx) = name_to_idx.get(name) {
            if fns[idx].mode == target_mode && !visited.contains(&idx) {
                visited.insert(idx);
                queue.push_back(idx);
            }
        }
    }

    while let Some(idx) = queue.pop_front() {
        for name in &fns[idx].body_calls {
            if let Some(&next) = name_to_idx.get(name.as_str()) {
                if fns[next].mode == target_mode && !visited.contains(&next) {
                    visited.insert(next);
                    queue.push_back(next);
                }
            }
        }
    }

    visited
}

// ─── Counting ─────────────────────────────────────────────────────────────────

pub fn tally(
    annos: &[LineAnno],
    fns: &[FnInfo],
    spec_reach: &HashSet<usize>,
    proof_reach: &HashSet<usize>,
) -> Counts {
    let mut c = Counts::default();
    for anno in annos {
        match anno {
            LineAnno::Blank => c.blank += 1,
            LineAnno::Comment => c.comment += 1,
            LineAnno::Exec => c.exec += 1,
            LineAnno::ReqEns(_) => c.spec_req_ens += 1,
            LineAnno::ProofBlk(_) => c.proof_block += 1,
            LineAnno::FnLine(idx) => match fns.get(*idx).map(|f| f.mode) {
                Some(Mode::Exec) | None => c.exec += 1,
                Some(Mode::Spec) => {
                    if spec_reach.contains(idx) {
                        c.spec_fn_reachable += 1;
                    } else {
                        c.spec_fn_unreferenced += 1;
                    }
                }
                Some(Mode::Proof) => {
                    if proof_reach.contains(idx) {
                        c.proof_fn_reachable += 1;
                    } else {
                        c.proof_fn_unreferenced += 1;
                    }
                }
            },
        }
    }
    c
}

/// Count assert/assert_by/assert_forall_by calls on a single (non-comment) line.
fn count_asserts_in_line(line: &str) -> usize {
    const ASSERT_NAMES: &[&str] = &["assert", "assert_by", "assert_forall_by"];
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut count = 0;
    let mut i = 0;
    while i < n {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let mut j = i;
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            if j < n && chars[j] == '(' {
                let name: String = chars[start..i].iter().collect();
                if ASSERT_NAMES.contains(&name.as_str()) {
                    count += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    count
}

pub fn analyze_source(source: &str, roots: &HashSet<String>) -> Counts {
    let (annos, fns) = parse_file(source);
    let (spec_reach, proof_reach) = compute_reachability(&fns, roots);
    let mut counts = tally(&annos, &fns, &spec_reach, &proof_reach);
    counts.assert_count = source
        .lines()
        .zip(annos.iter())
        .filter(|(_, anno)| !matches!(anno, LineAnno::Blank | LineAnno::Comment))
        .map(|(line, _)| count_asserts_in_line(line))
        .sum();
    counts
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// spec fn defined in file A, called from requires of exec fn in file B.
    /// Per-file analysis cannot see across the boundary; whole-crate analysis can.
    #[test]
    fn test_crate_cross_file_spec_reachable() {
        let file_a = r#"
verus! {
    spec fn helper(n: int) -> bool {
        n > 0
    }
}
"#;
        let file_b = r#"
verus! {
    exec fn foo(n: u32)
        requires helper(n as int)
    {
        let x = n;
    }
}
"#;
        // Per-file: helper is unreferenced (file_a doesn't know about file_b's requires).
        let per_a = analyze_source(file_a, &HashSet::new());
        assert_eq!(per_a.spec_fn_reachable, 0, "per-file A should not see cross-file reference");
        assert!(per_a.spec_fn_unreferenced > 0);

        // Whole-crate: helper is reachable.
        let result = analyze_crate(&[file_a, file_b], &HashSet::new());
        assert!(
            result.total.spec_fn_reachable > 0,
            "whole-crate should detect helper as reachable: {:?}",
            result.total
        );
        assert_eq!(result.total.spec_fn_unreferenced, 0);
        // Per-file counts are still split correctly.
        assert_eq!(result.per_file.len(), 2);
    }

    /// proof fn defined in file A, called from a proof{} block in file B.
    #[test]
    fn test_crate_cross_file_proof_reachable() {
        let file_a = r#"
verus! {
    proof fn lemma_pos(n: int)
        ensures n * n >= 0
    {
    }
}
"#;
        let file_b = r#"
verus! {
    exec fn run() {
        proof {
            lemma_pos(5);
        }
        let x = 1u32;
    }
}
"#;
        // Per-file: lemma_pos unreferenced.
        let per_a = analyze_source(file_a, &HashSet::new());
        assert_eq!(per_a.proof_fn_reachable, 0);
        assert!(per_a.proof_fn_unreferenced > 0);

        // Whole-crate: lemma_pos reachable.
        let result = analyze_crate(&[file_a, file_b], &HashSet::new());
        assert!(
            result.total.proof_fn_reachable > 0,
            "whole-crate should detect lemma_pos as reachable: {:?}",
            result.total
        );
        assert_eq!(result.total.proof_fn_unreferenced, 0);
    }

    /// Totals equal sum of per-file counts.
    #[test]
    fn test_crate_totals_consistent() {
        let file_a = "fn add(a: u32, b: u32) -> u32 { a + b }\n";
        let file_b = "// just a comment\n\n";
        let result = analyze_crate(&[file_a, file_b], &HashSet::new());
        let mut expected = Counts::default();
        for c in &result.per_file {
            expected.add(c);
        }
        assert_eq!(result.total.exec, expected.exec);
        assert_eq!(result.total.comment, expected.comment);
        assert_eq!(result.total.blank, expected.blank);
        assert_eq!(result.total.spec_total(), expected.spec_total());
        assert_eq!(result.total.proof_total(), expected.proof_total());
    }

    /// Transitive cross-file spec reachability: A defines base, B defines derived calling base,
    /// C has exec fn with requires derived(...).
    #[test]
    fn test_crate_transitive_cross_file() {
        let file_a = r#"
verus! {
    spec fn base(n: int) -> bool { n >= 0 }
}
"#;
        let file_b = r#"
verus! {
    spec fn derived(n: int) -> bool { base(n) && n < 100 }
}
"#;
        let file_c = r#"
verus! {
    exec fn foo(n: u32)
        requires derived(n as int)
    {
        let x = n;
    }
}
"#;
        let result = analyze_crate(&[file_a, file_b, file_c], &HashSet::new());
        assert!(
            result.total.spec_fn_reachable >= 2,
            "both base and derived should be reachable: {:?}",
            result.total
        );
        assert_eq!(result.total.spec_fn_unreferenced, 0);
    }
}
