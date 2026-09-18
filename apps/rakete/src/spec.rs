//! Die Beschreibung des Servers, gelesen als Liste von Vorgängen.
//!
//! **Hier steht keine einzige Zeile über Kunden, Einsätze oder Belege**,
//! und das ist die Entscheidung, an der alles hängt. Rakete hat
//! zweihundertvierundneunzig Vorgänge; die von Hand als Unterbefehle zu
//! schreiben wäre einmal viel Arbeit und danach für immer eine Liste,
//! die hinterherhinkt. Der nächste Endpunkt käme dazu, niemand trüge ihn
//! nach, und das Werkzeug wäre still unvollständig.
//!
//! Gelesen wird **zur Laufzeit** und nicht beim Bauen: zwei Betriebe
//! können verschiedene Fassungen von Rakete laufen haben, und dann soll
//! jeder die Befehle bekommen, die sein Server auch kennt.

use std::collections::BTreeMap;

use l3pus_cli::exit::{Failure, Outcome};

/// Ein einzelner Aufruf, so wie die Kommandozeile ihn anbietet.
#[derive(Debug, Clone)]
pub struct Operation {
    /// Die Gruppe, unter der er steht: `customers`, `dispatch`, …
    pub group: String,
    /// Der Befehl innerhalb der Gruppe: `list`, `get`, `create`, …
    pub name: String,
    pub method: String,
    pub path: String,
    pub summary: String,
    /// Was im Pfad steht und deshalb angegeben werden **muss**.
    pub path_params: Vec<Param>,
    /// Was hinten an die Adresse gehängt wird.
    pub query_params: Vec<Param>,
    /// Ob der Vorgang einen Rumpf erwartet.
    pub has_body: bool,
    /// Ob der Rumpf rohe Bytes sind statt JSON.
    ///
    /// **Das Hochladen eines Papiers ist der Fall** (ADR-038): dort geht
    /// eine Datei hinaus, kein Feldsatz. Ohne diese Unterscheidung böte
    /// die Kommandozeile den Befehl an und könnte ihn nicht ausführen,
    /// und das ist schlimmer, als ihn nicht anzubieten.
    pub wants_file: bool,
    /// Welche Felder der Rumpf mindestens braucht. Steht in der
    /// Fehlermeldung, damit niemand die Beschreibung lesen muss.
    pub required_fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub required: bool,
    pub description: String,
}

/// Alle Vorgänge eines Servers, nach Gruppe und Befehl.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub groups: BTreeMap<String, BTreeMap<String, Operation>>,
}

impl Catalog {
    pub fn find(&self, group: &str, name: &str) -> Option<&Operation> {
        self.groups.get(group)?.get(name)
    }

    pub fn count(&self) -> usize {
        self.groups.values().map(BTreeMap::len).sum()
    }
}

/// Aus einer OpenAPI-Beschreibung die Befehle bauen.
pub fn parse(document: &serde_json::Value) -> Outcome<Catalog> {
    let paths = document
        .get("paths")
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            Failure::unreachable("Die Beschreibung des Servers hat keine Wege. Ist das Rakete?")
        })?;

    let mut catalog = Catalog::default();
    // Wo ein Name zweimal fiele, bekommen beide den vollen: lieber
    // `list-customers` als zwei Befehle, von denen einer den anderen
    // verdeckt.
    let mut collisions: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut entries: Vec<Operation> = Vec::new();

    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            continue;
        };
        for (method, operation) in item {
            let upper = method.to_uppercase();
            if !matches!(upper.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
                continue;
            }
            let group = group_of(operation);
            let id = operation
                .get("operationId")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let name = command_name(&id, &group);
            *collisions.entry((group.clone(), name.clone())).or_default() += 1;
            entries.push(Operation {
                group,
                name,
                method: upper,
                path: path.clone(),
                summary: summary_of(operation),
                path_params: params_of(operation, "path"),
                query_params: params_of(operation, "query"),
                has_body: operation.get("requestBody").is_some(),
                wants_file: wants_file(operation),
                required_fields: required_fields(document, operation),
            });
        }
    }

    for mut operation in entries {
        let key = (operation.group.clone(), operation.name.clone());
        if collisions.get(&key).copied().unwrap_or(0) > 1 {
            // Zwei Vorgänge derselben Gruppe wollten denselben Namen.
            // Dann bekommt jeder seinen vollen, statt dass einer
            // verschwindet.
            operation.name = full_name(&operation);
        }
        catalog
            .groups
            .entry(operation.group.clone())
            .or_default()
            .insert(operation.name.clone(), operation);
    }

    if catalog.count() == 0 {
        return Err(Failure::unreachable(
            "Die Beschreibung des Servers enthält keinen einzigen Vorgang.",
        ));
    }
    Ok(catalog)
}

