//! Maintainer-only unsigned investigation. Never prints raw service data or URLs.
//! Client descriptions checked against yt-dlp's public client definitions:
//! https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/youtube/_base.py
//! Independently authored request/summary code; no extractor or script runtime.
use super::*;
use std::time::Instant;

pub(super) fn summary(id: &TrackId, value: &Value) -> Value {
    let status = match value
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
    {
        Some("OK") => "ok",
        Some("LOGIN_REQUIRED") => "login_required",
        Some("UNPLAYABLE") => "unplayable",
        Some("ERROR") => "error",
        Some("CONTENT_CHECK_REQUIRED") => "content_check_required",
        Some("AGE_CHECK_REQUIRED") => "age_check_required",
        Some(_) => "other",
        None => "missing",
    };
    let reason = value
        .pointer("/playabilityStatus/reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let category = if reason.contains("not a bot") || reason.contains("automated") {
        "bot_check"
    } else if reason.contains("age") || reason.contains("inappropriate") {
        "age_check"
    } else if reason.contains("premium") || reason.contains("members") {
        "subscription"
    } else if reason.contains("country") || reason.contains("region") {
        "region"
    } else if reason.contains("private") {
        "private"
    } else if reason.contains("sign in") || reason.contains("sign-in") {
        "sign_in"
    } else if reason.is_empty() {
        "none"
    } else {
        "other"
    };
    let formats = value
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let audio = |v: &&Value| {
        v["mimeType"]
            .as_str()
            .is_some_and(|mime| mime.starts_with("audio/"))
    };
    json!({
        "status": status, "reasonCategory": category,
        "directAudio": formats.iter().filter(audio).filter(|v| v["url"].is_string()).count(),
        "cipheredAudio": formats.iter().filter(audio).filter(|v| v["signatureCipher"].is_string() || v["cipher"].is_string()).count(),
        "drmAudio": formats.iter().filter(audio).filter(|v| v["drmFamilies"].as_array().is_some_and(|values| !values.is_empty())).count(),
        "hls": value.pointer("/streamingData/hlsManifestUrl").is_some(),
        "dash": value.pointer("/streamingData/dashManifestUrl").is_some(),
        "sabr": value.pointer("/streamingData/serverAbrStreamingUrl").is_some(),
        "identityMatches": value.pointer("/videoDetails/videoId").and_then(Value::as_str) == Some(&id.0),
        "resolver": match parse_stream(id, value) { Ok(_) => "direct_aac".to_owned(), Err(error) => format!("{error:?}") }
    })
}

#[test]
fn compatibility_summary_never_exposes_service_text_or_media_urls() {
    let id = TrackId("abcdefghijk".into());
    let value = json!({"playabilityStatus":{"status":"LOGIN_REQUIRED", "reason":"Sign in to confirm you are not a bot. secret"},
        "streamingData":{"hlsManifestUrl":"https://secret.invalid/token", "adaptiveFormats":[
            {"mimeType":"audio/mp4", "signatureCipher":"secret"}]}});
    let result = summary(&id, &value);
    assert_eq!(result["status"], "login_required");
    assert_eq!(result["reasonCategory"], "bot_check");
    assert_eq!(result["cipheredAudio"], 1);
    assert_eq!(result["resolver"], "BotCheckRequired");
    assert!(!result.to_string().contains("secret"));
    assert!(!result.to_string().contains("https"));
}

#[test]
fn compatibility_summary_counts_drm_without_exposing_license_data() {
    let id = TrackId("abcdefghijk".into());
    let value = json!({"playabilityStatus":{"status":"OK"},
    "streamingData":{"licenseInfos":[{"licenseUrl":"https://secret.invalid/license"}],
    "adaptiveFormats":[
        {"mimeType":"audio/mp4", "drmFamilies":["secret"], "signatureCipher":"secret"},
        {"mimeType":"audio/webm", "drmFamilies":[]},
        {"mimeType":"video/mp4", "drmFamilies":["secret"]}
    ]}});
    let result = summary(&id, &value);
    assert_eq!(result["drmAudio"], 1);
    assert_eq!(result["cipheredAudio"], 1);
    assert_eq!(result["directAudio"], 0);
    assert_eq!(result["resolver"], "UnsupportedStream");
    assert!(!result.to_string().contains("secret"));
    assert!(!result.to_string().contains("https"));
}

