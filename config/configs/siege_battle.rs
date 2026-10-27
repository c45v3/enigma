// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiegeBattle {
    #[serde(rename = "episodeId")]
    pub episode_id: i32,
    #[serde(rename = "heroId")]
    pub hero_id: i32,
    pub id: i32,
    #[serde(rename = "instructionDesc")]
    pub instruction_desc: String,
    pub sort: i32,
    pub stage: i32,
    #[serde(rename = "unlockCondition")]
    pub unlock_condition: String,
}
use std::collections::HashMap;

pub struct SiegeBattleTable {
    records: Vec<SiegeBattle>,
    by_id: HashMap<i32, usize>,
}

impl SiegeBattleTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<SiegeBattle> = crate::load_rows(path)?;

        let mut by_id = HashMap::with_capacity(records.len());

        for (idx, record) in records.iter().enumerate() {
            by_id.insert(record.id, idx);
        }

        Ok(Self {
            records,
            by_id,
        })
    }

    #[inline]
    pub fn get(&self, id: i32) -> Option<&SiegeBattle> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[SiegeBattle] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, SiegeBattle> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}