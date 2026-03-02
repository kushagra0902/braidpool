use crate::bead::Bead;
use crate::braid::{AddBeadStatus, Braid};
use crate::committed_metadata::CommittedMetadata;
use crate::db::audit_db_handlers::AuditDBHandler;
use crate::uncommitted_metadata::UnCommittedMetadata;
use crate::{TimeVec, TxIdVec};
use bitcoin::consensus::serialize;
use bitcoin::hashes::sha256d;
use bitcoin::{BlockHash, BlockHeader, CompactTarget, TxMerkleNode};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

pub const UPSTREAM_EXTRANONCE1_BYTES: usize = 4;
pub const MINER_PREFIX_BYTES: usize = 2;
pub const COMMITMENT_BYTES: usize = 5;
pub const MINER_ROLL_BYTES: usize = 1;
pub const TOTAL_EXTRANONCE1_BYTES: usize =
    UPSTREAM_EXTRANONCE1_BYTES + MINER_PREFIX_BYTES + COMMITMENT_BYTES;

pub type ShareId = BlockHash;

/// Create a genesis bead for audit mode with empty parents
fn create_genesis_bead_for_audit() -> Result<Bead, String> {
    let genesis_time = bitcoin::absolute::Time::from_consensus(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("System time error: {}", e))?
            .as_secs() as u32,
    )
    .map_err(|_| "Invalid genesis timestamp")?;

    // Create genesis block header
    let block_header = BlockHeader {
        version: bitcoin::block::Version::ONE,
        prev_blockhash: BlockHash::from_byte_array([0u8; 32]),
        merkle_root: TxMerkleNode::from_byte_array([0u8; 32]),
        time: bitcoin::BlockTime::from_u32(genesis_time.to_consensus_u32()),
        bits: CompactTarget::from_consensus(0x1d00ffff),
        nonce: 0,
    };

    let public_key = "020202020202020202020202020202020202020202020202020202020202020202"
        .parse::<bitcoin::PublicKey>()
        .unwrap();

    // Create committed metadata with no parents
    let committed_metadata = CommittedMetadata {
        transaction_ids: TxIdVec(Vec::new()),
        parents: std::collections::HashSet::new(),
        parent_bead_timestamps: TimeVec(Vec::new()),
        payout_address: "bc1qgdjqv0av3q56jvd82tkdjpy7gdp9ut8tlqmgrpmv24sq90ecnvqqjwvw97"
            .to_string(),
        start_timestamp: genesis_time,
        comm_pub_key: public_key,
        min_target: CompactTarget::from_consensus(0x1d00ffff),
        weak_target: CompactTarget::from_consensus(0x1d00ffff),
        miner_ip: "system".to_string(),
    };

    let default_sig_hex = "3046022100839c1fbc5304de944f697c9f4b1d01d1faeba32d751c0f7acb21ac8a0f436a72022100e89bd46bb3a5a62adc679f659b7ce876d83ee297c7a5587b2011c4fcc72eab45";
    let default_sig_bytes =
        hex::decode(default_sig_hex).map_err(|e| format!("Invalid signature hex: {}", e))?;
    let default_sig = bitcoin::ecdsa::Signature {
        signature: bitcoin::secp256k1::ecdsa::Signature::from_der(&default_sig_bytes)
            .map_err(|e| format!("Invalid signature DER: {}", e))?,
        sighash_type: bitcoin::sighash::EcdsaSighashType::All,
    };

    // Create uncommitted metadata
    let uncommitted_metadata = UnCommittedMetadata {
        extra_nonce_1: 0,
        extra_nonce_2: 0,
        broadcast_timestamp: genesis_time,
        signature: default_sig,
    };

    let genesis_bead = Bead {
        block_header,
        committed_metadata,
        uncommitted_metadata,
    };

    info!(
        block_hash = %genesis_bead.block_header.block_hash(),
        composite_hash = %compute_audit_bead_hash(&genesis_bead),
        "Genesis bead created"
    );

    Ok(genesis_bead)
}

