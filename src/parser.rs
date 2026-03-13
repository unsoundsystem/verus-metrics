pub mod classify;

use crate::types::{FnInfo, LineAnno, Mode};
use classify::classify_line;

// ─── Parser state ─────────────────────────────────────────────────────────────

pub struct Pending {
    pub mode: Mode,
    pub fn_idx: usize,
}

pub struct State {
    pub in_verus: bool,
    pub verus_depth: i32,
    /// (mode, Some(fn_idx) if this frame is a fn-body entry)
    pub mode_stack: Vec<(Mode, Option<usize>)>,
    pub pending: Option<Pending>,
    pub in_block_comment: bool,
    pub in_string: bool,
    pub raw_string_hashes: Option<usize>,
    /// verus_depth at entry of current proof{} block (None = not in one)
    pub proof_block_depth: Option<i32>,
}

impl State {
    pub fn new() -> Self {
        State {
            in_verus: false,
            verus_depth: 0,
            mode_stack: vec![(Mode::Exec, None)],
            pending: None,
            in_block_comment: false,
            in_string: false,
            raw_string_hashes: None,
            proof_block_depth: None,
        }
    }

    pub fn current_mode(&self) -> Mode {
        self.mode_stack.last().map(|(m, _)| *m).unwrap_or(Mode::Exec)
    }

    pub fn current_fn_idx(&self) -> Option<usize> {
        self.mode_stack.iter().rev().find_map(|(_, idx)| *idx)
    }

    pub fn is_in_proof_block(&self) -> bool {
        self.proof_block_depth.is_some()
    }
}

// ─── Call extraction ──────────────────────────────────────────────────────────

pub fn extract_calls(line: &str) -> Vec<String> {
    const KEYWORDS: &[&str] = &[
        "requires", "ensures", "decreases", "invariant", "opens_invariants",
        "no_unwind", "forall", "exists", "choose", "if", "while", "for",
        "match", "let", "fn", "spec", "proof", "exec", "assert", "assume",
        "assert_forall_by", "assert_by", "reveal", "reveal_with_fuel",
        "implies", "true", "false", "Self", "self",
    ];
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut calls = Vec::new();
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
                if !KEYWORDS.contains(&name.as_str()) {
                    calls.push(name);
                }
            }
        } else {
            i += 1;
        }
    }
    calls
}

// ─── Assert-argument call extractor ──────────────────────────────────────────

