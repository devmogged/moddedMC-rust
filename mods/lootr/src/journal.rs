//! Reference protocol for durable Lootr inventory transfers.
//!
//! This is NOT wired into the current native menu callback. The host exposes
//! only the proposed container slots, not the before/after player inventory,
//! cursor, or staged effects. A mod cannot guarantee cross-file conservation
//! with that interface. In particular, do not install this journal in a mod's
//! global config directory or infer player changes from container deltas.
//!
//! A future host adapter must serialize *all* operations for the affected
//! save/participants (including autosave, disconnect, login, refresh and
//! recovery), provide durable revisioned snapshots, and run recovery before
//! exposing either participant. Its player snapshot must include cursor escrow
//! and all inventory/equipment state affected by the trial click. World-drop
//! effects must be rejected or handled as additional durable participants.
//! The host must not use the current process-local autosave counter as a
//! persistent revision. Checkpoint the exact pre-click snapshots under the
//! fence before preparing; no proposed post-click state may reach disk
//! before the commit decision. No later transactions may start while a
//! journal exists. The adapter must validate the canonical payloads with
//! the real host codecs and ensure that the trial conserves items.
//!
//! The adapter must fence old queued saves, use fsync + atomic replacement +
//! directory durability (or equivalent), assign non-reused transaction IDs
//! in a stable save namespace, and store the journal separately from the
//! existing 8 MiB saved-data value. Failures after an uncertain write require
//! recovery, not a normal rejected click. This module supplies the bounded
//! record format, decision protocol and recovery algorithm, not that adapter.

use fastnbt::ByteArray;
use serde::{Deserialize, Serialize};

const VERSION: i32 = 1;
const PREPARED: i32 = 1;
const COMMITTED: i32 = 2;
const MAX_SNAPSHOT: usize = 8 << 20;
const MAX_JOURNAL: usize = 4 * MAX_SNAPSHOT + 4096;
const MAGIC: &[u8] = b"LOOTR-JOURNAL-1\0";

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x82f6_3b78
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = crc_table();

/// CRC-32C detects accidental torn/corrupted journal bytes, not hostile edits.
fn checksum(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| {
        (crc >> 8) ^ CRC_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize]
    })
}

/// The exact participant identity, including a stable save identity, rather
/// than a dimension ID or a transient runtime world UUID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub save: String,
    pub player: String,
    pub shard: i32,
}

impl Target {
    fn valid(&self) -> bool {
        crate::state::valid_id(&self.save)
            && crate::state::valid_id(&self.player)
            && (0..crate::shards::COUNT as i32).contains(&self.shard)
    }
}

/// A canonical, exact serialized snapshot plus an on-disk revision.
/// Both fields must be atomically durable in the participant's own file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    revision: i64,
    data: ByteArray,
}

impl Snapshot {
    pub fn new(revision: i64, data: &[u8]) -> Option<Self> {
        if revision < 0 || data.is_empty() || data.len() > MAX_SNAPSHOT {
            return None;
        }
        Some(Self {
            revision,
            data: ByteArray::new(data.iter().map(|&byte| byte as i8).collect()),
        })
    }

    pub fn revision(&self) -> i64 {
        self.revision
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.data.iter().map(|&byte| byte as u8).collect()
    }

    fn valid(&self) -> bool {
        self.revision >= 0 && !self.data.is_empty() && self.data.len() <= MAX_SNAPSHOT
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub before: Snapshot,
    pub after: Snapshot,
}

impl Transition {
    fn valid(&self) -> bool {
        self.before.valid()
            && self.after.valid()
            && self.before.revision.checked_add(1) == Some(self.after.revision)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    version: i32,
    phase: i32,
    transaction: i64,
    target: Target,
    player: Transition,
    shard: Transition,
}

impl Journal {
    /// Caller supplies host-validated canonical snapshots. Even an unchanged
    /// participant advances its revision, so an ABA payload cannot be replayed
    /// against a newer transaction.
    pub fn prepare(
        transaction: i64,
        target: Target,
        player: Transition,
        shard: Transition,
    ) -> Option<Self> {
        let journal = Self {
            version: VERSION,
            phase: PREPARED,
            transaction,
            target,
            player,
            shard,
        };
        journal.valid().then_some(journal)
    }

