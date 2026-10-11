//! Local L1 BAL research runner. It does not launch a networked node or validate full payloads.

use crate::{
    fixture::{Fixture, Workload},
    measurement::{usage, Counters},
};
use alloy_primitives::keccak256;
use clap::{Parser, Subcommand, ValueEnum};
use eyre::{ensure, Result, WrapErr};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output as ProcessOutput, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod fixture;
mod measurement;
mod replay;

const UPSTREAM_SHA: &str = "1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607";

#[derive(Debug, Parser)]
#[command(about = "Reproducible L1 serial/BAL execution pilot inside pinned Reth")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate a self-contained deterministic fixture using reference execution.
    Generate {
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 16)]
        tx_count: usize,
        #[arg(long, value_enum, default_value_t = Workload::Transfers)]
        workload: Workload,
        #[arg(long)]
        output: PathBuf,
    },
    /// Compare full execution outputs of A and B, without benchmark claims.
    Check {
        #[arg(long)]
        fixture: PathBuf,
        #[arg(long, default_value_t = 4)]
        workers: usize,
    },
    /// Check fixtures and execute randomized paired measurements in isolated processes.
    Pilot {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Internal child process: one measured replay after same-variant warmups.
    #[command(hide = true)]
    Sample {
        #[arg(long)]
        fixture: PathBuf,
        #[arg(long, value_enum)]
        variant: Variant,
        #[arg(long)]
        workers: usize,
        #[arg(long)]
        warmups: usize,
        #[arg(long, default_value_t = false)]
        collect_counters: bool,
    },
}

#[derive(Debug, Clone, Copy, Serialize, ValueEnum)]
enum Variant {
    A,
    B,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PilotConfig {
    schema_version: u32,
    fixtures: Vec<PathBuf>,
    workers: Vec<usize>,
    warmups: usize,
    repetitions: usize,
    order_seed: u64,
    collect_counters: bool,
    timeout_seconds: u64,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Generate { seed, tx_count, workload, output } => {
            let fixture = Fixture::generate(seed, tx_count, workload)?;
            create_parent(&output)?;
            reth_fs_util::write_json_file(&output, &fixture)?;
            println!("generated {}: {}", fixture.fixture_id, output.display());
        }
        Commands::Check { fixture, workers } => {
            let input = load_fixture(&fixture)?;
            replay::check(&input, workers)?;
            println!("L1 correctness passed: {} (B workers={workers})", input.fixture_id);
        }
        Commands::Pilot { config, output } => pilot(&config, &output)?,
        Commands::Sample { fixture, variant, workers, warmups, collect_counters } => {
            let record = sample(&fixture, variant, workers, warmups, collect_counters)?;
            println!("{}", serde_json::to_string(&record)?);
        }
    }
    Ok(())
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        reth_fs_util::create_dir_all(parent)?;
    }
    Ok(())
}

fn load_fixture(path: &Path) -> Result<Fixture> {
    let fixture: Fixture = reth_fs_util::read_json_file(path)?;
    fixture.validate()?;
    Ok(fixture)
}

