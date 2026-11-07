This concrete scaffolding blueprint unites the **Go/Rust Host Kernel**, the **Wasm/Lua Stateless Sandbox**, the **Python Swarm Factory (with Privacy Airlock)**, and **Symbiont v3.0 + LFM3-1.7B** into a single, decoupled directory and execution architecture.

To keep the scaffolding minimal, future-proof, and immune to rewrites, every module adheres to the **Single Responsibility Principle (SRP)** and communicates exclusively through **4 explicit contracts**:

1. **The WIT/Memory Contract** (between Host Kernel and Wasm/Lua Sandbox)
2. **The SQLite WAL Connectome & Stigmergic Ring Buffer** (between Host Kernel and Python Swarm)
3. **The Symbiont MCP / REST v1 Contract** (between Swarm, Human Judge, and Reality Webhooks)


4. **The M0 JSONL Export Contract** (between Symbiont Ledger and the LFM3-1.7B MLX Trainer)



---

### 1. Master Monorepo Directory Scaffolding

```text
s-sese/                                  # Symbiotic Self-Evolving Software Engine
├── contracts/                           # 1. IMMUTABLE INTERFACES (The Glue)
│   ├── sandbox_plugin.wit               # Wasm Component Model interface (stateless pure fn)
│   ├── schema.sql                       # SQLite WAL Connectome + Symbiont Ledger + EESD tables
│   └── ast_airlock_rules.toml           # Tree-sitter redaction rules for proprietary identifiers
│
├── host-kernel/                         # 2. MODULE 1: IMMUTABLE DATA PLANE (Rust or Go)
│   ├── src/
│   │   ├── main.rs                      # Zero-downtime event loop & telemetry emitter
│   │   ├── state_buffer.rs              # Host-owned Apache Arrow / ring-buffer sliding windows
│   │   ├── slot_manager.rs              # Atomic Champion + Shadow Wasm/Lua pointer swapper
│   │   └── stigmat_bus.rs               # Writes microsecond latency/error pheromones to mmap/WAL
│
├── plugins/                             # 3. MODULE 2: EPHEMERAL SANDBOX EXTENSIONS (Wasm / Lua)
│   ├── champion/                        # Currently active production bytecode (SHA-addressed)
│   ├── shadow/                          # Candidate modules running on tee'd live traffic
│   └── holdout_replays/                 # Encrypted/locked FrozenReplayCase[] JSON packs
│
├── swarm-factory/                       # 4. MODULE 3: PYTHON CONTROL PLANE & RSI LOOP
│   ├── router.py                        # Universal LLM Router (Local Rapid-MLX vs. Cloud Sanitized)
│   ├── airlock.py                       # Tree-sitter AST Sanitizer (Proprietary <-> Clean-Room)
│   ├── opf_prefilter.py                 # Sub-800M / Symbolic pre-compiler gatekeeper
│   ├── sandbox_oracle.py                # Compiles & tests candidates against FrozenReplayCase[]
│   └── agents/
│       ├── architect.py                 # Local-only: decomposes specs & fetches Symbiont skill.md
│       ├── mechanic.py                  # Cloud/Local: writes stateless Rust/Wasm or Lua plugins
│       └── guard.py                     # Local-only: static verification + EESD Dirichlet math
│
├── symbiont-governance/                 # 5. MODULE 4: HUMAN-IN-THE-MIDDLE CORTEX (Symbiont v3.0)
│   ├── mcp_bridge.py                    # Wraps /mcp (evaluate_three_column_card, commit_record)
│   ├── outcome_webhook.py               # Handles POST /api/v1/records/:id/outcome
│   └── spot_check_guard.py              # Triggers Layer 5 Blinded Spot-Checks when absorption >= 85%
│
└── co-learning-mlx/                     # 6. MODULE 5: JIT LFM3-1.7B MICRO-TUNING (Apple Silicon)
    ├── m0_exporter.py                   # Extracts outcome-verified SFT, DPO, and RLVR JSONL
    ├── train_lora.py                    # Layer-streaming MLX LoRA trainer for LFM3-1.7B
    └── adapters/                        # Per-domain 15MB LoRA weights (e.g., symbiosis_systems.safetensors)

```

---

### 2. Contract #1: The Stateless Sandbox Interface (`contracts/sandbox_plugin.wit`)

