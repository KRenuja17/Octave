//! Isolated maintainer investigation, compiled only for tests. No page JS runs.
//! Fresh server-issued guest state is scoped to one HTTPS origin and two minutes.
use super::*;
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, SET_COOKIE};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const PAGE_LIMIT: usize = 4 * 1024 * 1024;
const CONFIG_LIMIT: usize = 1024 * 1024;
const COOKIE_LIMIT: usize = 8192;
const GUEST_NAMES: &[&str] = &[
    "YSC",
    "VISITOR_INFO1_LIVE",
    "VISITOR_PRIVACY_METADATA",
    "PREF",
    "SOCS",
];
fn valid_visitor(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 2048
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-%=+/".contains(&b))
}

// Read JSON objects from the known assignment/call sites, ignoring JS strings
// and comments. This recognizes data; it never evaluates surrounding statements.
#[derive(Debug)]
enum PageIssue {
    PageLimit,
    ScriptLimit,
    ScriptShape,
    JsonEof,
    JsonOther,
    CallShape,
    ObjectLimit,
}
fn page_objects(page: &str, marker: &str, suffix: u8) -> Result<Vec<Value>, PageIssue> {
    if page.len() > PAGE_LIMIT {
        return Err(PageIssue::PageLimit);
    }
    let mut result = Vec::new();
    let mut rest = page;
    let mut scripts = 0;
    let mut candidates = 0;
    while let Some(start) = rest.find("<script") {
        scripts += 1;
        if scripts > 256 {
            return Err(PageIssue::ScriptLimit);
        }
        rest = &rest[start + 7..];
        let Some(open) = rest.find('>') else {
            return Err(PageIssue::ScriptShape);
        };
        rest = &rest[open + 1..];
        let Some(close) = rest.find("</script>") else {
            return Err(PageIssue::ScriptShape);
        };
        let script = &rest[..close];
        rest = &rest[close + 9..];
        let bytes = script.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            match bytes[at] {
                quote @ (b'\'' | b'"' | b'`') => {
                    at += 1;
                    while at < bytes.len() && bytes[at] != quote {
                        at += if bytes[at] == b'\\' { 2 } else { 1 };
                    }
                    at = (at + 1).min(bytes.len());
                    continue;
                }
                b'/' if bytes.get(at + 1) == Some(&b'/') => {
                    while at < bytes.len() && bytes[at] != b'\n' {
                        at += 1;
                    }
                    continue;
                }
                b'/' if bytes.get(at + 1) == Some(&b'*') => {
                    at = script[at + 2..]
                        .find("*/")
                        .map_or(bytes.len(), |n| at + n + 4);
                    continue;
                }
                _ => (),
            }
            let boundary = at == 0
                || !matches!(bytes[at - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$' | b'.');
            if boundary && bytes[at..].starts_with(marker.as_bytes()) {
                let mut tail = script[at + marker.len()..].trim_start();
                if !tail.starts_with(char::from(suffix)) {
                    at += marker.len();
                    continue;
                }
                tail = tail[1..].trim_start();
                // ytcfg also accepts key/value calls and computed arguments.
                // Ignore those; only a literal JSON object is bootstrap data.
                if !tail.starts_with('{') {
                    at = script.len() - tail.len();
                    continue;
                }
                candidates += 1;
                if candidates > 32 {
                    return Err(PageIssue::ObjectLimit);
                }
                let end = tail.floor_char_boundary(tail.len().min(CONFIG_LIMIT));
                let mut values =
                    serde_json::Deserializer::from_str(&tail[..end]).into_iter::<Value>();
                let value = match values.next().ok_or(PageIssue::CallShape)? {
                    Ok(value) => value,
                    Err(error) if error.is_syntax() => {
                        // Object literals containing expressions/unquoted JS keys
                        // are not JSON data and are never interpreted.
                        at += marker.len();
                        continue;
                    }
                    Err(error) => {
                        return Err(if error.is_eof() {
                            PageIssue::JsonEof
                        } else {
                            PageIssue::JsonOther
                        });
                    }
                };
                let consumed = values.byte_offset();
                if !value.is_object()
                    || (suffix == b'(' && !tail[consumed..].trim_start().starts_with(')'))
                {
                    return Err(PageIssue::CallShape);
                }
                if result.len() >= 16 {
                    return Err(PageIssue::ObjectLimit);
                }
                result.push(value);
                at = script.len() - tail.len() + consumed;
            } else {
                at += 1;
            }
        }
    }
    Ok(result)
}

#[derive(Debug)]
struct GuestConfig {
    context: Sensitive<Value>,
    visitor: Sensitive<String>,
    version: String,
    client_id: &'static str,
    logged_out_declared: bool,
}
fn config_shape(page: &str) -> Value {
    let objects = match page_objects(page, "ytcfg.set", b'(') {
        Ok(objects) => objects,
        Err(issue) => {
            return json!({"jsonObjectsParsed":false,"issue":format!("{issue:?}"),"scriptTags":page.matches("<script").count(),"markers":page.matches("ytcfg.set").count()});
        }
    };
    let mut config = serde_json::Map::new();
    for object in &objects {
        config.extend(object.as_object().unwrap().clone());
    }
    let context = config.get("INNERTUBE_CONTEXT").unwrap_or(&Value::Null);
    let visitor = context
        .pointer("/client/visitorData")
        .and_then(Value::as_str);
    let top = config.get("VISITOR_DATA").and_then(Value::as_str);
    let client = match context
        .pointer("/client/clientName")
        .and_then(Value::as_str)
    {
        Some("WEB") => "web",
        Some("WEB_REMIX") => "web_remix",
        Some(_) => "other",
        None => "missing",
    };
    json!({"jsonObjectsParsed":true,"objects":objects.len(),"contextPresent":context.is_object(),"clientKind":client,
        "versionPresent":context.pointer("/client/clientVersion").is_some(),
        "contextVisitorPresent":visitor.is_some_and(|s|!s.is_empty()),"topVisitorPresent":top.is_some_and(|s|!s.is_empty()),
        "visitorFieldsMatch":visitor.zip(top).map(|(a,b)| a==b),
        "loggedInDeclared":config.get("LOGGED_IN").and_then(Value::as_bool)})
}
impl GuestConfig {
    fn parse(page: &str, host: &str) -> Result<Self, Error> {
        let mut config = serde_json::Map::new();
        for mut object in
            page_objects(page, "ytcfg.set", b'(').map_err(|_| Error::InvalidResponse)?
        {
            for key in [
                "INNERTUBE_CONTEXT",
                "INNERTUBE_CONTEXT_CLIENT_NAME",
                "VISITOR_DATA",
                "LOGGED_IN",
            ] {
                if let Some(value) = object.as_object_mut().unwrap().remove(key) {
                    config.insert(key.into(), value);
                }
            }
        }
        if config.get("LOGGED_IN").and_then(Value::as_bool) == Some(true) {
            return Err(Error::InvalidResponse);
        }
        let mut context = config
            .get("INNERTUBE_CONTEXT")
            .cloned()
            .ok_or(Error::InvalidResponse)?;
        if context.to_string().len() > 64 * 1024 {
            return Err(Error::InvalidResponse);
        }
        let (name, client_id) = match host {
            "www.youtube.com" => ("WEB", "1"),
            "music.youtube.com" => ("WEB_REMIX", "67"),
            _ => return Err(Error::InvalidResponse),
        };
        if context
            .pointer("/client/clientName")
            .and_then(Value::as_str)
            != Some(name)
            || config
                .get("INNERTUBE_CONTEXT_CLIENT_NAME")
                .and_then(Value::as_u64)
                .is_some_and(|n| n.to_string() != client_id)
        {
            return Err(Error::InvalidResponse);
        }
        let version = context
            .pointer("/client/clientVersion")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidResponse)?;
        if version.is_empty()
            || version.len() > 64
            || !version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(Error::InvalidResponse);
        }
        let version = version.to_owned();
        let in_context = context
            .pointer("/client/visitorData")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let top = config
            .get("VISITOR_DATA")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        if in_context.zip(top).is_some_and(|(a, b)| a != b) {
            return Err(Error::InvalidResponse);
        }
        let visitor = in_context.or(top).ok_or(Error::InvalidResponse)?.to_owned();
        if !valid_visitor(&visitor) {
            return Err(Error::InvalidResponse);
        }
        context["client"]["visitorData"] = visitor.clone().into();
        Ok(Self {
            context: Sensitive::new(context),
            visitor: Sensitive::new(visitor),
            version,
            client_id,
            logged_out_declared: config.get("LOGGED_IN").and_then(Value::as_bool) == Some(false),
        })
    }
    fn accept_response(&mut self, value: &Value) -> Result<bool, Error> {
        if value
            .pointer("/responseContext/mainAppWebResponseContext/loggedOut")
            .and_then(Value::as_bool)
            == Some(false)
        {
            return Err(Error::InvalidResponse);
        }
        let Some(visitor) = value
            .pointer("/responseContext/visitorData")
            .and_then(Value::as_str)
        else {
            return Ok(false);
        };
        if !valid_visitor(visitor) {
            return Err(Error::InvalidResponse);
        }
        if visitor == self.visitor.expose() {
            return Ok(false);
        }
        let mut context = self.context.expose().clone();
        context["client"]["visitorData"] = visitor.into();
        self.context = Sensitive::new(context);
        self.visitor = Sensitive::new(visitor.into());
        Ok(true)
    }
}