    fn valid(&self) -> bool {
        self.version == VERSION
            && matches!(self.phase, PREPARED | COMMITTED)
            && self.transaction > 0
            && self.target.valid()
            && self.player.valid()
            && self.shard.valid()
    }

    pub fn encode(&self) -> Option<Vec<u8>> {
        if !self.valid() {
            return None;
        }
        let payload = fastnbt::to_bytes(self).ok()?;
        let mut bytes = Vec::with_capacity(MAGIC.len() + payload.len() + 4);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&checksum(&payload).to_be_bytes());
        (bytes.len() <= MAX_JOURNAL).then_some(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_JOURNAL {
            return None;
        }
        let bytes = bytes.strip_prefix(MAGIC)?;
        let (payload, crc) = bytes.split_at_checked(bytes.len().checked_sub(4)?)?;
        if checksum(payload) != u32::from_be_bytes(crc.try_into().ok()?) {
            return None;
        }
        let journal: Self = fastnbt::from_bytes(payload).ok()?;
        // A canonical round trip also rejects trailing data and unknown fields.
        (journal.valid() && fastnbt::to_bytes(&journal).ok()? == payload).then_some(journal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Unreadable or corrupt evidence: do not infer an empty journal.
    Corrupt,
    /// Scope, revision, or exact payload mismatch: quarantine and investigate.
    Conflict,
    /// I/O or durability failure. A write may have succeeded before the error;
    /// caller must fence participants and retry recovery, not repeat a click.
    Storage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    Aborted,
    Committed,
}

/// Contract for an exclusive, durable host adapter. Every compare/write must
/// be atomic with respect to readers and writers, including queued autosaves.
/// Read errors are distinct from absence. Errors can represent lost write
/// acknowledgements. Caller retains its exclusive fence across all calls.
pub trait Store {
    fn save_id(&self) -> &str;
    fn journal(&self) -> Result<Option<Vec<u8>>, Error>;
    fn compare_journal(
        &mut self,
        expected: Option<&[u8]>,
        replacement: Option<&[u8]>,
    ) -> Result<(), Error>;
    fn player(&self, target: &Target) -> Result<Snapshot, Error>;
    fn shard(&self, target: &Target) -> Result<Snapshot, Error>;
    fn compare_player(
        &mut self,
        target: &Target,
        expected: &Snapshot,
        replacement: &Snapshot,
    ) -> Result<(), Error>;
    fn compare_shard(
        &mut self,
        target: &Target,
        expected: &Snapshot,
        replacement: &Snapshot,
    ) -> Result<(), Error>;
}

/// Begin a new transaction only after prior recovery has completed.
/// The durable decision is the linearization point. After it, roll forward
/// even if the client was disconnected before receiving acknowledgement.
pub fn execute(store: &mut impl Store, mut journal: Journal) -> Result<Outcome, Error> {
    if !journal.valid() || journal.phase != PREPARED {
        return Err(Error::Corrupt);
    }
    if store.save_id() != journal.target.save
        || store.journal()?.is_some()
        || store.player(&journal.target)? != journal.player.before
        || store.shard(&journal.target)? != journal.shard.before
    {
        return Err(Error::Conflict);
    }
    let prepared = journal.encode().ok_or(Error::Corrupt)?;
    store.compare_journal(None, Some(&prepared))?;
    journal.phase = COMMITTED;
    let committed = journal.encode().ok_or(Error::Corrupt)?;
    store.compare_journal(Some(&prepared), Some(&committed))?;
    match recover(store)? {
        Outcome::Committed => Ok(Outcome::Committed),
        // With the exclusive adapter fence this cannot legitimately occur.
        // Do not acknowledge a transaction whose decision vanished.
        Outcome::Clean | Outcome::Aborted => Err(Error::Conflict),
    }
}

/// Recover a single pending decision. Inspect both sides before modifying
/// either. Exact revision + payload matching rejects external mutation,
/// stale journal replay and same-revision corruption. Retrying after any
/// partial projection is safe; never apply a numerical item delta twice.
pub fn recover(store: &mut impl Store) -> Result<Outcome, Error> {
    let Some(bytes) = store.journal()? else {
        return Ok(Outcome::Clean);
    };
    let journal = Journal::decode(&bytes).ok_or(Error::Corrupt)?;
    if store.save_id() != journal.target.save {
        return Err(Error::Conflict);
    }
    let player = store.player(&journal.target)?;
    let shard = store.shard(&journal.target)?;
    if journal.phase == PREPARED {
        // A prepared-only transaction never projected either participant.
        if player != journal.player.before || shard != journal.shard.before {
            return Err(Error::Conflict);
        }
        store.compare_journal(Some(&bytes), None)?;
        return Ok(Outcome::Aborted);
    }
    if (player != journal.player.before && player != journal.player.after)
        || (shard != journal.shard.before && shard != journal.shard.after)
    {
        return Err(Error::Conflict);
    }
    if player == journal.player.before {
        store.compare_player(&journal.target, &player, &journal.player.after)?;
    }
    if shard == journal.shard.before {
        store.compare_shard(&journal.target, &shard, &journal.shard.after)?;
    }
    // Do not clear the decision until the adapter confirms both exact durable
    // revisions. A lost acknowledgement leaves the decision replayable.
    if store.player(&journal.target)? != journal.player.after
        || store.shard(&journal.target)? != journal.shard.after
    {
        return Err(Error::Conflict);
    }
    store.compare_journal(Some(&bytes), None)?;
    Ok(Outcome::Committed)
}

/// Resolve the result of an uncertain execute() call under the same fence.
/// In particular, a lost acknowledgement of journal deletion leaves no
/// journal, but the exact after-snapshots prove that the decision committed.
/// Do not infer success from an absent journal alone. On startup without a
/// remembered request, use recover() and resynchronize clients from disk.
pub fn settle(store: &mut impl Store, request: &Journal) -> Result<Outcome, Error> {
    if !request.valid() || request.phase != PREPARED {
        return Err(Error::Corrupt);
    }
    if store.save_id() != request.target.save {
        return Err(Error::Conflict);
    }
    if let Some(bytes) = store.journal()? {
        let mut pending = Journal::decode(&bytes).ok_or(Error::Corrupt)?;
        pending.phase = PREPARED;
        if &pending != request {
            return Err(Error::Conflict);
        }
        return recover(store);
    }
    let player = store.player(&request.target)?;
    let shard = store.shard(&request.target)?;
    if player == request.player.before && shard == request.shard.before {
        Ok(Outcome::Aborted)
    } else if player == request.player.after && shard == request.shard.after {
        Ok(Outcome::Committed)
    } else {
        Err(Error::Conflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAVE: &str = "10000000000000000000000000000001";
    const OWNER: &str = "20000000000000000000000000000002";

    #[derive(Clone)]
    struct Memory {
        target: Target,
        player: Snapshot,
        shard: Snapshot,
        journal: Option<Vec<u8>>,
        writes: usize,
        fail_at: Option<usize>,
        fail_after: bool,
    }

    impl Memory {
        fn new(journal: &Journal) -> Self {
            Self {
                target: journal.target.clone(),
                player: journal.player.before.clone(),
                shard: journal.shard.before.clone(),
                journal: None,
                writes: 0,
                fail_at: None,
                fail_after: false,
            }
        }

        // Fail before a durable write or after it with a lost acknowledgement.
        fn write(&mut self, f: impl FnOnce(&mut Self)) -> Result<(), Error> {
            self.writes += 1;
            let failing = self.fail_at == Some(self.writes);
            if !failing || self.fail_after {
                f(self);
            }
            if failing { Err(Error::Storage) } else { Ok(()) }
        }

        fn verify_target(&self, target: &Target) -> Result<(), Error> {
            if target == &self.target {
                Ok(())
            } else {
                Err(Error::Conflict)
            }
        }
    }

    impl Store for Memory {
        fn save_id(&self) -> &str {
            &self.target.save
        }
        fn journal(&self) -> Result<Option<Vec<u8>>, Error> {
            Ok(self.journal.clone())
        }
        fn compare_journal(
            &mut self,
            expected: Option<&[u8]>,
            replacement: Option<&[u8]>,
        ) -> Result<(), Error> {
            if self.journal.as_deref() != expected {
                return Err(Error::Conflict);
            }
            self.write(|s| s.journal = replacement.map(<[u8]>::to_vec))
        }
        fn player(&self, target: &Target) -> Result<Snapshot, Error> {
            self.verify_target(target)?;
            Ok(self.player.clone())
        }
        fn shard(&self, target: &Target) -> Result<Snapshot, Error> {
            self.verify_target(target)?;
            Ok(self.shard.clone())
        }
        fn compare_player(
            &mut self,
            target: &Target,
            expected: &Snapshot,
            replacement: &Snapshot,
        ) -> Result<(), Error> {
            self.verify_target(target)?;
            if &self.player != expected {
                return Err(Error::Conflict);
            }
            self.write(|s| s.player = replacement.clone())
        }
        fn compare_shard(
            &mut self,
            target: &Target,
            expected: &Snapshot,
            replacement: &Snapshot,
        ) -> Result<(), Error> {
            self.verify_target(target)?;
            if &self.shard != expected {
                return Err(Error::Conflict);
            }
            self.write(|s| s.shard = replacement.clone())
        }
    }

    // Synthetic exact payloads: player inventory + durable cursor escrow,
    // and container contents. These are test data, not Minecraft NBT.
    fn transfer(deposit: bool) -> Journal {
        let (player_before, player_after, shard_before, shard_after) = if deposit {
            ([7, 0], [0, 0], [0], [7])
        } else {
            ([0, 0], [0, 4], [7], [3])
        };
        Journal::prepare(
            42,
            Target {
                save: SAVE.into(),
                player: OWNER.into(),
                shard: 0,
            },
            Transition {
                before: Snapshot::new(11, &player_before).unwrap(),
                after: Snapshot::new(12, &player_after).unwrap(),
            },
            Transition {
                before: Snapshot::new(21, &shard_before).unwrap(),
                after: Snapshot::new(22, &shard_after).unwrap(),
            },
        )
        .unwrap()
    }

    fn total(store: &Memory) -> u16 {
        store
            .player
            .bytes()
            .into_iter()
            .chain(store.shard.bytes())
            .map(u16::from)
            .sum()
    }

    #[test]
    fn every_write_failure_and_lost_ack_recovers_conservation() {
        for deposit in [false, true] {
            let journal = transfer(deposit);
            for fail_at in 1..=5 {
                for fail_after in [false, true] {
                    let mut disk = Memory::new(&journal);
                    disk.fail_at = Some(fail_at);
                    disk.fail_after = fail_after;
                    assert_eq!(execute(&mut disk, journal.clone()), Err(Error::Storage));
                    // Reconstruct from durable state, including the journal bytes.
                    let mut restarted = disk.clone();
                    restarted.fail_at = None;
                    let decision_durable = fail_at > 2 || (fail_at == 2 && fail_after);
                    let expected_outcome = if decision_durable {
                        Outcome::Committed
                    } else {
                        Outcome::Aborted
                    };
                    assert_eq!(settle(&mut restarted, &journal), Ok(expected_outcome));
                    assert_eq!(total(&restarted), 7);
                    assert_eq!(
                        restarted.player,
                        if decision_durable {
                            journal.player.after.clone()
                        } else {
                            journal.player.before.clone()
                        }
                    );
                    assert_eq!(
                        restarted.shard,
                        if decision_durable {
                            journal.shard.after.clone()
                        } else {
                            journal.shard.before.clone()
                        }
                    );
                    assert_eq!(recover(&mut restarted), Ok(Outcome::Clean));
                    assert_eq!(settle(&mut restarted, &journal), Ok(expected_outcome));
                    assert_eq!(total(&restarted), 7);
                }
            }
        }
    }

    #[test]
    fn repeated_recovery_failure_is_idempotent() {
        let journal = transfer(false);
        let mut disk = Memory::new(&journal);
        disk.fail_at = Some(3);
        assert_eq!(execute(&mut disk, journal.clone()), Err(Error::Storage));
        for _ in 0..2 {
            disk.fail_at = Some(disk.writes + 1);
            disk.fail_after = true;
            assert_eq!(recover(&mut disk), Err(Error::Storage));
        }
        disk.fail_at = None;
        assert_eq!(recover(&mut disk), Ok(Outcome::Committed));
        assert_eq!(disk.player, journal.player.after);
        assert_eq!(disk.shard, journal.shard.after);
        assert_eq!(total(&disk), 7);
    }

    #[test]
    fn conflicts_do_not_modify_either_participant_or_discard_evidence() {
        let mut journal = transfer(true);
        journal.phase = COMMITTED;
        for stale in [
            Snapshot::new(13, &[7, 0]).unwrap(),
            Snapshot::new(11, &[6, 0]).unwrap(),
        ] {
            let mut disk = Memory::new(&journal);
            disk.journal = journal.encode();
            disk.player = stale;
            let before = disk.clone();
            assert_eq!(recover(&mut disk), Err(Error::Conflict));
            assert_eq!(disk.player, before.player);
            assert_eq!(disk.shard, before.shard);
            assert_eq!(disk.journal, before.journal);
            assert_eq!(disk.writes, 0);
        }
        let mut disk = Memory::new(&journal);
        disk.journal = journal.encode();
        disk.shard = Snapshot::new(23, &[0]).unwrap();
        assert_eq!(recover(&mut disk), Err(Error::Conflict));
        assert_eq!(disk.player, journal.player.before);
        assert_eq!(disk.writes, 0);
    }

    #[test]
    fn prepared_only_cannot_have_projected_state_and_stale_decision_cannot_replay() {
        let journal = transfer(false);
        let mut disk = Memory::new(&journal);
        disk.journal = journal.encode();
        disk.player = journal.player.after.clone();
        assert_eq!(recover(&mut disk), Err(Error::Conflict));
        assert!(disk.journal.is_some());
        assert_eq!(disk.writes, 0);

        let mut disk = Memory::new(&journal);
        assert_eq!(execute(&mut disk, journal.clone()), Ok(Outcome::Committed));
        assert_eq!(execute(&mut disk, journal), Err(Error::Conflict));
        assert_eq!(total(&disk), 7);
    }

    #[test]
    fn uncertain_request_cannot_settle_another_transaction() {
        let journal = transfer(false);
        let mut other = journal.clone();
        other.transaction += 1;
        let mut disk = Memory::new(&journal);
        disk.journal = other.encode();
        assert_eq!(settle(&mut disk, &journal), Err(Error::Conflict));
        assert_eq!(disk.writes, 0);
        disk.journal = None;
        disk.player = journal.player.after.clone();
        assert_eq!(settle(&mut disk, &journal), Err(Error::Conflict));
        assert_eq!(disk.writes, 0);
    }

    #[test]
    fn malformed_or_wrong_save_journal_fails_closed() {
        let journal = transfer(false);
        let encoded = journal.encode().unwrap();
        assert_eq!(Journal::decode(&encoded), Some(journal.clone()));
        assert_eq!(checksum(b"123456789"), 0xe306_9283);
        for index in [0, MAGIC.len(), encoded.len() / 2, encoded.len() - 1] {
            let mut corrupt = encoded.clone();
            corrupt[index] ^= 1;
            assert!(Journal::decode(&corrupt).is_none());
        }
        assert!(Journal::decode(&encoded[..encoded.len() - 1]).is_none());
        let mut disk = Memory::new(&journal);
        disk.journal = Some(vec![1, 2, 3]);
        assert_eq!(recover(&mut disk), Err(Error::Corrupt));
        disk.journal = Some(encoded);
        disk.target.save = OWNER.into();
        assert_eq!(recover(&mut disk), Err(Error::Conflict));
        assert_eq!(disk.writes, 0);
        let mut malformed = journal.clone();
        malformed.phase = 99;
        let payload = fastnbt::to_bytes(&malformed).unwrap();
        let mut invalid = MAGIC.to_vec();
        invalid.extend_from_slice(&payload);
        invalid.extend_from_slice(&checksum(&payload).to_be_bytes());
        assert!(Journal::decode(&invalid).is_none());
        malformed = journal;
        malformed.player.after.revision = malformed.player.before.revision;
        assert!(malformed.encode().is_none());
    }
}
