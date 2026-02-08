use anyhow::{Context, Result};
use csv::{ReaderBuilder, WriterBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::path::Path;

const MEMORIES_HEADER: &str = "id,kind,text,ts_utc,cwd,git_branch,git_head\n";
const HANDOFFS_HEADER: &str =
    "id,ts_utc,from_memory_id,to_memory_id,suggested_window,cwd,git_branch,git_head\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub ts_utc: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffRecord {
    pub id: String,
    pub ts_utc: String,
    pub from_memory_id: Option<String>,
    pub to_memory_id: String,
    pub suggested_window: usize,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
}

pub type MemoryRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

pub fn ensure_memories_file(memories_csv_path: &Path) -> Result<()> {
    ensure_csv_file(memories_csv_path, MEMORIES_HEADER)?;
    Ok(())
}

pub fn ensure_handoffs_file(handoffs_csv_path: &Path) -> Result<()> {
    ensure_csv_file(handoffs_csv_path, HANDOFFS_HEADER)
}

pub fn read_memories(memories_csv_path: &Path) -> Result<Vec<MemoryRecord>> {
    if !memories_csv_path.exists() {
        return Ok(Vec::new());
    }

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_path(memories_csv_path)
        .with_context(|| format!("open {}", memories_csv_path.display()))?;

    let mut out = Vec::new();
    for row in reader.deserialize() {
        let record: MemoryRecord =
            row.with_context(|| format!("parse {}", memories_csv_path.display()))?;
        out.push(record);
    }
    Ok(out)
}

pub fn append_memory(memories_csv_path: &Path, rec: &MemoryRecord) -> Result<()> {
    append_csv_row(memories_csv_path, rec)
}

pub fn read_handoffs(handoffs_csv_path: &Path) -> Result<Vec<HandoffRecord>> {
    if !handoffs_csv_path.exists() {
        return Ok(Vec::new());
    }

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_path(handoffs_csv_path)
        .with_context(|| format!("open {}", handoffs_csv_path.display()))?;

    let mut out = Vec::new();
    for row in reader.deserialize() {
        let record: HandoffRecord =
            row.with_context(|| format!("parse {}", handoffs_csv_path.display()))?;
        out.push(record);
    }
    Ok(out)
}

pub fn append_handoff(handoffs_csv_path: &Path, rec: &HandoffRecord) -> Result<()> {
    append_csv_row(handoffs_csv_path, rec)
}

pub fn latest_memory(memories: &[MemoryRecord]) -> Option<MemoryRecord> {
    let mut rows = memories.to_vec();
    rows.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));
    rows.into_iter().next()
}

pub fn latest_handoff(handoffs: &[HandoffRecord]) -> Option<HandoffRecord> {
    let mut rows = handoffs.to_vec();
    rows.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));
    rows.into_iter().next()
}

pub fn resolve_handoff(handoffs: &[HandoffRecord], id_prefix: &str) -> Result<HandoffRecord> {
    let candidates = build_prefix_candidates(id_prefix, "hf-", "h_");
    let mut seen: HashSet<String> = HashSet::new();
    let mut matches: Vec<&HandoffRecord> = handoffs
        .iter()
        .filter(|h| matches_any_prefix(&h.id, &candidates))
        .filter(|h| seen.insert(h.id.clone()))
        .collect();
    if matches.is_empty() {
        anyhow::bail!("no handoff matching id prefix '{id_prefix}'");
    }
    if matches.len() > 1 {
        anyhow::bail!("handoff id prefix '{id_prefix}' is ambiguous");
    }
    matches.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));
    Ok(matches[0].clone())
}

pub fn resolve_memory_id(memories: &[MemoryRecord], id_prefix: &str) -> Result<String> {
    let candidates = build_prefix_candidates(id_prefix, "cr-", "c_");
    let mut seen: HashSet<String> = HashSet::new();
    let mut matches: Vec<&MemoryRecord> = memories
        .iter()
        .filter(|m| matches_any_prefix(&m.id, &candidates))
        .filter(|m| seen.insert(m.id.clone()))
        .collect();

    if matches.is_empty() {
        anyhow::bail!("no memory matching id prefix '{id_prefix}'");
    }
    if matches.len() > 1 {
        anyhow::bail!("id prefix '{id_prefix}' is ambiguous");
    }

    matches.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));
    Ok(matches[0].id.clone())
}

pub fn list_memories(memories: &[MemoryRecord], limit: usize) -> Vec<MemoryRow> {
    let mut rows = memories.to_vec();
    rows.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));
    rows.into_iter().take(limit).map(to_memory_row).collect()
}

pub fn show_memory(memories: &[MemoryRecord], id_prefix: &str) -> Result<MemoryRow> {
    let id = resolve_memory_id(memories, id_prefix)?;
    let rec = memories
        .iter()
        .find(|m| m.id == id)
        .with_context(|| format!("resolve id '{}'", id_prefix))?;
    Ok(to_memory_row(rec.clone()))
}

pub fn find_memories(memories: &[MemoryRecord], query: &str, limit: usize) -> Vec<MemoryRow> {
    let needle = query.to_lowercase();
    let mut rows: Vec<MemoryRecord> = memories
        .iter()
        .filter(|m| m.text.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    rows.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));
    rows.into_iter().take(limit).map(to_memory_row).collect()
}

fn ensure_csv_file(path: &Path, header: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let needs_init = !path.exists() || fs::metadata(path)?.len() == 0;
    if needs_init {
        fs::write(path, header).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn append_csv_row<T: Serialize>(path: &Path, row: &T) -> Result<()> {
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .with_context(|| format!("open {} for append", path.display()))?;

    let mut writer = WriterBuilder::new().has_headers(false).from_writer(file);
    writer
        .serialize(row)
        .with_context(|| format!("append {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("flush {}", path.display()))?;
    Ok(())
}

fn to_memory_row(rec: MemoryRecord) -> MemoryRow {
    (
        rec.id,
        rec.kind,
        rec.text,
        rec.ts_utc,
        rec.cwd,
        rec.git_branch,
        rec.git_head,
    )
}

fn build_prefix_candidates(id_prefix: &str, canonical: &str, legacy: &str) -> Vec<String> {
    let mut candidates = vec![id_prefix.to_ascii_lowercase()];
    if !id_prefix.contains('-') && !id_prefix.contains('_') {
        candidates.push(format!("{canonical}{id_prefix}").to_ascii_lowercase());
        candidates.push(format!("{legacy}{id_prefix}").to_ascii_lowercase());
    }
    candidates
}

fn matches_any_prefix(id: &str, candidates: &[String]) -> bool {
    let id_lower = id.to_ascii_lowercase();
    candidates.iter().any(|p| id_lower.starts_with(p))
}
