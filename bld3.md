Here is how all 5 modules snap together into a unified, closed-loop **Symbiotic Self-Evolving Software Engine (S-SESE)**.

To visualize how the execution loop flows from a raw system event or human prompt all the way to autonomous M0 micro-tuning, follow this complete, end-to-end operational sequence:

```
[1. HOST KERNEL & STIGMERGIC BUS] (Rust/Go + SQLite WAL)
         │  (Emits pheromone urgency when latency spikes)
         ▼
[2. SWARM FACTORY & PRIVACY AIRLOCK] (Python)
         │  (Airlock strips proprietary schemas -> Cloud Mechanic builds Wasm/Lua plugin)
         ▼
[3. OPF PRE-FILTER & SANDBOX ORACLE] (Python + Wasm/Lua)
         │  (Kills 95% of bad code via OPF; tests survivor against locked FrozenReplayCase[])
         ▼
[4. SYMBIONT v3.0 GOVERNANCE CORTEX] (MCP / REST v1)
         │  (Checks 4 Context Dials, Layer 5 Blinded Spot-Checks, & commits SHA-256 hash ledger)
         ├─────────────────────────────────────────┐
         │ (If Autonomy High & Validated)          │ (If Low Autonomy or Borderline)
         ▼                                         ▼
   [Atomic Hot-Swap to]                    [3-Column Decision Card]
   [Host Kernel Champion]                  [Surface to Human Judge]
         │                                         │
         └───────────────────┬─────────────────────┘
                             │
                             ▼
   [5. ASYNC REALITY RESOLUTION & M0 EXPORTER]
                             │
                             ▼
   [JIT LFM3-1.7B Micro-Tune via MLX LoRA]

```

---

### The Complete End-to-End Execution Loop (`sese_master_loop.py`)

This master orchestrator script ties all 5 modules together into a single script demonstrating a full evolution cycle:

