//! Bounded experimental `ReadIndexV1` codec and canonical-prefix verification.

use alloy_eip7928::bal::Bal;
use alloy_primitives::{Address, Bytes, U256};
use alloy_rlp::{Decodable, Encodable, Header};
use eyre::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) type Key = (Address, U256);
pub(crate) const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_INDEX_ENTRIES: usize = 100_000;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    alloy_rlp::RlpEncodable,
    alloy_rlp::RlpDecodable,
)]
pub(crate) struct Entry {
    pub address: Address,
    pub key: U256,
    pub first: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadIndex(pub Vec<Entry>);

impl ReadIndex {
    pub(crate) fn encode(&self) -> Bytes {
        let mut entries = vec![];
        self.0.encode(&mut entries);
        let mut bytes = vec![];
        Header { list: true, payload_length: 1 + entries.len() }.encode(&mut bytes);
        1_u8.encode(&mut bytes);
        bytes.extend(entries);
        bytes.into()
    }

    pub(crate) fn decode(bytes: &[u8], bal: &Bal, transaction_count: usize) -> Result<Self> {
        ensure!(bytes.len() <= MAX_INDEX_BYTES, "index byte limit exceeded");
        let mut input = bytes;
        let outer = Header::decode(&mut input)?;
        ensure!(
            outer.list && outer.payload_length == input.len(),
            "index outer list or length invalid"
        );
        ensure!(u8::decode(&mut input)? == 1, "unsupported index version");
        let list = Header::decode(&mut input)?;
        ensure!(
            list.list && list.payload_length == input.len(),
            "index entry list or length invalid"
        );
        let mut entries = vec![];
        while !input.is_empty() {
            ensure!(entries.len() < MAX_INDEX_ENTRIES, "index entry limit exceeded");
            let entry = Entry::decode(&mut input)?;
            ensure!(entry.first as usize <= transaction_count + 1, "index outside canonical range");
            if let Some(previous) = entries.last() {
                let previous: &Entry = previous;
                ensure!(
                    (previous.address, previous.key) < (entry.address, entry.key),
                    "index entries duplicate or unordered"
                );
            }
            entries.push(entry);
        }
        let actual: BTreeSet<_> = entries.iter().map(|entry| (entry.address, entry.key)).collect();
        let expected: BTreeSet<_> = bal
            .iter()
            .flat_map(|account| {
                account.storage_reads.iter().map(move |key| (account.address, *key))
            })
            .collect();
        ensure!(actual == expected, "index coverage differs from received BAL storage_reads");
        let index = Self(entries);
        ensure!(index.encode().as_ref() == bytes, "noncanonical index encoding");
        Ok(index)
    }

    pub(crate) fn from_trace(bal: &Bal, trace: &BTreeMap<Key, u32>, phantom_index: u32) -> Self {
        Self(
            bal.iter()
                .flat_map(|account| {
                    account.storage_reads.iter().map(move |key| Entry {
                        address: account.address,
                        key: *key,
                        first: trace
                            .get(&(account.address, *key))
                            .copied()
                            .unwrap_or(phantom_index),
                    })
                })
                .collect(),
        )
    }
}

#[derive(Debug)]
pub(crate) struct PrefixVerifier {
    declared: BTreeMap<Key, u32>,
    due: BTreeMap<u32, Vec<Key>>,
    observed: BTreeSet<Key>,
    next: u32,
}

impl PrefixVerifier {
    pub(crate) fn new(index: ReadIndex) -> Self {
        let mut due = BTreeMap::<u32, Vec<Key>>::new();
        let declared = index
            .0
            .into_iter()
            .map(|entry| {
                let key = (entry.address, entry.key);
                due.entry(entry.first).or_default().push(key);
                (key, entry.first)
            })
            .collect();
        Self { declared, due, observed: BTreeSet::new(), next: 0 }
    }