// Only the modern GMT forms issued by the observed service are accepted. This
// deliberately narrow test helper rejects ambiguous/obsolete cookie dates.
fn cookie_date(text: &str) -> Option<SystemTime> {
    let normalized = text.replace('-', " ");
    let p: Vec<_> = normalized.split_whitespace().collect();
    if p.len() != 6 || p[5] != "GMT" {
        return None;
    }
    let day: u64 = p[1].parse().ok()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == p[2])?;
    let year: u64 = p[3].parse().ok()?;
    if !(1970..=9999).contains(&year) {
        return None;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day == 0 || day > months[month] {
        return None;
    }
    let clock: Vec<u64> = p[4]
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if clock.len() != 3 || clock[0] > 23 || clock[1] > 59 || clock[2] > 59 {
        return None;
    }
    let days = 365 * (year - 1970) + (year - 1) / 4 - 1969 / 4 - ((year - 1) / 100 - 1969 / 100)
        + (year - 1) / 400
        - 1969 / 400
        + months[..month].iter().sum::<u64>()
        + day
        - 1;
    UNIX_EPOCH.checked_add(Duration::from_secs(
        days * 86400 + clock[0] * 3600 + clock[1] * 60 + clock[2],
    ))
}

#[derive(Debug)]
struct GuestCookie {
    name: String,
    value: Sensitive<String>,
    expires: SystemTime,
}
#[derive(Debug)]
struct GuestCookies {
    host: &'static str,
    values: Vec<GuestCookie>,
}
impl GuestCookies {
    fn new(host: &'static str) -> Self {
        Self {
            host,
            values: Vec::new(),
        }
    }
    fn update(&mut self, headers: &HeaderMap, now: SystemTime) -> Result<(), Error> {
        if headers.get_all(SET_COOKIE).iter().count() > 32 {
            return Err(Error::InvalidResponse);
        }
        for header in headers.get_all(SET_COOKIE) {
            let Ok(text) = header.to_str() else {
                continue;
            };
            let mut parts = text.split(';');
            let Some((name, value)) = parts.next().and_then(|p| p.trim().split_once('=')) else {
                continue;
            };
            if !GUEST_NAMES.contains(&name) {
                continue;
            }
            if value.len() > 4096
                || !value.bytes().all(
                    |b| matches!(b, 0x21 | 0x23..=0x2B | 0x2D..=0x3A | 0x3C..=0x5B | 0x5D..=0x7E),
                )
            {
                return Err(Error::InvalidResponse);
            }
            let mut valid = true;
            let mut expires = now + Duration::from_secs(120);
            let mut max_age = None;
            for part in parts {
                let (key, val) = part.trim().split_once('=').unwrap_or((part.trim(), ""));
                match key.to_ascii_lowercase().as_str() {
                    "domain" => {
                        let domain = val.trim_start_matches('.');
                        valid &= domain == self.host || domain == "youtube.com";
                    }
                    "path" => valid &= val == "/",
                    "expires" => match cookie_date(val) {
                        Some(date) => expires = expires.min(date),
                        None => valid = false,
                    },
                    "max-age" => match val.parse::<i64>() {
                        Ok(n) => max_age = Some(n),
                        Err(_) => valid = false,
                    },
                    _ => (),
                }
            }
            if !valid {
                continue;
            }
            if let Some(age) = max_age {
                expires = now + Duration::from_secs(age.clamp(0, 120) as u64);
            }
            self.values
                .retain(|cookie| cookie.name != name && cookie.expires > now);
            if expires > now && !value.is_empty() {
                let bytes: usize = self
                    .values
                    .iter()
                    .map(|c| c.name.len() + c.value.expose().len() + 3)
                    .sum();
                if bytes + name.len() + value.len() + 3 > COOKIE_LIMIT {
                    return Err(Error::InvalidResponse);
                }
                self.values.push(GuestCookie {
                    name: name.into(),
                    value: Sensitive::new(value.into()),
                    expires,
                });
            }
        }
        Ok(())
    }
    fn header(&self, url: &reqwest::Url, now: SystemTime) -> Result<Option<HeaderValue>, Error> {
        if url.scheme() != "https"
            || url.host_str() != Some(self.host)
            || url.port().is_some_and(|p| p != 443)
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(Error::InvalidResponse);
        }
        let text = self
            .values
            .iter()
            .filter(|c| c.expires > now)
            .map(|c| format!("{}={}", c.name, c.value.expose()))
            .collect::<Vec<_>>()
            .join("; ");
        if text.is_empty() {
            return Ok(None);
        }
        let mut header = HeaderValue::from_str(&text).map_err(|_| Error::InvalidResponse)?;
        header.set_sensitive(true);
        Ok(Some(header))
    }
}

