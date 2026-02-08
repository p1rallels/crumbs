mod csv_store;
mod paths;

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use clap::{CommandFactory, Parser, Subcommand};
use rand::Rng;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "cr",
    about = "crumbs - Tiny, git-friendly memory CLI for coding agents",
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Record a WHAT: constraints/facts/gotchas (short, atomic)
    What {
        /// Memory text (max 100 chars). If omitted, read from stdin.
        text: Option<String>,
    },

    /// Record a WHY: rationale/intent (short, atomic)
    Why {
        /// Memory text (max 100 chars). If omitted, read from stdin.
        text: Option<String>,
    },

    /// List last N memories (default: 20)
    Ls {
        /// Number of memories to show
        #[arg(default_value_t = 20)]
        n: usize,
    },

    /// Show a memory by id (or unique full-id prefix, e.g. cr-otht or otht)
    Show { id: String },

    /// Find memories by substring (case-insensitive, v0)
    Find {
        query: String,

        /// Max results (default: 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// Create/open handoff checkpoints over memory history
    Handoff {
        #[command(subcommand)]
        cmd: Option<HandoffCommand>,
    },
}

#[derive(Subcommand, Debug)]
enum HandoffCommand {
    /// Create a new checkpoint at the latest memory
    Mark {
        /// Suggested memory window for next-agent bootstrap
        #[arg(long, default_value_t = 10)]
        window: usize,
    },

    /// Open a checkpoint and print the memory slice to review
    Open {
        /// Checkpoint id (or unique full-id prefix, e.g. hf-ab12 or ab12). Defaults to latest.
        id: Option<String>,

        /// Max memories to show. Defaults to checkpoint window.
        #[arg(long)]
        limit: Option<usize>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    if is_root_help_request() {
        print_root_help()?;
        return Ok(());
    }

    let cli = Cli::parse();

    match cli.cmd {
        None => onboarding(),
        Some(Command::What { text }) => add_memory("what", text),
        Some(Command::Why { text }) => add_memory("why", text),
        Some(Command::Ls { n }) => list(n),
        Some(Command::Show { id }) => show(&id),
        Some(Command::Find { query, limit }) => find(&query, limit),
        Some(Command::Handoff { cmd }) => handoff(cmd),
    }
}

fn is_root_help_request() -> bool {
    let args: Vec<String> = std::env::args().collect();
    args.len() == 2 && (args[1] == "--help" || args[1] == "-h")
}

fn print_root_help() -> Result<()> {
    let onboarding = onboarding_block_for_help()?;
    let mut cmd = Cli::command().long_about(format!(
        "crumbs - Tiny, git-friendly memory CLI for coding agents\n\n{}",
        onboarding
    ));
    cmd.print_long_help().context("print help")?;
    println!();
    Ok(())
}

fn onboarding_block_for_help() -> Result<String> {
    let store = resolve_store()?;
    if store.dir.is_dir() {
        return Ok(format!(
            "Onboarding:\n  Detected .crumbs at: {}\n  Run: cr handoff open\n  The checkpoint is there to group memories for later use to get up to speed",
            store.dir.display()
        ));
    }

    Ok("Onboarding:\n  If no store exists yet, start with the following when needed:\n    cr what \"<fact/constraint/change>\"\n    cr why \"<decision/rationale>\"\n  If you need to create a checkpoint:\n    cr handoff mark --window 10\n  The checkpoint is there to group memories for later use to get up to speed".to_string())
}

fn onboarding() -> Result<()> {
    let store = resolve_store()?;
    if store.dir.is_dir() {
        println!("Onboarding:");
        println!("  Detected .crumbs at: {}", store.dir.display());
        println!("  Run: cr handoff open");
        println!("  The checkpoint is there to group memories for later use to get up to speed");
        return Ok(());
    }

    println!("Onboarding:");
    println!("  If no store exists yet, start with the following when needed:");
    println!("    cr what \"<fact/constraint/change>\"");
    println!("    cr why \"<decision/rationale>\"");
    println!("  If you need to create a checkpoint:");
    println!("    cr handoff mark --window 10");
    println!("  The checkpoint is there to group memories for later use to get up to speed");
    Ok(())
}

#[derive(Debug, Clone)]
struct Store {
    root: PathBuf,
    dir: PathBuf,
    memories_csv_path: PathBuf,
    handoffs_csv_path: PathBuf,
}

impl Store {
    fn memories_csv_path(&self) -> &Path {
        &self.memories_csv_path
    }