    pub(crate) fn complete(&mut self, index: u32, accesses: &[Key]) -> Result<()> {
        ensure!(index == self.next, "canonical completion out of order");
        for &key in accesses {
            if let Some(&claim) = self.declared.get(&key)
                && !self.observed.contains(&key)
            {
                ensure!(claim == index, "first access differs: observed {index}, declared {claim}");
                self.observed.insert(key);
            }
        }
        if let Some(due) = self.due.remove(&index) {
            ensure!(
                due.iter().all(|key| self.observed.contains(key)),
                "claimed access missing at canonical index {index}"
            );
        }
        self.next += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_eip7928::AccountChanges;

    fn example(first: u32) -> (ReadIndex, Bal, Key) {
        let key = (Address::repeat_byte(0x88), U256::ZERO);
        (
            ReadIndex(vec![Entry { address: key.0, key: key.1, first }]),
            Bal::from(vec![AccountChanges::new(key.0).with_storage_read(key.1)]),
            key,
        )
    }

    #[test]
    fn codec_bounds_order_coverage_and_version() -> Result<()> {
        let (index, bal, _) = example(1);
        assert_eq!(ReadIndex::decode(&index.encode(), &bal, 16)?, index);
        let mut duplicate = index.clone();
        duplicate.0.push(index.0[0].clone());
        assert_eq!(
            ReadIndex::decode(&duplicate.encode(), &bal, 16).unwrap_err().to_string(),
            "index entries duplicate or unordered"
        );
        assert_eq!(
            ReadIndex::decode(&ReadIndex(vec![]).encode(), &bal, 16).unwrap_err().to_string(),
            "index coverage differs from received BAL storage_reads"
        );
        let mut bad = index.encode().to_vec();
        let mut slice = bad.as_slice();
        Header::decode(&mut slice)?;
        let offset = bad.len() - slice.len();
        bad[offset] = 2;
        assert_eq!(
            ReadIndex::decode(&bad, &bal, 16).unwrap_err().to_string(),
            "unsupported index version"
        );
        assert_eq!(
            ReadIndex::decode(&example(18).0.encode(), &bal, 16).unwrap_err().to_string(),
            "index outside canonical range"
        );
        assert_eq!(
            ReadIndex::decode(&vec![0; MAX_INDEX_BYTES + 1], &bal, 16).unwrap_err().to_string(),
            "index byte limit exceeded"
        );
        let oversized = ReadIndex(
            (0..=MAX_INDEX_ENTRIES)
                .map(|i| Entry { address: Address::ZERO, key: U256::from(i), first: 1 })
                .collect(),
        );
        assert_eq!(
            ReadIndex::decode(&oversized.encode(), &bal, 16).unwrap_err().to_string(),
            "index entry limit exceeded"
        );
        Ok(())
    }

    #[test]
    fn prefix_verification_shared_early_late_and_missing() -> Result<()> {
        let (index, _, key) = example(1);
        let mut verifier = PrefixVerifier::new(index);
        verifier.complete(0, &[])?;
        verifier.complete(1, &[key])?;
        verifier.complete(2, &[key])?;
        assert_eq!(
            verifier.complete(4, &[]).unwrap_err().to_string(),
            "canonical completion out of order"
        );
        let mut missing = PrefixVerifier::new(example(1).0);
        missing.complete(0, &[])?;
        assert_eq!(
            missing.complete(1, &[]).unwrap_err().to_string(),
            "claimed access missing at canonical index 1"
        );
        let mut late = PrefixVerifier::new(example(2).0);
        late.complete(0, &[])?;
        assert_eq!(
            late.complete(1, &[key]).unwrap_err().to_string(),
            "first access differs: observed 1, declared 2"
        );
        Ok(())
    }

    #[test]
    fn every_canonical_first_access_and_false_claim_obeys_prefix_rule() -> Result<()> {
        for actual in 0..=17 {
            for declared in 0..=17 {
                let (index, bal, key) = example(declared);
                let index = ReadIndex::decode(&index.encode(), &bal, 16)?;
                let mut verifier = PrefixVerifier::new(index);
                let mut accepted = true;
                let accesses = [key];
                for boundary in 0..=17 {
                    if verifier
                        .complete(boundary, if boundary >= actual { &accesses } else { &[] })
                        .is_err()
                    {
                        accepted = false;
                        break;
                    }
                }
                assert_eq!(accepted, actual == declared);
            }
        }
        Ok(())
    }
}
