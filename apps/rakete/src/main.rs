//! `rakete`, die Kommandozeile zu Rakete.
//!
//! **Ein Binär für zwei Leser.** Ein Mensch tippt `rakete customers
//! list` und bekommt eine Tabelle; ein Agent hängt `--json` an und
//! bekommt die Antwort des Servers, sonst nichts. Das menschliche Format
//! ist das, das sich ändern darf.
//!
//! **Die Befehle stehen nicht in diesem Programm.** Sie kommen aus der
//! Beschreibung, die der Server selbst ausliefert, und werden beim Start
//! zu einem Befehlsbaum zusammengesetzt (siehe `spec` und `tree`). Damit
//! kann dieses Werkzeug alles, was sein Server kann, und zwar genau der
//! Server, mit dem gerade gesprochen wird.

mod mcp;
mod spec;
mod tree;

use std::io::Read;

use clap::ArgMatches;
use l3pus_cli::api::Client;
use l3pus_cli::exit::{Code, Failure, Outcome};
use l3pus_cli::output::{self, Format};
use l3pus_cli::{config, secrets};

/// Unter diesem Namen liegen Konfiguration und Zeichen.
const TOOL: &str = "rakete";
/// Woher ein Agent sein Zeichen nimmt, ohne Schlüsselbund.
const TOKEN_VAR: &str = "RAKETE_TOKEN";
/// Womit ein Agent den Server bestimmt, ohne `login`.
const SERVER_VAR: &str = "RAKETE_SERVER";

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    // **Vor clap, und das lässt sich nicht vermeiden.** Der Befehlsbaum
    // hängt davon ab, welcher Server gemeint ist, und clap kann erst
    // auswerten, wenn der Baum steht. Also werden diese beiden Angaben
    // einmal von Hand aus der Zeile geholt.
    let format = if arguments.iter().any(|a| a == "--json") {
        Format::Json
    } else {
        Format::Human
    };
    let server = server_from(&arguments);

    match run(&arguments, server, format) {
        Ok(()) => {}
        Err(failure) => {
            output::fail(&failure.message);
            std::process::exit(failure.code as i32);
        }
    }
}

fn server_from(arguments: &[String]) -> Option<String> {
    let mut iter = arguments.iter();
    while let Some(argument) = iter.next() {
        if argument == "--server" || argument == "-s" {
            return iter.next().map(|value| config::normalize(value));
        }
        if let Some(value) = argument.strip_prefix("--server=") {
            return Some(config::normalize(value));
        }
    }
    if let Ok(value) = std::env::var(SERVER_VAR)
        && !value.trim().is_empty()
    {
        return Some(config::normalize(&value));
    }
    config::load(TOOL).server
}