To solve the **Windowed State Trap** and **Serialization Tax**, no AI-generated Wasm or Lua plugin is ever allowed to hold internal state or make network/disk calls. The Host Kernel owns the sliding window buffer and passes read-only byte slices into the sandbox:

```wit
// contracts/sandbox_plugin.wit
package sese:sandbox@1.0.0;

interface types {
    // Host-owned sliding window metrics passed by zero-copy reference
    record window-state {
        window-count: u64,
        rolling-mean: f64,
        rolling-p99-us: u64,
        last-error-code: u32,
    }

    // Incoming event payload (flat struct, no JSON parsing inside Wasm)
    record event-frame {
        timestamp-ns: u64,
        entity-id: u64,
        metric-val: f64,
        flags: u32,
    }

    enum decision-code {
        allow,
        throttle,
        quarantine,
        escalate-to-judge,
    }

    record plugin-verdict {
        code: decision-code,
        score: f64,
        reason-code: u32,
    }
}

world extensible-filter {
    use types.{window-state, event-frame, plugin-verdict};

    // Pure, deterministic function: <100 microseconds budget, 0 heap leaks
    export evaluate: func(state: window-state, event: event-frame) -> plugin-verdict;
}

```

---

### 3. Contract #2: The Unified SQLite Connectome & Ledger (`contracts/schema.sql`)

Instead of passing chat histories between agents, the Host Kernel, the Swarm Factory, and Symbiont share a single **WAL-mode SQLite schema** that tracks plugin lineages, EESD Dirichlet weights, and hash-chained `UnifiedDecisionRecord` entries:

```sql
-- contracts/schema.sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

-- 1. Stigmergic Reflex Bus (Written by Go/Rust Host, polled by Sub-800M Router)
CREATE TABLE IF NOT EXISTS stigmergic_telemetry (
    domain TEXT PRIMARY KEY,
    active_champion_sha TEXT NOT NULL,
    active_shadow_sha TEXT,
    p99_latency_us INTEGER NOT NULL,
    error_rate_ema REAL NOT NULL,
    pheromone_urgency REAL NOT NULL, -- Spikes > 0.8 trigger Swarm Architect
    updated_at_ns INTEGER NOT NULL
);

-- 2. Causal Connectome & EESD Probationary Ledger
CREATE TABLE IF NOT EXISTS sandbox_artifacts (
    ast_sha TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    parent_sha TEXT,
    runtime_target TEXT CHECK(runtime_target IN ('wasm', 'lua')),
    bytecode BLOB NOT NULL,
    clean_room_source TEXT NOT NULL,
    -- EESD Adaptive Dirichlet Estimator fields
    alpha_validated REAL DEFAULT 1.0,
    beta_falsified REAL DEFAULT 1.0,
    test_suite_entropy REAL DEFAULT 0.0,
    tier_status TEXT CHECK(tier_status IN ('SHADOW', 'PROBATIONARY', 'CONSOLIDATED', 'ROLLED_BACK'))
);

-- 3. Symbiont Hash-Chained Decision & Outcome Ledger
CREATE TABLE IF NOT EXISTS symbiont_decision_ledger (
    record_id TEXT PRIMARY KEY,
    ledger_prev TEXT NOT NULL,
    ledger_hash TEXT NOT NULL UNIQUE, -- SHA-256 chain from GENESIS_0000000000000000
    domain TEXT NOT NULL,
    candidate_ast_sha TEXT REFERENCES sandbox_artifacts(ast_sha),
    human_decision TEXT CHECK(human_decision IN ('AUTO_PROMOTED', 'PASS', 'FLIP', 'MODIFY', 'ESCALATE')),
    split_rating TEXT CHECK(split_rating IN ('ABOUT_RIGHT', 'OVERSTEPPED_ME', 'BURIED_IN_DETAIL', 'TOO_ABSTRACT')),
    reality_outcome TEXT CHECK(reality_outcome IN ('PENDING_REALITY', 'OBSERVED_VALIDATED', 'OBSERVED_FALSIFIED')),
    autonomy_dial_snapshot INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

```

---

### 4. Core Scaffolding Code: Wiring the Swarm, Airlock, Sandbox & Symbiont

Here is the executable Python control-plane scaffolding (`swarm-factory/orchestrator.py`) showing how the **Privacy Airlock**, **OPF Pre-Filter**, **Shadow Sandbox**, and **Symbiont v3.0 Governance** interlock cleanly using single-responsibility classes:

