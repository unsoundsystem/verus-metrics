#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Mode {
    #[default]
    Exec,
    Spec,
    Proof,
}

// ─── Function info ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct FnInfo {
    pub name: String,
    pub mode: Mode,
    pub req_ens_calls: Vec<String>,
    pub proof_blk_calls: Vec<String>,
    pub body_calls: Vec<String>,
    /// Calls found inside assert/assert_by/assert_forall_by expressions in exec fn bodies.
    pub exec_assert_calls: Vec<String>,
}

// ─── Per-line annotation ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum LineAnno {
    Blank,
    Comment,
    Exec,
    /// Code outside `verus! { }` (use imports, module declarations, etc.) or the
    /// verus! delimiter lines themselves.  Not counted in any metric.
    NonVerus,
    ReqEns(usize),
    ProofBlk(Option<usize>),
    FnLine(usize),
}

// ─── Counts ───────────────────────────────────────────────────────────────────

#[derive(Default, Debug, Clone)]
pub struct Counts {
    pub spec_req_ens_reachable: usize,
    pub spec_req_ens_unreachable: usize,
    pub spec_fn_reachable: usize,
    pub spec_fn_unreferenced: usize,
    pub proof_block: usize,
    pub proof_fn_reachable: usize,
    pub proof_fn_unreferenced: usize,
    pub exec: usize,
    pub comment: usize,
    pub blank: usize,
    pub assert_count: usize,
    pub assume_count: usize,
    pub admit_count: usize,
}

impl Counts {
    /// Total requires/ensures lines (reachable + unreachable).
    pub fn spec_req_ens(&self) -> usize {
        self.spec_req_ens_reachable + self.spec_req_ens_unreachable
    }
    pub fn spec_total(&self) -> usize {
        self.spec_req_ens() + self.spec_fn_reachable + self.spec_fn_unreferenced
    }
    pub fn proof_total(&self) -> usize {
        self.proof_block + self.proof_fn_reachable + self.proof_fn_unreferenced
    }
    pub fn total(&self) -> usize {
        self.spec_total() + self.proof_total() + self.exec + self.comment + self.blank
    }
    /// Spec lines reachable from roots: reachable req/ens + reachable spec fn bodies.
    pub fn spec_reachable(&self) -> usize {
        self.spec_req_ens_reachable + self.spec_fn_reachable
    }
    /// Proof lines reachable from roots: proof blocks (always included, exec reachability
    /// not tracked) + reachable proof fn bodies.
    pub fn proof_reachable(&self) -> usize {
        self.proof_block + self.proof_fn_reachable
    }
    /// Total reachable lines (spec* + proof* + exec + comment + blank).
    pub fn total_reachable(&self) -> usize {
        self.spec_reachable() + self.proof_reachable() + self.exec + self.comment + self.blank
    }
    pub fn add(&mut self, other: &Counts) {
        self.spec_req_ens_reachable += other.spec_req_ens_reachable;
        self.spec_req_ens_unreachable += other.spec_req_ens_unreachable;
        self.spec_fn_reachable += other.spec_fn_reachable;
        self.spec_fn_unreferenced += other.spec_fn_unreferenced;
        self.proof_block += other.proof_block;
        self.proof_fn_reachable += other.proof_fn_reachable;
        self.proof_fn_unreferenced += other.proof_fn_unreferenced;
        self.exec += other.exec;
        self.comment += other.comment;
        self.blank += other.blank;
        self.assert_count += other.assert_count;
        self.assume_count += other.assume_count;
        self.admit_count += other.admit_count;
    }
}