fn sensitive_header(text: &str) -> Result<HeaderValue, Error> {
    let mut value = HeaderValue::from_str(text).map_err(|_| Error::InvalidResponse)?;
    value.set_sensitive(true);
    Ok(value)
}

#[test]
fn page_bootstrap_parses_json_without_executing_or_accepting_quoted_decoys() {
    let page = r#"<p>ytcfg.set({"LOGGED_IN":true});</p><script>
    const decoy = 'ytcfg.set({"LOGGED_IN":true});'; // ytcfg.set({"LOGGED_IN":true});
    ytcfg.set('COMPUTED_KEY', window.something); ytcfg.set(window.config);
    ytcfg.set({computedKey: window.something});
    /* ytcfg.set({"LOGGED_IN":true}); */
    ytcfg.set({"LOGGED_IN":false,"INNERTUBE_CONTEXT_CLIENT_NAME":1});
    ytcfg.set({"VISITOR_DATA":"secret","INNERTUBE_CONTEXT":{"client":{"clientName":"WEB","clientVersion":"2.20261023.00.00","visitorData":"secret","userAgent":"data } ) ; only"}}});
    var ytInitialPlayerResponse = {"playabilityStatus":{"status":"LOGIN_REQUIRED"}};
    throw new Error('not executed');</script>"#;
    let config = GuestConfig::parse(page, "www.youtube.com").unwrap();
    assert!(config.logged_out_declared);
    assert_eq!(config.client_id, "1");
    assert_eq!(config.visitor.expose(), "secret");
    assert!(!format!("{config:?}").contains("secret"));
    assert_eq!(
        page_objects(page, "ytInitialPlayerResponse", b'=')
            .unwrap()
            .len(),
        1
    );
    for bad in [
        page.replace("\"LOGGED_IN\":false", "\"LOGGED_IN\":true"),
        page.replace(
            "\"VISITOR_DATA\":\"secret\"",
            "\"VISITOR_DATA\":\"different\"",
        ),
        page.replace("2.20261023.00.00", "unsafe/version"),
        "<script>ytcfg.set((function(){return {};})());</script>".into(),
        "x".repeat(PAGE_LIMIT + 1),
    ] {
        assert!(GuestConfig::parse(&bad, "www.youtube.com").is_err());
    }
    assert!(GuestConfig::parse(page, "attacker.invalid").is_err());
    assert!(GuestConfig::parse(page, "music.youtube.com").is_err());
    let large = page.replace(
        "\"LOGGED_IN\":false",
        &format!(
            "\"unused\":\"{}\",\"LOGGED_IN\":false",
            "x".repeat(600 * 1024)
        ),
    );
    assert!(GuestConfig::parse(&large, "www.youtube.com").is_ok());
    let truncated = format!(
        "<script>ytcfg.set({{\"unused\":\"{}\"}});</script>",
        "é".repeat(CONFIG_LIMIT / 2)
    );
    assert!(page_objects(&truncated, "ytcfg.set", b'(').is_err());
}