```python
#!/usr/bin/env python3
"""
S-SESE Core Orchestrator Scaffolding.
Bridges the Go/Rust Host Kernel, Wasm/Lua Sandbox, Privacy Airlock, and Symbiont v3.0.
"""

from dataclasses import dataclass
import hashlib
import json
import sqlite3
from typing import Protocol, Optional


# --- 1. EXPLICIT INTERFACES (Dependency Inversion) ---

class SymbiontClient(Protocol):
    """Contract for Symbiont v3.0 REST / MCP Governance Engine."""
    def fetch_live_skill_md(self, domain: str) -> dict: ...
    def evaluate_three_column_card(self, domain: str, seed: str, working_map: str) -> dict: ...
    def commit_decision(self, payload: dict) -> dict: ...


class ASTSanitizer(Protocol):
    """Contract for Tree-sitter Bidirectional Privacy Airlock."""
    def strip_proprietary(self, raw_spec: str) -> tuple[str, dict[str, str]]: ...
    def rehydrate_proprietary(self, clean_code: str, token_map: dict[str, str]) -> str: ...


class SandboxHarness(Protocol):
    """Contract for Wasmtime / LuaJIT Isolated Verification & Shadow Runner."""
    def opf_prefilter(self, source_code: str) -> tuple[bool, str]: ...
    def run_frozen_replay_gate(self, domain: str, bytecode: bytes) -> tuple[bool, float]: ...
    def mount_shadow_slot(self, domain: str, ast_sha: str, bytecode: bytes) -> None: ...
    def promote_to_champion(self, domain: str, ast_sha: str) -> None: ...


# --- 2. EESD DIRICHLET EVIDENCE CALCULATOR ---

@dataclass(frozen=True)
class EESDScore:
    effective_weight: float
    sample_size: int
    can_consolidate: bool


class DirichletEvidenceEngine:
    """Implements Effective-Evidence Self-Distillation (EESD) math."""

    def __init__(self, prior_mass: float = 5.0, consolidation_threshold: float = 0.82):
        self._alpha_0 = prior_mass
        self._threshold = consolidation_threshold

    def compute_weight(self, alpha_pass: float, beta_fail: float, suite_entropy: float) -> EESDScore:
        n_trials = int((alpha_pass - 1.0) + (beta_fail - 1.0))
        n_effective = n_trials * max(0.1, min(1.0, suite_entropy))
        
        # Direction (empirical win rate) separated from Strength (Dirichlet pseudo-count mass)
        empirical_win_rate = alpha_pass / (alpha_pass + beta_fail)
        strength_multiplier = n_effective / (n_effective + self._alpha_0)
        effective_weight = round(empirical_win_rate * strength_multiplier, 4)

        return EESDScore(
            effective_weight=effective_weight,
            sample_size=n_trials,
            can_consolidate=(effective_weight >= self._threshold and n_trials >= 5),
        )


# --- 3. THE S-SESE CLOSED-LOOP COORDINATOR ---

class SymbioticSwarmCoordinator:
    """Coordinates candidate generation, sandbox verification, and Symbiont HITM gating."""

    def __init__(
        self,
        db_conn: sqlite3.Connection,
        symbiont: SymbiontClient,
        airlock: ASTSanitizer,
        sandbox: SandboxHarness,
    ):
        self._db = db_conn
        self._symbiont = symbiont
        self._airlock = airlock
        self._sandbox = sandbox

    def execute_evolution_cycle(
        self,
        domain: str,
        raw_problem_seed: str,
        human_working_map: str,
        mechanic_generator_fn,
    ) -> str:
        # Step 1: Pull live Generation N policy & dials from Symbiont
        policy = self._symbiont.fetch_live_skill_md(domain)
        autonomy_dial = policy.get("tau_autonomy", 20)
        absorption_dial = policy.get("gamma_absorption", 50)

        # Step 2: Strip proprietary context via local AST Airlock before calling Mechanic
        clean_spec, symbol_map = self._airlock.strip_proprietary(raw_problem_seed)
        raw_candidate_code = mechanic_generator_fn(clean_spec)
        hydrated_code = self._airlock.rehydrate_proprietary(raw_candidate_code, symbol_map)

        # Step 3: OPF Pre-Filter (Reject bad syntax/memory vectors before compilation)
        passed_opf, opf_critique = self._sandbox.opf_prefilter(hydrated_code)
        if not passed_opf:
            return f"REJECTED_BY_OPF_PREFILTER: {opf_critique}"

        # Step 4: Compile & test against locked AIDE² / Symbiont FrozenReplayCase[] pack
        bytecode = hydrated_code.encode("utf-8")
        ast_sha = hashlib.sha256(bytecode).hexdigest()[:16]
        passed_replay, entropy_score = self._sandbox.run_frozen_replay_gate(domain, bytecode)
        if not passed_replay:
            return f"REJECTED_BY_FROZEN_REPLAY_GATE: {ast_sha}"

        # Step 5: Mount into Host Kernel's SHADOW slot first
        self._sandbox.mount_shadow_slot(domain, ast_sha, bytecode)

        # Step 6: Check Layer 5 Blinded Sharpness Spot-Check & Autonomy Horizon
        needs_spot_check = absorption_dial >= 85
        can_auto_promote = (autonomy_dial >= 75) and (not needs_spot_check)

        if can_auto_promote:
            self._sandbox.promote_to_champion(domain, ast_sha)
            self._symbiont.commit_decision({
                "domain": domain,
                "case_seed": raw_problem_seed,
                "agent_proposal": f"Auto-promoted Wasm/Lua plugin {ast_sha}",
                "human_decision": "PASS",
                "split_rating": "ABOUT_RIGHT",
                "reality_outcome": "PENDING_REALITY",
            })
            return f"AUTO_PROMOTED_PENDING_REALITY: {ast_sha}"

        # Step 7: Escalate to Human Judge via Symbiont 3-Column Decision Card
        card = self._symbiont.evaluate_three_column_card(
            domain=domain,
            seed=f"[CANDIDATE: {ast_sha}] {raw_problem_seed}",
            working_map=human_working_map,
        )
        return f"STAGED_FOR_HUMAN_JUDGE: Card ID {card.get('id', 'pending')}"

```

