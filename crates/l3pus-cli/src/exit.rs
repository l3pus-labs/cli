//! Rückgabewerte, die etwas bedeuten.
//!
//! **Die Trennung von 1 und 3 ist der ganze Punkt.** Ein Skript, das die
//! Datenbank nicht erreicht, und eines, das ein echtes Problem gefunden
//! hat, geben sonst beide 1 zurück, und dann wird eine rote Kette neu
//! gestartet statt gelesen. Wer 3 bekommt, weiß: die Frage ist noch
//! offen, es lohnt sich, es gleich noch einmal zu versuchen. Wer 1
//! bekommt, weiß: noch einmal versuchen ändert nichts.
//!
//! Abgeschaut bei der clikd-CLI, wo dieselbe Tabelle steht und aus
//! demselben Grund.

use std::fmt;

/// Was ein Aufrufer aus dem Rückgabewert lesen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Es hat geklappt.
    Ok = 0,
    /// Die Sache, nach der gefragt wurde, stimmt nicht: eine Ablehnung
    /// vom Server, ein fehlender Datensatz, eine verletzte Regel. Noch
    /// einmal ausführen ändert nichts.
    Wrong = 1,
    /// Der Befehl war falsch benutzt: ein unbekannter Name, ein
    /// fehlendes Argument. Das liegt am Aufrufer.
    Usage = 2,
    /// Wir konnten nicht nachsehen: kein Zugang, kein Netz, eine
    /// Antwort, die wir nicht verstehen. **Die Frage ist noch offen.**
    Unreachable = 3,
}

/// Ein Abbruch mit Grund und Rückgabewert.
#[derive(Debug)]
pub struct Failure {
    pub code: Code,
    pub message: String,
}

impl Failure {
    pub fn wrong(message: impl Into<String>) -> Self {
        Self {
            code: Code::Wrong,
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            code: Code::Usage,
            message: message.into(),
        }
    }

    pub fn unreachable(message: impl Into<String>) -> Self {
        Self {
            code: Code::Unreachable,
            message: message.into(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// Was eine Befehlsfunktion zurückgibt.
pub type Outcome<T> = Result<T, Failure>;

/// Wie ein Aufruf, der aus dem Netz kam, auf die vier Werte fällt.
///
/// **Die Statuszahl allein reicht nicht.** 401 und 403 sehen nach
/// „falsch gefragt" aus und sind es nicht: beim ersten fehlt der
/// Zugang, beim zweiten reicht er nicht. Ohne Zugang ist die Frage
/// unbeantwortet, also 3; mit zu wenig Rechten ist sie beantwortet,
/// nämlich mit nein, also 1.
pub fn from_status(status: u16) -> Code {
    match status {
        200..=299 => Code::Ok,
        401 => Code::Unreachable,
        408 | 429 => Code::Unreachable,
        500..=599 => Code::Unreachable,
        _ => Code::Wrong,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_credential_leaves_the_question_open() {
        // Der Fall, für den es die Trennung gibt: wer 401 bekommt, hat
        // nicht erfahren, ob die Sache stimmt.
        assert_eq!(from_status(401), Code::Unreachable);
    }

    #[test]
    fn too_few_rights_is_an_answer() {
        // 403 heißt: wir haben nachgesehen, du darfst nicht. Noch
        // einmal fragen ändert nichts.
        assert_eq!(from_status(403), Code::Wrong);
    }

    #[test]
    fn a_broken_server_is_worth_another_try() {
        for status in [500, 502, 503, 504] {
            assert_eq!(from_status(status), Code::Unreachable, "{status}");
        }
    }

    #[test]
    fn a_missing_record_is_a_real_answer() {
        assert_eq!(from_status(404), Code::Wrong);
        assert_eq!(from_status(422), Code::Wrong);
    }

    #[test]
    fn everything_in_the_two_hundreds_worked() {
        for status in [200, 201, 204] {
            assert_eq!(from_status(status), Code::Ok, "{status}");
        }
    }

    #[test]
    fn being_told_to_slow_down_is_not_a_verdict() {
        // 429 ist keine Auskunft über die Sache, sondern über den
        // Zeitpunkt.
        assert_eq!(from_status(429), Code::Unreachable);
    }
}