fn run(arguments: &[String], server: Option<String>, format: Format) -> Outcome<()> {
    // Die Beschreibung des Servers. Liegt sie im Zwischenspeicher, wird
    // sie von dort genommen; sonst wird sie geholt.
    //
    // **Geholt wird ohne Anmeldung**, denn `/api/openapi.json` ist
    // offen. Das ist der Grund, warum ein Agent nur RAKETE_SERVER und
    // RAKETE_TOKEN setzen muss und nie `login` aufruft: der erste Befehl
    // bringt sich seine Befehle selbst mit.
    let mut unreachable: Option<Failure> = None;
    let server_unbekannt = server.is_none();
    let catalog = match server.as_deref() {
        Some(address) => match cached_spec(address)
            .ok()
            .and_then(|document| spec::parse(&document).ok())
        {
            Some(catalog) => Some(catalog),
            None => {
                match Client::new(address, None).and_then(|client| fetch_spec(&client, address)) {
                    Ok(catalog) => Some(catalog),
                    Err(failure) => {
                        unreachable = Some(failure);
                        None
                    }
                }
            }
        },
        None => None,
    };

    let command = tree::build(catalog.as_ref());
    let matches = command
        .clone()
        .try_get_matches_from(arguments)
        .map_err(|error| {
            // clap schreibt `--help` und `--version` selbst; das ist
            // kein Fehlschlag.
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                std::process::exit(0);
            }
            // **Ein unbekannter Befehl, weil der Server nicht antwortete,
            // ist kein Tippfehler.** Ohne diese Unterscheidung bekäme
            // eine Kette, die den Server nicht erreicht, den Wert 2 und
            // damit die Auskunft „du hast dich vertippt", obwohl die
            // Frage offen ist.
            let unbekannter_befehl = matches!(
                error.kind(),
                clap::error::ErrorKind::InvalidSubcommand | clap::error::ErrorKind::UnknownArgument
            );
            if unbekannter_befehl {
                if let Some(failure) = unreachable.take() {
                    return failure;
                }
                // **Ohne Server gibt es nur die eingebauten Befehle**,
                // und clap nennt jeden anderen einen Tippfehler. Das ist
                // die falsche Auskunft: der Befehl mag es geben, wir
                // wissen nur nicht, wo wir fragen sollen.
                if server_unbekannt {
                    return no_server();
                }
            }
            Failure::usage(error.to_string())
        })?;

    let (name, sub) = matches.subcommand().expect("subcommand_required");
    match name {
        "login" => login(server, sub, format),
        "logout" => logout(server, format),
        "whoami" => whoami(server, format),
        "refresh" => refresh(server, format),
        "describe" => describe(catalog.as_ref(), format),
        "completions" => completions(catalog.as_ref(), sub),
        "man" => manual(catalog.as_ref()),
        "mcp" => mcp::serve(
            std::sync::Arc::new(catalog.ok_or_else(no_server)?),
            server.clone().ok_or_else(no_server)?,
            secrets::read(TOOL, &server.ok_or_else(no_server)?, TOKEN_VAR),
        ),
        group => dispatch(
            catalog.as_ref().ok_or_else(no_server)?,
            server.ok_or_else(no_server)?,
            group,
            sub,
            format,
        ),
    }
}

fn no_server() -> Failure {
    Failure::unreachable(
        "Kein Server bekannt. `rakete login --server https://…`, oder RAKETE_SERVER setzen.",
    )
}

// ── Anmelden ─────────────────────────────────────────────────────────

fn login(server: Option<String>, matches: &ArgMatches, format: Format) -> Outcome<()> {
    let server = server.ok_or_else(|| {
        Failure::usage("Welcher Server? `rakete login --server https://rakete-app.l3p.us`")
    })?;

    // **Ein fertiges Zeichen ist der Weg für Maschinen**, Adresse und
    // Kennwort der für Menschen. Beides nebeneinander, damit ein Agent
    // nie ein Kennwort in die Hand bekommt.
    let token = match matches.get_one::<String>("token") {
        Some(token) => token.clone(),
        None => {
            let email = match matches.get_one::<String>("email") {
                Some(email) => email.clone(),
                None => ask("E-Mail: ")?,
            };
            let password = rpassword::prompt_password("Kennwort: ")
                .map_err(|error| Failure::usage(format!("Kennwort nicht gelesen: {error}")))?;
            let client = Client::new(&server, None)?;
            let body = serde_json::json!({
                "email": email,
                "password": password,
                "device": device_name(),
            });
            let answer = client.expect_ok("POST", "/api/auth/login", &[], Some(&body))?;
            match answer.get("token").and_then(|value| value.as_str()) {
                Some(token) => token.to_string(),
                None if answer.get("totp_challenge").is_some() => {
                    // Der zweite Faktor braucht einen zweiten Schritt, und
                    // den gibt es hier noch nicht. Lieber sagen als etwas
                    // Halbes tun.
                    return Err(Failure::wrong(
                        "Dieses Konto verlangt einen zweiten Faktor. \
                         Dafür bitte ein Zeichen anlegen: in Rakete unter \
                         Einstellungen, Zugang, Zeichen für Maschinen, dann \
                         `rakete login --token …`.",
                    ));
                }
                None => return Err(Failure::wrong("Der Server gab keine Marke zurück.")),
            }
        }
    };

    // Erst prüfen, dann merken: ein Zeichen, das nicht geht, im
    // Schlüsselbund ist schlimmer als keines.
    let client = Client::new(&server, Some(token.clone()))?;
    let me = client.expect_ok("GET", "/api/me", &[], None)?;

    secrets::write(TOOL, &server, &token)
        .map_err(|error| Failure::unreachable(error.to_string()))?;
    config::save(
        TOOL,
        &config::Settings {
            server: Some(server.clone()),
        },
    )
    .map_err(|error| Failure::unreachable(error.to_string()))?;
    let catalog = fetch_spec(&client, &server)?;

    output::note(
        format,
        &format!(
            "Angemeldet an {server}. {} Befehle in {} Gruppen.",
            catalog.count(),
            catalog.groups.len()
        ),
    );
    output::emit(format, &me);
    Ok(())
}