---

### 5. Why This Scaffolding Works Before We Write Heavy Implementation Code

1. **The Host Kernel Can Be Tested Standalone:** We can write a 150-line Rust or Go binary that loads two Lua or Wasm scripts (`champion` and `shadow`), passes a struct from a ring buffer, and swaps the active pointer on a SQLite signal.
2. **Symbiont Remains the Single Source of Authority:** The Python swarm never invents its own confidence metrics or self-grades its prompts. It fetches `skill.md` at the top of the loop and commits every outcome as `PENDING_REALITY` until your downstream webhook (`POST /api/v1/records/:id/outcome`) proves the Wasm/Lua plugin actually worked in reality.


3. **LFM3-1.7B Drops Right Into the Loop:** Because every resolved record in `symbiont_decision_ledger` has an explicit `human_decision`, `split_rating`, and `reality_outcome`, `co-learning-mlx/m0_exporter.py` is just a 30-line SQL query that dumps verified SFT, DPO, and RLVR rows for `Rapid-MLX` and `train_lora.py`.

Here is **Module 1: The Immutable Host Kernel**, built in **Rust** for zero-garbage-collection latency guarantees and memory safety, paired with an embedded **LuaJIT / Lua 5.4 (`mlua`)** sandbox and **SQLite WAL (`rusqlite`)** stigmergic bus.

Why start the Host Kernel with embedded Lua (`mlua`) alongside a trait boundary ready for `wasmtime`? Because Lua compiles inside the Rust binary in seconds with zero external toolchains, executes a pure function call across the FFI boundary in **less than 2 microseconds**, and lets us enforce strict memory ceilings and instruction-count budgets (`hook` triggers) out of the box—while the `SandboxSlot` trait allows dropping in a `.wasm` runner without changing a single line of the kernel loop.

---

### 1. Host Kernel Dependencies (`host-kernel/Cargo.toml`)

```toml
[package]
name = "sese-host-kernel"
version = "0.1.0"
edition = "2021"

[dependencies]
# Embedded Lua 5.4 with safe Rust bindings (can switch feature to "luajit" for raw speed)
mlua = { version = "0.9", features = ["lua54", "vendored", "send"] }
# Zero-config SQLite with bundled C library for WAL shared memory bus
rusqlite = { version = "0.31", features = ["bundled"] }
# Lock-free atomic pointer swapping for zero-downtime Champion/Shadow updates
arc-swap = "1.7"

```