    fn handoffs_csv_path(&self) -> &Path {
        &self.handoffs_csv_path
    }
}

fn resolve_store() -> Result<Store> {
    let cwd = std::env::current_dir().context("get current dir")?;
    let root = paths::store_root_from_cwd(&cwd);
    let dir = root.join(".crumbs");

    Ok(Store {
        root,
        dir: dir.clone(),
        memories_csv_path: dir.join("memories.csv"),
        handoffs_csv_path: dir.join("handoffs.csv"),
    })
}

fn ensure_store_scaffold(store: &Store) -> Result<()> {
    std::fs::create_dir_all(&store.dir)
        .with_context(|| format!("create {}", store.dir.display()))?;

    csv_store::ensure_memories_file(store.memories_csv_path())?;
    csv_store::ensure_handoffs_file(store.handoffs_csv_path())?;

    Ok(())
}

fn add_memory(kind: &str, text: Option<String>) -> Result<()> {
    let store = resolve_store()?;
    ensure_store_scaffold(&store)?;

    let text = read_text(text)?;
    validate_text(&text)?;

    let cwd = std::env::current_dir().context("get current dir")?;
    let cwd_saved = path_rel(&store.root, &cwd);

    let (git_branch, git_head) = git_info(&store.root).unwrap_or((None, None));

    let memories = csv_store::read_memories(store.memories_csv_path())?;
    let id = next_memory_id(&memories);
    let ts_utc = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let rec = csv_store::MemoryRecord {
        id: id.clone(),
        kind: kind.to_string(),
        text,
        ts_utc,
        cwd: cwd_saved,
        git_branch,
        git_head,
    };
    csv_store::append_memory(store.memories_csv_path(), &rec)?;

    println!("{id}");
    Ok(())
}

fn list(n: usize) -> Result<()> {
    let store = resolve_store()?;
    ensure_store_scaffold(&store)?;

    let memories = csv_store::read_memories(store.memories_csv_path())?;
    let rows = csv_store::list_memories(&memories, n);
    for (id, kind, text, ts, cwd, _branch, _head) in rows {
        println!("{id}\t{kind}\t{ts}\t{cwd}\t{text}");
    }

    Ok(())
}

fn show(id_prefix: &str) -> Result<()> {
    let store = resolve_store()?;
    ensure_store_scaffold(&store)?;

    let memories = csv_store::read_memories(store.memories_csv_path())?;
    let (id, kind, text, ts, cwd, branch, head) = csv_store::show_memory(&memories, id_prefix)?;

    println!("id:   {id}");
    println!("kind: {kind}");
    println!("ts:   {ts}");
    println!("cwd:  {cwd}");
    if let Some(b) = branch {
        println!("git_branch: {b}");
    }
    if let Some(h) = head {
        println!("git_head:   {h}");
    }
    println!("text: {text}");

    Ok(())
}

fn find(query: &str, limit: usize) -> Result<()> {
    let store = resolve_store()?;
    ensure_store_scaffold(&store)?;

    let memories = csv_store::read_memories(store.memories_csv_path())?;
    let rows = csv_store::find_memories(&memories, query, limit);
    for (id, kind, text, ts, cwd, _branch, _head) in rows {
        println!("{id}\t{kind}\t{ts}\t{cwd}\t{text}");
    }

    Ok(())
}

fn handoff(cmd: Option<HandoffCommand>) -> Result<()> {
    match cmd {
        None => handoff_open(None, None),
        Some(HandoffCommand::Mark { window }) => handoff_mark(window),
        Some(HandoffCommand::Open { id, limit }) => handoff_open(id.as_deref(), limit),
    }
}

fn handoff_mark(window: usize) -> Result<()> {
    if window == 0 {
        anyhow::bail!("window must be >= 1");
    }

    let store = resolve_store()?;
    ensure_store_scaffold(&store)?;

    let memories = csv_store::read_memories(store.memories_csv_path())?;
    let latest = csv_store::latest_memory(&memories)
        .context("no memories found; add at least one `what` or `why` first")?;

    let handoffs = csv_store::read_handoffs(store.handoffs_csv_path())?;
    let prev = csv_store::latest_handoff(&handoffs);
    if let Some(prev_handoff) = prev.as_ref() {
        if prev_handoff.to_memory_id == latest.id {
            anyhow::bail!("no new memories since last handoff; run `cr handoff open`");
        }
    }

    let cwd = std::env::current_dir().context("get current dir")?;
    let cwd_saved = path_rel(&store.root, &cwd);
    let (git_branch, git_head) = git_info(&store.root).unwrap_or((None, None));
    let ts_utc = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let from_memory_id = if let Some(prev_handoff) = prev.as_ref() {
        Some(prev_handoff.to_memory_id.clone())
    } else {
        // For the first checkpoint, cap scope to approximately `window` newest memories.
        let mut sorted = memories.clone();
        sorted.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));
        if sorted.len() > window {
            Some(sorted[window].id.clone())
        } else {
            None
        }
    };

    let handoff_id = next_handoff_id(&handoffs);
    let rec = csv_store::HandoffRecord {
        id: handoff_id.clone(),
        ts_utc,
        from_memory_id,
        to_memory_id: latest.id.clone(),
        suggested_window: window,
        cwd: cwd_saved,
        git_branch,
        git_head,
    };
    csv_store::append_handoff(store.handoffs_csv_path(), &rec)?;

    println!("handoff: {handoff_id}");
    println!("to:      {}", rec.to_memory_id);
    if let Some(from_id) = rec.from_memory_id.as_deref() {
        println!("from:    {from_id}");
    } else {
        println!("from:    <start>");
    }
    println!("window:  {}", rec.suggested_window);
    println!("open:    cr handoff open {handoff_id}");
    Ok(())
}