#[test]
fn guest_cookies_are_expiring_bounded_redacted_and_origin_scoped() {
    let now = UNIX_EPOCH + Duration::from_secs(1792771200); // 23 October 2026 UTC
    assert_eq!(cookie_date("Fri, 23 Oct 2026 16:00:00 GMT"), Some(now));
    assert_eq!(cookie_date("Fri, 23-Oct-2026 16:00:00 GMT"), Some(now));
    assert!(cookie_date("Fri, 31 Feb 2026 16:00:00 GMT").is_none());
    let mut cookies = GuestCookies::new("www.youtube.com");
    let mut headers = HeaderMap::new();
    for text in [
        "YSC=secret; Domain=.youtube.com; Path=/; Secure; HttpOnly",
        "SAPISID=account-secret; Domain=.youtube.com; Path=/",
        "PREF=foreign; Domain=attacker.invalid; Path=/",
        "SOCS=wrong-path; Path=/private",
        "VISITOR_INFO1_LIVE=expired; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
    ] {
        headers.append(SET_COOKIE, HeaderValue::from_str(text).unwrap());
    }
    cookies.update(&headers, now).unwrap();
    let target = reqwest::Url::parse("https://www.youtube.com/youtubei/v1/player").unwrap();
    let header = cookies.header(&target, now).unwrap().unwrap();
    assert_eq!(header.to_str().unwrap(), "YSC=secret");
    assert!(header.is_sensitive());
    assert!(!format!("{cookies:?} {header:?}").contains("secret"));
    for bad in [
        "http://www.youtube.com/",
        "https://www.youtube.com.attacker.invalid/",
        "https://music.youtube.com/",
        "https://user@www.youtube.com/",
    ] {
        assert!(
            cookies
                .header(&reqwest::Url::parse(bad).unwrap(), now)
                .is_err()
        );
    }
    assert!(
        cookies
            .header(&target, now + Duration::from_secs(121))
            .unwrap()
            .is_none()
    );
    let mut delete = HeaderMap::new();
    delete.insert(
        SET_COOKIE,
        HeaderValue::from_static("YSC=gone; Max-Age=0; Path=/"),
    );
    cookies.update(&delete, now).unwrap();
    assert!(cookies.header(&target, now).unwrap().is_none());
    delete.insert(
        SET_COOKIE,
        HeaderValue::from_str(&format!("YSC={}", "x".repeat(4097))).unwrap(),
    );
    assert_eq!(cookies.update(&delete, now), Err(Error::InvalidResponse));
}