#[test]
#[ignore = "Explicit bounded unsigned profile matrix; metadata only, no account/audio/URLs logged"]
fn live_unsigned_profile_compatibility() {
    let provider = Provider::new().unwrap();
    let started = Instant::now();
    let budget = Duration::from_secs(85);
    let visitor = provider.post("https://www.youtube.com/youtubei/v1/visitor_id?prettyPrint=false",
        json!({"context":{"client":{"clientName":"WEB","clientVersion":"2.20260707.00.00","hl":"en"}}}),
        "1", "2.20260707.00.00", WEB_UA).unwrap();
    let visitor = visitor
        .pointer("/responseContext/visitorData")
        .and_then(Value::as_str)
        .unwrap();
    // Explicit client descriptions only. No cookies, tokens, challenges, age/content
    // acknowledgements, proxies, remote config or script execution are added.
    let profiles = [
        (
            "visionos",
            "101",
            "1.02",
            PLAYER_UA,
            json!({"clientName":"VISIONOS","deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471"}),
        ),
        (
            "android",
            "3",
            "21.26.364",
            "com.google.android.youtube/21.26.364 (Linux; U; Android 11) gzip",
            json!({"clientName":"ANDROID","androidSdkVersion":30,"osName":"Android","osVersion":"11"}),
        ),
        (
            "ios",
            "5",
            "21.26.4",
            "com.google.ios.youtube/21.26.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
            json!({"clientName":"IOS","deviceMake":"Apple","deviceModel":"iPhone16,2","osName":"iPhone","osVersion":"18.3.2.22D82"}),
        ),
        (
            "web_remix",
            "67",
            "1.20260707.12.00",
            WEB_UA,
            json!({"clientName":"WEB_REMIX"}),
        ),
    ];
    for raw_id in [
        "hpSrLjc5SMs",
        "6Zt4P1scOwE",
        "dQw4w9WgXcQ",
        "fJ9rUzIMcZQ",
        "kJQP7kiw5Fk",
    ] {
        let id = TrackId(raw_id.into());
        for (name, client_id, version, ua, mut client) in profiles.clone() {
            if started.elapsed() >= budget {
                println!("compatibility_budget_exhausted=true");
                return;
            }
            client["clientVersion"] = version.into();
            client["hl"] = "en".into();
            client["visitorData"] = visitor.into();
            if name != "visionos" {
                client["userAgent"] = ua.into();
            }
            let url = if name == "web_remix" {
                "https://music.youtube.com/youtubei/v1/player?prettyPrint=false"
            } else {
                "https://www.youtube.com/youtubei/v1/player?prettyPrint=false"
            };
            let request = provider
                .post_request(
                    url,
                    json!({"context":{"client":client},"videoId":raw_id}),
                    client_id,
                    version,
                    ua,
                )
                .unwrap();
            let attempt = Instant::now();
            let result = provider
                .transport
                .bytes(
                    request,
                    transport::Expected::Bytes(JSON_LIMIT as usize),
                    transport::Lane::Media,
                    transport::REQUEST_TIMEOUT.min(budget.saturating_sub(started.elapsed())),
                    || false,
                )
                .and_then(|bytes| {
                    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)
                });
            let result = match result {
                Ok(value) => summary(&id, &value),
                Err(error) => json!({"error":format!("{error:?}")}),
            };
            println!(
                "compatibility={}",
                json!({"videoId":raw_id,"profile":name,"elapsedMs":attempt.elapsed().as_millis(),"result":result})
            );
        }
    }
}

#[test]
#[ignore = "Explicit unsigned request-consistency experiment; no accounts/audio/response text"]
fn live_unsigned_request_consistency() {
    let provider = Provider::new().unwrap();
    let started = Instant::now();
    let budget = Duration::from_secs(50);
    let visitor = provider.post("https://www.youtube.com/youtubei/v1/visitor_id?prettyPrint=false",
        json!({"context":{"client":{"clientName":"WEB","clientVersion":"2.20260707.00.00","hl":"en"}}}),
        "1", "2.20260707.00.00", WEB_UA).unwrap();
    let visitor = visitor
        .pointer("/responseContext/visitorData")
        .and_then(Value::as_str)
        .unwrap();
    // Isolate two ordinary request-shape differences from the baseline: endpoint
    // Origin and a body User-Agent matching the header. Retain the same visitor;
    // do not rotate identities, acknowledge age/content checks or add tokens.
    for raw_id in ["hpSrLjc5SMs", "6Zt4P1scOwE", "dQw4w9WgXcQ"] {
        if started.elapsed() >= budget {
            println!("consistency_budget_exhausted=true");
            return;
        }
        let id = TrackId(raw_id.into());
        let mut request = provider.post_request("https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
            json!({"context":{"client":{"clientName":"VISIONOS","clientVersion":"1.02","deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471","hl":"en","userAgent":PLAYER_UA,"visitorData":visitor}},"videoId":raw_id}),
            "101", "1.02", PLAYER_UA).unwrap();
        request.headers_mut().insert(
            reqwest::header::ORIGIN,
            reqwest::header::HeaderValue::from_static("https://www.youtube.com"),
        );
        let attempt = Instant::now();
        let result = provider
            .transport
            .bytes(
                request,
                transport::Expected::Bytes(JSON_LIMIT as usize),
                transport::Lane::Media,
                transport::REQUEST_TIMEOUT.min(budget.saturating_sub(started.elapsed())),
                || false,
            )
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse));
        let result = match result {
            Ok(value) => summary(&id, &value),
            Err(error) => json!({"error":format!("{error:?}")}),
        };
        println!(
            "consistency={}",
            json!({"videoId":raw_id,"elapsedMs":attempt.elapsed().as_millis(),"result":result})
        );
    }
}