fn logout(server: Option<String>, format: Format) -> Outcome<()> {
    let server = server.ok_or_else(no_server)?;
    secrets::forget(TOOL, &server).map_err(|error| Failure::unreachable(error.to_string()))?;
    output::note(format, &format!("Zeichen für {server} vergessen."));
    output::emit(
        format,
        &serde_json::json!({"logged_out": true, "server": server}),
    );
    Ok(())
}

fn whoami(server: Option<String>, format: Format) -> Outcome<()> {
    let (client, _) = connect(server)?;
    let me = client.expect_ok("GET", "/api/me", &[], None)?;
    output::emit(format, &me);
    Ok(())
}

fn refresh(server: Option<String>, format: Format) -> Outcome<()> {
    let (client, server) = connect(server)?;
    let catalog = fetch_spec(&client, &server)?;
    output::note(
        format,
        &format!(
            "{} Befehle in {} Gruppen.",
            catalog.count(),
            catalog.groups.len()
        ),
    );
    output::emit(
        format,
        &serde_json::json!({
            "server": server,
            "groups": catalog.groups.len(),
            "commands": catalog.count(),
        }),
    );
    Ok(())
}

// ── Für einen Agenten ────────────────────────────────────────────────

/// Der ganze Baum als JSON.
///
/// **Gelesen aus derselben Quelle wie die Befehle selbst.** Eine
/// gepflegte Beschreibung daneben wäre eine zweite Liste, und die wäre
/// falsch, sobald jemand einen Endpunkt hinzufügt.
fn describe(catalog: Option<&spec::Catalog>, format: Format) -> Outcome<()> {
    let mut groups = serde_json::Map::new();
    if let Some(catalog) = catalog {
        for (group, operations) in &catalog.groups {
            let mut list = Vec::new();
            for operation in operations.values() {
                list.push(serde_json::json!({
                    "command": format!("{group} {}", operation.name),
                    "summary": operation.summary,
                    "method": operation.method,
                    "path": operation.path,
                    "arguments": operation.path_params.iter().map(|param| serde_json::json!({
                        "name": param.name,
                        "kind": "positional",
                        "required": true,
                        "help": param.description,
                    })).collect::<Vec<_>>(),
                    "options": operation.query_params.iter().map(|param| serde_json::json!({
                        "name": param.name,
                        "kind": "flag",
                        "required": param.required,
                        "help": param.description,
                    })).collect::<Vec<_>>(),
                    "body": operation.has_body.then(|| serde_json::json!({
                        "flags": ["--data", "--field"],
                        "required_fields": operation.required_fields,
                    })),
                }));
            }
            groups.insert(group.clone(), serde_json::Value::Array(list));
        }
    }

    let document = serde_json::json!({
        "tool": "rakete",
        "version": env!("CARGO_PKG_VERSION"),
        "global_options": [
            {"name": "server", "help": "Gegen welchen Server, statt des gemerkten"},
            {"name": "json", "help": "Antwort als JSON, ohne Begleittext"},
        ],
        "builtin": ["login", "logout", "whoami", "refresh", "describe", "completions", "man"],
        "exit_codes": {
            "0": "Es hat geklappt",
            "1": "Die Sache stimmt nicht. Noch einmal ändert nichts",
            "2": "Der Befehl war falsch benutzt",
            "3": "Wir konnten nicht nachsehen. Die Frage ist offen",
        },
        "groups": groups,
    });
    // `describe` ist für Agenten da, also immer JSON, auch ohne die
    // Fahne. Ein Mensch, der es aufruft, will dasselbe sehen.
    output::emit(Format::Json, &document);
    let _ = format;
    Ok(())
}