/// Die Gruppe kommt aus dem Schildchen, ohne den Modulpfad davor.
///
/// utoipa schreibt `http::customers`; für einen Menschen auf der
/// Kommandozeile heißt das `customers`.
fn group_of(operation: &serde_json::Value) -> String {
    let tag = operation
        .get("tags")
        .and_then(|value| value.as_array())
        .and_then(|tags| tags.first())
        .and_then(|tag| tag.as_str())
        .unwrap_or("api");
    let short = tag.rsplit("::").next().unwrap_or(tag);
    kebab(short)
}

/// Aus `list_absences` in der Gruppe `absences` wird `list`.
///
/// **Die Wiederholung fällt weg und sonst nichts.** `rakete absences
/// list-absences` liest sich wie ein Formular; `rakete absences
/// list-calendars` dagegen sagt etwas, also bleibt es stehen.
fn command_name(id: &str, group: &str) -> String {
    let name = kebab(id);
    let group_singular = group.strip_suffix('s').unwrap_or(group);
    for suffix in [format!("-{group}"), format!("-{group_singular}")] {
        if let Some(rest) = name.strip_suffix(&suffix)
            && !rest.is_empty()
        {
            return rest.to_string();
        }
    }
    name
}

fn full_name(operation: &Operation) -> String {
    // Methode davor macht zwei gleichnamige Vorgänge unterscheidbar,
    // ohne dass man die Beschreibung lesen muss.
    format!("{}-{}", operation.method.to_lowercase(), operation.name)
}

fn kebab(text: &str) -> String {
    text.replace('_', "-").to_lowercase()
}

fn summary_of(operation: &serde_json::Value) -> String {
    for key in ["summary", "description"] {
        if let Some(text) = operation.get(key).and_then(|value| value.as_str())
            && !text.trim().is_empty()
        {
            // Nur die erste Zeile: `--help` ist eine Liste und kein
            // Fließtext.
            return text.lines().next().unwrap_or("").trim().to_string();
        }
    }
    String::new()
}