fn handoff_open(id_prefix: Option<&str>, limit: Option<usize>) -> Result<()> {
    let store = resolve_store()?;
    ensure_store_scaffold(&store)?;

    let handoffs = csv_store::read_handoffs(store.handoffs_csv_path())?;
    if handoffs.is_empty() {
        anyhow::bail!("no handoffs found; run `cr handoff mark --window 10` to create one");
    }

    let handoff = match id_prefix {
        Some(prefix) => csv_store::resolve_handoff(&handoffs, prefix)?,
        None => csv_store::latest_handoff(&handoffs).context("no handoffs found")?,
    };

    let memories = csv_store::read_memories(store.memories_csv_path())?;
    let to = memories
        .iter()
        .find(|m| m.id == handoff.to_memory_id)
        .with_context(|| format!("handoff target memory not found: {}", handoff.to_memory_id))?;

    let from_ts = handoff
        .from_memory_id
        .as_ref()
        .and_then(|from_id| memories.iter().find(|m| m.id == *from_id))
        .map(|m| m.ts_utc.clone());

    let mut slice: Vec<&csv_store::MemoryRecord> = memories
        .iter()
        .filter(|m| m.ts_utc <= to.ts_utc)
        .filter(|m| match &from_ts {
            Some(ts) => m.ts_utc > *ts,
            None => true,
        })
        .collect();
    slice.sort_by(|a, b| b.ts_utc.cmp(&a.ts_utc));

    let total = slice.len();
    let show_limit = limit.unwrap_or(handoff.suggested_window);
    let shown = std::cmp::min(total, show_limit);

    println!("handoff: {}", handoff.id);
    println!("to:      {}", handoff.to_memory_id);
    if let Some(from_id) = handoff.from_memory_id.as_deref() {
        println!("from:    {from_id}");
    } else {
        println!("from:    <start>");
    }
    println!("window:  {}", handoff.suggested_window);
    println!("slice:   {shown}/{total} memories (newest first)");
    println!("instructions:");
    println!("1. Read the memory rows below from newest to oldest.");
    println!("2. Continue work and record new context with `cr what` / `cr why`.");
    println!(
        "3. When handing off again, run `cr handoff mark --window {}`.",
        handoff.suggested_window
    );
    if shown < total {
        println!("more:    cr handoff open {} --limit {}", handoff.id, total);
    }

    for row in slice.into_iter().take(show_limit) {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            row.id, row.kind, row.ts_utc, row.cwd, row.text
        );
    }

    Ok(())
}

