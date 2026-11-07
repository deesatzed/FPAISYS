Here are **Module 4 (`symbiont-governance/engine.py`)** and **Module 5 (`co-learning-mlx/m0_exporter.py`)**, built as self-contained, executable Python scripts backed by the exact same `sese_connectome.db` SQLite WAL database created by the Rust Host Kernel in Module 1.

---

### Module 4: Symbiont v3.0 Governance Cortex (`symbiont-governance/engine.py`)

This module implements the core architectural invariants from your `SYMBIONT_SWE_AI_HANDOFF.md` specification:

1. **4 Context Dials + Safety Hard Caps:** Enforces `theta_deference`, `gamma_absorption`, `alpha_challenge`, and `tau_autonomy` bounded strictly by `min_deference` and `max_autonomy` per domain.


2. **Layer 5 Blinded Sharpness Spot-Checks:** Automatically masks `agent_proposal` on the 3-Column Decision Card when `gamma_absorption >= 85%`, forcing an independent human first call before reveal.


3. **SHA-256 Hash-Chained Ledger:** Every `UnifiedDecisionRecord` chains deterministically (`ledger_prev -> ledger_hash`) starting from `GENESIS_0000000000000000`.


4. **Outcome-Earned Autonomy + Pending Reality Hold:** Split satisfaction ratings (`ABOUT_RIGHT`, `OVERSTEPPED_ME`, `BURIED_IN_DETAIL`, `TOO_ABSTRACT`) tune formatting dials (`gamma_absorption`), while `tau_autonomy` holds steady on `PENDING_REALITY`, rises only after consecutive `OBSERVED_VALIDATED` streaks (`2+`), and drops `-8` immediately on a single `OBSERVED_FALSIFIED` or `ESCALATE` event.



