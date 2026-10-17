#![allow(clippy::disallowed_types)]

use plugin_toolkit::prelude::*;
use plugin_toolkit::progenitor_client::QueryParam;
use plugin_toolkit::reqwest::Client as HttpClient;
use plugin_toolkit::serde_json::{self, Value};

pub struct Ll {
    base_url: String,
    api_key: String,
    http: HttpClient,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub author_id: Option<String>,
    pub author: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    pub published: Option<String>,
    pub ebook_status: Option<String>,
    pub audio_status: Option<String>,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub status: Option<String>,
    pub have_books: Option<u64>,
    pub total_books: Option<u64>,
}

impl Ll {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http: HttpClient::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn send(&self, cmd: &str, params: &[(&str, &str)]) -> Result<String> {
        let req = params.iter().fold(
            self.http
                .get(format!("{}/api", self.base_url))
                .query(&QueryParam::new("apikey", &self.api_key))
                .query(&QueryParam::new("cmd", &cmd)),
            |r, (k, v)| r.query(&QueryParam::new(k, v)),
        );
        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("{cmd}: {}", self.excerpt(&e.to_string())))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| anyhow!("{cmd}: {}", self.excerpt(&e.to_string())))?;
        if !status.is_success() {
            bail!("{cmd}: HTTP {} {}", status.as_u16(), self.excerpt(&body));
        }
        Ok(body)
    }

    pub async fn call(&self, cmd: &str, params: &[(&str, &str)]) -> Result<Value> {
        let body = self.send(cmd, params).await?;
        let Ok(v) = serde_json::from_str::<Value>(&body) else {
            bail!("{cmd}: {}", self.excerpt(&body));
        };
        if v.get("Success").and_then(Value::as_bool) == Some(false) {
            let msg = v["Error"]["Message"].as_str().unwrap_or("request refused");
            bail!("{cmd}: {}", self.excerpt(msg));
        }
        Ok(v)
    }

    pub async fn command(&self, cmd: &str, params: &[(&str, &str)]) -> Result<()> {
        let body = self.send(cmd, params).await?;
        if body.trim() != "OK" {
            bail!("{cmd}: {}", self.excerpt(&body));
        }
        Ok(())
    }

    pub async fn authors(&self) -> Result<Vec<Author>> {
        let v = self.call("getIndex", &[]).await?;
        Ok(rows(&v).iter().map(parse_author).collect())
    }

    pub async fn books(&self, params: &[(&str, &str)]) -> Result<Vec<Book>> {
        let v = self.call("getAllBooks", params).await?;
        Ok(rows(&v).iter().map(parse_book).collect())
    }

    pub async fn find_books(&self, name: &str) -> Result<Vec<Book>> {
        let v = self.call("findBook", &[("name", name)]).await?;
        Ok(rows(&v).iter().map(parse_book).collect())
    }

    pub async fn queue_book(&self, book_id: &str, library: &str) -> Result<()> {
        check_id(book_id)?;
        self.command("queueBook", &[("id", book_id), ("type", library)])
            .await
    }

    pub async fn force_search(&self, library: Option<&str>) -> Result<()> {
        match library {
            Some(ty) => self.command("forceBookSearch", &[("type", ty)]).await,
            None => self.command("forceBookSearch", &[]).await,
        }
    }

    pub async fn scan(&self, library: &str, author_id: Option<&str>) -> Result<()> {
        let cmd = match library {
            "AudioBook" => "forceAudioBookScan",
            _ => "forceLibraryScan",
        };
        match author_id {
            Some(id) => {
                check_id(id)?;
                self.command(cmd, &[("id", id)]).await
            }
            None => self.command(cmd, &[]).await,
        }
    }

    fn excerpt(&self, text: &str) -> String {
        let text = redact(text.trim(), &self.api_key);
        let cut = text
            .char_indices()
            .nth(ERROR_EXCERPT)
            .map_or(text.as_str(), |(i, _)| &text[..i]);
        plugin_toolkit::logging::scrub(cut).into_owned()
    }
}

const ERROR_EXCERPT: usize = 256;

pub fn redact(text: &str, key: &str) -> String {
    let mut text = text.to_string();
    if !key.is_empty() {
        text = text
            .replace(key, "***")
            .replace(&percent_encoded(key), "***");
    }
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    while let Some(at) = lower[rest..].find("apikey=") {
        let start = rest + at + "apikey=".len();
        let end = text[start..]
            .find(|c: char| {
                c == '&' || c == '"' || c == '\'' || c == '<' || c == '>' || c.is_whitespace()
            })
            .map_or(text.len(), |i| start + i);
        out.push_str(&text[rest..start]);
        out.push_str("***");
        rest = end;
    }
    out.push_str(&text[rest..]);
    out
}

