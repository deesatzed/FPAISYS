//! Signature primitives for future evidence imports; not an eligibility decision.
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorityRole {
    TraceSource,
    OutcomeObserver,
    PreferenceReviewer,
}

impl AuthorityRole {
    fn label(self) -> &'static str {
        match self {
            Self::TraceSource => "trace-source",
            Self::OutcomeObserver => "outcome-observer",
            Self::PreferenceReviewer => "preference-reviewer",
        }
    }
}

/// Supplied by a caller; this type does not load or establish trusted configuration.
pub struct Authority {
    pub id: String,
    pub role: AuthorityRole,
    pub public_key: [u8; 32],
    pub revoked: bool,
}

/// Validated configuration bytes matched to a caller-supplied digest, not a custody guarantee.
pub struct AuthoritySet {
    authorities: Vec<Authority>,
    digest: [u8; 32],
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoritySetJson {
    schema: String,
    authorities: Vec<AuthorityJson>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityJson {
    id: String,
    role: AuthorityRole,
    public_key: String,
    revoked: bool,
}

impl AuthoritySet {
    /// Check exact bytes against an independently supplied pin and reject ambiguous keys or IDs.
    pub fn from_pinned_json(bytes: &[u8], expected_digest: &[u8; 32]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > 65_536 {
            return Err("invalid authority configuration size".into());
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if &digest != expected_digest {
            return Err("authority configuration digest mismatch".into());
        }
        let config: AuthoritySetJson =
            serde_json::from_slice(bytes).map_err(|_| "invalid authority configuration")?;
        if config.schema != "sese-authority-set-v1"
            || config.authorities.is_empty()
            || config.authorities.len() > 128
        {
            return Err("invalid authority configuration".into());
        }
        let mut authorities = Vec::with_capacity(config.authorities.len());
        let mut ids = std::collections::HashSet::new();
        let mut keys = std::collections::HashSet::new();
        for item in config.authorities {
            if item.id.trim().is_empty()
                || item.id.trim() != item.id
                || item.id.len() > 256
                || !ids.insert(item.id.clone())
                || item.public_key.len() != 64
                || !item
                    .public_key
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("invalid authority identity".into());
            }
            let mut public_key = [0u8; 32];
            for (index, byte) in public_key.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&item.public_key[index * 2..index * 2 + 2], 16)
                    .map_err(|_| "invalid public key")?;
            }
            let key = VerifyingKey::from_bytes(&public_key).map_err(|_| "invalid public key")?;
            if key.is_weak() || !keys.insert(public_key) {
                return Err("weak or duplicate public key".into());
            }
            authorities.push(Authority {
                id: item.id,
                role: item.role,
                public_key,
                revoked: item.revoked,
            });
        }
        Ok(Self {
            authorities,
            digest,
        })
    }

