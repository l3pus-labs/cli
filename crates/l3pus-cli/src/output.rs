//! Wohin was geschrieben wird.
//!
//! **Die eine Regel:** was der Aufrufer wollte, geht nach stdout, alles
//! andere nach stderr. Ein Hinweis, eine Warnung, eine Fortschrittszeile
//! auf stdout macht `… --json | jq` kaputt, und zwar nicht sofort,
//! sondern irgendwann bei jemand anderem.
//!
//! **Und unter `--json` schweigt alles Höfliche.** Kein „gespeichert",
//! kein „drei Zeilen", keine Aktualisierungsmeldung. Ein Programm liest
//! die Antwort und keinen Begleittext.

use std::io::Write;

/// Wie ausgegeben wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Für einen Menschen: Tabellen, Sätze, Farbe.
    Human,
    /// Für ein Programm: eine JSON-Struktur, sonst nichts.
    Json,
}

impl Format {
    pub fn is_json(self) -> bool {
        self == Format::Json
    }
}

/// Das Ergebnis nach stdout.
pub fn emit(format: Format, value: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    match format {
        Format::Json => {
            let _ = writeln!(
                out,
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            );
        }
        Format::Human => {
            let _ = writeln!(out, "{}", human(value));
        }
    }
}

/// Ein Hinweis an den Menschen. Unter `--json` fällt er weg.
pub fn note(format: Format, message: &str) {
    if format.is_json() {
        return;
    }
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{message}");
}

/// Ein Fehler, immer nach stderr, auch unter `--json`.
///
/// **Auch unter `--json` als Text und nicht als JSON.** Wer die Ausgabe
/// weiterverarbeitet, prüft den Rückgabewert; wer sie am Bildschirm
/// sieht, will lesen können, was los war. Ein JSON-Fehler auf stderr
/// wäre für beide der schlechtere Kompromiss.
pub fn fail(message: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{message}");
}