fn completions(catalog: Option<&spec::Catalog>, matches: &ArgMatches) -> Outcome<()> {
    let shell = *matches
        .get_one::<clap_complete::Shell>("shell")
        .ok_or_else(|| Failure::usage("Welche Shell?"))?;
    let mut command = tree::build(catalog);
    clap_complete::generate(shell, &mut command, "rakete", &mut std::io::stdout());
    Ok(())
}

fn manual(catalog: Option<&spec::Catalog>) -> Outcome<()> {
    let command = tree::build(catalog);
    clap_mangen::Man::new(command)
        .render(&mut std::io::stdout())
        .map_err(|error| Failure::unreachable(error.to_string()))?;
    Ok(())
}

// ── Der eigentliche Versand ──────────────────────────────────────────

fn dispatch(
    catalog: &spec::Catalog,
    server: String,
    group: &str,
    matches: &ArgMatches,
    format: Format,
) -> Outcome<()> {
    let (name, sub) = matches
        .subcommand()
        .ok_or_else(|| Failure::usage(format!("Welcher Vorgang in {group}?")))?;
    let operation = catalog
        .find(group, name)
        .ok_or_else(|| Failure::usage(format!("{group} {name} kennt dieser Server nicht")))?;

    // Pfadangaben einsetzen. Was hier fehlt, hat clap schon abgefangen.
    let mut path = operation.path.clone();
    for param in &operation.path_params {
        let value = sub
            .get_one::<String>(&param.name)
            .ok_or_else(|| Failure::usage(format!("{} fehlt", param.name)))?;
        path = path.replace(&format!("{{{}}}", param.name), value);
    }

    let mut query: Vec<(String, String)> = Vec::new();
    for param in &operation.query_params {
        let id = if operation.path_params.iter().any(|p| p.name == param.name) {
            format!("query-{}", param.name)
        } else {
            param.name.clone()
        };
        if let Some(value) = sub.get_one::<String>(&id) {
            query.push((param.name.clone(), value.clone()));
        }
    }

    let body = body_from(operation, sub)?;
    let token = secrets::read(TOOL, &server, TOKEN_VAR);
    let client = Client::new(&server, token)?;
    let response = client.call(&operation.method, &path, &query, body.as_ref())?;

    let ziel = sub
        .get_one::<String>("output")
        .or_else(|| matches.get_one::<String>("output"));

    match l3pus_cli::exit::from_status(response.status) {
        Code::Ok => {
            // **Kein JSON heißt: Bytes durchreichen, nicht deuten.** Ein
            // PDF als Text auf den Bildschirm zu schreiben macht ein
            // Terminal unbrauchbar, und die ersten Bytes eines PDF sind
            // lesbar genug, dass man es erst merkt, wenn es zu spät ist.
            if !response.is_json() && !response.bytes.is_empty() {
                return write_bytes(&response.bytes, ziel, &response.content_type, format);
            }
            // 204 hat keinen Rumpf, und „null" auf dem Bildschirm sieht
            // aus wie ein Fehler.
            if response.body.is_null() {
                output::note(format, "Erledigt.");
                output::emit(format, &serde_json::json!({"ok": true}));
            } else if let Some(pfad) = ziel {
                std::fs::write(
                    pfad,
                    serde_json::to_vec_pretty(&response.body).unwrap_or_default(),
                )
                .map_err(|error| Failure::unreachable(format!("{pfad}: {error}")))?;
                output::note(format, &format!("Geschrieben nach {pfad}."));
            } else {
                output::emit(format, &response.body);
            }
            Ok(())
        }
        code => Err(Failure {
            code,
            message: l3pus_cli::api::complain(response.status, &response.body),
        }),
    }
}