/// Compute composite hash for audit mode, hash(block_header || committed_metadata)
/// This is only used in audit mode where we cannot use OP_RETURN commitments, which
/// we will use as an extranonce commitment.
pub fn compute_audit_bead_hash(bead: &Bead) -> BlockHash {
    let header_bytes = serialize(&bead.block_header);
    let metadata_bytes = serialize(&bead.committed_metadata);
    let mut combined = Vec::with_capacity(header_bytes.len() + metadata_bytes.len());
    combined.extend_from_slice(&header_bytes);
    combined.extend_from_slice(&metadata_bytes);
    BlockHash::from_byte_array(sha256d::Hash::hash(&combined).to_byte_array())
}

/// Defined rule for comparing two 32-byte hashes.
fn compare_hash(a: &BlockHash, b: &BlockHash) -> Ordering {
    let a_bytes = a.as_byte_array();
    let b_bytes = b.as_byte_array();

    // Compare byte-by-byte from left to right
    for i in 0..32 {
        if a_bytes[i] < b_bytes[i] {
            return Ordering::Less;
        } else if a_bytes[i] > b_bytes[i] {
            return Ordering::Greater;
        }
    }
    error!("A very rare event has occurred, the chances of this are fewer than the atoms in the observable universe.");
    Ordering::Equal
}