fn sample(
    path: &Path,
    variant: Variant,
    workers: usize,
    warmups: usize,
    collect_counters: bool,
) -> Result<Value> {
    let fixture = load_fixture(path)?;
    let runtime = replay::runtime(workers)?;
    let execute = |parent, counters| match variant {
        Variant::A => replay::run_serial_with_db(&fixture, parent, counters),
        Variant::B => replay::run_bal_with_db(&fixture, parent, &runtime, counters),
    };
    for _ in 0..warmups {
        let warmup_counters = collect_counters.then(|| Arc::new(Counters::default()));
        fixture.assert_output(&execute(fixture.db()?, warmup_counters)?)?;
    }
    let counters = collect_counters.then(|| Arc::new(Counters::default()));
    let parent = fixture.db()?;
    let epoch = Instant::now();
    let before = usage()?;
    let start = Instant::now();
    let result = execute(parent, counters.clone());
    let stop = Instant::now();
    let after = usage()?;
    let wall_ns = stop.duration_since(start).as_nanos() as u64;
    let mut record = json!({
        "schema_version": 1, "measurement_level": "L1",
        "fixture_id": fixture.fixture_id, "seed": fixture.seed, "variant": variant,
        "verdict": "execution_ok", "rejection_reason": null, "rejection_phase": null,
        "canonical_index_at_detection": null, "t_detect_ns": null,
        "t_start_ns": start.duration_since(epoch).as_nanos() as u64,
        "t_stop_ns": stop.duration_since(epoch).as_nanos() as u64, "wall_ns": wall_ns,
        "cpu_user_ns": after.user_ns - before.user_ns,
        "cpu_system_ns": after.system_ns - before.system_ns,
        "peak_rss_bytes": after.peak_rss_bytes,
        "peak_rss_scope": "process_lifetime_including_setup_and_warmups",
        "execution_workers": if matches!(variant, Variant::A) { 1 } else { workers.min(fixture.block.body.transactions.len()) },
        "configured_bal_workers": workers, "prewarm_workers": 0, "state_root_workers": 0,
        "cache_condition": "fresh_revm_parent_state_each_replay_warmed_worker_pool",
        "database_backend": "revm_in_memory", "collect_counters": collect_counters,
        "cache_hits": null, "cache_misses": null, "physical_read_bytes": null,
        "phantom_count": 0, "index_bytes": 0,
        "phantom_lookups_started": 0, "phantom_lookups_completed": 0, "prewarm_jobs_skipped": 0,
        "tx_count": fixture.block.body.transactions.len(), "bal_bytes": fixture.bal_rlp.len(),
        "gas_used": null, "output_digest": null,
    });
    let counts = counters.map(|c| serde_json::to_value(c.snapshot())).transpose()?;
    for name in
        ["logical_reads", "account_reads", "storage_reads", "code_reads", "block_hash_reads"]
    {
        record[name] = counts.as_ref().map_or(Value::Null, |c| c[name].clone());
    }
    match result.and_then(|output| {
        fixture.assert_output(&output)?;
        Ok(output)
    }) {
        Ok(output) => {
            record["gas_used"] = json!(output.execution.result.gas_used);
            record["output_digest"] = json!(output.digest()?);
        }
        Err(error) => {
            record["verdict"] = json!("failed");
            record["rejection_reason"] = json!(error.to_string());
            record["rejection_phase"] = json!("execution_or_output_check");
        }
    }
    Ok(record)
}

fn pilot(config_path: &Path, output_dir: &Path) -> Result<()> {
    ensure!(!cfg!(debug_assertions), "pilot measurements require the documented release build");
    let config: PilotConfig = reth_fs_util::read_json_file(config_path)?;
    ensure!(config.schema_version == 1, "unsupported config schema");
    ensure!(!config.fixtures.is_empty() && !config.workers.is_empty(), "empty pilot matrix");
    ensure!((1..=10_000).contains(&config.repetitions), "repetitions must be between 1 and 10000");
    ensure!((1..=100).contains(&config.warmups), "warmups must be between 1 and 100");
    ensure!((1..=3600).contains(&config.timeout_seconds), "invalid sample timeout");
    ensure!(config.workers.iter().all(|w| (1..=64).contains(w)), "invalid workers");
    ensure!(
        config.fixtures.iter().collect::<std::collections::HashSet<_>>().len()
            == config.fixtures.len(),
        "duplicate fixtures in pilot configuration"
    );
    ensure!(
        config.workers.iter().collect::<std::collections::HashSet<_>>().len()
            == config.workers.len(),
        "duplicate worker counts in pilot configuration"
    );
    // Gate the campaign on direct equivalence before collecting any measurements.
    for path in &config.fixtures {
        let fixture = load_fixture(path)?;
        for &workers in &config.workers {
            replay::check(&fixture, workers)?;
        }
    }
    ensure!(!output_dir.exists(), "output directory already exists; choose a new run directory");
    reth_fs_util::create_dir_all(output_dir)?;
    let binary = std::env::current_exe()?;
    let run_id = format!(
        "{}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        std::process::id()
    );
    let manifest = manifest(&config, &binary, &run_id)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let manifest_hash = keccak256(&manifest_bytes);
    reth_fs_util::write(output_dir.join("manifest.json"), &manifest_bytes)?;
    let mut file = BufWriter::new(reth_fs_util::create_file(output_dir.join("runs.jsonl"))?);
    let mut sequence = 0;
    let mut failed = 0;
    for path in &config.fixtures {
        let fixture = load_fixture(path)?;
        let fixture_hash = keccak256(reth_fs_util::read(path)?);
        for &workers in &config.workers {
            for repetition in 0..config.repetitions {
                // Hash-derived order is deterministic and independent of workload generation.
                let order_hash = keccak256(format!(
                    "{}:{}:{workers}:{repetition}",
                    config.order_seed, fixture.fixture_id
                ));
                let order = if order_hash[0] & 1 == 0 {
                    [Variant::A, Variant::B]
                } else {
                    [Variant::B, Variant::A]
                };
                let pair = format!("{}:{workers}:{repetition}", fixture.fixture_id);
                for (position, variant) in order.into_iter().enumerate() {
                    let mut record = child_sample(&binary, path, variant, workers, &config)
                        .unwrap_or_else(|err| {
                            failed_record(
                                &fixture,
                                variant,
                                workers,
                                config.collect_counters,
                                &err.to_string(),
                            )
                        });
                    if record["verdict"] != "execution_ok" {
                        failed += 1;
                    }
                    record["run_id"] = json!(format!("{run_id}:{sequence}"));
                    record["host_id"] = manifest["host_id"].clone();
                    record["fixture_hash"] = json!(fixture_hash);
                    record["manifest_hash"] = json!(manifest_hash);
                    record["comparison_pair"] = json!(pair);
                    record["repetition"] = json!(repetition);
                    record["execution_order"] = json!(position);
                    serde_json::to_writer(&mut file, &record)?;
                    file.write_all(b"\n")?;
                    file.flush()?;
                    sequence += 1;
                }
            }
            println!(
                "{} B-workers={workers}: {} pairs recorded",
                fixture.fixture_id, config.repetitions
            );
        }
    }
    reth_fs_util::write_json_file(
        &output_dir.join("completion.json"),
        &json!({
            "run_id": run_id, "records": sequence, "failed_records": failed,
            "complete": true, "manifest_hash": manifest_hash,
        }),
    )?;
    ensure!(failed == 0, "{failed} failed records retained; inspect runs.jsonl");
    println!("L1 pilot complete: {}", output_dir.display());
    Ok(())
}