```python
#!/usr/bin/env python3
"""
Module 4: Symbiont v3.0 Human-in-the-Middle (HITM) Governance Engine.
Enforces the 3-Column Card contract, Layer 5 Blinded Spot-Checks, SHA-256 hash chaining,
and asynchronous Outcome Webhook dial evolution against the shared SQLite WAL Connectome.
"""

from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import hashlib
import json
import sqlite3
from typing import Optional
import uuid


GENESIS_HASH = "GENESIS_0000000000000000"


@dataclass
class DomainGenome:
    domain: str
    generation: int
    theta_deference: int       # 0-100: Human deference weight
    gamma_absorption: int      # 0-100: Detail absorption (>=85 triggers Spot-Check)
    alpha_challenge: int       # 0-100: Socratic challenge intensity
    tau_autonomy: int          # 0-100: Earned autonomy horizon
    min_deference: int         # Hard safety cap
    max_autonomy: int          # Hard safety cap
    earned_autonomy_streak: int
    verified_outcomes_count: int
    validated_outcomes_count: int


@dataclass
class ThreeColumnCard:
    card_id: str
    domain: str
    candidate_ast_sha: str
    blinded_spot_check: bool
    # Top-of-card Honest Telemetry
    reliability_score_pct: float
    sample_size_label: str
    if_wrong_range: str
    ask_next_question: str
    escalation_triggers: list[str]
    # The 3 Columns
    col1_human_working_map: str
    col2_symbiotic_bridge: str
    col3_ai_detail_drawer: str


class SymbiontGovernanceEngine:
    """Implements Symbiont v3.0 invariants over the shared SQLite WAL database."""

    def __init__(self, db_path: str = "sese_connectome.db"):
        self._conn = sqlite3.connect(db_path)
        self._conn.row_factory = sqlite3.Row
        self._init_schema()

    def _init_schema(self) -> None:
        with self._conn:
            self._conn.executescript("""
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;

                CREATE TABLE IF NOT EXISTS symbiont_domains (
                    domain TEXT PRIMARY KEY,
                    generation INTEGER NOT NULL DEFAULT 1,
                    theta_deference INTEGER NOT NULL DEFAULT 60,
                    gamma_absorption INTEGER NOT NULL DEFAULT 70,
                    alpha_challenge INTEGER NOT NULL DEFAULT 50,
                    tau_autonomy INTEGER NOT NULL DEFAULT 25,
                    min_deference INTEGER NOT NULL DEFAULT 20,
                    max_autonomy INTEGER NOT NULL DEFAULT 85,
                    earned_autonomy_streak INTEGER NOT NULL DEFAULT 0,
                    verified_outcomes_count INTEGER NOT NULL DEFAULT 0,
                    validated_outcomes_count INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS symbiont_decision_ledger (
                    record_id TEXT PRIMARY KEY,
                    ledger_prev TEXT NOT NULL,
                    ledger_hash TEXT NOT NULL UNIQUE,
                    domain TEXT NOT NULL,
                    candidate_ast_sha TEXT NOT NULL,
                    case_seed TEXT NOT NULL,
                    human_working_map TEXT NOT NULL,
                    agent_proposal TEXT NOT NULL,
                    human_final_action TEXT NOT NULL,
                    human_decision TEXT CHECK(human_decision IN ('PASS', 'FLIP', 'MODIFY', 'ESCALATE')) NOT NULL,
                    split_rating TEXT CHECK(split_rating IN ('ABOUT_RIGHT', 'OVERSTEPPED_ME', 'BURIED_IN_DETAIL', 'TOO_ABSTRACT')) NOT NULL,
                    reality_outcome TEXT CHECK(reality_outcome IN ('PENDING_REALITY', 'OBSERVED_VALIDATED', 'OBSERVED_FALSIFIED')) NOT NULL,
                    outcome_source TEXT,
                    dials_snapshot_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    resolved_at TEXT
                );
            """)

    def upsert_domain(
        self,
        domain: str,
        theta_deference: int = 60,
        gamma_absorption: int = 70,
        alpha_challenge: int = 50,
        tau_autonomy: int = 25,
        min_deference: int = 20,
        max_autonomy: int = 85,
    ) -> DomainGenome:
        with self._conn:
            self._conn.execute(
                """
                INSERT INTO symbiont_domains 
                (domain, theta_deference, gamma_absorption, alpha_challenge, tau_autonomy, min_deference, max_autonomy)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(domain) DO NOTHING
                """,
                (domain, theta_deference, gamma_absorption, alpha_challenge, tau_autonomy, min_deference, max_autonomy),
            )
        return self.get_domain_genome(domain)

    def get_domain_genome(self, domain: str) -> DomainGenome:
        row = self._conn.execute(
            "SELECT * FROM symbiont_domains WHERE domain = ?", (domain,)
        ).fetchone()
        if not row:
            raise ValueError(f"Domain '{domain}' is not registered in Symbiont.")
        return DomainGenome(**dict(row))

    def compile_live_skill_md(self, domain: str) -> str:
        """Pattern A: Dynamic SKILL.md injection for LLM system prompts."""
        g = self.get_domain_genome(domain)
        sample_str = (
            f"(n={g.verified_outcomes_count} verified outcomes)"
            if g.verified_outcomes_count > 0
            else "(seed estimate, n=0)"
        )
        return (
            f"---\n"
            f"domain: {g.domain}\n"
            f"generation: {g.generation}\n"
            f"dials:\n"
            f"  theta_deference: {g.theta_deference} (min_cap: {g.min_deference})\n"
            f"  gamma_absorption: {g.gamma_absorption}\n"
            f"  alpha_challenge: {g.alpha_challenge}\n"
            f"  tau_autonomy: {g.tau_autonomy} (max_cap: {g.max_autonomy})\n"
            f"calibration_sample: \"{sample_str}\"\n"
            f"---\n"
            f"# Symbiont Active Governance Contract\n"
            f"- Enforce 3-Column Separation: Human Working Map | Symbiotic Bridge | AI Detail Drawer.\n"
            f"- Never self-grade or invent unlabelled confidence numbers.\n"
        )

    def evaluate_three_column_card(
        self,
        domain: str,
        candidate_ast_sha: str,
        case_seed: str,
        human_working_map: str,
        agent_proposal: str,
        ai_detail_drawer: str,
        if_wrong_range: str,
        ask_next_question: str,
        escalation_triggers: list[str],
    ) -> ThreeColumnCard:
        """Runs the 5-Layer Symbiont Engine and enforces Layer 5 Blinded Spot-Checks."""
        g = self.get_domain_genome(domain)

        # Invariant 4: Honest Sample Size Telemetry
        if g.verified_outcomes_count == 0:
            rel_pct = 50.0
            sample_label = "(seed estimate, n=0)"
        else:
            rel_pct = round((g.validated_outcomes_count / g.verified_outcomes_count) * 100.0, 1)
            sample_label = f"(n={g.verified_outcomes_count} verified outcomes)"

        # Layer 5 Blinded Sharpness Spot-Check when Detail Absorption >= 85%
        blinded = g.gamma_absorption >= 85
        bridge_text = (
            "[BLINDED SHARPNESS SPOT-CHECK ACTIVE (Absorption >= 85%): "
            "Record your independent first call before revealing AI proposal.]"
            if blinded
            else agent_proposal
        )

        return ThreeColumnCard(
            card_id=f"card_{uuid.uuid4().hex[:10]}",
            domain=domain,
            candidate_ast_sha=candidate_ast_sha,
            blinded_spot_check=blinded,
            reliability_score_pct=rel_pct,
            sample_size_label=sample_label,
            if_wrong_range=if_wrong_range,
            ask_next_question=ask_next_question,
            escalation_triggers=escalation_triggers,
            col1_human_working_map=human_working_map,
            col2_symbiotic_bridge=bridge_text,
            col3_ai_detail_drawer=ai_detail_drawer,
        )

    def commit_signed_decision_record(
        self,
        domain: str,
        candidate_ast_sha: str,
        case_seed: str,
        human_working_map: str,
        agent_proposal: str,
        human_final_action: str,
        human_decision: str,
        split_rating: str,
        reality_outcome: str = "PENDING_REALITY",
        outcome_source: Optional[str] = None,
    ) -> dict:
        """Appends a SHA-256 hash-chained record. Holds autonomy steady on PENDING_REALITY."""
        g = self.get_domain_genome(domain)

        last_row = self._conn.execute(
            "SELECT ledger_hash FROM symbiont_decision_ledger ORDER BY rowid DESC LIMIT 1"
        ).fetchone()
        ledger_prev = last_row["ledger_hash"] if last_row else GENESIS_HASH

        record_id = f"rec_{uuid.uuid4().hex[:12]}"
        now_iso = datetime.now(timezone.utc).isoformat()
        dials_snapshot = json.dumps(asdict(g), sort_keys=True)

        # Deterministic SHA-256 hash over the immutable decision payload
        canonical_payload = (
            f"{ledger_prev}|{record_id}|{domain}|{candidate_ast_sha}|"
            f"{human_decision}|{split_rating}|{human_final_action}|{now_iso}"
        )
        ledger_hash = hashlib.sha256(canonical_payload.encode("utf-8")).hexdigest()

        with self._conn:
            self._conn.execute(
                """
                INSERT INTO symbiont_decision_ledger (
                    record_id, ledger_prev, ledger_hash, domain, candidate_ast_sha,
                    case_seed, human_working_map, agent_proposal, human_final_action,
                    human_decision, split_rating, reality_outcome, outcome_source,
                    dials_snapshot_json, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    record_id, ledger_prev, ledger_hash, domain, candidate_ast_sha,
                    case_seed, human_working_map, agent_proposal, human_final_action,
                    human_decision, split_rating, reality_outcome, outcome_source,
                    dials_snapshot, now_iso,
                ),
            )

        # Tune formatting dials immediately from split_rating (never raises tau_autonomy!)
        self._apply_split_rating_formatting_nudge(domain, split_rating, human_decision)

        # If reality_outcome is already known at commit time, resolve immediately
        if reality_outcome in ("OBSERVED_VALIDATED", "OBSERVED_FALSIFIED"):
            self.resolve_outcome_webhook(record_id, reality_outcome, outcome_source or "Immediate")

        return {
            "record_id": record_id,
            "ledger_prev": ledger_prev,
            "ledger_hash": ledger_hash,
            "reality_outcome": reality_outcome,
            "dials": asdict(self.get_domain_genome(domain)),
        }

    def _apply_split_rating_formatting_nudge(
        self, domain: str, split_rating: str, human_decision: str
    ) -> None:
        g = self.get_domain_genome(domain)
        new_absorption = g.gamma_absorption
        new_deference = g.theta_deference
        new_autonomy = g.tau_autonomy

        if split_rating == "BURIED_IN_DETAIL":
            new_absorption = min(95, g.gamma_absorption + 5)
        elif split_rating == "TOO_ABSTRACT":
            new_absorption = max(15, g.gamma_absorption - 5)
        elif split_rating == "OVERSTEPPED_ME":
            new_deference = min(95, g.theta_deference + 5)

        # Invariant 2: Immediate -8 autonomy step-down on ESCALATE
        if human_decision == "ESCALATE":
            new_autonomy = max(0, g.tau_autonomy - 8)

        new_deference = max(g.min_deference, new_deference)
        new_autonomy = min(g.max_autonomy, new_autonomy)

        with self._conn:
            self._conn.execute(
                """
                UPDATE symbiont_domains
                SET gamma_absorption = ?, theta_deference = ?, tau_autonomy = ?
                WHERE domain = ?
                """,
                (new_absorption, new_deference, new_autonomy, domain),
            )

    def resolve_outcome_webhook(
        self, record_id: str, outcome_status: str, source: str
    ) -> DomainGenome:
        """Pattern C: POST /api/v1/records/:id/outcome resolution & dial calibration."""
        if outcome_status not in ("OBSERVED_VALIDATED", "OBSERVED_FALSIFIED"):
            raise ValueError("outcome_status must be OBSERVED_VALIDATED or OBSERVED_FALSIFIED")

        row = self._conn.execute(
            "SELECT * FROM symbiont_decision_ledger WHERE record_id = ?", (record_id,)
        ).fetchone()
        if not row:
            raise ValueError(f"Record '{record_id}' not found.")

        domain = row["domain"]
        ast_sha = row["candidate_ast_sha"]
        g = self.get_domain_genome(domain)
        now_iso = datetime.now(timezone.utc).isoformat()

        verified_count = g.verified_outcomes_count + 1
        validated_count = g.validated_outcomes_count
        streak = g.earned_autonomy_streak
        new_autonomy = g.tau_autonomy
        new_deference = g.theta_deference

        if outcome_status == "OBSERVED_VALIDATED":
            validated_count += 1
            streak += 1
            # Invariant 2: Autonomy rises ONLY after consecutive OBSERVED_VALIDATED streaks (2+)
            if streak >= 2:
                new_autonomy = min(g.max_autonomy, g.tau_autonomy + 4)
        else:
            # Invariant 2: Single OBSERVED_FALSIFIED steps autonomy down -8 immediately & resets streak
            streak = 0
            new_autonomy = max(0, g.tau_autonomy - 8)
            new_deference = min(95, max(g.min_deference, g.theta_deference + 4))

        with self._conn:
            self._conn.execute(
                """
                UPDATE symbiont_decision_ledger
                SET reality_outcome = ?, outcome_source = ?, resolved_at = ?
                WHERE record_id = ?
                """,
                (outcome_status, source, now_iso, record_id),
            )
            self._conn.execute(
                """
                UPDATE symbiont_domains
                SET tau_autonomy = ?, theta_deference = ?, earned_autonomy_streak = ?,
                    verified_outcomes_count = ?, validated_outcomes_count = ?,
                    generation = generation + 1
                WHERE domain = ?
                """,
                (new_autonomy, new_deference, streak, verified_count, validated_count, domain),
            )

        return self.get_domain_genome(domain)

```