    pub fn get(&self, id: &str, role: AuthorityRole) -> Option<&Authority> {
        self.authorities
            .iter()
            .find(|a| a.id == id && a.role == role && !a.revoked)
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// A pinned operator declaration for synthetic evidence; it does not prove its assertions.
pub struct SyntheticTrustPolicy {
    digest: [u8; 32],
    authority_digest: [u8; 32],
    training_export_allowed: bool,
    declarations: Vec<SyntheticTrustDeclaration>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SyntheticTrustPolicyJson {
    schema: String,
    authority_config_sha256: String,
    #[serde(default)]
    training_export_allowed: bool,
    declarations: Vec<SyntheticTrustDeclaration>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SyntheticTrustDeclaration {
    trace_source: String,
    outcome_observer: String,
    preference_reviewer: Option<String>,
    operator: String,
    observer_relationship_basis: String,
    reviewer_relationship_basis: Option<String>,
}

impl SyntheticTrustPolicy {
    /// Bind exact policy bytes to a separately supplied digest and a pinned authority set.
    pub fn from_pinned_json(
        bytes: &[u8],
        expected_digest: &[u8; 32],
        authorities: &AuthoritySet,
    ) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > 65_536 {
            return Err("invalid trust policy size".into());
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if &digest != expected_digest {
            return Err("trust policy digest mismatch".into());
        }
        let policy: SyntheticTrustPolicyJson =
            serde_json::from_slice(bytes).map_err(|_| "invalid trust policy")?;
        let authority_digest: String = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if policy.schema != "sese-synthetic-trust-policy-v1"
            || policy.authority_config_sha256 != authority_digest
            || policy.declarations.is_empty()
            || policy.declarations.len() > 128
        {
            return Err("invalid trust policy".into());
        }
        let mut pairs = std::collections::HashSet::new();
        let valid_text = |s: &str| !s.trim().is_empty() && s.trim() == s && s.len() <= 4096;
        for d in &policy.declarations {
            if authorities
                .get(&d.trace_source, AuthorityRole::TraceSource)
                .is_none()
                || authorities
                    .get(&d.outcome_observer, AuthorityRole::OutcomeObserver)
                    .is_none()
                || !valid_text(&d.operator)
                || !valid_text(&d.observer_relationship_basis)
                || !pairs.insert((&d.trace_source, &d.outcome_observer))
            {
                return Err("invalid trust declaration".into());
            }
            match (&d.preference_reviewer, &d.reviewer_relationship_basis) {
                (Some(reviewer), Some(basis))
                    if authorities
                        .get(reviewer, AuthorityRole::PreferenceReviewer)
                        .is_some()
                        && valid_text(basis) =>
                {
                    ()
                }
                (None, None) => (),
                _ => return Err("invalid reviewer declaration".into()),
            }
        }
        Ok(Self {
            digest,
            authority_digest: authorities.digest(),
            training_export_allowed: policy.training_export_allowed,
            declarations: policy.declarations,
        })
    }

    /// Report only what the supplied policy declares, not whether the declaration is true.
    pub fn declares(&self, source: &str, observer: &str, reviewer: Option<&str>) -> bool {
        self.declarations.iter().any(|d| {
            d.trace_source == source
                && d.outcome_observer == observer
                && reviewer.map_or(true, |id| d.preference_reviewer.as_deref() == Some(id))
        })
    }

    /// Return the operator's declared relationship; this is not a verification of that claim.
    pub fn common_declaration(&self, source: &str, observer: &str) -> Option<serde_json::Value> {
        self.declarations.iter().find(|d| d.trace_source == source && d.outcome_observer == observer)
            .map(|d| serde_json::json!({"operator":d.operator,"trace_source":d.trace_source,
                "outcome_observer":d.outcome_observer,"observer_relationship_basis":d.observer_relationship_basis}))
    }

    pub fn reviewer_declaration(
        &self,
        source: &str,
        observer: &str,
        reviewer: &str,
    ) -> Option<serde_json::Value> {
        self.declarations.iter().find(|d| d.trace_source == source && d.outcome_observer == observer && d.preference_reviewer.as_deref() == Some(reviewer))
            .map(|d| serde_json::json!({"operator":d.operator,"reviewer":reviewer,"reviewer_relationship_basis":d.reviewer_relationship_basis}))
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub fn training_export_allowed(&self) -> bool {
        self.training_export_allowed
    }

    pub fn authority_digest(&self) -> [u8; 32] {
        self.authority_digest
    }
}

/// A claim and signature presented with caller-supplied authority configuration.
pub struct SignedClaim<'a, T> {
    pub evidence: &'a T,
    pub authority: &'a Authority,
    pub signature: &'a [u8; 64],
}

pub struct NamedSignedClaim<'a, T> {
    pub evidence: &'a T,
    pub authority_id: &'a str,
    pub signature: &'a [u8; 64],
}

/// References are claims until checked against the verified database and retained checkpoint.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReference {
    pub review_id: i64,
    pub replay_id: i64,
    pub candidate_sha256: String,
    pub fixture_sha256: String,
    pub review_ledger_hash: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticTraceEvidence {
    pub reference: EvidenceReference,
    pub prompt: String,
    pub response: String,
    pub source: String,
    pub method: String,
    pub captured_unix_ns: u64,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyntheticOutcome {
    Validated,
    Falsified,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticOutcomeEvidence {
    pub reference: EvidenceReference,
    pub outcome: SyntheticOutcome,
    pub source: String,
    pub method: String,
    pub evidence_reference: String,
    pub observed_unix_ns: u64,
}

/// An explicit synthetic pair; a signature does not prove comparability or reviewer independence.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticPreferenceEvidence {
    pub reference: EvidenceReference,
    pub trace_id: i64,
    pub trace_ledger_hash: String,
    pub prompt: String,
    pub preferred: String,
    pub rejected: String,
    pub source: String,
    pub comparability_basis: String,
    pub reviewed_unix_ns: u64,
}

fn evidence_reference_bytes(reference: &EvidenceReference) -> Result<Vec<u8>, String> {
    let is_hash = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if reference.review_id <= 0
        || reference.replay_id <= 0
        || !is_hash(&reference.candidate_sha256)
        || !is_hash(&reference.fixture_sha256)
        || !is_hash(&reference.review_ledger_hash)
    {
        return Err("invalid evidence reference".into());
    }
    let mut out = Vec::new();
    out.extend_from_slice(&reference.review_id.to_be_bytes());
    out.extend_from_slice(&reference.replay_id.to_be_bytes());
    for hash in [
        &reference.candidate_sha256,
        &reference.fixture_sha256,
        &reference.review_ledger_hash,
    ] {
        out.extend_from_slice(hash.as_bytes());
    }
    Ok(out)
}

fn append_evidence_field(out: &mut Vec<u8>, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 16_384 {
        return Err("invalid evidence field".into());
    }
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

struct EvidenceReader<'a> {
    remaining: &'a [u8],
}

impl<'a> EvidenceReader<'a> {
    fn new(bytes: &'a [u8], prefix: &[u8]) -> Result<Self, String> {
        if bytes.len() > 131_072 || !bytes.starts_with(prefix) {
            return Err("invalid evidence prefix or size".into());
        }
        Ok(Self {
            remaining: &bytes[prefix.len()..],
        })
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.remaining.len() {
            return Err("truncated evidence".into());
        }
        let (head, tail) = self.remaining.split_at(n);
        self.remaining = tail;
        Ok(head)
    }
    fn reference(&mut self) -> Result<EvidenceReference, String> {
        let review_id = i64::from_be_bytes(self.take(8)?.try_into().map_err(|_| "invalid review")?);
        let replay_id = i64::from_be_bytes(self.take(8)?.try_into().map_err(|_| "invalid replay")?);
        let mut hash = || -> Result<String, String> {
            Ok(std::str::from_utf8(self.take(64)?)
                .map_err(|_| "invalid hash")?
                .to_owned())
        };
        let reference = EvidenceReference {
            review_id,
            replay_id,
            candidate_sha256: hash()?,
            fixture_sha256: hash()?,
            review_ledger_hash: hash()?,
        };
        evidence_reference_bytes(&reference)?;
        Ok(reference)
    }
    fn field(&mut self) -> Result<String, String> {
        let size = u32::from_be_bytes(self.take(4)?.try_into().map_err(|_| "invalid field size")?)
            as usize;
        if size == 0 || size > 16_384 {
            return Err("invalid evidence field size".into());
        }
        let value = std::str::from_utf8(self.take(size)?)
            .map_err(|_| "invalid evidence UTF-8")?
            .to_owned();
        if value.trim().is_empty() {
            return Err("blank evidence field".into());
        }
        Ok(value)
    }
    fn timestamp(&mut self) -> Result<u64, String> {
        let value = u64::from_be_bytes(self.take(8)?.try_into().map_err(|_| "invalid timestamp")?);
        if value == 0 || !self.remaining.is_empty() {
            return Err("invalid evidence timestamp or trailing bytes".into());
        }
        Ok(value)
    }
}

impl SyntheticTraceEvidence {
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = EvidenceReader::new(bytes, b"sese-synthetic-trace-attestation-v1\0")?;
        let value = Self {
            reference: reader.reference()?,
            prompt: reader.field()?,
            response: reader.field()?,
            source: reader.field()?,
            method: reader.field()?,
            captured_unix_ns: reader.timestamp()?,
        };
        if value.canonical_bytes()? != bytes {
            return Err("noncanonical trace".into());
        }
        Ok(value)
    }
    /// Encodes asserted synthetic evidence, without verifying its database references or source.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        if self.captured_unix_ns == 0 {
            return Err("invalid capture time".into());
        }
        let mut out = b"sese-synthetic-trace-attestation-v1\0".to_vec();
        out.extend_from_slice(&evidence_reference_bytes(&self.reference)?);
        for value in [&self.prompt, &self.response, &self.source, &self.method] {
            append_evidence_field(&mut out, value)?;
        }
        out.extend_from_slice(&self.captured_unix_ns.to_be_bytes());
        Ok(out)
    }
}

impl SyntheticOutcomeEvidence {
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = EvidenceReader::new(bytes, b"sese-synthetic-outcome-attestation-v1\0")?;
        let reference = reader.reference()?;
        let outcome = match reader.take(1)? {
            [1] => SyntheticOutcome::Validated,
            [2] => SyntheticOutcome::Falsified,
            _ => return Err("invalid outcome".into()),
        };
        let value = Self {
            reference,
            outcome,
            source: reader.field()?,
            method: reader.field()?,
            evidence_reference: reader.field()?,
            observed_unix_ns: reader.timestamp()?,
        };
        if value.canonical_bytes()? != bytes {
            return Err("noncanonical outcome".into());
        }
        Ok(value)
    }
    /// Encodes an asserted synthetic outcome; it does not validate the observation or its time.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        if self.observed_unix_ns == 0 {
            return Err("invalid observation time".into());
        }
        let mut out = b"sese-synthetic-outcome-attestation-v1\0".to_vec();
        out.extend_from_slice(&evidence_reference_bytes(&self.reference)?);
        out.push(match self.outcome {
            SyntheticOutcome::Validated => 1,
            SyntheticOutcome::Falsified => 2,
        });
        for value in [&self.source, &self.method, &self.evidence_reference] {
            append_evidence_field(&mut out, value)?;
        }
        out.extend_from_slice(&self.observed_unix_ns.to_be_bytes());
        Ok(out)
    }
}