/// Bytes, die kein JSON sind: ein PDF, eine XML-Rechnung, ein Foto.
///
/// Nach stdout, wenn kein Ziel genannt ist, damit `> datei.pdf` und eine
/// Pipe funktionieren wie überall. **Aber nicht in ein Terminal**: dort
/// macht ein PDF den Bildschirm kaputt, und der Hinweis dazu ist
/// nützlicher als der Unrat.
fn write_bytes(
    bytes: &[u8],
    ziel: Option<&String>,
    content_type: &str,
    format: Format,
) -> Outcome<()> {
    use std::io::{IsTerminal, Write};

    if let Some(pfad) = ziel {
        std::fs::write(pfad, bytes)
            .map_err(|error| Failure::unreachable(format!("{pfad}: {error}")))?;
        output::note(
            format,
            &format!("{} Bytes nach {pfad} geschrieben.", bytes.len()),
        );
        return Ok(());
    }

    let stdout = std::io::stdout();
    if stdout.is_terminal() {
        return Err(Failure::usage(format!(
            "Die Antwort ist {}, {} Bytes, und kein Text. \
             Mit -o datei schreiben oder umleiten.",
            if content_type.is_empty() {
                "binär"
            } else {
                content_type.split(';').next().unwrap_or(content_type)
            },
            bytes.len()
        )));
    }

    let mut out = stdout.lock();
    out.write_all(bytes)
        .map_err(|error| Failure::unreachable(format!("stdout: {error}")))?;
    let _ = out.flush();
    Ok(())
}

/// Den Rumpf zusammensetzen: aus `--data`, aus `--field`, oder beides.
fn body_from(
    operation: &spec::Operation,
    matches: &ArgMatches,
) -> Outcome<Option<serde_json::Value>> {
    if !operation.has_body {
        return Ok(None);
    }
    let mut value = match matches.get_one::<String>("data") {
        Some(raw) => {
            let text = if let Some(rest) = raw.strip_prefix('@') {
                if rest == "-" {
                    let mut buffer = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buffer)
                        .map_err(|error| Failure::usage(format!("stdin: {error}")))?;
                    buffer
                } else {
                    std::fs::read_to_string(rest)
                        .map_err(|error| Failure::usage(format!("{rest}: {error}")))?
                }
            } else {
                raw.clone()
            };
            serde_json::from_str(&text)
                .map_err(|error| Failure::usage(format!("Das ist kein JSON: {error}")))?
        }
        None => serde_json::Value::Object(serde_json::Map::new()),
    };

    if let Some(fields) = matches.get_many::<String>("field") {
        let map = value
            .as_object_mut()
            .ok_or_else(|| Failure::usage("--field geht nur, wenn der Rumpf ein Objekt ist"))?;
        for field in fields {
            let (key, raw) = field
                .split_once('=')
                .ok_or_else(|| Failure::usage(format!("{field} hat kein Gleichheitszeichen")))?;
            map.insert(key.to_string(), guess(raw));
        }
    }

    // Ein leerer Rumpf für einen Vorgang, der einen braucht, ist fast
    // immer ein vergessenes Argument. Der Server sagte es auch, aber
    // erst nach einem Netzaufruf und weniger genau.
    if value.as_object().is_some_and(|map| map.is_empty()) && !operation.required_fields.is_empty()
    {
        return Err(Failure::usage(format!(
            "Dieser Vorgang braucht einen Rumpf. Nötig: {}. \
             Etwa: -f {}=…",
            operation.required_fields.join(", "),
            operation.required_fields[0]
        )));
    }
    Ok(Some(value))
}

