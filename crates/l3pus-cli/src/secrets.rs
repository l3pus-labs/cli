//! Das Zugangszeichen, und wo es liegt.
//!
//! **Im Schlüsselbund des Systems und nicht in einer Datei.** Eine Datei
//! unter `~/.config` wandert in jede Sicherung, in jedes
//! Zeitmaschinen-Abbild und in jede Kopie des Rechners, und sie ist
//! lesbar für alles, was unter demselben Konto läuft. Der Schlüsselbund
//! ist genau dafür gebaut, es gibt ihn auf allen drei Systemen, und er
//! kostet nichts.
//!
//! **Ausnahme ist die Umgebung.** Steht `…_TOKEN` gesetzt, gilt das und
//! nichts anderes: in einer Kette, in einem Container und bei einer KI
//! gibt es keinen Schlüsselbund, den jemand entsperren könnte.

use anyhow::{Context, Result};

/// Der Eintrag, unter dem ein Werkzeug sein Zeichen ablegt.
///
/// Je Server ein Eintrag, weil ein Mensch mit mehreren Betrieben zu tun
/// haben kann: der eigene und der des Kunden, den er betreut.
fn entry(tool: &str, server: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(&format!("l3pus {tool}"), server)
        .context("Der Schlüsselbund war nicht ansprechbar")
}

/// Das Zeichen für diesen Server, aus der Umgebung oder dem
/// Schlüsselbund.
pub fn read(tool: &str, server: &str, env_var: &str) -> Option<String> {
    if let Ok(value) = std::env::var(env_var)
        && !value.trim().is_empty()
    {
        return Some(value.trim().to_string());
    }
    entry(tool, server).ok()?.get_password().ok()
}

pub fn write(tool: &str, server: &str, token: &str) -> Result<()> {
    entry(tool, server)?
        .set_password(token)
        .context("Das Zeichen ließ sich nicht im Schlüsselbund ablegen")
}

/// Vergisst das Zeichen. Dass keines da war, ist kein Fehler: `logout`
/// soll auch dann durchgehen, wenn man schon abgemeldet ist.
pub fn forget(tool: &str, server: &str) -> Result<()> {
    match entry(tool, server)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("Das Zeichen ließ sich nicht entfernen"),
    }
}