impl SyntheticPreferenceEvidence {
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = EvidenceReader::new(bytes, b"sese-synthetic-preference-attestation-v1\0")?;
        let reference = reader.reference()?;
        let trace_id =
            i64::from_be_bytes(reader.take(8)?.try_into().map_err(|_| "invalid trace ID")?);
        let trace_ledger_hash = std::str::from_utf8(reader.take(64)?)
            .map_err(|_| "invalid trace hash")?
            .to_owned();
        let value = Self {
            reference,
            trace_id,
            trace_ledger_hash,
            prompt: reader.field()?,
            preferred: reader.field()?,
            rejected: reader.field()?,
            source: reader.field()?,
            comparability_basis: reader.field()?,
            reviewed_unix_ns: reader.timestamp()?,
        };
        if value.canonical_bytes()? != bytes {
            return Err("noncanonical preference".into());
        }
        Ok(value)
    }
    /// Encode a reviewer assertion; callers must independently check the referenced trace.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        if self.trace_id <= 0
            || self.reviewed_unix_ns == 0
            || self.preferred == self.rejected
            || self.trace_ledger_hash.len() != 64
            || !self
                .trace_ledger_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid synthetic preference".into());
        }
        let mut out = b"sese-synthetic-preference-attestation-v1\0".to_vec();
        out.extend_from_slice(&evidence_reference_bytes(&self.reference)?);
        out.extend_from_slice(&self.trace_id.to_be_bytes());
        out.extend_from_slice(self.trace_ledger_hash.as_bytes());
        for value in [
            &self.prompt,
            &self.preferred,
            &self.rejected,
            &self.source,
            &self.comparability_basis,
        ] {
            append_evidence_field(&mut out, value)?;
        }
        out.extend_from_slice(&self.reviewed_unix_ns.to_be_bytes());
        Ok(out)
    }
}