fn read_text(text: Option<String>) -> Result<String> {
    if let Some(t) = text {
        return Ok(t);
    }

    // If you want to use stdin, pipe it in.
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("read stdin")?;

    // Allow a trailing newline from typical pipes/heredocs.
    let trimmed = buf.trim_end_matches(['\n', '\r']);
    Ok(trimmed.to_string())
}

fn validate_text(text: &str) -> Result<()> {
    if text.is_empty() {
        anyhow::bail!("text is empty");
    }

    let n = text.chars().count();
    if n > 100 {
        anyhow::bail!("too long ({} > 100). split into multiple crumbs.", n);
    }

    if text.contains('\n') || text.contains('\r') {
        anyhow::bail!("newlines are not allowed");
    }

    Ok(())
}

fn path_rel(root: &Path, cwd: &Path) -> String {
    match cwd.strip_prefix(root) {
        Ok(p) if p.as_os_str().is_empty() => ".".to_string(),
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => cwd.to_string_lossy().to_string(),
    }
}

fn git_info(root: &Path) -> Result<(Option<String>, Option<String>)> {
    let branch = run_git(root, ["rev-parse", "--abbrev-ref", "HEAD"]).ok();
    let head = run_git(root, ["rev-parse", "HEAD"]).ok();
    Ok((branch, head))
}

fn run_git<I, S>(cwd: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("run git")?;

    if !out.status.success() {
        anyhow::bail!("git exited with {}", out.status);
    }

    let s = String::from_utf8(out.stdout).context("git output utf8")?;
    Ok(s.trim().to_string())
}

fn next_memory_id(memories: &[csv_store::MemoryRecord]) -> String {
    next_short_id(memories.iter().map(|m| m.id.as_str()), "cr")
}

fn next_handoff_id(handoffs: &[csv_store::HandoffRecord]) -> String {
    next_short_id(handoffs.iter().map(|h| h.id.as_str()), "hf")
}

fn next_short_id<'a, I>(existing_ids: I, prefix: &str) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    const INITIAL_LEN: usize = 4;
    const MAX_RETRIES_PER_LEN: usize = 64;

    let used: HashSet<String> = existing_ids
        .into_iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();

    let mut rng = rand::thread_rng();
    let mut len = INITIAL_LEN;
    loop {
        for _ in 0..MAX_RETRIES_PER_LEN {
            let suffix = random_base36(&mut rng, len);
            let candidate = format!("{prefix}-{suffix}");
            if !used.contains(&candidate) {
                return candidate;
            }
        }
        // Rare collision pressure: increase suffix length.
        len += 1;
    }
}

fn random_base36(rng: &mut impl Rng, len: usize) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let idx = rng.gen_range(0..36);
        out.push(DIGITS[idx] as char);
    }
    out
}
