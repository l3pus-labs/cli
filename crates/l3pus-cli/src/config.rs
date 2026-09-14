//! Wo das Werkzeug sich merkt, mit welchem Server es spricht.
//!
//! **Die Adresse steht in einer Datei, das Zeichen nicht.** Eine Adresse
//! ist kein Geheimnis und gehört dorthin, wo ein Mensch sie ändern kann;
//! ein Zeichen gehört in den Schlüsselbund (siehe `secrets`). Die
//! Trennung ist der Grund, warum diese Datei gefahrlos in einer
//! Sicherung landen darf.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Was zwischen zwei Aufrufen stehen bleibt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Der Server, mit dem gesprochen wird, etwa
    /// `https://rakete-app.l3p.us`.
    #[serde(default)]
    pub server: Option<String>,
}

/// Der Ordner, in dem ein Werkzeug seine Sachen ablegt.
///
/// `directories` und keine eigene Rechnung: die richtige Stelle heißt
/// auf jedem der drei Systeme anders, und sie falsch zu raten merkt man
/// erst bei einem Benutzer, dessen Konto woanders liegt.
pub fn directory(tool: &str) -> Result<PathBuf> {
    let base = directories::ProjectDirs::from("us", "l3p", tool)
        .context("Kein Platz für die Konfiguration gefunden")?;
    let path = base.config_dir().to_path_buf();
    std::fs::create_dir_all(&path)
        .with_context(|| format!("{} ließ sich nicht anlegen", path.display()))?;
    Ok(path)
}

fn settings_path(tool: &str) -> Result<PathBuf> {
    Ok(directory(tool)?.join("einstellungen.json"))
}

pub fn load(tool: &str) -> Settings {
    // **Ein kaputter Inhalt ist kein Grund abzubrechen.** Wer in der
    // Datei etwas verstellt hat, soll `login` noch aufrufen können.
    settings_path(tool)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(tool: &str, settings: &Settings) -> Result<()> {
    let path = settings_path(tool)?;
    let text = serde_json::to_string_pretty(settings)?;
    std::fs::write(&path, text)
        .with_context(|| format!("{} ließ sich nicht schreiben", path.display()))
}

/// Wo die Beschreibung eines Servers zwischengelagert wird.
///
/// **Je Server eine Datei**, denn zwei Betriebe können verschiedene
/// Fassungen von Rakete laufen haben. Ein gemeinsamer Zwischenspeicher
/// würde dem einen die Befehle des anderen anbieten.
pub fn spec_path(tool: &str, server: &str) -> Result<PathBuf> {
    Ok(directory(tool)?.join(format!("beschreibung-{}.json", slug(server))))
}

/// Aus einer Adresse einen Dateinamen machen, der auf jedem System geht.
fn slug(server: &str) -> String {
    server
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Die Adresse säubern: ohne Schrägstrich am Ende, mit Schema.
pub fn normalize(server: &str) -> String {
    let trimmed = server.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        // **https und nicht http.** Wer eine Adresse ohne Schema
        // eintippt, meint die im Netz, und die soll nicht im Klartext
        // laufen, nur weil jemand acht Zeichen gespart hat.
        format!("https://{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_hostname_becomes_https() {
        assert_eq!(normalize("rakete-app.l3p.us"), "https://rakete-app.l3p.us");
    }

    #[test]
    fn a_trailing_slash_goes_away() {
        // Sonst entstehen Adressen mit zwei Schrägstrichen, und manche
        // Server antworten darauf mit 404.
        assert_eq!(
            normalize("https://rakete-app.l3p.us/"),
            "https://rakete-app.l3p.us"
        );
    }

    #[test]
    fn plain_http_stays_plain_http() {
        // Für die Entwicklung gegen den eigenen Rechner.
        assert_eq!(normalize("http://127.0.0.1:3000"), "http://127.0.0.1:3000");
    }

    #[test]
    fn two_servers_get_two_files() {
        assert_ne!(slug("https://a.example.com"), slug("https://b.example.com"));
        assert!(!slug("https://a.example.com:3000/").contains('/'));
        assert!(!slug("https://a.example.com:3000/").contains(':'));
    }
}