/// Canonical message for a single role, authority and exact payload. No key material is handled.
pub fn attestation_message(
    id: &str,
    role: AuthorityRole,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    if id.trim().is_empty() || id.len() > 256 || payload.is_empty() || payload.len() > 131_072 {
        return Err("invalid attestation input".into());
    }
    let mut message = b"sese-evidence-attestation-v1\0".to_vec();
    for field in [id.as_bytes(), role.label().as_bytes()] {
        message.extend_from_slice(&(field.len() as u32).to_be_bytes());
        message.extend_from_slice(field);
    }
    message.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    message.extend_from_slice(&Sha256::digest(payload));
    Ok(message)
}

/// Checks a strict Ed25519 signature; it does not establish authority or truth.
pub fn verify_ed25519(public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    !key.is_weak()
        && key
            .verify_strict(message, &Signature::from_bytes(signature))
            .is_ok()
}

/// Checks a caller-provided authority's role and revocation flag before signature validation.
pub fn verify_attestation(
    authority: &Authority,
    expected_role: AuthorityRole,
    payload: &[u8],
    signature: &[u8; 64],
) -> bool {
    if authority.revoked || authority.role != expected_role {
        return false;
    }
    let Ok(message) = attestation_message(&authority.id, expected_role, payload) else {
        return false;
    };
    verify_ed25519(&authority.public_key, &message, signature)
}

/// Verify a typed synthetic trace claim with a trace-source authority.
pub fn verify_synthetic_trace(
    authority: &Authority,
    evidence: &SyntheticTraceEvidence,
    signature: &[u8; 64],
) -> bool {
    evidence.canonical_bytes().ok().is_some_and(|payload| {
        verify_attestation(authority, AuthorityRole::TraceSource, &payload, signature)
    })
}

/// Verify a typed synthetic outcome claim with an outcome-observer authority.
pub fn verify_synthetic_outcome(
    authority: &Authority,
    evidence: &SyntheticOutcomeEvidence,
    signature: &[u8; 64],
) -> bool {
    evidence.canonical_bytes().ok().is_some_and(|payload| {
        verify_attestation(
            authority,
            AuthorityRole::OutcomeObserver,
            &payload,
            signature,
        )
    })
}

/// Verify a typed synthetic preference claim with a preference-reviewer authority.
pub fn verify_synthetic_preference(
    authority: &Authority,
    evidence: &SyntheticPreferenceEvidence,
    signature: &[u8; 64],
) -> bool {
    evidence.canonical_bytes().ok().is_some_and(|payload| {
        verify_attestation(
            authority,
            AuthorityRole::PreferenceReviewer,
            &payload,
            signature,
        )
    })
}