/// JSON so hinschreiben, dass ein Mensch es liest.
///
/// **Keine Bibliothek dafür.** Eine Tabellenbibliothek will Spalten, die
/// sie vorher kennt; hier kommen die Felder aus einer Beschreibung, die
/// sich mit jedem Server ändern kann. Was hier steht, deckt die drei
/// Formen ab, die wirklich vorkommen: eine Liste von Objekten, ein
/// einzelnes Objekt, ein nackter Wert.
pub fn human(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(rows) if rows.iter().all(|row| row.is_object()) => {
            if rows.is_empty() {
                return "Nichts gefunden.".into();
            }
            table(rows)
        }
        serde_json::Value::Object(_) => pairs(value, 0),
        serde_json::Value::Null => "Nichts.".into(),
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Wie breit das Fenster ist, in dem das hier gelesen wird.
///
/// Ohne Terminal (in einer Kette, in einer Pipe) wird großzügig
/// gerechnet: dort liest kein Mensch mit, und wer die Ausgabe
/// weiterverarbeitet, hat ohnehin `--json` genommen.
fn width_budget() -> usize {
    terminal_size::terminal_size()
        .map(|(terminal_size::Width(w), _)| w as usize)
        .unwrap_or(200)
}

/// Eine Liste von Objekten als Tabelle, aber nur mit den Spalten, die
/// überall vorkommen, flach sind und ins Fenster passen.
///
/// Verschachtelte Felder fallen weg statt als `{…}` zu erscheinen: eine
/// Spalte, in der überall dasselbe Klammerpaar steht, kostet Breite und
/// sagt nichts. Wer sie braucht, nimmt `--json`.
///
/// **Und breiter als das Fenster wird nicht gedruckt.** Ein Kunde hat
/// zwanzig Felder; die alle nebeneinander ergeben eine Tabelle, die
/// umbricht und danach von niemandem mehr gelesen werden kann. Was
/// wegfällt, wird gezählt und darunter genannt, damit die Auslassung
/// sichtbar ist statt still.
fn table(rows: &[serde_json::Value]) -> String {
    let mut columns: Vec<String> = Vec::new();
    for row in rows {
        if let Some(map) = row.as_object() {
            for (key, value) in map {
                if flat(value) && !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }
    if columns.is_empty() {
        return format!("{} Einträge.", rows.len());
    }

    let mut widths: Vec<usize> = columns.iter().map(|c| c.chars().count()).collect();
    for row in rows {
        for (i, key) in columns.iter().enumerate() {
            let text = scalar(row.get(key).unwrap_or(&serde_json::Value::Null));
            widths[i] = widths[i].max(text.chars().count());
        }
    }

    // So viele Spalten, wie ins Fenster passen, in der Reihenfolge, in
    // der der Server sie geschrieben hat. Die erste bleibt immer, auch
    // wenn sie allein zu breit ist: eine Tabelle ohne Spalte ist keine.
    let budget = width_budget();
    let mut used = 0usize;
    let mut keep = 0usize;
    for (i, width) in widths.iter().enumerate() {
        let next = if i == 0 { *width } else { used + 2 + width };
        if i > 0 && next > budget {
            break;
        }
        used = next;
        keep = i + 1;
    }
    let dropped = columns.len() - keep;
    columns.truncate(keep);
    widths.truncate(keep);

    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .map(|key| scalar(row.get(key).unwrap_or(&serde_json::Value::Null)))
                .collect()
        })
        .collect();

    let mut lines = Vec::with_capacity(cells.len() + 2);
    lines.push(
        columns
            .iter()
            .enumerate()
            .map(|(i, c)| pad(c, widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string(),
    );
    lines.push(
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("  "),
    );
    for row in cells {
        lines.push(
            row.iter()
                .enumerate()
                .map(|(i, c)| pad(c, widths[i]))
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_string(),
        );
    }
    if dropped > 0 {
        lines.push(String::new());
        lines.push(format!(
            "({dropped} weitere {}, mit --json zu sehen)",
            if dropped == 1 { "Feld" } else { "Felder" }
        ));
    }
    lines.join("\n")
}

/// Ein Objekt als Feld-Wert-Paare, verschachtelt eingerückt.
fn pairs(value: &serde_json::Value, depth: usize) -> String {
    let Some(map) = value.as_object() else {
        return scalar(value);
    };
    let indent = "  ".repeat(depth);
    let width = map
        .keys()
        .filter(|key| flat(&map[*key]))
        .map(|key| key.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for (key, entry) in map {
        if flat(entry) {
            lines.push(format!("{indent}{}  {}", pad(key, width), scalar(entry)));
        } else if let Some(rows) = entry.as_array() {
            lines.push(format!("{indent}{key}:"));
            if rows.is_empty() {
                lines.push(format!("{indent}  (leer)"));
            } else {
                for row in rows {
                    lines.push(pairs(row, depth + 1));
                }
            }
        } else {
            lines.push(format!("{indent}{key}:"));
            lines.push(pairs(entry, depth + 1));
        }
    }
    lines.join("\n")
}

fn flat(value: &serde_json::Value) -> bool {
    !matches!(
        value,
        serde_json::Value::Array(_) | serde_json::Value::Object(_)
    )
}

fn scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Bool(true) => "ja".into(),
        serde_json::Value::Bool(false) => "nein".into(),
        other => other.to_string(),
    }
}

fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_list_of_objects_becomes_a_table() {
        let value = json!([
            {"id": 1, "name": "Anke"},
            {"id": 2, "name": "Britta"},
        ]);
        let text = human(&value);
        assert!(text.contains("id"), "{text}");
        assert!(text.contains("Anke"), "{text}");
        assert!(text.lines().count() == 4, "{text}");
    }

    #[test]
    fn nested_fields_stay_out_of_the_table() {
        // Eine Spalte, in der überall dasselbe Klammerpaar steht, kostet
        // Breite und sagt nichts.
        let value = json!([{"id": 1, "kontakt": {"mail": "a@b.c"}}]);
        let text = human(&value);
        assert!(!text.contains("kontakt"), "{text}");
    }

    #[test]
    fn an_empty_list_says_so_rather_than_printing_nothing() {
        // Eine leere Ausgabe sieht aus wie ein Fehler.
        assert_eq!(human(&json!([])), "Nichts gefunden.");
    }

    #[test]
    fn booleans_read_as_german_words() {
        let text = human(&json!({"aktiv": true, "gesperrt": false}));
        assert!(text.contains("ja"), "{text}");
        assert!(text.contains("nein"), "{text}");
    }

    #[test]
    fn notes_are_silent_under_json() {
        // Hier wird nur geprüft, dass die Entscheidung existiert; dass
        // nichts geschrieben wird, prüft die Probe von außen.
        assert!(Format::Json.is_json());
        assert!(!Format::Human.is_json());
    }
}