/// Extract function calls from *inside* assert / assert_by / assert_forall_by
/// argument lists on a single line, ignoring everything outside those expressions.
pub fn extract_assert_spec_calls(line: &str) -> Vec<String> {
    const ASSERT_KWS: &[&str] = &["assert_forall_by", "assert_by", "assert"];
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut calls = Vec::new();
    let mut i = 0;

    while i < n {
        // Skip non-identifier characters.
        if !(chars[i].is_alphabetic() || chars[i] == '_') {
            i += 1;
            continue;
        }

        // Collect the identifier.
        let id_start = i;
        while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        let id: String = chars[id_start..i].iter().collect();

        // Skip whitespace after identifier.
        let mut j = i;
        while j < n && chars[j].is_whitespace() {
            j += 1;
        }
        if j >= n || chars[j] != '(' {
            continue; // not a function call
        }

        if !ASSERT_KWS.contains(&id.as_str()) {
            continue; // not an assert keyword
        }

        // Find the matching closing ')' for this assert call.
        let arg_start = j + 1;
        let mut depth = 1usize;
        let mut k = arg_start;
        while k < n && depth > 0 {
            match chars[k] {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            k += 1;
        }
        let arg_end = if depth == 0 { k - 1 } else { k };
        let arg: String = chars[arg_start..arg_end].iter().collect();
        calls.extend(extract_calls(&arg));
        i = k;
    }

    calls
}

// ─── Comment-state scanner (used by classify for req/ens lines) ───────────────

pub fn scan_comment_state(line: &str, state: &mut State) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if state.in_block_comment {
            if i + 1 < n && chars[i] == '*' && chars[i + 1] == '/' {
                state.in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if state.in_string {
            if chars[i] == '\\' {
                i += 2;
            } else {
                if chars[i] == '"' {
                    state.in_string = false;
                }
                i += 1;
            }
            continue;
        }
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '/' {
            break;
        }
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '*' {
            state.in_block_comment = true;
            i += 2;
            continue;
        }
        if chars[i] == '"' {
            state.in_string = true;
        }
        i += 1;
    }
}

// ─── Phase 1: parse ───────────────────────────────────────────────────────────

pub fn parse_file(source: &str) -> (Vec<LineAnno>, Vec<FnInfo>) {
    let mut state = State::new();
    let mut fns: Vec<FnInfo> = Vec::new();
    let mut annos: Vec<LineAnno> = Vec::new();

    for line in source.lines() {
        let anno = classify_line(line, &mut state, &mut fns);
        match &anno {
            LineAnno::ReqEns(idx) => {
                fns[*idx].req_ens_calls.extend(extract_calls(line));
            }
            LineAnno::ProofBlk(Some(idx)) => {
                fns[*idx].proof_blk_calls.extend(extract_calls(line));
            }
            LineAnno::FnLine(idx) => {
                match fns.get(*idx).map(|f| f.mode) {
                    Some(Mode::Spec) | Some(Mode::Proof) => {
                        fns[*idx].body_calls.extend(extract_calls(line));
                    }
                    Some(Mode::Exec) => {
                        fns[*idx].exec_assert_calls.extend(extract_assert_spec_calls(line));
                    }
                    None => {}
                }
            }
            _ => {}
        }
        annos.push(anno);
    }

    (annos, fns)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── scan_comment_state edge cases ─────────────────────────────────────────

    #[test]
    fn test_scan_comment_state_line_comment() {
        // // in a requires line → stops processing, no state change.
        let mut state = State::new();
        scan_comment_state("requires n > 0 // comment", &mut state);
        assert!(!state.in_block_comment);
        assert!(!state.in_string);
    }

    #[test]
    fn test_scan_comment_state_block_comment_open() {
        // /* without closing */ → in_block_comment stays true after the line.
        let mut state = State::new();
        scan_comment_state("requires n > /* still open", &mut state);
        assert!(state.in_block_comment, "block comment should remain open");
    }

    #[test]
    fn test_scan_comment_state_block_comment_close() {
        // Start in a block comment; */ closes it on this line.
        let mut state = State::new();
        state.in_block_comment = true;
        scan_comment_state("*/ n > 0", &mut state);
        assert!(!state.in_block_comment, "block comment should be closed");
    }

    #[test]
    fn test_scan_comment_state_string() {
        // String literal in a requires line: in_string opens and closes.
        let mut state = State::new();
        scan_comment_state(r#"requires x == "hello""#, &mut state);
        assert!(!state.in_string, "string should be closed after scan");
    }

    #[test]
    fn test_scan_comment_state_string_open() {
        // Unclosed string — in_string remains true.
        let mut state = State::new();
        scan_comment_state(r#"requires x == "hello"#, &mut state);
        assert!(state.in_string, "unclosed string should leave in_string = true");
    }

    // ── extract_assert_spec_calls edge cases ──────────────────────────────────

    #[test]
    fn test_extract_assert_unclosed_parens() {
        // Malformed: assert( without closing ) — should not panic, returns what it can.
        let calls = extract_assert_spec_calls("assert(foo(n)");
        // foo is inside the assert args even though ) is missing
        assert!(calls.contains(&"foo".to_string()));
    }

    #[test]
    fn test_extract_assert_by() {
        let calls = extract_assert_spec_calls("assert_by(pred(n), { lemma(n); })");
        assert!(calls.contains(&"pred".to_string()) || calls.contains(&"lemma".to_string()));
    }

    #[test]
    fn test_extract_assert_forall_by() {
        let calls = extract_assert_spec_calls("assert_forall_by(|n: int| { check(n) })");
        assert!(calls.contains(&"check".to_string()));
    }

    #[test]
    fn test_extract_assert_no_assert_keyword() {
        // No assert call → empty result.
        let calls = extract_assert_spec_calls("let x = foo(n);");
        assert!(calls.is_empty());
    }
}