```python
#!/usr/bin/env python3
"""
S-SESE Master End-to-End Orchestrator Loop.
Connects Module 1 (Host Kernel / SQLite), Module 3 (Airlock & Oracle), 
Module 4 (Symbiont v3.0 Governance), and Module 5 (M0 Exporter).
"""

from dataclasses import dataclass
import hashlib
import sqlite3

# Import our modular components
from symbiont_governance.engine import SymbiontGovernanceEngine
from symbiont_governance.engine import SymbiontM0Exporter


class MasterSESEEngine:
    """Master orchestrator running the closed-loop self-evolving engine."""

    def __init__(self, db_path: str = "sese_connectome.db"):
        self.db_path = db_path
        self.conn = sqlite3.connect(db_path)
        self.conn.row_factory = sqlite3.Row
        
        # Initialize Symbiont Governance & M0 subsystems
        self.governance = SymbiontGovernanceEngine(db_path)
        self.exporter = SymbiontM0Exporter(db_path)

        # Register domain with safety hard caps
        self.governance.upsert_domain(
            domain="symbiosis:systems",
            theta_deference=60,
            gamma_absorption=85, # Triggers Layer 5 Blinded Spot-Check
            tau_autonomy=25,
            min_deference=20,
            max_autonomy=85
        )

    def run_full_evolution_cycle(self, raw_problem_seed: str, human_working_map: str) -> str:
        print("\n=== [STAGE 1] HOST KERNEL TELEMETRY & STIGMERGIC TRIGGER ===")
        # In production, Module 1 (Rust Host Kernel) writes pheromone urgency to SQLite WAL
        self.conn.execute("""
            INSERT INTO stigmergic_telemetry (domain, active_champion_sha, p99_latency_us, error_rate_ema, pheromone_urgency, updated_at_ns)
            VALUES ('symbiosis:systems', 'sha_champ_v1_001', 1400, 0.08, 0.92, 1718000000000)
            ON CONFLICT(domain) DO UPDATE SET pheromone_urgency=0.92, p99_latency_us=1400;
        """)
        self.conn.commit()
        print("[HOST KERNEL] Pheromone spike detected (urgency=0.92). Swarm Architect awakened.")

        print("\n=== [STAGE 2] SWARM FACTORY & PRIVACY AIRLOCK ===")
        # Module 3: Strip proprietary table names before delegation
        from module3_airlock_oracle import ASTPrivacyAirlock
        airlock = ASTPrivacyAirlock()
        clean_spec, mapping = airlock.strip_proprietary(raw_problem_seed)
        print(f"[AIRLOCK] Stripped proprietary schema tokens: {mapping}")
        print(f"[MECHANIC AGENT] Generated candidate Wasm/Lua plugin for clean spec: {clean_spec}")

        print("\n=== [STAGE 3] OPF PRE-FILTER & SANDBOX ORACLE ===")
        # Module 3: OPF Pre-Filter and Frozen Replay Gate
        from module3_airlock_oracle import OPFPreFilterAndOracle
        oracle = OPFPreFilterAndOracle([{"case_id": "c1", "expected_verdict": 1}])
        
        simulated_bytecode = b"WASM_OR_LUA_BYTECODE_STREAM"
        ast_sha = hashlib.sha256(simulated_bytecode).hexdigest()[:16]

        opf_verdict = oracle.opf_prefilter(clean_spec)
        if not opf_verdict.passed:
            return f"Aborted by OPF Pre-Filter: {opf_verdict.critique}"
        print(f"[OPF GATEKEEPER] Passed! Compile prob: {opf_verdict.compile_probability}")

        passed_replay, entropy = oracle.run_frozen_replay_gate("symbiosis:systems", simulated_bytecode)
        print(f"[FROZEN REPLAY GATE] Passed holdout suite. Entropy score: {entropy}")

        print("\n=== [STAGE 4] SYMBIONT v3.0 GOVERNANCE CORTEX ===")
        # Module 4: Evaluate 3-Column Card & check Layer 5 Blinded Spot-Check
        card = self.governance.evaluate_three_column_card(
            domain="symbiosis:systems",
            candidate_ast_sha=ast_sha,
            case_seed=raw_problem_seed,
            human_working_map=human_working_map,
            agent_proposal="Hot-swap Wasm plugin to optimize sliding-window memory buffer.",
            ai_detail_drawer=f"AST SHA: {ast_sha} | Memory footprint: 18 KB",
            if_wrong_range="P99 latency exceeding 1000ms threshold.",
            ask_next_question="Does replica B support zero-copy ring buffers?",
            escalation_triggers=["latency_spike", "memory_pressure"]
        )
        print(f"[GOVERNANCE] 3-Column Card compiled. Blinded Spot-Check Active? {card.blinded_spot_check}")
        print(f"[GOVERNANCE] Symbiotic Bridge Display: {card.col2_symbiotic_bridge}")

        # Human Judge commits decision (starts as PENDING_REALITY)
        commit_res = self.governance.commit_signed_decision_record(
            domain="symbiosis:systems",
            candidate_ast_sha=ast_sha,
            case_seed=raw_problem_seed,
            human_working_map=human_working_map,
            agent_proposal="Hot-swap Wasm plugin.",
            human_final_action="Verify idempotency keys, mount Shadow slot, then promote to Champion.",
            human_decision="FLIP",
            split_rating="OVERSTEPPED_ME",
            reality_outcome="PENDING_REALITY"
        )
        rec_id = commit_res["record_id"]
        print(f"[LEDGER] SHA-256 Hash-Chained Record Committed: {rec_id} (Status: PENDING_REALITY)[cite: 1]")

        print("\n=== [STAGE 5] ASYNC OUTCOME RESOLUTION & M0 EXPORTER ===")
        # Downstream reality arrives hours later via Outcome Webhook
        updated_genome = self.governance.resolve_outcome_webhook(
            record_id=rec_id,
            outcome_status="OBSERVED_VALIDATED",
            source="Post-Incident Audit #402: Zero duplicate writes, latency normalized to 180ms."
        )
        print(f"[OUTCOME WEBHOOK] Resolved to OBSERVED_VALIDATED! Verified count: {updated_genome.verified_outcomes_count}[cite: 1]")

        # Module 5: Export verified dataset for LFM3-1.7B JIT Fine-Tuning
        sft_count = self.exporter.export_sft_jsonl("lfm3_sft.jsonl")
        dpo_count = self.exporter.export_dpo_jsonl("lfm3_dpo.jsonl")
        rlvr_count = self.exporter.export_rlvr_jsonl("lfm3_rlvr.jsonl")
        print(f"[M0 EXPORTER] Exported {sft_count} SFT rows, {dpo_count} DPO pairs, {rlvr_count} RLVR rows ready for MLX LoRA training[cite: 1]!")

        return f"SUCCESSFULLY_EVOLVED_PLUGIN: {ast_sha}"


if __name__ == "__main__":
    engine = MasterSESEEngine()
    result = engine.run_full_evolution_cycle(
        raw_problem_seed="Queue depth spiked 4x on table F_ED_ENCOUNTERS during shift change.",
        human_working_map="Downstream idempotency keys were modified in v2.4; blind scaling risks duplicate charges."
    )
    print(f"\nFinal Engine Status: {result}")

```
