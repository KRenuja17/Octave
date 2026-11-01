//! Server-issued identities and bounded cookie rotation. No provider text in errors.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub channel_id: Option<String>,
    pub handle: Option<String>,
    pub premium: Option<bool>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub selected: bool,
}
impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Identity([REDACTED])")
    }
}
impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Channel([REDACTED])")
    }
}
fn find<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut pending = vec![value];
    for _ in 0..16384 {
        let next = pending.pop()?;
        if let Some(found) = next.get(key) {
            return Some(found);
        }
        match next {
            Value::Object(map) => pending.extend(map.values()),
            Value::Array(items) => pending.extend(items),
            _ => (),
        }
    }
    None
}
fn selector(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn display(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| text(value))
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}
fn normalized_id(raw: &str) -> Option<&str> {
    raw.split("||")
        .map(str::trim)
        .find(|s| !s.is_empty())
        .filter(|s| selector(s))
}
fn response_id(value: &Value) -> Option<&str> {
    let context = value.pointer("/responseContext/mainAppWebResponseContext")?;
    context
        .get("datasyncId")
        .or_else(|| context.get("dataSyncId"))?
        .as_str()
        .and_then(normalized_id)
}
pub(super) fn parse(value: &Value) -> Result<Identity, Error> {
    parse_with_id(value, None)
}
fn parse_with_id(value: &Value, fallback: Option<&str>) -> Result<Identity, Error> {
    let header = find(value, "activeAccountHeaderRenderer").ok_or(Error::Expired)?;
    let name = display(&header["accountName"]);
    if name.is_empty() {
        return Err(Error::AccountNameMissing);
    }
    let channel_id = header
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("UC") && selector(id))
        .map(str::to_owned);
    let id = response_id(value)
        .or(fallback.filter(|id| selector(id)))
        .map(str::to_owned)
        .ok_or(Error::IdentityMissing)?;
    let handle = display(&header["channelHandle"]);
    Ok(Identity {
        id,
        name,
        channel_id,
        handle: (!handle.is_empty()).then_some(handle),
        premium: find(value, "isPremiumSubscriber")
            .or_else(|| find(value, "isPremium"))
            .and_then(Value::as_bool),
    })
}
fn selected_list_id(value: &Value) -> Result<String, Error> {
    if let Some(id) = response_id(value) {
        return Ok(id.to_owned());
    }
    let items = channels(value);
    let selected: Vec<_> = items.iter().filter(|c| c.selected).collect();
    if selected.len() == 1 {
        return Ok(selected[0].id.clone());
    }
    if selected.is_empty() && items.len() == 1 {
        return Ok(items[0].id.clone());
    }
    Err(Error::IdentityMissing)
}
pub(super) fn channels(value: &Value) -> Vec<Channel> {
    let mut output = Vec::new();
    let mut pending = vec![value];
    for _ in 0..16384 {
        let Some(next) = pending.pop() else {
            break;
        };
        if let Some(item) = next.get("accountItem") {
            if item["isDisabled"] == true || item["hasChannel"] == false {
                continue;
            }
            let tokens = item
                .pointer("/serviceEndpoint/selectActiveIdentityEndpoint/supportedTokens")
                .and_then(Value::as_array);
            let token = tokens
                .and_then(|tokens| {
                    tokens.iter().take(32).find_map(|token| {
                        let data = token
                            .get("datasyncIdToken")
                            .or_else(|| token.get("dataSyncIdToken"))?;
                        ["datasyncIdToken", "datasyncId", "dataSyncId"]
                            .iter()
                            .find_map(|key| {
                                data.get(*key)
                                    .and_then(Value::as_str)
                                    .and_then(normalized_id)
                            })
                    })
                })
                .or_else(|| {
                    tokens.and_then(|tokens| {
                        tokens.iter().take(32).find_map(|token| {
                            token
                                .pointer("/pageIdToken/pageId")
                                .and_then(Value::as_str)
                                .filter(|s| selector(s))
                        })
                    })
                })
                .or_else(|| {
                    (item["isSelected"] == true)
                        .then(|| response_id(value))
                        .flatten()
                });
            let name = display(&item["accountName"]);
            if let Some(id) = token.filter(|id| selector(id))
                && !name.is_empty()
                && !output.iter().any(|c: &Channel| c.id == id)
                && output.len() < 100
            {
                output.push(Channel {
                    id: id.into(),
                    name,
                    selected: item["isSelected"] == true,
                });
            }
        }
        match next {
            Value::Object(m) => pending.extend(m.values()),
            Value::Array(a) => pending.extend(a),
            _ => (),
        }
    }
    output
}
pub(super) fn rotate(session: &mut Session, updates: &[Zeroizing<String>]) -> Result<(), Error> {
    if updates.is_empty() {
        return Ok(());
    }
    let mut jar = BTreeMap::new();
    for (name, value) in session
        .cookie
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
    {
        jar.insert(name.to_owned(), Zeroizing::new(value.to_owned()));
    }
    for update in updates {
        let mut parts = update.split(';');
        let Some((name, value)) = parts.next().and_then(|p| p.trim().split_once('=')) else {
            continue;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || value
                .bytes()
                .any(|b| !(0x21..=0x7e).contains(&b) || b == b';')
        {
            continue;
        }
        let attrs: Vec<_> = parts.map(str::trim).collect();
        if attrs.iter().any(|a| {
            a.split_once('=').is_some_and(|(k, v)| {
                k.eq_ignore_ascii_case("domain")
                    && !matches!(
                        v.trim_start_matches('.').to_ascii_lowercase().as_str(),
                        "youtube.com" | "music.youtube.com"
                    )
            })
        }) {
            continue;
        }
        if attrs.iter().any(|a| {
            a.split_once('=')
                .is_some_and(|(k, v)| k.eq_ignore_ascii_case("path") && v != "/")
        }) {
            continue;
        }
        let delete = value.is_empty()
            || attrs.iter().any(|a| {
                a.split_once('=').is_some_and(|(k, v)| {
                    k.eq_ignore_ascii_case("max-age") && v.parse::<i64>().is_ok_and(|v| v <= 0)
                })
            });
        if delete {
            jar.remove(name);
        } else {
            jar.insert(name.into(), Zeroizing::new(value.into()));
        }
    }
    let mut cookie = Zeroizing::new(String::new());
    for (name, value) in &jar {
        if !cookie.is_empty() {
            cookie.push_str("; ");
        }
        cookie.push_str(name);
        cookie.push('=');
        cookie.push_str(value);
    }
    if cookie.len() > 16384 {
        return Err(Error::InvalidSession);
    }
    let sapisid = ["SAPISID", "__Secure-3PAPISID", "__Secure-1PAPISID"]
        .iter()
        .find_map(|name| jar.get(*name))
        .cloned()
        .ok_or(Error::Expired)?;
    if session.cookie.as_str() != cookie.as_str() {
        session.revision = session.revision.saturating_add(1);
    }
    session.cookie = cookie;
    session.sapisid = sapisid;
    Ok(())
}

impl Account {
    pub fn session_revision(&self) -> u64 {
        self.credentials.session.lock().unwrap().revision
    }
    pub fn is_active(&self) -> bool {
        self.active().is_ok()
    }
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.credentials, &other.credentials)
    }
    pub fn restore_selection(
        cookie: String,
        index: u8,
        expected: &str,
        delegated: Option<&str>,
    ) -> Result<Self, Error> {
        if !selector(expected) || delegated.is_some_and(|id| !selector(id)) {
            return Err(Error::InvalidSession);
        }
        let account = Self::new(cookie, index)?;
        account.credentials.session.lock().unwrap().delegated = delegated.map(str::to_owned);
        Ok(account)
    }
    pub fn delegated_id(&self) -> Option<String> {
        self.credentials.session.lock().unwrap().delegated.clone()
    }
    /// Called only by native account/vault work; never return this to a web page.
    pub fn session_cookie(&self) -> Result<Zeroizing<String>, Error> {
        self.active()?;
        Ok(self.credentials.session.lock().unwrap().cookie.clone())
    }
    pub fn account_index(&self) -> u8 {
        self.credentials.index
    }
    pub fn selected_id(&self) -> Option<String> {
        self.credentials
            .session
            .lock()
            .unwrap()
            .identity
            .as_ref()
            .map(|i| i.id.clone())
    }
    pub fn verify(&self) -> Result<Identity, Error> {
        let response = self.request("account/account_menu", json!({}))?;
        let parsed = match parse(&response) {
            Err(Error::IdentityMissing) => {
                let list = self.request("account/accounts_list", json!({}))?;
                let selected = selected_list_id(&list)?;
                parse_with_id(&response, Some(&selected))
            }
            result => result,
        };
        let identity = parsed.inspect_err(|error| {
            if *error == Error::Expired {
                self.disconnect();
            }
        })?;
        self.accept_identity(identity)
    }
    fn accept_identity(&self, identity: Identity) -> Result<Identity, Error> {
        let mut session = self.credentials.session.lock().unwrap();
        if session
            .identity
            .as_ref()
            .is_some_and(|old| old.id != identity.id)
            || session
                .delegated
                .as_ref()
                .is_some_and(|id| *id != identity.id)
        {
            self.disconnect();
            return Err(Error::InvalidSession);
        }
        session.identity = Some(identity.clone());
        Ok(identity)
    }
    pub fn channels(&self) -> Result<Vec<Channel>, Error> {
        let response = self.request("account/accounts_list", json!({}))?;
        Ok(channels(&response))
    }
    pub fn select_channel(&self, channel: &Channel) -> Result<(Self, Identity), Error> {
        if !selector(&channel.id) {
            return Err(Error::InvalidSession);
        }
        let next = Self::new(self.session_cookie()?.to_string(), self.credentials.index)?;
        next.credentials.session.lock().unwrap().delegated = Some(channel.id.clone());
        let identity = next.verify()?;
        Ok((next, identity))
    }
    /// Authenticated resolution is explicit, with no anonymous fallback or
    /// credentials attached to the signed CDN URL. Unsupported/DRM stays typed.
    pub fn resolve_cancellable(
        &self,
        id: &TrackId,
        cancelled: impl Fn() -> bool,
    ) -> Result<crate::ResolvedStream, crate::Error> {
        if cancelled() {
            return Err(crate::Error::Cancelled);
        }
        if !crate::valid_id(&id.0) {
            return Err(crate::Error::InvalidTrack);
        }
        let provider = crate::Provider::new()?;
        let cancelled = || cancelled() || self.active().is_err();
        let player = crate::signature::player(&provider, cancelled)?;
        let response=self.request_with("player",json!({"videoId":id.0,"contentCheckOk":true,"racyCheckOk":true,"playbackContext":{"contentPlaybackContext":{"html5Preference":"HTML5_PREF_WANTS", "signatureTimestamp":player.timestamp}}}), cancelled)
            .map_err(|e|match e{Error::Expired|Error::InvalidSession=>crate::Error::SignInRequired,Error::Cancelled=>crate::Error::Cancelled,_=>crate::Error::Network})?;
        if cancelled() {
            return Err(crate::Error::Cancelled);
        }
        self.active().map_err(|_| crate::Error::Cancelled)?;
        crate::signature::resolve_response(id, response, |n, signature| {
            crate::signature::decode(&player, n, signature, cancelled)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn menu(id: &str) -> Value {
        json!({"responseContext":{"mainAppWebResponseContext":{"datasyncId":format!("{id}||device")}},"actions":[{"activeAccountHeaderRenderer":{"accountName":{"simpleText":"Synthetic channel"},"channelHandle":{"simpleText":"@fixture"}}}]})
    }
    #[test]
    fn identity_requires_authenticated_header_and_stable_server_id() {
        assert!(parse(&json!({"responseContext":{}})).is_err());
        let i = parse(&menu("fixture-id")).unwrap();
        assert_eq!(i.id, "fixture-id");
        assert_eq!(i.premium, None);
        assert!(!format!("{i:?}").contains("Synthetic"));
        assert!(parse(&menu("bad\nidentifier")).is_err());
    }
    #[test]
    fn missing_menu_identity_only_uses_an_unambiguous_server_list_selector() {
        let header = json!({"activeAccountHeaderRenderer":{"accountName":{"simpleText":"Synthetic"},"navigationEndpoint":{"browseEndpoint":{"browseId":"UCnot-a-selector"}}}});
        assert_eq!(parse(&header), Err(Error::IdentityMissing));
        let row = |id: &str, selected| json!({"accountItem":{"accountName":"Fixture", "isSelected":selected,"serviceEndpoint":{"selectActiveIdentityEndpoint":{"supportedTokens":[{"pageIdToken":{"pageId":id}}]}}}});
        let list = json!([row("fixture-one", false), row("fixture-two", true)]);
        let selected = selected_list_id(&list).unwrap();
        assert_eq!(selected, "fixture-two");
        assert_eq!(
            parse_with_id(&header, Some(&selected)).unwrap().id,
            "fixture-two"
        );
        assert_eq!(
            selected_list_id(&json!([
                row("fixture-one", false),
                row("fixture-two", false)
            ])),
            Err(Error::IdentityMissing)
        );
        assert_eq!(
            selected_list_id(&json!([row("fixture-one", true), row("fixture-two", true)])),
            Err(Error::IdentityMissing)
        );
        assert_eq!(
            account_json(b")]}'\n{\"responseContext\":{}}").unwrap(),
            json!({"responseContext":{}})
        );
        assert_eq!(
            account_json(b"<html>{\"responseContext\":{}}</html>"),
            Err(Error::MalformedResponse)
        );
    }
    #[test]
    fn server_selector_variants_and_plain_names_preserve_identity() {
        let response = json!({"responseContext":{"mainAppWebResponseContext":{"dataSyncId":"||fixture-selected"}}, "activeAccountHeaderRenderer":{"accountName":"Synthetic"}});
        assert_eq!(parse(&response).unwrap().id, "fixture-selected");
        let selected = json!({"responseContext":{"mainAppWebResponseContext":{"datasyncId":"fixture-current||device"}},"items":[
            {"accountItem":{"accountName":"Current", "isSelected":true}},
            {"accountItem":{"accountName":"Alternate", "serviceEndpoint":{"selectActiveIdentityEndpoint":{"supportedTokens":[{"dataSyncIdToken":{"dataSyncId":"fixture-other||device"}}]}}}},
            {"accountItem":{"accountName":"Disabled", "isDisabled":true,"serviceEndpoint":{"selectActiveIdentityEndpoint":{"supportedTokens":[{"pageIdToken":{"pageId":"disabled"}}]}}}}
        ]});
        let items = channels(&selected);
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|c| c.id == "fixture-other"));
        assert!(items.iter().any(|c| c.id == "fixture-current"));
    }
    #[test]
    fn restore_does_not_invent_a_delegated_selector_for_a_default_account() {
        let default =
            Account::restore_selection("SAPISID=synthetic".into(), 0, "UCfixture", None).unwrap();
        assert_eq!(default.delegated_id(), None);
        let selected = Account::restore_selection(
            "SAPISID=synthetic".into(),
            0,
            "fixture-other",
            Some("fixture-other"),
        )
        .unwrap();
        assert_eq!(selected.delegated_id().as_deref(), Some("fixture-other"));
        assert!(
            Account::restore_selection(
                "SAPISID=synthetic".into(),
                0,
                "fixture",
                Some("bad\nselector")
            )
            .is_err()
        );
    }
    #[test]
    fn channel_picker_only_uses_bounded_server_tokens() {
        let a = json!({"accountItem":{"accountName":{"simpleText":"Fixture"},"serviceEndpoint":{"selectActiveIdentityEndpoint":{"supportedTokens":[{"datasyncIdToken":{"datasyncIdToken":"id-a"}}]}}}});
        let out = channels(
            &json!([a.clone(),a,{"accountItem":{"accountName":{"simpleText":"No selector"}}}]),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "id-a");
    }
    #[test]
    fn rotation_is_origin_scoped_and_expiry_never_installs_a_partial_session() {
        let account = Account::new("SAPISID=before; SID=keep".into(), 0).unwrap();
        let mut session = account.credentials.session.lock().unwrap();
        rotate(
            &mut session,
            &[
                Zeroizing::new("SAPISID=after; Domain=.youtube.com; Path=/; Secure".into()),
                Zeroizing::new("SID=wrong; Domain=example.com".into()),
            ],
        )
        .unwrap();
        assert!(session.cookie.contains("SAPISID=after"));
        assert!(session.cookie.contains("SID=keep"));
        assert_eq!(
            rotate(
                &mut session,
                &[Zeroizing::new(
                    "SAPISID=; Max-Age=0; Domain=.youtube.com".into()
                )]
            ),
            Err(Error::Expired)
        );
        assert!(session.cookie.contains("SAPISID=after"));
    }
    #[test]
    fn refresh_cannot_silently_change_the_selected_account() {
        let account = Account::new("SAPISID=synthetic".into(), 0).unwrap();
        account
            .accept_identity(parse(&menu("first-id")).unwrap())
            .unwrap();
        let clone = account.clone();
        assert_eq!(
            account.accept_identity(parse(&menu("second-id")).unwrap()),
            Err(Error::InvalidSession)
        );
        assert_eq!(clone.active(), Err(Error::Cancelled));
    }
}