/// Computes a deterministic generation hash by concatenating all current DAG tips using compare_hash function.
/// This is used for calculating the current commitment by combining the composite hash of the current briad tips,
/// by doing this we are commiting to the entire set of tips in a variable interval. Now future beads will
/// point/contain this commitment.
pub fn compute_generation_hash(
    tips: &[(BlockHash, bitcoin::absolute::Time)],
) -> Result<BlockHash, &'static str> {
    if tips.is_empty() {
        return Err("Cannot compute generation hash, tips array is empty. Genesis bead must be loaded first.");
    }

    let mut consensus_tips: Vec<BlockHash> = tips.iter().map(|(hash, _)| *hash).collect();
    consensus_tips.sort_unstable_by(compare_hash);

    // Concatenate all the sorted composite hashes together
    let mut combined_bytes = Vec::with_capacity(consensus_tips.len() * 32);
    for tip_hash in consensus_tips {
        combined_bytes.extend_from_slice(tip_hash.as_byte_array());
    }

    let generation_hash = sha256d::Hash::hash(&combined_bytes);

    Ok(BlockHash::from_byte_array(generation_hash.to_byte_array()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditCommitment {
    pub commitment_bytes: [u8; COMMITMENT_BYTES],
    pub parent_bead_hash: Option<BlockHash>,
}

impl Default for AuditCommitment {
    fn default() -> Self {
        Self {
            commitment_bytes: [0u8; COMMITMENT_BYTES],
            parent_bead_hash: None,
        }
    }
}

impl AuditCommitment {
    pub fn genesis() -> Self {
        Self::default()
    }

    pub fn from_audit_bead(bead: &Bead) -> Self {
        let composite_hash = compute_audit_bead_hash(bead);
        Self::from_bead_hash(composite_hash)
    }

    pub fn from_hash_prefix(hash_prefix: &[u8]) -> Self {
        let mut commitment_bytes = [0u8; COMMITMENT_BYTES];
        let len = hash_prefix.len().min(COMMITMENT_BYTES);
        commitment_bytes[..len].copy_from_slice(&hash_prefix[..len]);

        Self {
            commitment_bytes,
            parent_bead_hash: None,
        }
    }

    pub fn from_bead_hash(bead_hash: BlockHash) -> Self {
        let hash_bytes = bead_hash.to_byte_array();
        let mut commitment_bytes = [0u8; COMMITMENT_BYTES];
        commitment_bytes.copy_from_slice(&hash_bytes[..COMMITMENT_BYTES]);

        Self {
            commitment_bytes,
            parent_bead_hash: Some(bead_hash),
        }
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.commitment_bytes)
    }

    pub fn verify_in_extranonce1(&self, extranonce1_bytes: &[u8], miner_prefix: &[u8]) -> bool {
        if extranonce1_bytes.len() != TOTAL_EXTRANONCE1_BYTES {
            return false;
        }
        if &extranonce1_bytes
            [UPSTREAM_EXTRANONCE1_BYTES..UPSTREAM_EXTRANONCE1_BYTES + MINER_PREFIX_BYTES]
            != miner_prefix
        {
            return false;
        }
        let commitment_start = UPSTREAM_EXTRANONCE1_BYTES + MINER_PREFIX_BYTES;
        let commitment_end = commitment_start + COMMITMENT_BYTES;
        &extranonce1_bytes[commitment_start..commitment_end] == &self.commitment_bytes
    }

    pub fn extract_miner_prefix_from_ext1(extranonce1_bytes: &[u8]) -> Option<Vec<u8>> {
        if extranonce1_bytes.len() < UPSTREAM_EXTRANONCE1_BYTES + MINER_PREFIX_BYTES {
            return None;
        }
        Some(
            extranonce1_bytes
                [UPSTREAM_EXTRANONCE1_BYTES..UPSTREAM_EXTRANONCE1_BYTES + MINER_PREFIX_BYTES]
                .to_vec(),
        )
    }
}

/// Per miner audit state tracking commitment chain
#[derive(Debug, Clone)]
pub struct MinerAuditState {
    pub current_commitment: AuditCommitment,
    pub miner_prefix: Vec<u8>,
    pub commitment_pending: bool,
    pub previous_commitment: Option<AuditCommitment>,
}

impl MinerAuditState {
    pub fn new(miner_prefix: Vec<u8>) -> Self {
        Self {
            current_commitment: AuditCommitment::genesis(),
            miner_prefix,
            commitment_pending: false,
            previous_commitment: None,
        }
    }

    pub fn update_commitment_audit(&mut self, bead: &Bead) {
        self.previous_commitment = Some(self.current_commitment.clone());
        let composite_hash = compute_audit_bead_hash(bead);
        let new_commitment = AuditCommitment::from_bead_hash(composite_hash);
        info!(
            miner_prefix = %hex::encode(&self.miner_prefix),
            old_commitment = %self.current_commitment.to_hex(),
            new_commitment = %new_commitment.to_hex(),
            block_hash = %bead.block_header.block_hash(),
            composite_hash = %composite_hash,
            "Updating miner audit commitment"
        );
        self.current_commitment = new_commitment;
        self.commitment_pending = true;
    }

    pub fn verify_share(
        &self,
        extranonce1_bytes: &[u8],
        extranonce2_hex: &str,
    ) -> AuditVerificationResult {
        if extranonce1_bytes.len() != TOTAL_EXTRANONCE1_BYTES {
            return AuditVerificationResult::Invalid {
                reason: format!(
                    "Wrong extranonce1 length: expected {} bytes, got {}",
                    TOTAL_EXTRANONCE1_BYTES,
                    extranonce1_bytes.len()
                ),
            };
        }
        if extranonce2_hex.len() != MINER_ROLL_BYTES * 2 {
            return AuditVerificationResult::Invalid {
                reason: format!(
                    "Wrong extranonce2 length: expected {} hex chars, got {}",
                    MINER_ROLL_BYTES * 2,
                    extranonce2_hex.len()
                ),
            };
        }
        if let Some(prefix) = AuditCommitment::extract_miner_prefix_from_ext1(extranonce1_bytes) {
            if prefix != self.miner_prefix {
                return AuditVerificationResult::Invalid {
                    reason: format!(
                        "Miner prefix mismatch: expected {}, got {}",
                        hex::encode(&self.miner_prefix),
                        hex::encode(&prefix)
                    ),
                };
            }
        } else {
            return AuditVerificationResult::Invalid {
                reason: "Could not extract miner prefix from extranonce1".to_string(),
            };
        }
        if self
            .current_commitment
            .verify_in_extranonce1(extranonce1_bytes, &self.miner_prefix)
        {
            let miner_roll = hex::decode(extranonce2_hex)
                .ok()
                .and_then(|bytes| bytes.first().copied());

            AuditVerificationResult::Valid {
                commitment: self.current_commitment.clone(),
                miner_roll,
            }
        } else {
            let commitment_start = UPSTREAM_EXTRANONCE1_BYTES + MINER_PREFIX_BYTES;
            let commitment_end = commitment_start + COMMITMENT_BYTES;
            let actual = hex::encode(&extranonce1_bytes[commitment_start..commitment_end]);

            AuditVerificationResult::Invalid {
                reason: format!(
                    "Commitment mismatch in extranonce1: expected {}, got {}",
                    self.current_commitment.to_hex(),
                    actual
                ),
            }
        }
    }

    pub fn verify_share_with_fallback(
        &self,
        extranonce1_bytes: &[u8],
        extranonce2_hex: &str,
        previous_commitment: Option<&AuditCommitment>,
    ) -> AuditVerificationResult {
        let result = self.verify_share(extranonce1_bytes, extranonce2_hex);
        if matches!(result, AuditVerificationResult::Invalid { .. }) {
            if let Some(prev_commitment) = previous_commitment {
                if prev_commitment.verify_in_extranonce1(extranonce1_bytes, &self.miner_prefix) {
                    warn!(
                        old = %prev_commitment.to_hex(),
                        current = %self.current_commitment.to_hex(),
                        "Share used previous commitment, accepting it as valid under conditions"
                    );
                    return AuditVerificationResult::Valid {
                        commitment: prev_commitment.clone(),
                        miner_roll: hex::decode(extranonce2_hex)
                            .ok()
                            .and_then(|bytes| bytes.first().copied()),
                    };
                }
            }
        }
        result
    }

    pub fn mark_commitment_sent(&mut self) {
        self.commitment_pending = false;
    }
}

#[derive(Debug, Clone)]
pub enum AuditVerificationResult {
    Valid {
        commitment: AuditCommitment,
        miner_roll: Option<u8>,
    },
    Invalid {
        reason: String,
    },
}

/// Links a share to audit verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub share_id: ShareId,
    pub timestamp: SystemTime,
    pub miner_ip: String,
    pub worker_name: String,
    pub job_id: String,
    pub extranonce2: String,
    pub nonce: String,
    pub ntime: String,
    pub audit_verified: bool,
    pub audit_commitment: Option<AuditCommitment>,
    pub upstream_accepted: Option<bool>,
    pub upstream_eligible: bool,
    pub bead_hash: BlockHash,
}

