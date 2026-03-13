use crate::types::{FnInfo, LineAnno, Mode};
use super::{Pending, State, scan_comment_state};

pub fn classify_line(line: &str, state: &mut State, fns: &mut Vec<FnInfo>) -> LineAnno {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return LineAnno::Blank;
    }

    if state.in_block_comment {
        if trimmed.contains("*/") {
            state.in_block_comment = false;
        }
        return LineAnno::Comment;
    }

    // requires/ensures lines in signature section → always spec
    let is_req_ens = state.in_verus
        && state.pending.is_some()
        && (trimmed.starts_with("requires")
            || trimmed.starts_with("ensures")
            || trimmed.starts_with("decreases")
            || trimmed.starts_with("opens_invariants")
            || trimmed.starts_with("no_unwind"));

    if is_req_ens {
        let fn_idx = state.pending.as_ref().unwrap().fn_idx;
        scan_comment_state(trimmed, state);
        return LineAnno::ReqEns(fn_idx);
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut has_code = false;
    let mut has_comment = false;
    // Track the fn context active during this line (even if it opens+closes on same line)
    let mut line_fn_idx: Option<usize> = state.current_fn_idx();
    let mut line_had_proof_blk = false;

    while i < len {
        // ── Block comment ──
        if state.in_block_comment {
            has_comment = true;
            if i + 1 < len && chars[i] == '*' && chars[i + 1] == '/' {
                state.in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        // ── Raw string ──
        if let Some(hashes) = state.raw_string_hashes {
            if chars[i] == '"' {
                let mut h = 0;
                while i + 1 + h < len && chars[i + 1 + h] == '#' {
                    h += 1;
                }
                if h >= hashes {
                    state.raw_string_hashes = None;
                    i += 1 + h;
                    has_code = true;
                    continue;
                }
            }
            has_code = true;
            i += 1;
            continue;
        }
        // ── Normal string ──
        if state.in_string {
            if chars[i] == '\\' {
                i += 2;
            } else {
                if chars[i] == '"' {
                    state.in_string = false;
                }
                i += 1;
            }
            has_code = true;
            continue;
        }
        // ── Line comment ──
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            if !has_code {
                return LineAnno::Comment;
            }
            has_comment = true;
            break;
        }
        // ── Block comment start ──
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            state.in_block_comment = true;
            has_comment = true;
            i += 2;
            continue;
        }
        // ── Raw string start ──
        if chars[i] == 'r' {
            let mut h = 0;
            while i + 1 + h < len && chars[i + 1 + h] == '#' {
                h += 1;
            }
            if i + 1 + h < len && chars[i + 1 + h] == '"' {
                state.raw_string_hashes = Some(h);
                i += 2 + h;
                has_code = true;
                continue;
            }
        }
        // ── Normal string start ──
        if chars[i] == '"' {
            state.in_string = true;
            i += 1;
            has_code = true;
            continue;
        }

        // ── Outside verus! ──
        if !state.in_verus {
            let rest: String = chars[i..].iter().collect();
            if rest.starts_with("verus!") {
                let after: String = chars[i + 6..].iter().collect();
                if let Some(off) = after.find('{') {
                    state.in_verus = true;
                    state.verus_depth = 1;
                    i += 6 + off + 1;
                    has_code = true;
                    continue;
                }
            }
            if !chars[i].is_whitespace() {
                has_code = true;
            }
            i += 1;
            continue;
        }

        // ── Inside proof block ──
        if state.is_in_proof_block() {
            line_had_proof_blk = true;
            line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
            match chars[i] {
                '{' => {
                    state.verus_depth += 1;
                    has_code = true;
                }
                '}' => {
                    state.verus_depth -= 1;
                    if state.proof_block_depth.map_or(false, |d| state.verus_depth < d) {
                        state.proof_block_depth = None;
                    }
                    has_code = true;
                }
                c => {
                    if !c.is_whitespace() {
                        has_code = true;
                    }
                }
            }
            i += 1;
            continue;
        }

        // ── Inside verus!, not in proof block ──
        line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
        let rest: String = chars[i..].iter().collect();

        // fn definition keywords
        const FN_KW: &[(&str, Mode)] = &[
            ("spec(checked) fn ", Mode::Spec),
            ("spec fn ", Mode::Spec),
            ("proof fn ", Mode::Proof),
            ("exec fn ", Mode::Exec),
        ];
        let mut kw_matched = false;
        for &(kw, mode) in FN_KW {
            if rest.starts_with(kw) {
                let name_start = i + kw.len();
                let mut name_end = name_start;
                while name_end < len
                    && (chars[name_end].is_alphanumeric() || chars[name_end] == '_')
                {
                    name_end += 1;
                }
                let name: String = chars[name_start..name_end].iter().collect();
                let fn_idx = fns.len();
                fns.push(FnInfo {
                    name: name.clone(),
                    mode,
                    ..Default::default()
                });
                state.pending = Some(Pending { mode, fn_idx });
                i = name_end;
                has_code = true;
                kw_matched = true;
                break;
            }
        }
        if kw_matched {
            continue;
        }

        // Plain `fn ` (exec by default)
        if rest.starts_with("fn ") && state.pending.is_none() {
            let name_start = i + 3;
            let mut name_end = name_start;
            while name_end < len
                && (chars[name_end].is_alphanumeric() || chars[name_end] == '_')
            {
                name_end += 1;
            }
            let name: String = chars[name_start..name_end].iter().collect();
            let fn_idx = fns.len();
            fns.push(FnInfo {
                name: name.clone(),
                mode: Mode::Exec,
                ..Default::default()
            });
            state.pending = Some(Pending { mode: Mode::Exec, fn_idx });
            i = name_end;
            has_code = true;
            continue;
        }

        // proof { } block inside exec fn body
        if rest.starts_with("proof") && state.pending.is_none() {
            let after_proof = &rest[5..];
            let after_ws = after_proof.trim_start();
            if after_ws.starts_with('{') && state.current_mode() == Mode::Exec {
                let brace_pos = rest.find('{').unwrap();
                i += brace_pos + 1;
                state.verus_depth += 1;
                state.proof_block_depth = Some(state.verus_depth);
                line_had_proof_blk = true;
                line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
                has_code = true;
                continue;
            }
        }

        // spec { } override block
        if rest.starts_with("spec {") || rest.starts_with("spec{") {
            if state.pending.is_none() {
                let brace_pos = rest.find('{').unwrap();
                state.mode_stack.push((Mode::Spec, None));
                state.verus_depth += 1;
                i += brace_pos + 1;
                has_code = true;
                continue;
            }
        }

        // Brace handling
        match chars[i] {
            '{' => {
                if let Some(pending) = state.pending.take() {
                    line_fn_idx = Some(pending.fn_idx);
                    state.mode_stack.push((pending.mode, Some(pending.fn_idx)));
                } else {
                    let (cur_mode, _) = *state.mode_stack.last().unwrap_or(&(Mode::Exec, None));
                    state.mode_stack.push((cur_mode, None));
                }
                state.verus_depth += 1;
                has_code = true;
            }
            '}' => {
                state.verus_depth -= 1;
                if state.mode_stack.len() > 1 {
                    state.mode_stack.pop();
                }
                if state.verus_depth == 0 {
                    state.in_verus = false;
                }
                has_code = true;
            }
            c => {
                if !c.is_whitespace() {
                    has_code = true;
                }
            }
        }
        i += 1;
    }

    if !has_code {
        return if has_comment || state.in_block_comment {
            LineAnno::Comment
        } else {
            LineAnno::Blank
        };
    }

    if !state.in_verus {
        return LineAnno::Exec;
    }

    if line_had_proof_blk || state.is_in_proof_block() {
        let fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
        return LineAnno::ProofBlk(fn_idx);
    }

    if let Some(ref p) = state.pending {
        return LineAnno::FnLine(p.fn_idx);
    }

    match line_fn_idx.or_else(|| state.current_fn_idx()) {
        Some(idx) => LineAnno::FnLine(idx),
        None => LineAnno::Exec,
    }
}
