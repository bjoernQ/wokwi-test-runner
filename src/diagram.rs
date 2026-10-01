use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

pub const BOARD_ID: &str = "esp";

/// `--part ID=TYPE[,KEY=VALUE...]`
#[derive(Debug, Clone)]
pub struct Part {
    pub id: String,
    pub kind: String,
    pub attrs: Vec<(String, String)>,
}

pub fn parse_part(s: &str) -> Result<Part, String> {
    let mut fields = s.split(',');
    let head = fields.next().unwrap_or_default();
    let (id, kind) = head.split_once('=').ok_or("expected ID=TYPE[,KEY=VALUE...], e.g. bmp=board-bmp180")?;
    let (id, kind) = (id.trim(), kind.trim());
    if id.is_empty() || kind.is_empty() || id.contains(':') {
        return Err("expected ID=TYPE[,KEY=VALUE...], e.g. bmp=board-bmp180".into());
    }
    if id == BOARD_ID {
        return Err(format!("part id '{BOARD_ID}' is reserved for the board"));
    }
    let attrs = fields
        .map(|f| {
            f.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .ok_or_else(|| format!("attribute '{f}' is not KEY=VALUE"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Part { id: id.to_string(), kind: kind.to_string(), attrs })
}

/// `--connect PIN=PIN[=PIN...]`. A pin without `ID:` belongs to the board.
pub fn parse_connection(s: &str) -> Result<Vec<String>, String> {
    let pins: Vec<String> = s
        .split('=')
        .map(|p| {
            let p = p.trim();
            if p.is_empty() { Err(format!("empty pin in '{s}'")) }
            else if p.contains(':') { Ok(p.to_string()) }
            else { Ok(format!("{BOARD_ID}:{p}")) }
        })
        .collect::<Result<_, _>>()?;
    if pins.len() < 2 {
        return Err("expected PIN=PIN[=PIN...], e.g. 2=3 or bmp:SDA=6".into());
    }
    Ok(pins)
}

pub fn diagram_json(board: &str, parts: &[Part], connections: &[Vec<String>]) -> Result<String> {
    let mut all_parts = vec![json!({ "type": board, "id": BOARD_ID, "attrs": {} })];
    for part in parts {
        if all_parts.iter().any(|p| p["id"] == part.id.as_str()) {
            bail!("duplicate part id '{}'", part.id);
        }
        let attrs: Map<String, Value> = part.attrs.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))).collect();
        all_parts.push(json!({ "type": part.kind, "id": part.id, "attrs": attrs }));
    }
    let mut wires = vec![json!(["esp:TX", "$serialMonitor:RX", ""]), json!(["esp:RX", "$serialMonitor:TX", ""])];
    for chain in connections {
        for pin in chain {
            let id = pin.split_once(':').map(|(id, _)| id).unwrap_or_default();
            if !id.starts_with('$') && !all_parts.iter().any(|p| p["id"] == id) {
                bail!("connection pin '{pin}' refers to unknown part '{id}' (add it with --part {id}=TYPE)");
            }
        }
        for pair in chain.windows(2) {
            wires.push(json!([pair[0], pair[1], "", []]));
        }
    }
    let diagram = json!({
        "version": 1,
        "author": "wokwi-test-runner",
        "editor": "wokwi",
        "parts": all_parts,
        "connections": wires,
    });
    Ok(serde_json::to_string_pretty(&diagram)?)
}