/// Wraps Braid and adds audit verification layer
pub struct AuditDAG {
    /// The underlying braid structure
    pub braid: Arc<RwLock<Braid>>,
    /// Audit records for shares
    records: HashMap<ShareId, AuditRecord>,
    /// Per miner audit state for commitment verification
    pub miner_states: HashMap<String, MinerAuditState>,
    /// Mapping from composite bead hash to share ID that created it
    bead_to_share: HashMap<BlockHash, ShareId>,
    /// Database handler
    db_handler: Option<Arc<AuditDBHandler>>,
    /// The set of parents that all shares in the current job must point to.
    pub active_parents: Vec<(BlockHash, BlockHash, bitcoin::absolute::Time)>,
    /// The accumulating set of valid shares mined during the current job.
    pub current_siblings: Vec<(BlockHash, BlockHash, bitcoin::absolute::Time)>,
}

impl AuditDAG {
    pub fn new(braid: Arc<RwLock<Braid>>) -> Self {
        Self {
            braid,
            records: HashMap::new(),
            miner_states: HashMap::new(),
            bead_to_share: HashMap::new(),
            db_handler: None,
            active_parents: Vec::new(),
            current_siblings: Vec::new(),
        }
    }

    pub async fn new_with_db(braid: Arc<RwLock<Braid>>) -> Result<Self, String> {
        let db_handler = AuditDBHandler::new()
            .await
            .map_err(|e| format!("Failed to initialize audit database: {}", e))?;

        Ok(Self {
            braid,
            records: HashMap::new(),
            miner_states: HashMap::new(),
            bead_to_share: HashMap::new(),
            db_handler: Some(Arc::new(db_handler)),
            active_parents: Vec::new(),
            current_siblings: Vec::new(),
        })
    }
    pub async fn load_from_db(&mut self) -> Result<Option<BlockHash>, String> {
        if self.db_handler.is_none() {
            info!("No database handler available, starting from genesis");
            return Ok(None);
        }

        let db_handler = self.db_handler.as_ref().unwrap();

        // Load the latest bead from the database to get its commitment
        match db_handler.get_tips().await {
            Ok(beads) => {
                if beads.is_empty() {
                    info!("No beads found in audit database, creating and persisting genesis bead");

                    // Create genesis bead in memory
                    let genesis_bead = create_genesis_bead_for_audit()?;
                    let genesis_block_hash = genesis_bead.block_header.block_hash();
                    let genesis_composite_hash = compute_audit_bead_hash(&genesis_bead);
                    let genesis_timestamp = genesis_bead.committed_metadata.start_timestamp;

                    info!(
                        genesis_block_hash = %genesis_block_hash,
                        genesis_composite_hash = %genesis_composite_hash,
                        "Created genesis bead for audit DAG"
                    );

                    // Persist genesis bead to database
                    if let Some(ref db_handler) = self.db_handler {
                        match db_handler
                            .insert_bead(
                                &genesis_bead,
                                genesis_composite_hash,
                                "system".to_string(),
                            )
                            .await
                        {
                            Ok(bead_id) => {
                                info!(
                                    bead_id = %bead_id,
                                    composite_hash = %genesis_composite_hash,
                                    "Genesis bead persisted to audit database"
                                );
                            }
                            Err(e) => {
                                error!(
                                    error = %e,
                                    "Failed to persist genesis bead to database"
                                );
                                return Err(format!("Genesis bead persistence failed: {}", e));
                            }
                        }
                    }

                    {
                        let mut braid = self.braid.write().await;
                        *braid = crate::braid::Braid::new(vec![genesis_bead.clone()]);
                    }

                    self.active_parents = vec![(
                        genesis_composite_hash,
                        genesis_block_hash,
                        genesis_timestamp,
                    )];

                    let generation_hash =
                        compute_generation_hash(&[(genesis_composite_hash, genesis_timestamp)])?;

                    info!(
                        generation_hash = %generation_hash,
                        "Initialized DAG with genesis bead"
                    );

                    Ok(Some(generation_hash))
                } else {
                    let sibling_count = beads.len();
                    {
                        // This allows us to start with the last mined bead tip retrieved from the database
                        // instead of the genesis, this creates a valid in-memory DAG which correctly
                        // refers to the past history (commitment), but if the database doesn't contain any
                        // entry of a bead, possibly running the node for the first time or after flushing
                        // the database then the in-memory bead will start from the genesis.
                        let mut braid = self.braid.write().await;
                        let only_beads: Vec<Bead> = beads.iter().map(|(b, _)| b.clone()).collect();
                        *braid = crate::braid::Braid::new(only_beads);
                    }

                    self.active_parents = beads
                        .into_iter()
                        .map(|(bead, hash)| {
                            (
                                hash,
                                bead.block_header.block_hash(),
                                bead.committed_metadata.start_timestamp,
                            )
                        })
                        .collect();

                    let generation_inputs: Vec<_> = self
                        .active_parents
                        .iter()
                        .map(|(comp, _, time)| (*comp, *time))
                        .collect();

                    let generation_hash = crate::audit::compute_generation_hash(&generation_inputs)
                        .expect("No generation hash generated");

                    info!(tip_count = sibling_count, "Restored DAG tips from database");

                    Ok(Some(generation_hash))
                }
            }
            Err(e) => {
                error!(
                    error = %e,
                    "Critical database failure while loading DAG tips. Aborting startup to prevent state corruption."
                );
                return Err(format!(
                    "Database read failure during state snapshotting: {}",
                    e
                ));
            }
        }
    }