fn child_sample(
    binary: &Path,
    fixture: &Path,
    variant: Variant,
    workers: usize,
    config: &PilotConfig,
) -> Result<Value> {
    let mut command = Command::new(binary);
    command
        .args(["sample", "--fixture"])
        .arg(fixture)
        .args(["--variant", if matches!(variant, Variant::A) { "a" } else { "b" }, "--workers"])
        .arg(workers.to_string())
        .arg("--warmups")
        .arg(config.warmups.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if config.collect_counters {
        command.arg("--collect-counters");
    }
    let output = wait_for_child(command.spawn()?, Duration::from_secs(config.timeout_seconds))?;
    ensure!(
        output.status.success(),
        "sample exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).wrap_err("invalid sample JSON")
}

fn failed_record(
    fixture: &Fixture,
    variant: Variant,
    workers: usize,
    collect_counters: bool,
    reason: &str,
) -> Value {
    let mut record = json!({
        "schema_version": 1, "measurement_level": "L1", "variant": variant,
        "fixture_id": fixture.fixture_id, "seed": fixture.seed, "configured_bal_workers": workers,
        "execution_workers": if matches!(variant, Variant::A) { 1 } else { workers.min(fixture.block.body.transactions.len()) },
        "collect_counters": collect_counters, "verdict": "failed", "rejection_phase": "child_process",
        "rejection_reason": reason, "tx_count": fixture.block.body.transactions.len(),
        "bal_bytes": fixture.bal_rlp.len(), "index_bytes": 0, "phantom_count": 0,
        "cache_condition": "fresh_revm_parent_state_each_replay_warmed_worker_pool", "database_backend": "revm_in_memory",
        "prewarm_workers": 0, "state_root_workers": 0,
    });
    for field in [
        "t_start_ns",
        "t_detect_ns",
        "t_stop_ns",
        "wall_ns",
        "cpu_user_ns",
        "cpu_system_ns",
        "peak_rss_bytes",
        "logical_reads",
        "account_reads",
        "storage_reads",
        "code_reads",
        "block_hash_reads",
        "cache_hits",
        "cache_misses",
        "physical_read_bytes",
        "gas_used",
        "output_digest",
        "canonical_index_at_detection",
        "phantom_lookups_started",
        "phantom_lookups_completed",
        "prewarm_jobs_skipped",
    ] {
        record[field] = Value::Null;
    }
    record
}

fn wait_for_child(mut child: Child, timeout: Duration) -> Result<ProcessOutput> {
    let deadline = Instant::now() + timeout;
    let result = (|| -> Result<()> {
        while child.try_wait()?.is_none() {
            ensure!(Instant::now() < deadline, "sample timed out after {} ms", timeout.as_millis());
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })();
    if let Err(error) = result {
        // Preserve the original error if cleanup also fails; never leave a timed-out sample running.
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(child.wait_with_output()?)
}

fn command_text(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn manifest(config: &PilotConfig, binary: &Path, run_id: &str) -> Result<Value> {
    ensure!(
        Command::new("git")
            .args(["merge-base", "--is-ancestor", UPSTREAM_SHA, "HEAD"])
            .status()?
            .success(),
        "research revision does not descend from the pinned upstream"
    );
    let baseline_diff = command_text("git", &["diff", UPSTREAM_SHA, "--", "crates"])?;
    ensure!(baseline_diff.is_empty(), "L1 pilot requires unmodified upstream execution code");
    let dirty_diff = command_text("git", &["diff", "HEAD"])?;
    let fixtures = config
        .fixtures
        .iter()
        .map(|path| {
            Ok(json!({
                "path": path, "keccak256": keccak256(reth_fs_util::read(path)?),
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "schema_version": 1, "run_id": run_id, "measurement_level": "L1", "config": config,
        "upstream_sha": UPSTREAM_SHA, "research_sha": command_text("git", &["rev-parse", "HEAD"])?,
        "git_status": command_text("git", &["status", "--porcelain"])?,
        "dirty_diff_keccak256": keccak256(dirty_diff),
        "lockfile_keccak256": keccak256(reth_fs_util::read("Cargo.lock")?),
        "binary_keccak256": keccak256(reth_fs_util::read(binary)?),
        "rustc": command_text("rustc", &["--version"])?, "cargo": command_text("cargo", &["--version"])?,
        "build_profile": "release (use documented --release build)",
        "rustflags": std::env::var("RUSTFLAGS").unwrap_or_default(),
        "os": command_text("uname", &["-srm"])?, "host_id": command_text("hostname", &[])?,
        "hardware": if cfg!(target_os = "macos") { command_text("sysctl", &["-n", "machdep.cpu.brand_string"]).ok() } else { None },
        "ram_bytes": if cfg!(target_os = "macos") { command_text("sysctl", &["-n", "hw.memsize"]).ok() } else { None },
        "fixtures": fixtures, "fork": "ChainSpecBuilder::mainnet().amsterdam_activated()",
        "stock_execution_code_unchanged": true, "sample_process_isolation": true,
        "timer": "per-block execution setup through joined execution helper; excludes parent snapshot construction, reference generation, file loading, process startup, runtime construction, warmups and output comparison; includes BAL worker database clones",
        "cpu": "RUSAGE_SELF user/system deltas across timed execution, all threads",
        "peak_rss": "RUSAGE_SELF process-lifetime high-water mark through measurement; includes setup and same-variant warmups; bytes on macOS, KiB converted to bytes on Linux",
        "logical_reads": "CountedDb account/code/storage/block-hash calls; not EVM opcode counts or physical I/O",
        "unsupported_counters": {"physical_read_bytes": "in-memory execution", "cache_hits": "BAL and REVM caches not separately instrumented", "cache_misses": "BAL and REVM caches not separately instrumented"},
        "scope": "execution component only; no payload acceptance, storage prewarm or state-root tasks; not a full A/B optimization-bundle result",
        "cache": "fresh independently reconstructed parent state per replay; OS cache uncontrolled",
        "environment": "local development pilot; background load, CPU affinity, thermal and power conditions uncontrolled",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_sample_is_stopped_and_following_sample_runs() -> Result<()> {
        let slow = Command::new("sleep").arg("5").spawn()?;
        assert_eq!(
            wait_for_child(slow, Duration::from_millis(20)).unwrap_err().to_string(),
            "sample timed out after 20 ms"
        );
        let next = Command::new("true").spawn()?;
        assert!(wait_for_child(next, Duration::from_secs(1))?.status.success());
        Ok(())
    }
}