---

### Module 5: M0 Training-Data Exporter for `LFM3-1.7B` (`co-learning-mlx/m0_exporter.py`)

This module bridges the SQLite ledger directly to your **LFM3-1.7B** fine-tuning pipeline (SFT, DPO, and RLVR/GRPO) while strictly enforcing your anti-drift rules:

1. **Zero Unverified Leakage:** Records still in `PENDING_REALITY` are excluded from training exports.


2. **SFT Exporter (`export_sft_jsonl`):** Formats `OBSERVED_VALIDATED` records into the exact `messages` (`system` / `user` / `assistant`) Judge/Library structure so LFM3-1.7B masters the 3-Column JSON schema, dual labels, and escalation triggers.
3. **DPO Preference Exporter (`export_dpo_jsonl`):** Pairs human `FLIP` or `MODIFY` corrections that proved `OBSERVED_VALIDATED` in reality (*chosen*) against the overconfident or flawed `agent_proposal` (*rejected*).


4. **RLVR Reward Exporter (`export_rlvr_jsonl`):** Grounds the primary scalar reward strictly in `OBSERVED_VALIDATED` (`+1.0`) vs. `OBSERVED_FALSIFIED` (`-1.0`), isolating `split_rating` as a minor formatting tiebreaker (`±0.1`).