/// Aus `aktiv=ja` wird `true`, aus `menge=3` wird `3`.
///
/// **Geraten wird nur, was eindeutig ist.** Eine Zeichenkette, die wie
/// eine Zahl aussieht, kommt in der Wirklichkeit vor: Postleitzahlen,
/// Kundennummern, IBAN-Teile. Deshalb bleibt alles, was mit einer Null
/// anfängt, Text; wer eine echte Zahl will, die so aussieht, nimmt
/// `--data`.
fn guess(raw: &str) -> serde_json::Value {
    match raw {
        "ja" | "true" => return serde_json::Value::Bool(true),
        "nein" | "false" => return serde_json::Value::Bool(false),
        "null" => return serde_json::Value::Null,
        _ => {}
    }
    if raw.len() > 1 && raw.starts_with('0') && !raw.starts_with("0.") {
        return serde_json::Value::String(raw.to_string());
    }
    if let Ok(number) = raw.parse::<i64>() {
        return serde_json::Value::Number(number.into());
    }
    if let Ok(number) = raw.parse::<f64>()
        && let Some(number) = serde_json::Number::from_f64(number)
    {
        return serde_json::Value::Number(number);
    }
    serde_json::Value::String(raw.to_string())
}

// ── Beschreibung holen und zwischenlagern ────────────────────────────

fn cached_spec(server: &str) -> anyhow::Result<serde_json::Value> {
    let path = config::spec_path(TOOL, server)?;
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn fetch_spec(client: &Client, server: &str) -> Outcome<spec::Catalog> {
    let document = client.expect_ok("GET", "/api/openapi.json", &[], None)?;
    let catalog = spec::parse(&document)?;
    if let Ok(path) = config::spec_path(TOOL, server) {
        let _ = std::fs::write(path, serde_json::to_string(&document).unwrap_or_default());
    }
    Ok(catalog)
}

fn connect(server: Option<String>) -> Outcome<(Client, String)> {
    let server = server.ok_or_else(no_server)?;
    let token = secrets::read(TOOL, &server, TOKEN_VAR);
    if token.is_none() {
        return Err(Failure::unreachable(format!(
            "Kein Zeichen für {server}. `rakete login`, oder {TOKEN_VAR} setzen."
        )));
    }
    let client = Client::new(&server, token)?;
    Ok((client, server))
}

fn ask(prompt: &str) -> Outcome<String> {
    use std::io::Write;
    let mut err = std::io::stderr();
    let _ = write!(err, "{prompt}");
    let _ = err.flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|error| Failure::usage(format!("Eingabe nicht gelesen: {error}")))?;
    Ok(line.trim().to_string())
}

/// Wie dieser Rechner in der Sitzungsliste steht.
fn device_name() -> String {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unbekannt".into());
    format!("rakete-cli auf {host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_and_no_become_booleans() {
        assert_eq!(guess("ja"), serde_json::json!(true));
        assert_eq!(guess("nein"), serde_json::json!(false));
        assert_eq!(guess("true"), serde_json::json!(true));
    }

    #[test]
    fn a_leading_zero_stays_text() {
        // Postleitzahlen, Kundennummern, Kontoauszüge: eine 0 vorn ist
        // Teil der Angabe und keine Ziffer zu viel.
        assert_eq!(guess("028217"), serde_json::json!("028217"));
        assert_eq!(guess("0"), serde_json::json!(0));
    }

    #[test]
    fn plain_numbers_become_numbers() {
        assert_eq!(guess("42"), serde_json::json!(42));
        assert_eq!(guess("-3"), serde_json::json!(-3));
        assert_eq!(guess("1.5"), serde_json::json!(1.5));
    }

    #[test]
    fn everything_else_stays_text() {
        assert_eq!(guess("Müller"), serde_json::json!("Müller"));
        assert_eq!(guess("2026-09-14"), serde_json::json!("2026-09-14"));
    }
}