fn percent_encoded(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn check_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("invalid lazylibrarian id {id:?}");
    }
    Ok(())
}

fn rows(v: &Value) -> &[Value] {
    v.get("data")
        .unwrap_or(v)
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.as_object()
        .and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)))
        .map_or(&Value::Null, |(_, v)| v)
}

fn opt_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn parse_author(row: &Value) -> Author {
    Author {
        id: opt_str(field(row, "AuthorID")).unwrap_or_default(),
        name: opt_str(field(row, "AuthorName")).unwrap_or_default(),
        status: opt_str(field(row, "Status")),
        have_books: opt_u64(field(row, "HaveBooks")),
        total_books: opt_u64(field(row, "TotalBooks")),
    }
}

pub fn parse_book(row: &Value) -> Book {
    Book {
        id: opt_str(field(row, "BookID")).unwrap_or_default(),
        title: opt_str(field(row, "BookName")).unwrap_or_default(),
        subtitle: opt_str(field(row, "BookSub")),
        author_id: opt_str(field(row, "AuthorID")),
        author: opt_str(field(row, "AuthorName")),
        isbn: opt_str(field(row, "BookIsbn")),
        language: opt_str(field(row, "BookLang")),
        published: opt_str(field(row, "BookDate")),
        ebook_status: opt_str(field(row, "Status")),
        audio_status: opt_str(field(row, "AudioStatus")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_decodes_getallbooks_row() {
        let b = parse_book(&json!({
            "BookID": "GB123", "BookName": "The Way of Kings", "AuthorID": "A1",
            "AuthorName": "Brandon Sanderson", "BookIsbn": "9780765326355",
            "Status": "Open", "audiostatus": "Wanted", "BookDate": "2010-08-31"
        }));
        assert_eq!(b.id, "GB123");
        assert_eq!(b.ebook_status.as_deref(), Some("Open"));
        assert_eq!(b.audio_status.as_deref(), Some("Wanted"));
        assert_eq!(b.isbn.as_deref(), Some("9780765326355"));
    }

    #[test]
    fn book_decodes_lowercase_findbook_row() {
        let b = parse_book(&json!({"bookid": "GB9", "bookname": "Elantris", "bookisbn": ""}));
        assert_eq!(b.id, "GB9");
        assert_eq!(b.title, "Elantris");
        assert_eq!(b.isbn, None);
    }

    #[test]
    fn author_decodes_getindex_row_without_paths() {
        let a = parse_author(&json!({
            "AuthorID": "A1", "AuthorName": "Brandon Sanderson", "Status": "Active",
            "HaveBooks": 12, "TotalBooks": "40", "AuthorImg": "cache/author/A1.jpg"
        }));
        assert_eq!(a.name, "Brandon Sanderson");
        assert_eq!(a.have_books, Some(12));
        assert_eq!(a.total_books, Some(40));
    }

    #[test]
    fn rows_accept_bare_and_data_wrapped_arrays() {
        assert_eq!(rows(&json!([1, 2])).len(), 2);
        assert_eq!(rows(&json!({"data": [1]})).len(), 1);
        assert!(rows(&json!("OK")).is_empty());
    }

    #[test]
    fn ids_reject_query_injection() {
        assert!(check_id("GB123").is_ok());
        assert!(check_id("OL123W").is_ok());
        assert!(check_id("1&cmd=shutdown").is_err());
        assert!(check_id("a:b").is_err());
        assert!(check_id("").is_err());
    }

    #[test]
    fn redact_strips_raw_encoded_and_unknown_keys() {
        let key = "k3y/with+chars";
        let r = redact(
            "a k3y/with+chars b k3y%2Fwith%2Bchars c ?APIKEY=other&cmd=x apikey=tail",
            key,
        );
        assert!(!r.contains("k3y"), "{r}");
        assert!(!r.contains("other"), "{r}");
        assert!(!r.contains("tail"), "{r}");
        assert!(r.contains("&cmd=x"), "{r}");
    }

    #[test]
    fn excerpt_redacts_the_api_key() {
        let ll = Ll::new("http://ll.test", "0123456789abcdef0123456789abcdef");
        let e = ll.excerpt("GET http://ll.test/api?apikey=0123456789abcdef0123456789abcdef&cmd=x");
        assert!(!e.contains("0123456789abcdef0123456789abcdef"), "{e}");
    }
}