#[test]
fn bounded_reply_metadata_is_redacted_and_does_not_install_cookies() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
            }
            assert!(
                !String::from_utf8(bytes)
                    .unwrap()
                    .to_ascii_lowercase()
                    .contains("cookie:")
            );
            stream.write_all(b"HTTP/1.1 200 OK\r\nSet-Cookie: YSC=secret; Path=/\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret").unwrap();
        }
    });
    let transport = transport::Transport::new().unwrap();
    for _ in 0..2 {
        let reply = transport
            .reply(
                transport.client.get(&url).build().unwrap(),
                transport::Expected::Bytes(6),
                transport::Lane::Metadata,
                Duration::from_secs(2),
                || false,
            )
            .unwrap();
        assert!(reply.headers[SET_COOKIE].is_sensitive());
        assert_eq!(reply.bytes, b"secret");
        assert!(!format!("{reply:?} {:?}", reply.headers).contains("secret"));
    }
    server.join().unwrap();
}

fn page_cohort(host: &'static str, profile: &str, ua: &str, started: Instant, budget: Duration) {
    if started.elapsed() >= budget {
        return;
    }
    let provider = Provider {
        transport: Arc::new(transport::Transport::new().unwrap()),
        cancelled: None,
    };
    let mut cookies = GuestCookies::new(host);
    let request = provider
        .transport
        .client
        .get(format!("https://{host}/?hl=en"))
        .header("User-Agent", ua)
        .header("Accept", "text/html")
        .header("Accept-Encoding", "identity")
        .header("Accept-Language", "en-US,en;q=0.9")
        .build()
        .unwrap();
    let reply = match provider.transport.reply(
        request,
        transport::Expected::Bytes(PAGE_LIMIT),
        transport::Lane::Metadata,
        transport::REQUEST_TIMEOUT.min(budget.saturating_sub(started.elapsed())),
        || false,
    ) {
        Ok(reply) => reply,
        Err(error) => {
            println!(
                "bootstrap={}",
                json!({"profile":profile,"host":host,"stage":"page","error":format!("{error:?}")})
            );
            return;
        }
    };
    let page = match std::str::from_utf8(&reply.bytes) {
        Ok(page) => page,
        Err(_) => {
            println!("bootstrap_error=InvalidUtf8");
            return;
        }
    };
    let mut config = match GuestConfig::parse(page, host) {
        Ok(config) => config,
        Err(error) => {
            println!(
                "bootstrap={}",
                json!({"profile":profile,"host":host,"stage":"config","pageBytes":page.len(),"shape":config_shape(page),"error":format!("{error:?}")})
            );
            return;
        }
    };
    cookies.update(&reply.headers, SystemTime::now()).unwrap();
    println!(
        "bootstrap={}",
        json!({"profile":profile,"host":host,"stage":"ready","pageBytes":page.len(),"guestCookies":cookies.values.len(),"loggedOutDeclared":config.logged_out_declared,"visitorPresent":true,"elapsedMs":started.elapsed().as_millis()})
    );
    drop(reply);
    for raw_id in [
        "hpSrLjc5SMs",
        "6Zt4P1scOwE",
        "fJ9rUzIMcZQ",
        "kJQP7kiw5Fk",
        "dQw4w9WgXcQ",
    ] {
        if started.elapsed() >= budget {
            println!("bootstrap_budget_exhausted=true");
            return;
        }
        let id = TrackId(raw_id.into());
        let (mut context, client_id, version) = if profile == "guest_visionos" {
            (
                json!({"client":{"clientName":"VISIONOS","clientVersion":"1.02","deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471","hl":"en","visitorData":config.visitor.expose()}}),
                "101",
                "1.02",
            )
        } else {
            (
                config.context.expose().clone(),
                config.client_id,
                config.version.as_str(),
            )
        };
        context["client"]["userAgent"] = ua.into();
        let mut request = provider
            .post_request(
                &format!("https://{host}/youtubei/v1/player?prettyPrint=false"),
                json!({"context":context,"videoId":raw_id}),
                client_id,
                version,
                ua,
            )
            .unwrap();
        request.headers_mut().insert(
            reqwest::header::ORIGIN,
            HeaderValue::from_str(&format!("https://{host}")).unwrap(),
        );
        request.headers_mut().insert(
            "X-Goog-Visitor-Id",
            sensitive_header(config.visitor.expose()).unwrap(),
        );
        request.headers_mut().insert(
            "X-Youtube-Bootstrap-Logged-In",
            HeaderValue::from_static("false"),
        );
        let cookie = cookies.header(request.url(), SystemTime::now()).unwrap();
        let sent_cookies = cookie.is_some();
        if let Some(cookie) = cookie {
            request.headers_mut().insert(COOKIE, cookie);
        }
        // The response-authoritative visitor must be identical in both places.
        assert!(
            request.headers()["X-Goog-Visitor-Id"].to_str().unwrap()
                == context
                    .pointer("/client/visitorData")
                    .and_then(Value::as_str)
                    .unwrap(),
            "Guest visitor header and context diverged"
        );
        let attempt = Instant::now();
        let result = provider.transport.reply(
            request,
            transport::Expected::Bytes(JSON_LIMIT as usize),
            transport::Lane::Media,
            transport::REQUEST_TIMEOUT.min(budget.saturating_sub(started.elapsed())),
            || false,
        );
        let result = match result {
            Ok(reply) => match serde_json::from_slice::<Value>(&reply.bytes) {
                Ok(value) => {
                    let mut summary = super::compatibility_tests::summary(&id, &value);
                    summary["responseVisitorMatches"] = value
                        .pointer("/responseContext/visitorData")
                        .and_then(Value::as_str)
                        .map(|s| s == config.visitor.expose())
                        .into();
                    match config.accept_response(&value) {
                        Ok(updated) => {
                            summary["visitorUpdated"] = updated.into();
                            cookies.update(&reply.headers, SystemTime::now()).unwrap();
                            summary
                        }
                        Err(error) => {
                            println!(
                                "bootstrap={}",
                                json!({"profile":profile,"stage":"response_state","error":format!("{error:?}")})
                            );
                            return;
                        }
                    }
                }
                Err(_) => json!({"error":"InvalidResponse"}),
            },
            Err(error) => json!({"error":format!("{error:?}")}),
        };
        println!(
            "anonymous={}",
            json!({"videoId":raw_id,"host":host,"profile":profile,"sentGuestCookies":sent_cookies,"elapsedMs":attempt.elapsed().as_millis(),"result":result})
        );
    }
}

