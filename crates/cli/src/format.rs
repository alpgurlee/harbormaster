use std::time::Duration;

const MIB: f64 = 1024.0 * 1024.0;
const GIB: f64 = MIB * 1024.0;

pub(crate) fn bytes(value: u64) -> String {
    let value = value as f64;
    if value >= GIB {
        format!("{:.1} GB", value / GIB)
    } else {
        format!("{:.0} MB", value / MIB)
    }
}

pub(crate) fn uptime(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    match (days, hours, minutes) {
        (0, 0, 0) => "<1m".to_string(),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

/// Cuts `text` to `width` characters, marking the cut with an ellipsis.
pub(crate) fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Command lines and project names come from other programs. Control
/// characters could rewrite the terminal (titles, colours, even the clipboard
/// through OSC 52) and bidi overrides could disguise text, so both are replaced.
pub(crate) fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            let bidi = matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
            if c.is_control() || bidi { '?' } else { c }
        })
        .collect()
}

/// Pads every column to its widest cell, two spaces apart.
pub(crate) fn table(rows: &[Vec<String>]) -> String {
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().map(|cell| printable(cell)).collect())
        .collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut output = String::new();
    for row in rows {
        let line: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        output.push_str(line.join("  ").trim_end());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{bytes, table, truncate, uptime};

    #[test]
    fn shows_memory_in_megabytes_then_gigabytes() {
        assert_eq!(bytes(851_443_712), "812 MB");
        assert_eq!(bytes(2_942_000_000), "2.7 GB");
        assert_eq!(bytes(300_000), "0 MB");
    }

    #[test]
    fn shows_uptime_with_the_two_largest_units() {
        assert_eq!(uptime(Duration::from_secs(20)), "<1m");
        assert_eq!(uptime(Duration::from_secs(41 * 60)), "41m");
        assert_eq!(uptime(Duration::from_secs(3 * 3600 + 12 * 60)), "3h 12m");
        assert_eq!(uptime(Duration::from_secs(2 * 86_400 + 5 * 3600)), "2d 5h");
    }

    #[test]
    fn neutralises_terminal_control_sequences_from_other_processes() {
        let rows = vec![vec![
            "evil\u{1b}]52;c;aGk=\u{7}name".to_string(),
            "safe\u{202e}txt.exe".to_string(),
        ]];

        assert_eq!(table(&rows), "evil?]52;c;aGk=?name  safe?txt.exe\n");
    }

    #[test]
    fn truncates_long_text_with_an_ellipsis() {
        assert_eq!(truncate("npm run dev", 20), "npm run dev");
        assert_eq!(
            truncate("Code Helper (Plugin) --type=utility", 12),
            "Code Helper…"
        );
    }

    #[test]
    fn aligns_columns_without_trailing_spaces() {
        let rows = vec![
            vec!["PORT".to_string(), "PROJECT".to_string()],
            vec!["3000".to_string(), "acme-store".to_string()],
        ];

        assert_eq!(table(&rows), "PORT  PROJECT\n3000  acme-store\n");
    }
}