    pub fn advance_generation(&mut self) -> Result<BlockHash, &'static str> {
        // Shift siblings to become the new parents only if we found shares
        if !self.current_siblings.is_empty() {
            self.active_parents = self.current_siblings.clone();
            self.current_siblings.clear();
        }

        // Compute the deterministic generation hash
        let generation_inputs: Vec<_> = self
            .active_parents
            .iter()
            .map(|(comp, _, time)| (*comp, *time))
            .collect();
        let generation_hash = compute_generation_hash(&generation_inputs)?;

        info!(
            parent_count = self.active_parents.len(),
            generation_hash = %generation_hash,
            "Advanced DAG generation on new upstream job"
        );

        Ok(generation_hash)
    }

    pub async fn add_and_record_bead(
        &mut self,
        mut record: AuditRecord,
        bead: Bead,
        extranonce1_bytes: &[u8],
    ) -> Result<(ShareId, bool), String> {
        let share_id = record.share_id.clone();
        let miner_ip = record.miner_ip.clone();
        let composite_hash = compute_audit_bead_hash(&bead);
        if let Some(miner_state) = self.miner_states.get_mut(&miner_ip) {
            let verification = miner_state.verify_share_with_fallback(
                extranonce1_bytes,
                &record.extranonce2,
                miner_state.previous_commitment.as_ref(),
            );
            match verification {
                AuditVerificationResult::Valid {
                    commitment,
                    miner_roll,
                } => {
                    record.audit_commitment = Some(commitment);
                    record.audit_verified = true;
                    debug!(
                        share_id = %share_id,
                        miner = %miner_ip,
                        miner_roll = ?miner_roll,
                        extranonce1 = %hex::encode(extranonce1_bytes),
                        extranonce2 = %record.extranonce2,
                        block_hash = %bead.block_header.block_hash(),
                        composite_hash = %composite_hash,
                        "Bead passed audit verification"
                    );
                }
                AuditVerificationResult::Invalid { reason } => {
                    record.audit_verified = false;
                    error!(
                        share_id = %share_id,
                        miner = %miner_ip,
                        reason = %reason,
                        extranonce1 = %hex::encode(extranonce1_bytes),
                        extranonce2 = %record.extranonce2,
                        "Bead failed audit verification thus rejecting."
                    );
                    return Err(format!("Audit verification failed: {}", reason));
                }
            }
        } else {
            warn!(miner = %miner_ip, "No miner state for audit verification");
            record.audit_verified = false;
            return Err("No miner state".to_string());
        }
        record.bead_hash = composite_hash;
        let mut bead_added = false;
        {
            let mut braid = self.braid.write().await;
            let status = braid.extend(&bead);
            match status {
                AddBeadStatus::BeadAdded => {
                    bead_added = true;

                    if let Some(ref db_handler) = self.db_handler {
                        match db_handler
                            .insert_bead(&bead, composite_hash, miner_ip.clone())
                            .await
                        {
                            Ok(_bead_id) => {}
                            Err(e) => {
                                error!(
                                    composite_hash = %composite_hash,
                                    error = %e,
                                    "Failed to persist bead to audit database"
                                );
                            }
                        }
                    }
                    let start_time = bead.committed_metadata.start_timestamp;
                    let block_hash = bead.block_header.block_hash();
                    self.current_siblings
                        .push((composite_hash, block_hash, start_time));

                    info!(
                        block_hash = %bead.block_header.block_hash(),
                        composite_hash = %composite_hash,
                        parents = ?bead.committed_metadata.parents,
                        sibling_count = self.current_siblings.len(),
                        miner = %miner_ip,
                        "Bead added to braid"
                    );
                }
                AddBeadStatus::DagAlreadyContainsBead => {
                    warn!(
                        composite_hash = %composite_hash,
                        "Bead already in DAG, treating as idempotent success"
                    );
                    // Do not return error as this process can be re-trigger by the same miner who got disconnect
                    // for a reason but and now retrying to submit that share again
                    bead_added = false;
                }
                AddBeadStatus::InvalidBead => {
                    error!(
                        composite_hash = %composite_hash,
                        "Invalid bead"
                    );
                    return Err("Invalid bead".to_string());
                }
                AddBeadStatus::ParentsNotYetReceived => {
                    warn!(
                        composite_hash = %composite_hash,
                        parents = ?bead.committed_metadata.parents,
                        "Parents not yet received, treating as orphan"
                    );
                }
            }
        }
        self.records.insert(share_id.clone(), record);
        self.bead_to_share.insert(composite_hash, share_id.clone());
        debug!(
            miner = %miner_ip,
            total_beads = %self.records.len(),
            "Bead recorded successfully"
        );
        Ok((share_id, bead_added))
    }

    pub fn mark_upstream_forwarded(&mut self, share_id: &ShareId) {
        if let Some(record) = self.records.get_mut(share_id) {
            record.upstream_eligible = true;
            info!(
                share_id = %share_id,
                "Bead marked as forwarded to upstream"
            );
        }
    }

    pub fn update_upstream_response(&mut self, share_id: &ShareId, accepted: bool) {
        if let Some(record) = self.records.get_mut(share_id) {
            record.upstream_accepted = Some(accepted);
            info!(
                share_id = %share_id,
                accepted = %accepted,
                "Updated upstream response"
            );
        }
    }

    pub fn register_miner(&mut self, miner_ip: String, prefix: Vec<u8>) {
        let prefix_hex = hex::encode(&prefix);
        let state = MinerAuditState::new(prefix);
        self.miner_states.insert(miner_ip.clone(), state);
        info!(
            miner = %miner_ip,
            prefix = %prefix_hex,
            "Registered miner for audit tracking"
        );
    }

    pub fn get_record(&self, share_id: &ShareId) -> Option<&AuditRecord> {
        self.records.get(share_id)
    }

    pub fn get_share_for_bead(&self, bead_hash: &BlockHash) -> Option<&ShareId> {
        self.bead_to_share.get(bead_hash)
    }

    pub fn get_miner_stats(&self, miner_ip: &str) -> MinerStats {
        let miner_records: Vec<&AuditRecord> = self
            .records
            .values()
            .filter(|r| r.miner_ip == miner_ip)
            .collect();

        let total_beads = miner_records.len();
        let upstream_eligible = miner_records.iter().filter(|r| r.upstream_eligible).count();
        let upstream_accepted = miner_records
            .iter()
            .filter(|r| r.upstream_accepted == Some(true))
            .count();
        let upstream_rejected = miner_records
            .iter()
            .filter(|r| r.upstream_accepted == Some(false))
            .count();
        let audit_verified = miner_records.iter().filter(|r| r.audit_verified).count();

        let miner_state = self.miner_states.get(miner_ip);

        MinerStats {
            total_beads,
            audit_verified_beads: audit_verified,
            audit_failed_beads: total_beads - audit_verified,
            upstream_eligible_beads: upstream_eligible,
            upstream_accepted_beads: upstream_accepted,
            upstream_rejected_beads: upstream_rejected,
            current_commitment: miner_state.map(|s| s.current_commitment.to_hex()),
            audit_rate: if total_beads > 0 {
                audit_verified as f64 / total_beads as f64
            } else {
                0.0
            },
            upstream_acceptance_rate: if upstream_eligible > 0 {
                upstream_accepted as f64 / upstream_eligible as f64
            } else {
                0.0
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct MinerStats {
    pub total_beads: usize,
    pub audit_verified_beads: usize,
    pub audit_failed_beads: usize,
    pub upstream_eligible_beads: usize,
    pub upstream_accepted_beads: usize,
    pub upstream_rejected_beads: usize,
    pub current_commitment: Option<String>,
    pub audit_rate: f64,
    pub upstream_acceptance_rate: f64,
}
