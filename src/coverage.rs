use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// wokwi-cli's coverage file, format "wokwi-coverage" v1.
#[derive(Deserialize, Serialize)]
struct Report {
    format: String,
    version: u32,
    chip: String,
    granularity: u32,
    /// `[pc, hits, taken]`, sorted by `pc`.
    pcs: Vec<(u64, u64, u64)>,
    stats: Stats,
    nanos: u64,
}

#[derive(Deserialize, Serialize)]
struct Stats {
    instructions: u64,
    addresses: u64,
    taken: u64,
    /// Simulator-internal; cannot be derived when merging, so it is dropped then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pages: Option<u64>,
}

/// Combine the coverage of every simulation into `out`, summing hits and taken counts per address.
/// Simulations that wrote no file (e.g. killed while starting) are skipped.
pub fn merge(parts: &[PathBuf], out: &Path) -> Result<()> {
    let parts: Vec<&PathBuf> = parts.iter().filter(|p| p.exists()).collect();
    match parts.as_slice() {
        [] => {
            eprintln!("warning: wokwi-cli wrote no coverage; does the simulation server support it?");
            Ok(())
        }
        [only] => {
            std::fs::copy(only, out).with_context(|| format!("failed to write {}", out.display()))?;
            Ok(())
        }
        _ => {
            let mut merged: Option<Report> = None;
            let mut pcs: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
            let mut nanos = 0;
            for path in parts {
                let text = std::fs::read_to_string(path)?;
                let report: Report = serde_json::from_str(&text).with_context(|| format!("invalid coverage file {}", path.display()))?;
                if report.format != "wokwi-coverage" || report.version != 1 {
                    bail!("unsupported coverage format {} v{}", report.format, report.version);
                }
                if let Some(first) = &merged {
                    if (first.chip.as_str(), first.granularity) != (report.chip.as_str(), report.granularity) {
                        bail!("coverage files disagree on chip: {} vs {}", first.chip, report.chip);
                    }
                }
                for &(pc, hits, taken) in &report.pcs {
                    let entry = pcs.entry(pc).or_default();
                    entry.0 += hits;
                    entry.1 += taken;
                }
                nanos += report.nanos;
                merged.get_or_insert(report);
            }
            let mut report = merged.unwrap();
            report.pcs = pcs.into_iter().map(|(pc, (hits, taken))| (pc, hits, taken)).collect();
            report.stats = Stats {
                instructions: report.pcs.iter().map(|p| p.1).sum(),
                addresses: report.pcs.len() as u64,
                taken: report.pcs.iter().map(|p| p.2).sum(),
                pages: None,
            };
            report.nanos = nanos;
            std::fs::write(out, serde_json::to_string(&report)?).with_context(|| format!("failed to write {}", out.display()))?;
            Ok(())
        }
    }
}
