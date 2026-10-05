use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use addr2line::gimli;
use anyhow::{Context, Result, bail};
use object::{Object, ObjectSection, SectionKind};

use crate::coverage::Report;

type Reader = gimli::EndianArcSlice<gimli::RunTimeEndian>;

/// Write an LCOV tracefile (read by grcov, genhtml and Codecov) with a hit count per source line.
/// Every line with code in an executable section is listed, so lines that never ran show up as missed.
/// A line's count is the highest count of its instructions.
pub fn write(elf_path: &Path, report: &Report, out: &Path) -> Result<()> {
    let data = std::fs::read(elf_path).with_context(|| format!("failed to read {}", elf_path.display()))?;
    let elf = object::File::parse(&*data)?;
    let endian = if elf.is_little_endian() { gimli::RunTimeEndian::Little } else { gimli::RunTimeEndian::Big };
    let dwarf = gimli::Dwarf::load(|id| -> Result<Reader> {
        let bytes = match elf.section_by_name(id.name()) {
            Some(section) => section.uncompressed_data()?,
            None => Cow::Borrowed(&[][..]),
        };
        Ok(gimli::EndianArcSlice::new(Arc::from(&*bytes), endian))
    })?;
    let ctx = addr2line::Context::from_dwarf(dwarf)?;

    let mut pcs = report.pcs.clone();
    pcs.sort_unstable();
    let mut files: BTreeMap<String, BTreeMap<u32, u64>> = BTreeMap::new();
    // Restricting to code sections skips line rows of functions the linker discarded.
    for section in elf.sections().filter(|s| s.kind() == SectionKind::Text) {
        for (addr, len, location) in ctx.find_location_range(section.address(), section.address() + section.size())? {
            let (Some(file), Some(line @ 1..)) = (location.file, location.line) else { continue };
            let lo = pcs.partition_point(|p| p.0 < addr);
            let hi = pcs.partition_point(|p| p.0 < addr + len);
            let hits = pcs[lo..hi].iter().map(|p| p.1).max().unwrap_or(0);
            let entry = files.entry(file.to_owned()).or_default().entry(line).or_default();
            *entry = (*entry).max(hits);
        }
    }
    if files.is_empty() {
        bail!("{} has no DWARF line info for its code; build it with debug info", elf_path.display());
    }

    let mut lcov = String::new();
    for (file, lines) in &files {
        writeln!(lcov, "SF:{file}")?;
        for (line, hits) in lines {
            writeln!(lcov, "DA:{line},{hits}")?;
        }
        writeln!(lcov, "LF:{}", lines.len())?;
        writeln!(lcov, "LH:{}", lines.values().filter(|&&hits| hits > 0).count())?;
        writeln!(lcov, "end_of_record")?;
    }
    std::fs::write(out, lcov).with_context(|| format!("failed to write {}", out.display()))
}
