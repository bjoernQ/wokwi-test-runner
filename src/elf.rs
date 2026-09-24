use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use object::{Object, ObjectSection, ObjectSymbol, Section, Symbol, SymbolKind};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome { Pass, Panic }

#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub expected_outcome: TestOutcome,
    pub ignored: bool,
    pub address: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TestDefinition {
    name: String,
    #[serde(rename = "should_panic", deserialize_with = "outcome_from_should_panic")]
    expected_outcome: TestOutcome,
    ignored: bool,
}

fn outcome_from_should_panic<'de, D>(deserializer: D) -> Result<TestOutcome, D::Error>
where D: serde::Deserializer<'de> {
    let should_panic = bool::deserialize(deserializer)?;
    Ok(if should_panic { TestOutcome::Panic } else { TestOutcome::Pass })
}

const TESTCASE_SYM_SIZE: u64 = 12;

pub fn parse_elf(path: &Path) -> Result<Option<EmbeddedTestElf>> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let elf = match object::File::parse(buffer.as_slice()) {
        Ok(elf) => elf,
        Err(_) => return Ok(None),
    };
    if elf.symbols().next().is_none() { return Ok(None); }
    let et_section = elf.section_by_name(".embedded_test").context("no .embedded_test section")?;
    let version_sym = elf.symbol_by_name("EMBEDDED_TEST_VERSION").context("no EMBEDDED_TEST_VERSION")?;
    let version_bytes = et_section.data_range(version_sym.address(), version_sym.size())?.unwrap();
    let version = u32::from_le_bytes(version_bytes.try_into().expect("version is 4 bytes"));
    match version {
        0 => Ok(Some(EmbeddedTestElf { version, tests: vec![] })),
        1 => {
            let mut tests = Vec::new();
            for sym in elf.symbols() {
                if let Some(test) = decode_testcase_sym(&buffer, &elf, &sym, &et_section)? {
                    tests.push(test);
                }
            }
            Ok(Some(EmbeddedTestElf { version, tests }))
        }
        v => Err(anyhow!("unsupported embedded-test protocol version {v}")),
    }
}

#[derive(Debug)]
pub struct EmbeddedTestElf {
    pub version: u32,
    pub tests: Vec<TestCase>,
}

fn decode_testcase_sym(buffer: &[u8], elf: &object::File, sym: &Symbol, et_section: &Section) -> Result<Option<TestCase>> {
    if !sym.is_global() || sym.kind() != SymbolKind::Data || sym.section_index() != Some(et_section.index()) || sym.size() != TESTCASE_SYM_SIZE {
        return Ok(None);
    }
    let sym_data = et_section.data_range(sym.address(), TESTCASE_SYM_SIZE)?.unwrap();
    let test_fn_ptr = u32::from_le_bytes(sym_data[0..4].try_into().unwrap());
    let mod_path_ptr = u32::from_le_bytes(sym_data[4..8].try_into().unwrap());
    let mod_path_len = u32::from_le_bytes(sym_data[8..12].try_into().unwrap());
    let mod_path = read_mod_path(buffer, elf, mod_path_ptr, mod_path_len)?;
    let def: TestDefinition = serde_json::from_str(sym.name()?)?;
    Ok(Some(TestCase {
        name: format!("{mod_path}::{}", def.name),
        expected_outcome: def.expected_outcome,
        ignored: def.ignored,
        address: Some(test_fn_ptr),
    }))
}

fn read_mod_path(buffer: &[u8], elf: &object::File, mod_path_ptr: u32, mod_path_len: u32) -> Result<String> {
    let section = elf.sections().find(|section| {
        mod_path_ptr as u64 >= section.address() && mod_path_ptr as u64 + mod_path_len as u64 <= section.address() + section.size()
    }).with_context(|| format!("section not found for mod path {mod_path_ptr:#x}"))?;
    let (start, _) = section.file_range().unwrap();
    let offset = mod_path_ptr as u64 - section.address();
    let file_offset = (start + offset) as usize;
    let full_path = std::str::from_utf8(&buffer[file_offset..file_offset + mod_path_len as usize])?;
    let first_col = full_path.find("::").ok_or_else(|| anyhow!("module path missing ::"))?;
    Ok(full_path[first_col + 2..].to_string())
}
