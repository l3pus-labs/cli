//! Der Weg zum Server.
//!
//! Blockierend und ohne Laufzeitumgebung: ein Kommandozeilenwerkzeug
//! macht einen Aufruf und wartet darauf. Ein Tokio-Unterbau dafür wäre
//! ein Megabyte Binär und eine Abhängigkeit mehr, für einen Nebenläufer,
//! den es nicht gibt.

use std::time::Duration;

use crate::exit::{Code, Failure, Outcome, from_status};

pub struct Client {
    inner: reqwest::blocking::Client,
    pub server: String,
    token: Option<String>,
}

/// Was vom Server zurückkam.
pub struct Response {
    pub status: u16,
    pub body: serde_json::Value,
}

impl Client {
    pub fn new(server: &str, token: Option<String>) -> Outcome<Self> {
        let inner = reqwest::blocking::Client::builder()
            // Dreißig Sekunden: ein Bericht über ein Jahr darf rechnen,
            // ein hängender Aufruf darf keine Kette blockieren.
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("l3pus-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Failure::unreachable(format!("Kein Netzzugang: {error}")))?;
        Ok(Self {
            inner,
            server: server.to_string(),
            token,
        })
    }

    /// Ein Aufruf, ohne Deutung des Ergebnisses.
    ///
    /// **Ein Fehlschlag am Netz ist etwas anderes als eine Absage vom
    /// Server.** Das erste ist „ich konnte nicht nachsehen", das zweite
    /// eine Antwort. Deshalb landet nur das erste hier als `Err`.
    pub fn call(
        &self,
        method: &str,
        path: &str,
        query: &[(String, String)],
        body: Option<&serde_json::Value>,
    ) -> Outcome<Response> {
        let url = format!("{}{}", self.server, path);
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| Failure::usage(format!("{method} ist keine Methode")))?;
        let mut request = self.inner.request(method, &url);
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().map_err(|error| {
            Failure::unreachable(format!("{url} war nicht erreichbar: {error}"))
        })?;
        let status = response.status().as_u16();
        let text = response.text().unwrap_or_default();
        // Ein leerer Rumpf ist gültig: 204 hat keinen.
        let body = if text.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
        };
        Ok(Response { status, body })
    }

    /// Ein Aufruf, dessen Antwort nur im Erfolgsfall etwas taugt.
    pub fn expect_ok(
        &self,
        method: &str,
        path: &str,
        query: &[(String, String)],
        body: Option<&serde_json::Value>,
    ) -> Outcome<serde_json::Value> {
        let response = self.call(method, path, query, body)?;
        match from_status(response.status) {
            Code::Ok => Ok(response.body),
            code => Err(Failure {
                code,
                message: complain(response.status, &response.body),
            }),
        }
    }
}

/// Aus der Antwort des Servers einen Satz machen, den ein Mensch liest.
///
/// Rakete antwortet mit `{"fehler": "..."}`; andere Server machen es
/// anders, und ein nackter Statuscode ist als Auskunft wertlos. Deshalb
/// erst nach dem Satz suchen und ihn sonst selbst bauen.
pub fn complain(status: u16, body: &serde_json::Value) -> String {
    for key in ["fehler", "error", "message", "detail"] {
        if let Some(text) = body.get(key).and_then(|v| v.as_str()) {
            return format!("{text} (HTTP {status})");
        }
    }
    if let Some(text) = body.as_str() {
        return format!("{text} (HTTP {status})");
    }
    match status {
        401 => "Nicht angemeldet. `rakete login`, oder RAKETE_TOKEN setzen.".into(),
        403 => "Dafür fehlt die Berechtigung.".into(),
        404 => "Das gibt es nicht.".into(),
        _ => format!("Der Server antwortete mit HTTP {status}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_servers_own_sentence_wins() {
        let text = complain(422, &json!({"fehler": "Gewerbekunden brauchen eine Firma"}));
        assert!(
            text.starts_with("Gewerbekunden brauchen eine Firma"),
            "{text}"
        );
        assert!(text.contains("422"), "{text}");
    }

    #[test]
    fn a_silent_server_still_gets_a_sentence() {
        // Ein nackter Statuscode ist als Auskunft wertlos.
        let text = complain(401, &json!(null));
        assert!(text.contains("rakete login"), "{text}");
    }

    #[test]
    fn a_foreign_shape_is_still_read() {
        let text = complain(400, &json!({"message": "bad request"}));
        assert!(text.starts_with("bad request"), "{text}");
    }
}