#[test]
fn server_visitor_updates_are_coherent_and_never_generate_new_identities() {
    let page = r#"<script>ytcfg.set({"LOGGED_IN":false,"INNERTUBE_CONTEXT":{"client":{"clientName":"WEB","clientVersion":"2.20261023","visitorData":"initial-secret"}}});</script>"#;
    let mut config = GuestConfig::parse(page, "www.youtube.com").unwrap();
    assert!(
        !config
            .accept_response(&json!({"playabilityStatus":{"status":"LOGIN_REQUIRED"}}))
            .unwrap()
    );
    assert!(
        config
            .accept_response(&json!({"responseContext":{"visitorData":"updated-secret"}}))
            .unwrap()
    );
    assert_eq!(config.visitor.expose(), "updated-secret");
    assert_eq!(
        config
            .context
            .expose()
            .pointer("/client/visitorData")
            .and_then(Value::as_str),
        Some("updated-secret")
    );
    assert!(
        !config
            .accept_response(&json!({"responseContext":{"visitorData":"updated-secret"}}))
            .unwrap()
    );
    for bad in [
        json!({"responseContext":{"visitorData":"injected; value"}}),
        json!({"responseContext":{"visitorData":""}}),
        json!({"responseContext":{"visitorData":"account-secret","mainAppWebResponseContext":{"loggedOut":false}}}),
    ] {
        assert_eq!(config.accept_response(&bad), Err(Error::InvalidResponse));
        assert_eq!(config.visitor.expose(), "updated-secret");
    }
    assert!(!format!("{config:?}").contains("secret"));
}

