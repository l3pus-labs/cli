//! Der gemeinsame Unterbau der l3pus-Werkzeuge.
//!
//! **Ein Binär für zwei Leser**, einen Menschen am Terminal und ein
//! Programm über `--json`. Alles hier drin folgt daraus:
//!
//! - Ergebnisse gehen nach stdout, alles andere nach stderr, damit
//!   `… --json | jq` nie etwas Fremdes zu sehen bekommt
//! - Rückgabewerte bedeuten etwas, damit ein Aufrufer die Frage
//!   „lag ich falsch" von „ich konnte nicht nachsehen" unterscheiden
//!   kann, ohne Text zu lesen
//! - Zugangszeichen liegen im Schlüsselbund des Systems und nie in
//!   einer Datei neben der Konfiguration

pub mod api;
pub mod config;
pub mod exit;
pub mod output;
pub mod secrets;

pub use exit::{Failure, Outcome};