fn params_of(operation: &serde_json::Value, place: &str) -> Vec<Param> {
    operation
        .get("parameters")
        .and_then(|value| value.as_array())
        .map(|list| {
            list.iter()
                .filter(|param| param.get("in").and_then(|v| v.as_str()) == Some(place))
                .filter_map(|param| {
                    Some(Param {
                        name: param.get("name")?.as_str()?.to_string(),
                        required: param
                            .get("required")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(place == "path"),
                        description: param
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Welche Felder der Rumpf mindestens braucht.
///
/// Ein Verweis tief in `components` muss dafür aufgelöst werden. Mehr
/// als eine Ebene wird nicht verfolgt: was dann noch fehlt, sagt der
/// Server selbst, und zwar genauer als jede Ableitung hier.
/// Ob dieser Vorgang eine Datei will.
///
/// Erkannt an der Sorte des Rumpfes: alles, was nicht JSON ist, geht
/// byteweise hinaus. Der Server sagt es selbst in seiner Beschreibung,
/// also wird es gelesen und nicht geraten.
fn wants_file(operation: &serde_json::Value) -> bool {
    let Some(content) = operation
        .get("requestBody")
        .and_then(|body| body.get("content"))
        .and_then(|content| content.as_object())
    else {
        return false;
    };
    !content.is_empty() && !content.contains_key("application/json")
}

fn required_fields(document: &serde_json::Value, operation: &serde_json::Value) -> Vec<String> {
    let schema = operation
        .get("requestBody")
        .and_then(|body| body.get("content"))
        .and_then(|content| content.get("application/json"))
        .and_then(|json| json.get("schema"));
    let Some(schema) = schema else {
        return Vec::new();
    };
    let resolved = match schema.get("$ref").and_then(|value| value.as_str()) {
        Some(reference) => {
            let name = reference.rsplit('/').next().unwrap_or("");
            document
                .get("components")
                .and_then(|c| c.get("schemas"))
                .and_then(|s| s.get(name))
                .unwrap_or(schema)
        }
        None => schema,
    };
    resolved
        .get("required")
        .and_then(|value| value.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document() -> serde_json::Value {
        json!({
            "paths": {
                "/api/absences": {
                    "get": {"tags": ["http::absences"], "operationId": "list_absences",
                            "summary": "Alle Abwesenheiten"},
                    "post": {"tags": ["http::absences"], "operationId": "create_absence",
                             "requestBody": {"content": {"application/json":
                                 {"schema": {"$ref": "#/components/schemas/NewAbsence"}}}}}
                },
                "/api/absences/calendars": {
                    "get": {"tags": ["http::absences"], "operationId": "list_calendars"}
                },
                "/api/customers/{id}": {
                    "get": {"tags": ["http::customers"], "operationId": "get_customer",
                            "parameters": [
                                {"name": "id", "in": "path", "required": true},
                                {"name": "full", "in": "query", "required": false}
                            ]}
                }
            },
            "components": {"schemas": {"NewAbsence": {"required": ["type", "from"]}}}
        })
    }

    #[test]
    fn the_group_loses_the_module_path() {
        // utoipa schreibt http::customers; auf der Kommandozeile heißt
        // das customers.
        let catalog = parse(&document()).unwrap();
        assert!(
            catalog.groups.contains_key("customers"),
            "{:?}",
            catalog.groups.keys()
        );
        assert!(!catalog.groups.keys().any(|key| key.contains("::")));
    }

    #[test]
    fn the_repetition_in_the_name_falls_away() {
        let catalog = parse(&document()).unwrap();
        assert!(catalog.find("absences", "list").is_some());
        assert!(catalog.find("absences", "create").is_some());
    }

    #[test]
    fn a_name_that_says_something_stays() {
        // list-calendars ist keine Wiederholung der Gruppe, also bleibt
        // es stehen.
        let catalog = parse(&document()).unwrap();
        assert!(catalog.find("absences", "list-calendars").is_some());
    }

    #[test]
    fn path_and_query_are_told_apart() {
        let catalog = parse(&document()).unwrap();
        let operation = catalog.find("customers", "get").unwrap();
        assert_eq!(operation.path_params.len(), 1);
        assert_eq!(operation.path_params[0].name, "id");
        assert_eq!(operation.query_params.len(), 1);
        assert_eq!(operation.query_params[0].name, "full");
    }

    #[test]
    fn the_required_fields_come_through_the_reference() {
        let catalog = parse(&document()).unwrap();
        let operation = catalog.find("absences", "create").unwrap();
        assert_eq!(operation.required_fields, vec!["type", "from"]);
        assert!(operation.has_body);
    }

    #[test]
    fn a_document_without_operations_is_refused_rather_than_silently_empty() {
        // Ein Werkzeug ohne einen einzigen Befehl sieht aus wie ein
        // kaputtes Werkzeug, und das wäre die falsche Diagnose.
        let empty = json!({"paths": {}});
        assert!(parse(&empty).is_err());
    }

    #[test]
    fn two_operations_wanting_the_same_name_both_keep_one() {
        let clash = json!({
            "paths": {
                "/a": {"get": {"tags": ["http::x"], "operationId": "do_x"}},
                "/b": {"post": {"tags": ["http::x"], "operationId": "do_x"}}
            }
        });
        let catalog = parse(&clash).unwrap();
        assert_eq!(catalog.count(), 2, "{:?}", catalog.groups);
        assert!(catalog.find("x", "get-do").is_some());
        assert!(catalog.find("x", "post-do").is_some());
    }
}

#[cfg(test)]
mod file_body_tests {
    use super::*;

    /// Ein Rumpf, der keine JSON-Sorte führt, will eine Datei.
    ///
    /// **Sonst böte die Kommandozeile einen Befehl an, den sie nicht
    /// ausführen kann.** Genau das war bis zum 18.09.2026 der Fall: das
    /// Hochladen eines Papiers stand im Baum, und `--data` hätte die
    /// Bytes durch eine Zeichenkette gezwängt.
    #[test]
    fn an_octet_stream_body_asks_for_a_file() {
        let document = serde_json::json!({
            "paths": {
                "/api/papers/upload": {
                    "post": {"tags": ["http::papers"], "operationId": "upload",
                             "requestBody": {"content": {"application/octet-stream": {}}}}
                },
                "/api/customers": {
                    "post": {"tags": ["http::customers"], "operationId": "create",
                             "requestBody": {"content": {"application/json": {}}}}
                },
                "/api/customers/{id}": {
                    "get": {"tags": ["http::customers"], "operationId": "get",
                            "parameters": [{"name": "id", "in": "path", "required": true}]}
                }
            }
        });
        let catalog = parse(&document).unwrap();
        let von = |gruppe: &str, name: &str| {
            catalog
                .find(gruppe, name)
                .unwrap_or_else(|| panic!("{gruppe} {name} fehlt"))
        };
        assert!(von("papers", "upload").wants_file);
        assert!(!von("customers", "create").wants_file);
        // Ohne Rumpf ist auch keine Datei gemeint.
        assert!(!von("customers", "get").wants_file);
    }
}