#[test]
#[ignore = "Explicit anonymous page/session bootstrap cohort; metadata only, no sign-in or JS"]
fn live_anonymous_page_bootstrap() {
    let started = Instant::now();
    let budget = Duration::from_secs(100);
    // Each predeclared client has one fresh, isolated in-memory session. There
    // is no re-bootstrap/retry loop in response to a bot check.
    for (host, profile, ua) in [
        ("www.youtube.com", "page_web", WEB_UA),
        ("www.youtube.com", "guest_visionos", PLAYER_UA),
        ("music.youtube.com", "page_remix", WEB_UA),
    ] {
        page_cohort(host, profile, ua, started, budget);
    }
}

fn inline_player(page: &str, id: &TrackId) -> Result<Value, Error> {
    let mut players = Vec::new();
    for marker in [
        "ytInitialPlayerResponse",
        "window.ytInitialPlayerResponse",
        "window[\"ytInitialPlayerResponse\"]",
        "window['ytInitialPlayerResponse']",
    ] {
        players.extend(page_objects(page, marker, b'=').map_err(|_| Error::InvalidResponse)?);
    }
    if players.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    let value = &players[0];
    if value
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        == Some("OK")
        && value
            .pointer("/videoDetails/videoId")
            .and_then(Value::as_str)
            != Some(&id.0)
    {
        return Err(Error::InvalidResponse);
    }
    Ok(super::compatibility_tests::summary(id, value))
}

#[test]
fn inline_watch_player_is_data_only_and_must_match_the_requested_recording() {
    let id = TrackId("abcdefghijk".into());
    for marker in [
        "var ytInitialPlayerResponse",
        "window.ytInitialPlayerResponse",
        "window[\"ytInitialPlayerResponse\"]",
    ] {
        let page = format!(
            r#"<script>{marker} = {{"playabilityStatus":{{"status":"LOGIN_REQUIRED","reason":"not a bot secret"}}}};</script>"#
        );
        let result = inline_player(&page, &id).unwrap();
        assert_eq!(result["resolver"], "BotCheckRequired");
        assert!(!result.to_string().contains("secret"));
    }
    assert!(inline_player("<script>var ytInitialPlayerResponse = {\"playabilityStatus\":{\"status\":\"OK\"},\"videoDetails\":{\"videoId\":\"wrong-id\"}};</script>", &id).is_err());
}