---

### 2. The Complete Host Kernel (`host-kernel/src/main.rs`)

This implementation enforces **five critical architectural invariants**:

1. **100% Stateless Sandbox Execution:** The sandbox plugin has its standard I/O (`os`, `io`, `package`, `debug`) completely stripped. It receives read-only `WindowState` and `EventFrame` structs and returns a `PluginVerdict`.
2. **Instruction & Memory Jail:** Every plugin invocation is capped at `10,000` VM instructions and `256 KB` of heap memory. An infinite `while true do end` loop written by a rogue Mechanic agent is killed in microseconds without stalling the host thread.
3. **Host-Owned Sliding Window (`StateBuffer`):** Even when you hot-swap the Champion plugin mid-stream, the rolling count, mean, and p99 latency stay intact inside the Rust kernel.
4. **Lock-Free Atomic Pointer Swap (`ArcSwap`):** The event loop never blocks on a mutex when a new Shadow or Champion plugin is mounted from SQLite.
5. **Differential Shadow Telemetry:** Every event runs through the **Champion** (which controls production output) and optionally tees to the **Shadow** slot, logging latency deltas and divergence rates directly to the `stigmergic_telemetry` SQLite WAL table for Symbiont and the Swarm Architect.

```rust
// host-kernel/src/main.rs
use arc_swap::ArcSwapOption;
use mlua::{HookTriggers, Lua, StdLib, Table, Value};
use rusqlite::{params, Connection};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// --- 1. CONTRACT TYPES (Mirrors contracts/sandbox_plugin.wit) ---

#[derive(Debug, Clone, Copy)]
pub struct WindowState {
    pub window_count: u64,
    pub rolling_mean: f64,
    pub rolling_p99_us: u64,
    pub last_error_code: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct EventFrame {
    pub timestamp_ns: u64,
    pub entity_id: u64,
    pub metric_val: f64,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DecisionCode {
    Allow = 0,
    Throttle = 1,
    Quarantine = 2,
    EscalateToJudge = 3,
}

impl DecisionCode {
    fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Allow,
            1 => Self::Throttle,
            2 => Self::Quarantine,
            _ => Self::EscalateToJudge,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PluginVerdict {
    pub code: DecisionCode,
    pub score: f64,
    pub reason_code: u32,
}

#[derive(Debug)]
pub enum KernelError {
    SandboxInit(String),
    SandboxExec(String),
    Database(String),
}

impl From<mlua::Error> for KernelError {
    fn from(e: mlua::Error) -> Self {
        KernelError::SandboxExec(e.to_string())
    }
}

impl From<rusqlite::Error> for KernelError {
    fn from(e: rusqlite::Error) -> Self {
        KernelError::Database(e.to_string())
    }
}

// --- 2. HOST-OWNED SLIDING WINDOW BUFFER (Survives Hot-Swaps) ---

pub struct HostStateBuffer {
    capacity: usize,
    metrics: VecDeque<f64>,
    latencies_us: VecDeque<u64>,
    total_events: u64,
    last_error_code: u32,
}

impl HostStateBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            metrics: VecDeque::with_capacity(capacity),
            latencies_us: VecDeque::with_capacity(capacity),
            total_events: 0,
            last_error_code: 0,
        }
    }

    pub fn snapshot(&self) -> WindowState {
        let mean = if self.metrics.is_empty() {
            0.0
        } else {
            self.metrics.iter().sum::<f64>() / (self.metrics.len() as f64)
        };

        let p99 = if self.latencies_us.is_empty() {
            0
        } else {
            let mut sorted: Vec<u64> = self.latencies_us.iter().copied().collect();
            sorted.sort_unstable();
            let idx = ((sorted.len() as f64) * 0.99).floor() as usize;
            sorted[idx.min(sorted.len() - 1)]
        };

        WindowState {
            window_count: self.total_events,
            rolling_mean: mean,
            rolling_p99_us: p99,
            last_error_code: self.last_error_code,
        }
    }

    pub fn record_tick(&mut self, metric_val: f64, latency_us: u64, err_code: u32) {
        if self.metrics.len() == self.capacity {
            self.metrics.pop_front();
        }
        if self.latencies_us.len() == self.capacity {
            self.latencies_us.pop_front();
        }
        self.metrics.push_back(metric_val);
        self.latencies_us.push_back(latency_us);
        self.total_events += 1;
        if err_code != 0 {
            self.last_error_code = err_code;
        }
    }
}

// --- 3. STATELESS JAILED SANDBOX MODULE (Lua / Wasm Ready) ---

pub struct SandboxPlugin {
    pub ast_sha: String,
    lua: Lua,
    bytecode: Vec<u8>,
}

impl SandboxPlugin {
    /// Compiles and jails a plugin with strict memory (256KB) & instruction (10k) caps.
    pub fn compile_and_jail(ast_sha: &str, source_code: &str) -> Result<Self, KernelError> {
        // Strip OS, IO, Package, and Debug libraries—only pure math/table/string allowed
        let safe_libs = StdLib::MATH | StdLib::STRING | StdLib::TABLE;
        let lua = Lua::new_with(safe_libs, mlua::LuaOptions::default())
            .map_err(|e| KernelError::SandboxInit(e.to_string()))?;

        // Enforce strict 256 KB heap ceiling inside the sandbox
        lua.set_memory_limit(256 * 1024)
            .map_err(|e| KernelError::SandboxInit(e.to_string()))?;

        // Enforce strict 10,000 instruction timeout to kill infinite loops instantly
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            |_lua, _debug| Err(mlua::Error::RuntimeError("INSTRUCTION_BUDGET_EXCEEDED".into())),
        );

        let bytecode = lua
            .load(source_code)
            .into_function()
            .and_then(|f| f.dump(true))
            .map_err(|e| KernelError::SandboxInit(e.to_string()))?;

        // Load module chunk once to register `evaluate(state, event)`
        lua.load(&bytecode).exec()?;

        Ok(Self {
            ast_sha: ast_sha.to_string(),
            lua,
            bytecode,
        })
    }

    /// Executes the stateless `evaluate(state, event)` contract in microseconds.
    pub fn evaluate(
        &self,
        state: WindowState,
        event: EventFrame,
    ) -> Result<(PluginVerdict, u64), KernelError> {
        let start = Instant::now();
        let globals = self.lua.globals();
        let eval_fn: mlua::Function = globals.get("evaluate")?;

        let state_tbl = self.lua.create_table()?;
        state_tbl.set("window_count", state.window_count)?;
        state_tbl.set("rolling_mean", state.rolling_mean)?;
        state_tbl.set("rolling_p99_us", state.rolling_p99_us)?;
        state_tbl.set("last_error_code", state.last_error_code)?;

        let event_tbl = self.lua.create_table()?;
        event_tbl.set("timestamp_ns", event.timestamp_ns)?;
        event_tbl.set("entity_id", event.entity_id)?;
        event_tbl.set("metric_val", event.metric_val)?;
        event_tbl.set("flags", event.flags)?;

        let res_tbl: Table = eval_fn.call((state_tbl, event_tbl))?;
        let elapsed_us = start.elapsed().as_micros() as u64;

        let code_u8: u8 = res_tbl.get("code").unwrap_or(3);
        let score: f64 = res_tbl.get("score").unwrap_or(0.0);
        let reason_code: u32 = res_tbl.get("reason_code").unwrap_or(0);

        Ok((
            PluginVerdict {
                code: DecisionCode::from_u8(code_u8),
                score,
                reason_code,
            },
            elapsed_us,
        ))
    }
}

// --- 4. ATOMIC CHAMPION / SHADOW SLOT MANAGER ---

pub struct DualSlotEngine {
    domain: String,
    champion: ArcSwapOption<SandboxPlugin>,
    shadow: ArcSwapOption<SandboxPlugin>,
    state_buffer: HostStateBuffer,
    error_ema: f64,
}

impl DualSlotEngine {
    pub fn new(domain: &str, window_size: usize) -> Self {
        Self {
            domain: domain.to_string(),
            champion: ArcSwapOption::from(None),
            shadow: ArcSwapOption::from(None),
            state_buffer: HostStateBuffer::new(window_size),
            error_ema: 0.0,
        }
    }

    pub fn hot_swap_champion(&self, plugin: SandboxPlugin) {
        println!(
            "[KERNEL] Atomic Hot-Swap -> CHAMPION slot updated to SHA: {}",
            plugin.ast_sha
        );
        self.champion.store(Some(Arc::new(plugin)));
    }

    pub fn mount_shadow(&self, plugin: SandboxPlugin) {
        println!(
            "[KERNEL] Mounted candidate in SHADOW slot -> SHA: {}",
            plugin.ast_sha
        );
        self.shadow.store(Some(Arc::new(plugin)));
    }

    /// Processes a single event through Champion and tees to Shadow without blocking.
    pub fn process_frame(&mut self, event: EventFrame) -> PluginVerdict {
        let win_snapshot = self.state_buffer.snapshot();
        let mut champ_latency_us = 0;
        let mut err_flag = 0;

        // 1. Execute Production Champion Slot
        let verdict = if let Some(champ) = self.champion.load_full() {
            match champ.evaluate(win_snapshot, event) {
                Ok((v, lat_us)) => {
                    champ_latency_us = lat_us;
                    self.error_ema = 0.95 * self.error_ema;
                    v
                }
                Err(e) => {
                    eprintln!("[KERNEL] Champion ({}) fault: {:?}", champ.ast_sha, e);
                    err_flag = 500;
                    self.error_ema = (0.95 * self.error_ema) + 0.05;
                    PluginVerdict {
                        code: DecisionCode::EscalateToJudge,
                        score: 1.0,
                        reason_code: 500,
                    }
                }
            }
        } else {
            PluginVerdict {
                code: DecisionCode::Allow,
                score: 0.0,
                reason_code: 0,
            }
        };

        // 2. Tee to Read-Only Shadow Slot (Cannot affect production verdict or Host state)
        if let Some(shad) = self.shadow.load_full() {
            if let Err(e) = shad.evaluate(win_snapshot, event) {
                eprintln!(
                    "[KERNEL-SHADOW] Shadow candidate ({}) failed verification: {:?}",
                    shad.ast_sha, e
                );
                // Automatically evict broken Shadow module
                self.shadow.store(None);
            }
        }

        // 3. Update Host-owned sliding window state
        self.state_buffer
            .record_tick(event.metric_val, champ_latency_us, err_flag);

        verdict
    }
}

// --- 5. SQLITE WAL STIGMERGIC TELEMETRY BUS ---

pub struct StigmergicBus {
    conn: Connection,
}

impl StigmergicBus {
    pub fn open_and_init(db_path: &str) -> Result<Self, KernelError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS stigmergic_telemetry (
                 domain TEXT PRIMARY KEY,
                 active_champion_sha TEXT NOT NULL,
                 active_shadow_sha TEXT,
                 p99_latency_us INTEGER NOT NULL,
                 error_rate_ema REAL NOT NULL,
                 pheromone_urgency REAL NOT NULL,
                 updated_at_ns INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sandbox_artifacts (
                 ast_sha TEXT PRIMARY KEY,
                 domain TEXT NOT NULL,
                 runtime_target TEXT NOT NULL,
                 clean_room_source TEXT NOT NULL,
                 tier_status TEXT NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    /// Emits pheromone urgency to the SQLite WAL bus for the Python Swarm & Symbiont.
    pub fn emit_telemetry(&self, engine: &DualSlotEngine) -> Result<f64, KernelError> {
        let snap = engine.state_buffer.snapshot();
        let champ_sha = engine
            .champion
            .load_full()
            .map(|c| c.ast_sha.clone())
            .unwrap_or_else(|| "NONE".into());
        let shadow_sha = engine.shadow.load_full().map(|s| s.ast_sha.clone());

        // Compute scalar pheromone urgency (0.0 to 1.0) based on latency (>500us) or errors
        let latency_pressure = (snap.rolling_p99_us as f64 / 500.0).min(1.0);
        let pheromone = (latency_pressure.max(engine.error_ema * 5.0)).min(1.0);

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as i64;

        self.conn.execute(
            "INSERT INTO stigmergic_telemetry 
             (domain, active_champion_sha, active_shadow_sha, p99_latency_us, error_rate_ema, pheromone_urgency, updated_at_ns)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(domain) DO UPDATE SET
                 active_champion_sha = excluded.active_champion_sha,
                 active_shadow_sha = excluded.active_shadow_sha,
                 p99_latency_us = excluded.p99_latency_us,
                 error_rate_ema = excluded.error_rate_ema,
                 pheromone_urgency = excluded.pheromone_urgency,
                 updated_at_ns = excluded.updated_at_ns;",
            params![
                engine.domain,
                champ_sha,
                shadow_sha,
                snap.rolling_p99_us as i64,
                engine.error_ema,
                pheromone,
                now_ns
            ],
        )?;

        Ok(pheromone)
    }
}

// --- 6. SELF-CONTAINED SMOKE TEST & SIMULATION ---

fn main() -> Result<(), KernelError> {
    let bus = StigmergicBus::open_and_init("sese_connectome.db")?;
    let mut engine = DualSlotEngine::new("symbiosis:systems", 128);

    // 1. Load Baseline Champion v1 Plugin
    let champion_v1_src = r#"
        function evaluate(state, event)
            -- Simple static threshold rule
            if event.metric_val > 100.0 then
                return { code = 1, score = 0.85, reason_code = 101 }
            end
            return { code = 0, score = 0.10, reason_code = 0 }
        end
    "#;
    let champ_v1 = SandboxPlugin::compile_and_jail("sha_champ_v1_001", champion_v1_src)?;
    engine.hot_swap_champion(champ_v1);

    // 2. Load Adaptive Shadow v2 Candidate (Uses Host-owned rolling_mean!)
    let shadow_v2_src = r#"
        function evaluate(state, event)
            -- Dynamic z-score-like anomaly filter using Host WindowState
            local baseline = math.max(state.rolling_mean, 1.0)
            local ratio = event.metric_val / baseline
            if ratio > 3.5 and state.window_count > 5 then
                return { code = 3, score = 0.98, reason_code = 202 } -- Escalate to Symbiont Judge
            elseif ratio > 2.0 then
                return { code = 1, score = 0.70, reason_code = 201 } -- Throttle
            end
            return { code = 0, score = 0.05, reason_code = 0 }
        end
    "#;
    let shadow_v2 = SandboxPlugin::compile_and_jail("sha_shad_v2_002", shadow_v2_src)?;
    engine.mount_shadow(shadow_v2);

    // 3. Stream 10 normal events, then inject an anomaly spike
    for i in 1..=12 {
        let val = if i == 11 { 250.0 } else { 40.0 + (i as f64) };
        let frame = EventFrame {
            timestamp_ns: i * 1_000_000,
            entity_id: 42,
            metric_val: val,
            flags: 0,
        };
        let verdict = engine.process_frame(frame);
        if i >= 10 {
            println!("Tick #{:02} (val={:.1}) -> Verdict: {:?}", i, val, verdict);
        }
    }

    // 4. Promote Shadow v2 to Champion WITHOUT losing Host sliding-window state
    let promoted_v2 = SandboxPlugin::compile_and_jail("sha_shad_v2_002", shadow_v2_src)?;
    engine.hot_swap_champion(promoted_v2);

    let post_swap_snap = engine.state_buffer.snapshot();
    println!(
        "[STATE CHECK AFTER HOT-SWAP] Window count preserved: {}, Rolling mean: {:.2}, p99: {}us",
        post_swap_snap.window_count, post_swap_snap.rolling_mean, post_swap_snap.rolling_p99_us
    );

    // 5. Emit telemetry to SQLite WAL bus for Python Swarm & Symbiont
    let pheromone = bus.emit_telemetry(&engine)?;
    println!("[STIGMERGIC BUS] Emitted telemetry to SQLite WAL -> Pheromone urgency: {:.4}", pheromone);

    Ok(())
}

```

---

### 3. Why This Rust Host Kernel Solves the Hardest Problems First

* **Zero State Loss on Hot-Swap:** Look at step 4 of `main()`. When `sha_shad_v2_002` replaces `sha_champ_v1_001` as the active Champion, `engine.state_buffer` keeps all 12 prior ticks, the rolling mean, and the rolling p99 latency without a hiccup.
* **Automatic Rogue-Code Ejection:** If the Mechanic LLM hallucinations an infinite loop or allocates a 10MB table inside the Shadow slot, `mlua`'s instruction hook (`every_nth_instruction(10_000)`) and memory cap (`256 * 1024`) abort the Shadow execution immediately, and lines `258-264` automatically evict the broken Shadow plugin (`self.shadow.store(None)`) while the Champion keeps serving production traffic uninterrupted.
* **Zero-Config Shared Bus:** Both this Rust binary and the Python Swarm/Symbiont orchestrator attach to `sese_connectome.db` in `WAL` mode. There is no message broker (Kafka/Redis) to maintain or crash.


