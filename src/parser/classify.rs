use crate::types::{FnInfo, LineAnno, Mode};
use super::{Pending, State, scan_comment_state};

pub fn classify_line(line: &str, state: &mut State, fns: &mut Vec<FnInfo>) -> LineAnno {
    let trimmed = line.trim();
    // Capture whether we started this line inside verus! (used at the end to classify
    // top-level non-fn-body lines and the verus! delimiter itself).
    let started_in_verus = state.in_verus;
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
        // Count paren depth to detect multi-line req/ens clauses like `ensures ({...})`.
        // Braces inside the clause are spec-expression braces, not fn-body boundaries,
        // so we do NOT update verus_depth here.
        for c in trimmed.chars() {
            match c {
                '(' => state.req_ens_paren_depth += 1,
                ')' => {
                    if state.req_ens_paren_depth > 0 {
                        state.req_ens_paren_depth -= 1;
                    }
                }
                _ => {}
            }
        }
        // Mark that we are now inside a req/ens clause. This handles the case where
        // the keyword sits alone on its own line and the condition begins on the next line.
        state.in_req_ens_clause = true;
        scan_comment_state(trimmed, state);
        return LineAnno::ReqEns(fn_idx);
    }

    // Continuation of a multi-line req/ens clause.
    // Fires when:
    //   (a) req_ens_paren_depth > 0  — inside a block-expr clause like `ensures ({...})`
    //   (b) in_req_ens_clause        — keyword was on a prior line with no open parens yet,
    //                                  e.g. `ensures\n  r matches Some(...) ==> ({`
    //
    // Before returning ReqEns we scan for a top-level `{` (not nested inside parens).
    // Such a `{` is the fn-body opening brace, NOT a spec-expression brace, so we must NOT
    // swallow it here — instead we clear clause state and fall through to the char loop.
    if state.in_verus && state.pending.is_some()
        && (state.req_ens_paren_depth > 0 || state.in_req_ens_clause)
    {
        let mut depth = state.req_ens_paren_depth;
        let mut toplevel_brace = false;
        for c in trimmed.chars() {
            match c {
                '(' => depth += 1,
                ')' => { if depth > 0 { depth -= 1; } }
                '{' if depth == 0 => { toplevel_brace = true; break; }
                _ => {}
            }
        }
        if toplevel_brace {
            // The fn-body `{` is on this line — clear clause state and fall through so
            // the char loop can consume `pending` normally.
            state.req_ens_paren_depth = 0;
            state.in_req_ens_clause = false;
            // (fall through to char loop below)
        } else {
            let fn_idx = state.pending.as_ref().unwrap().fn_idx;
            state.req_ens_paren_depth = depth;
            // in_req_ens_clause stays true until the fn-body brace is encountered.
            scan_comment_state(trimmed, state);
            return LineAnno::ReqEns(fn_idx);
        }
    }

    // Loop-body invariant/decreases → spec regardless of pending
    let is_loop_spec = state.in_verus
        && state.pending.is_none()
        && !state.is_in_proof_block()
        && state.current_mode() == Mode::Exec
        && state.current_fn_idx().is_some()
        && (trimmed.starts_with("invariant")
            || trimmed.starts_with("decreases"));

    if is_loop_spec {
        let fn_idx = state.current_fn_idx().unwrap();
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
    let mut line_had_spec_blk = false;
    // Continuation of a multi-line assert statement → whole line is proof
    if state.in_assert_stmt {
        line_had_proof_blk = true;
    }

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

        // ── Inside spec block ──
        if state.is_in_spec_block() {
            line_had_spec_blk = true;
            line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
            match chars[i] {
                '{' => {
                    state.verus_depth += 1;
                    has_code = true;
                }
                '}' => {
                    state.verus_depth -= 1;
                    if state.spec_block_depth.map_or(false, |d| state.verus_depth < d) {
                        state.spec_block_depth = None;
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

        // ── Inside verus!, not in proof/spec block ──
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

        // calc! { } macro inside exec fn body → proof block
        // Note: calc! with { on the next line is not supported (same limitation as proof {})
        if rest.starts_with("calc!")
            && state.current_mode() == Mode::Exec
            && state.pending.is_none()
        {
            let suffix = &rest[5..]; // after "calc!"
            if let Some(brace_offset) = suffix.chars().position(|c| c == '{') {
                i += 5 + brace_offset + 1;
                state.verus_depth += 1;
                state.proof_block_depth = Some(state.verus_depth);
                line_had_proof_blk = true;
                line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
                has_code = true;
                continue;
            }
        }

        // spec { } override block inside exec fn body → spec block
        if rest.starts_with("spec {") || rest.starts_with("spec{") {
            if state.pending.is_none() {
                let brace_pos = rest.find('{').unwrap();
                i += brace_pos + 1;
                state.verus_depth += 1;
                state.spec_block_depth = Some(state.verus_depth);
                line_had_spec_blk = true;
                line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
                has_code = true;
                continue;
            }
        }

        // assert / assert_by / assert_forall_by in exec fn body → proof line
        if state.current_mode() == Mode::Exec
            && state.pending.is_none()
            && !state.in_assert_stmt
        {
            const ASSERT_KWS: &[&str] = &["assert_forall_by", "assert_by", "assert", "assume", "admit"];
            'assert_detect: for &kw in ASSERT_KWS {
                if rest.starts_with(kw) {
                    let after = &rest[kw.len()..];
                    if after.trim_start().starts_with('(') {
                        state.in_assert_stmt = true;
                        state.assert_paren_depth = 0;
                        line_had_proof_blk = true;
                        line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
                        i += kw.len();
                        has_code = true;
                        break 'assert_detect;
                    }
                }
            }
        }

        // Process characters while inside an assert statement
        if state.in_assert_stmt {
            line_had_proof_blk = true;
            line_fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
            match chars[i] {
                '(' => {
                    state.assert_paren_depth += 1;
                }
                ')' => {
                    if state.assert_paren_depth > 0 {
                        state.assert_paren_depth -= 1;
                    }
                }
                ';' if state.assert_paren_depth == 0 => {
                    state.in_assert_stmt = false;
                }
                '{' => {
                    if state.assert_paren_depth == 0 {
                        // by { } block: treat as a proof block
                        state.in_assert_stmt = false;
                        state.verus_depth += 1;
                        state.proof_block_depth = Some(state.verus_depth);
                    } else {
                        state.assert_paren_depth += 1;
                    }
                }
                '}' => {
                    if state.assert_paren_depth > 0 {
                        state.assert_paren_depth -= 1;
                    }
                }
                _ => {}
            }
            if !chars[i].is_whitespace() {
                has_code = true;
            }
            i += 1;
            continue;
        }

        // Bodyless fn (e.g. uninterp spec fn foo();) — clear stale pending at `;`
        if chars[i] == ';' && state.pending.is_some() && !state.in_assert_stmt {
            state.pending = None;
            has_code = true;
            i += 1;
            continue;
        }

        // broadcast group { } — Verus mechanism for grouping broadcast lemmas → proof block
        if (rest.starts_with("broadcast group") || rest.starts_with("group "))
            && state.pending.is_none()
        {
            if let Some(brace_offset) = rest.chars().position(|c| c == '{') {
                i += brace_offset + 1;
                state.verus_depth += 1;
                state.proof_block_depth = Some(state.verus_depth);
                line_had_proof_blk = true;
                has_code = true;
                continue;
            }
        }

        // Brace handling
        match chars[i] {
            '{' => {
                if let Some(pending) = state.pending.take() {
                    state.in_req_ens_clause = false;
                    state.req_ens_paren_depth = 0;
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
        // Either a line that was always outside verus! (use imports, module decls, etc.)
        // or the closing `}` of the verus! block.  Neither is counted as exec.
        return LineAnno::NonVerus;
    }

    if line_had_proof_blk || state.is_in_proof_block() {
        let fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
        return LineAnno::ProofBlk(fn_idx);
    }

    if line_had_spec_blk || state.is_in_spec_block() {
        let fn_idx = line_fn_idx.or_else(|| state.current_fn_idx());
        return LineAnno::SpecBlk(fn_idx);
    }

    if let Some(ref p) = state.pending {
        // Signature lines of spec/proof fns are part of their specification;
        // exec fn signature lines (params, return type, where clauses) are declarations,
        // not executable code.
        return if p.mode == Mode::Exec {
            LineAnno::NonVerus
        } else {
            LineAnno::FnLine(p.fn_idx)
        };
    }

    match line_fn_idx.or_else(|| state.current_fn_idx()) {
        Some(idx) => LineAnno::FnLine(idx),
        None => {
            // Inside verus! but not in any fn body: top-level type/impl declarations.
            // Only `struct` definitions are counted as exec (they define exec-mode data types).
            // Everything else (enum, impl wrappers, use inside verus!, etc.) is NonVerus.
            if started_in_verus
                && trimmed.split_whitespace().any(|w| w == "struct")
            {
                LineAnno::Exec
            } else {
                LineAnno::NonVerus
            }
        }
    }
}
