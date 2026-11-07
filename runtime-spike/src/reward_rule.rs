//! A pinned synthetic mapping, not an outcome validation or training eligibility decision.
use sha2::{Digest, Sha256};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleInput {
    schema: String,
    version: String,
    validated: i8,
    falsified: i8,
    actor: String,
    source: String,
}

pub struct SyntheticRewardRule {
    digest: [u8; 32],
    actor: String,
    source: String,
}

impl SyntheticRewardRule {
    pub fn from_pinned_json(bytes: &[u8], pin: &[u8; 32]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > 65_536 {
            return Err("invalid synthetic reward rule size".into());
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if &digest != pin {
            return Err("synthetic reward rule digest mismatch".into());
        }
        let input: RuleInput =
            serde_json::from_slice(bytes).map_err(|_| "invalid synthetic reward rule")?;
        let valid = |s: &str| !s.trim().is_empty() && s.trim() == s && s.len() <= 4096;
        if input.schema != "sese-synthetic-reward-rule-v1"
            || input.version != "outcome-sign-v1"
            || input.validated != 1
            || input.falsified != -1
            || !valid(&input.actor)
            || !valid(&input.source)
        {
            return Err("unsupported synthetic reward rule".into());
        }
        Ok(Self {
            digest,
            actor: input.actor,
            source: input.source,
        })
    }

    pub fn reward(&self, outcome: &str) -> Result<i8, String> {
        match outcome {
            "validated" => Ok(1),
            "falsified" => Ok(-1),
            _ => Err("unresolved or unknown outcome".into()),
        }
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub fn actor(&self) -> &str {
        &self.actor
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn version(&self) -> &'static str {
        "outcome-sign-v1"
    }
}
