#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptSave {
    pub title: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardSavePackage {
    pub cells: Vec<i64>,
    pub scripts: Vec<ScriptSave>,
}

pub fn encode_board_save(game: &str, cells: &[i64], scripts: &[(&str, &str)]) -> String {
    let mut output = String::new();
    output.push_str("rustscript-board-save-v1\n");
    output.push_str(&format!("game={game}\n"));
    output.push_str("cells=");
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&cell.to_string());
    }
    output.push('\n');
    for &(title, source) in scripts {
        output.push_str("script.");
        output.push_str(title);
        output.push_str(".hex=");
        output.push_str(&hex_encode(source.as_bytes()));
        output.push('\n');
    }
    output
}

pub fn decode_board_save(
    input: &str,
    expected_game: &str,
    expected_cell_count: usize,
    expected_scripts: &[&str],
) -> Result<BoardSavePackage, String> {
    let mut lines = input.lines();
    if lines.next() != Some("rustscript-board-save-v1") {
        return Err("save text must start with rustscript-board-save-v1".to_string());
    }

    let mut game = None;
    let mut cells = None;
    let mut scripts = Vec::new();
    for line in lines {
        if let Some(value) = line.strip_prefix("game=") {
            game = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("cells=") {
            let parsed = if value.trim().is_empty() {
                Vec::new()
            } else {
                value
                    .split(',')
                    .map(|cell| {
                        cell.trim()
                            .parse::<i64>()
                            .map_err(|_| format!("invalid board cell value: {cell}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            cells = Some(parsed);
        } else if let Some(rest) = line.strip_prefix("script.") {
            let Some((title, hex)) = rest.split_once(".hex=") else {
                return Err(format!("invalid script entry: {line}"));
            };
            scripts.push(ScriptSave {
                title: title.to_string(),
                source: String::from_utf8(hex_decode(hex)?)
                    .map_err(|_| format!("script {title} is not UTF-8"))?,
            });
        }
    }

    if game.as_deref() != Some(expected_game) {
        return Err(format!("save text is not for {expected_game}"));
    }
    let cells = cells.ok_or_else(|| "save text is missing cells".to_string())?;
    if cells.len() != expected_cell_count {
        return Err(format!(
            "save text has {} cells; expected {expected_cell_count}",
            cells.len()
        ));
    }
    for expected in expected_scripts {
        if !scripts.iter().any(|script| script.title == *expected) {
            return Err(format!("save text is missing script {expected}"));
        }
    }

    Ok(BoardSavePackage { cells, scripts })
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode(input: &str) -> Result<Vec<u8>, String> {
    if !input.len().is_multiple_of(2) {
        return Err("hex value has an odd length".to_string());
    }
    let mut output = Vec::with_capacity(input.len() / 2);
    let bytes = input.as_bytes();
    for chunk in bytes.chunks_exact(2) {
        let high = hex_value(chunk[0])?;
        let low = hex_value(chunk[1])?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_value(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("invalid hex byte: {}", byte as char)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_save_roundtrips_cells_and_scripts() {
        let text = encode_board_save(
            "gomoku",
            &[0, 1, 2],
            &[
                ("move.rss", "let move_x: int = 1;\nmove_x"),
                ("ai.rss", "let note = \"脚本\";\n0"),
            ],
        );

        let decoded = decode_board_save(&text, "gomoku", 3, &["move.rss", "ai.rss"]).unwrap();

        assert_eq!(decoded.cells, vec![0, 1, 2]);
        assert_eq!(decoded.scripts[0].title, "move.rss");
        assert_eq!(decoded.scripts[0].source, "let move_x: int = 1;\nmove_x");
        assert_eq!(decoded.scripts[1].source, "let note = \"脚本\";\n0");
    }
}