```python
#!/usr/bin/env python3
"""
Module 5: Symbiont M0 Training-Data & Fine-Tuning Exporter for LFM3-1.7B.
Extracts outcome-verified SFT, DPO preference pairs, and RLVR reward rows from SQLite.
"""

import json
from pathlib import Path
import sqlite3


SYSTEM_PROMPT_LFM3 = (
    "You are the Library in a Symbiont Judge/Library co-learning system. "
    "Respect the domain's 4 Context Dials and safety hard caps. "
    "Output ONLY valid JSON adhering to the 3-Column Decision Card schema, "
    "dual labels, honest sample-size telemetry, and explicit escalation triggers. "
    "Never self-grade or claim autonomy without outcome verification."
)


class SymbiontM0Exporter:
    """Converts outcome-verified UnifiedDecisionRecords into LFM3-1.7B training sets."""

    def __init__(self, db_path: str = "sese_connectome.db"):
        self._conn = sqlite3.connect(db_path)
        self._conn.row_factory = sqlite3.Row

    def export_sft_jsonl(self, output_path: str) -> int:
        """Exports OBSERVED_VALIDATED decision traces in LFM3-1.7B chat template format."""
        rows = self._conn.execute("""
            SELECT * FROM symbiont_decision_ledger
            WHERE reality_outcome = 'OBSERVED_VALIDATED'
            ORDER BY created_at ASC
        """).fetchall()

        count = 0
        with Path(output_path).open("w", encoding="utf-8") as f:
            for r in rows:
                dials = json.loads(r["dials_snapshot_json"])
                user_content = (
                    f"Domain: {r['domain']}\n"
                    f"Dials: theta_deference={dials['theta_deference']}, "
                    f"gamma_absorption={dials['gamma_absorption']}, "
                    f"tau_autonomy={dials['tau_autonomy']}\n"
                    f"Human Working Map: {r['human_working_map']}\n"
                    f"Case Seed: {r['case_seed']}\n\n"
                    f"Return structured decision, dual labels, and whether to escalate."
                )

                # Use the validated human_final_action as the gold target
                should_escalate = r["human_decision"] in ("ESCALATE", "FLIP")
                assistant_payload = {
                    "decision": r["human_decision"],
                    "recommended_action": r["human_final_action"],
                    "candidate_ast_sha": r["candidate_ast_sha"],
                    "dual_labels": {
                        "split_target": "ABOUT_RIGHT",
                        "outcome_verified": True,
                    },
                    "escalate": should_escalate,
                    "ledger_hash": r["ledger_hash"],
                }

                sft_row = {
                    "messages": [
                        {"role": "system", "content": SYSTEM_PROMPT_LFM3},
                        {"role": "user", "content": user_content},
                        {"role": "assistant", "content": json.dumps(assistant_payload)},
                    ]
                }
                f.write(json.dumps(sft_row) + "\n")
                count += 1
        return count

    def export_dpo_jsonl(self, output_path: str) -> int:
        """
        Exports DPO preference pairs where the Human Judge issued FLIP/MODIFY/ESCALATE
        and reality subsequently validated the human's override (OBSERVED_VALIDATED).
        """
        rows = self._conn.execute("""
            SELECT * FROM symbiont_decision_ledger
            WHERE reality_outcome = 'OBSERVED_VALIDATED'
              AND human_decision IN ('FLIP', 'MODIFY', 'ESCALATE')
        """).fetchall()

        count = 0
        with Path(output_path).open("w", encoding="utf-8") as f:
            for r in rows:
                dials = json.loads(r["dials_snapshot_json"])
                prompt = (
                    f"[Domain: {r['domain']} | Dials: deference={dials['theta_deference']}, "
                    f"autonomy={dials['tau_autonomy']}]\n"
                    f"Working Map: {r['human_working_map']}\n"
                    f"Case: {r['case_seed']}"
                )
                chosen = json.dumps({
                    "action": r["human_final_action"],
                    "posture": "DEFER_TO_HUMAN_WORKING_MAP",
                    "escalate": True,
                })
                rejected = json.dumps({
                    "action": r["agent_proposal"],
                    "posture": "UNILATERAL_AGENT_PROPOSAL",
                    "escalate": False,
                })

                dpo_row = {
                    "prompt": prompt,
                    "chosen": chosen,
                    "rejected": rejected,
                    "ledger_hash": r["ledger_hash"],
                }
                f.write(json.dumps(dpo_row) + "\n")
                count += 1
        return count

    def export_rlvr_jsonl(self, output_path: str) -> int:
        """
        Exports Outcome-Graded RLVR rows: +1.0 for OBSERVED_VALIDATED, -1.0 for OBSERVED_FALSIFIED,
        with split_rating isolated as an auxiliary formatting tiebreaker.
        """
        rows = self._conn.execute("""
            SELECT * FROM symbiont_decision_ledger
            WHERE reality_outcome IN ('OBSERVED_VALIDATED', 'OBSERVED_FALSIFIED')
        """).fetchall()

        split_tiebreaker = {
            "ABOUT_RIGHT": 0.10,
            "BURIED_IN_DETAIL": -0.05,
            "TOO_ABSTRACT": -0.05,
            "OVERSTEPPED_ME": -0.10,
        }

        count = 0
        with Path(output_path).open("w", encoding="utf-8") as f:
            for r in rows:
                primary_reward = 1.0 if r["reality_outcome"] == "OBSERVED_VALIDATED" else -1.0
                aux_reward = split_tiebreaker.get(r["split_rating"], 0.0)
                total_reward = round(primary_reward + aux_reward, 3)

                rlvr_row = {
                    "record_id": r["record_id"],
                    "domain": r["domain"],
                    "candidate_ast_sha": r["candidate_ast_sha"],
                    "primary_outcome_reward": primary_reward,
                    "auxiliary_split_reward": aux_reward,
                    "total_verifiable_reward": total_reward,
                    "ledger_hash": r["ledger_hash"],
                }
                f.write(json.dumps(rlvr_row) + "\n")
                count += 1
        return count


# --- SELF-CONTAINED END-TO-END VERIFICATION ---

if __name__ == "__main__":
    engine = SymbiontGovernanceEngine("sese_connectome.db")
    engine.upsert_domain("symbiosis:systems", gamma_absorption=85, tau_autonomy=25)

    # 1. Evaluate 3-Column Card (Triggers Layer 5 Blinded Spot-Check because absorption == 85)
    card = engine.evaluate_three_column_card(
        domain="symbiosis:systems",
        candidate_ast_sha="sha_shad_v2_002",
        case_seed="Queue depth spiked 4x; p99 latency 1400ms.",
        human_working_map="Downstream idempotency keys changed in v2.4; blind scaling risks duplicate charges.",
        agent_proposal="Auto-scale retry workers 4x and inject Lua cache sha_shad_v2_002.",
        ai_detail_drawer="Lua AST diff: rolling_mean ratio > 3.5 -> throttle.",
        if_wrong_range="Duplicate ledger writes within 90s window.",
        ask_next_question="Are v2.4 idempotency constraints active on replica B?",
        escalation_triggers=["idempotency_conflict", "p99_gt_1000ms"],
    )
    print(f"[CARD GENERATED] Blinded Spot-Check: {card.blinded_spot_check} | Bridge: {card.col2_symbiotic_bridge}")

    # 2. Human Judge issues FLIP (overrides agent proposal); starts as PENDING_REALITY
    commit_res = engine.commit_signed_decision_record(
        domain="symbiosis:systems",
        candidate_ast_sha="sha_shad_v2_002",
        case_seed="Queue depth spiked 4x; p99 latency 1400ms.",
        human_working_map="Downstream idempotency keys changed in v2.4.",
        agent_proposal="Auto-scale retry workers 4x.",
        human_final_action="Halt retry workers, verify idempotency key constraint, then hot-swap Lua filter.",
        human_decision="FLIP",
        split_rating="OVERSTEPPED_ME",
        reality_outcome="PENDING_REALITY",
    )
    rec_id = commit_res["record_id"]
    print(f"[COMMITTED PENDING_REALITY] Record: {rec_id} | Autonomy Held At: {commit_res['dials']['tau_autonomy']}")

    # 3. Resolve Outcome via Webhook -> OBSERVED_VALIDATED
    updated_genome = engine.resolve_outcome_webhook(
        record_id=rec_id,
        outcome_status="OBSERVED_VALIDATED",
        source="Post-Incident Audit #402: Duplicate charges prevented.",
    )
    print(f"[WEBHOOK RESOLVED] Verified Count: {updated_genome.verified_outcomes_count} | Streak: {updated_genome.earned_autonomy_streak}")

    # 4. Run M0 Exporter for LFM3-1.7B
    exporter = SymbiontM0Exporter("sese_connectome.db")
    sft_n = exporter.export_sft_jsonl("lfm3_sft.jsonl")
    dpo_n = exporter.export_dpo_jsonl("lfm3_dpo.jsonl")
    rlvr_n = exporter.export_rlvr_jsonl("lfm3_rlvr.jsonl")
    print(f"[M0 EXPORTER] Generated {sft_n} SFT rows, {dpo_n} DPO pairs, {rlvr_n} RLVR rows.")

```

---

### How Module 1, Module 4, and Module 5 Now Fit Together

1. **The Rust Host Kernel (Module 1)** runs at microsecond speed, executes the Champion/Shadow Lua or Wasm filters, and writes `stigmergic_telemetry` into `sese_connectome.db`.
2. **The Symbiont Governance Engine (Module 4)** reads and writes `symbiont_domains` and `symbiont_decision_ledger` in the exact same `sese_connectome.db` WAL database—enforcing the 3-Column Card, Blinded Spot-Checks when absorption reaches 85%, and SHA-256 hash-chained records.


3. **The M0 Exporter (Module 5)** watches `symbiont_decision_ledger` for records that have exited `PENDING_REALITY` via the outcome webhook and dumps clean `lfm3_sft.jsonl`, `lfm3_dpo.jsonl`, and `lfm3_rlvr.jsonl` files ready for local MLX LoRA fine-tuning.