#[test]
#[ignore = "Explicit two-recording anonymous watch-page bootstrap; no sign-in, JS, or audio"]
fn live_anonymous_watch_bootstrap() {
    let provider = Provider {
        transport: Arc::new(transport::Transport::new().unwrap()),
        cancelled: None,
    };
    let started = Instant::now();
    let budget = Duration::from_secs(55);
    let mut cookies = GuestCookies::new("www.youtube.com");
    for raw_id in ["hpSrLjc5SMs", "dQw4w9WgXcQ"] {
        if started.elapsed() >= budget {
            println!("watch_budget_exhausted=true");
            return;
        }
        let id = TrackId(raw_id.into());
        let mut request = provider
            .transport
            .client
            .get(format!("https://www.youtube.com/watch?v={raw_id}&hl=en"))
            .header("User-Agent", PLAYER_UA)
            .header("Accept", "text/html")
            .header("Accept-Encoding", "identity")
            .header("Accept-Language", "en-US,en;q=0.9")
            .build()
            .unwrap();
        if let Some(cookie) = cookies.header(request.url(), SystemTime::now()).unwrap() {
            request.headers_mut().insert(COOKIE, cookie);
        }
        let reply = match provider.transport.reply(
            request,
            transport::Expected::Bytes(PAGE_LIMIT),
            transport::Lane::Metadata,
            transport::REQUEST_TIMEOUT.min(budget.saturating_sub(started.elapsed())),
            || false,
        ) {
            Ok(reply) => reply,
            Err(error) => {
                println!(
                    "watch={}",
                    json!({"videoId":raw_id,"stage":"page","error":format!("{error:?}")})
                );
                return;
            }
        };
        let page = std::str::from_utf8(&reply.bytes).unwrap();
        let config = match GuestConfig::parse(page, "www.youtube.com") {
            Ok(config) => config,
            Err(error) => {
                println!(
                    "watch={}",
                    json!({"videoId":raw_id,"stage":"config","shape":config_shape(page),"error":format!("{error:?}")})
                );
                return;
            }
        };
        cookies.update(&reply.headers, SystemTime::now()).unwrap();
        let inline = match inline_player(page, &id) {
            Ok(result) => result,
            Err(error) => json!({"error":format!("{error:?}")}),
        };
        println!(
            "watch={}",
            json!({"videoId":raw_id,"stage":"inline_player","pageBytes":page.len(),"guestCookies":cookies.values.len(),"loggedOutDeclared":config.logged_out_declared,"result":inline})
        );
        drop(reply);
        if started.elapsed() >= budget {
            return;
        }
        let context = json!({"client":{"clientName":"VISIONOS","clientVersion":"1.02","deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471","hl":"en","userAgent":PLAYER_UA,"visitorData":config.visitor.expose()}});
        let mut request = provider
            .post_request(
                "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
                json!({"context":context,"videoId":raw_id}),
                "101",
                "1.02",
                PLAYER_UA,
            )
            .unwrap();
        request.headers_mut().insert(
            reqwest::header::ORIGIN,
            HeaderValue::from_static("https://www.youtube.com"),
        );
        if let Some(cookie) = cookies.header(request.url(), SystemTime::now()).unwrap() {
            request.headers_mut().insert(COOKIE, cookie);
        }
        let result = provider.transport.reply(
            request,
            transport::Expected::Bytes(JSON_LIMIT as usize),
            transport::Lane::Media,
            transport::REQUEST_TIMEOUT.min(budget.saturating_sub(started.elapsed())),
            || false,
        );
        let result = match result {
            Ok(reply) => {
                cookies.update(&reply.headers, SystemTime::now()).unwrap();
                match serde_json::from_slice::<Value>(&reply.bytes) {
                    Ok(value) => super::compatibility_tests::summary(&id, &value),
                    Err(_) => json!({"error":"InvalidResponse"}),
                }
            }
            Err(error) => json!({"error":format!("{error:?}")}),
        };
        println!(
            "watch={}",
            json!({"videoId":raw_id,"stage":"after_watch_visionos","result":result})
        );
    }
}
