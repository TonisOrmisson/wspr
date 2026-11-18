//! Speech-to-text over OpenAI-style /audio/transcriptions endpoints (OpenAI, OpenRouter).

pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub key_label: &'static str,
    pub url: &'static str,
    /// Offered in the model dropdown. The field is editable, so other model IDs work too.
    pub models: &'static [&'static str],
}

/// Index 0 is the default. Settings store per-provider keys and models by this order.
pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        label: "OpenAI",
        key_label: "OpenAI API key",
        url: "https://api.openai.com/v1/audio/transcriptions",
        // All but gpt-transcribe shut down on 2027-02-26.
        models: &[
            "gpt-transcribe",
            "gpt-4o-transcribe",
            "gpt-4o-mini-transcribe",
            "whisper-1",
        ],
    },
    Provider {
        id: "openrouter",
        label: "OpenRouter (Microsoft MAI and more)",
        key_label: "OpenRouter API key",
        url: "https://openrouter.ai/api/v1/audio/transcriptions",
        models: &["microsoft/mai-transcribe-2"],
    },
];

pub fn provider_index(id: &str) -> usize {
    PROVIDERS.iter().position(|p| p.id == id).unwrap_or(0)
}

const BOUNDARY: &str = "wsprBoundary7MA4YWxkTrZu0gW";

/// gpt-transcribe takes a `languages[]` list; other models (and OpenRouter) take one `language`.
/// Never send both.
pub fn multipart(model: &str, languages: &[String], wav: &[u8]) -> (String, Vec<u8>) {
    let mut body = Vec::with_capacity(wav.len() + 1024);
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    };
    field("model", model);
    field("response_format", "json");
    if model.starts_with("gpt-transcribe") {
        for l in languages {
            field("languages[]", l);
        }
    } else if let Some(l) = languages.first() {
        field("language", l);
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

pub fn parse_response(status: u16, body: &str) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        format!(
            "HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        )
    })?;
    if (200..300).contains(&status) {
        json["text"]
            .as_str()
            .map(|t| t.trim().to_string())
            .ok_or_else(|| "No text in the response".into())
    } else {
        Err(format!(
            "HTTP {status}: {}",
            json["error"]["message"].as_str().unwrap_or("unknown error")
        ))
    }
}

#[cfg(windows)]
pub fn transcribe(
    url: &str,
    api_key: &str,
    model: &str,
    languages: &[String],
    wav: &[u8],
) -> Result<String, String> {
    use std::time::Duration;
    let (content_type, body) = multipart(model, languages, wav);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(180)))
        .build()
        .into();
    let mut resp = agent
        .post(url)
        .header("Authorization", &format!("Bearer {}", api_key.trim()))
        .header("Content-Type", &content_type)
        .send(&body[..])
        .map_err(|e| format!("Connection error: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Could not read the response: {e}"))?;
    parse_response(status, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_str(model: &str, langs: &[&str]) -> String {
        let langs: Vec<String> = langs.iter().map(|s| s.to_string()).collect();
        String::from_utf8(multipart(model, &langs, b"WAVDATA").1).unwrap()
    }

    #[test]
    fn gpt_transcribe_uses_languages_list() {
        let b = body_str("gpt-transcribe", &["et", "en"]);
        assert!(b.contains("name=\"languages[]\"\r\n\r\net\r\n"));
        assert!(b.contains("name=\"languages[]\"\r\n\r\nen\r\n"));
        assert!(!b.contains("name=\"language\""));
        assert!(b.ends_with("WAVDATA\r\n--wsprBoundary7MA4YWxkTrZu0gW--\r\n"));
    }

    #[test]
    fn older_models_use_single_language() {
        let b = body_str("whisper-1", &["et", "en"]);
        assert!(b.contains("name=\"language\"\r\n\r\net\r\n"));
        assert!(!b.contains("languages[]"));
    }

    #[test]
    fn openrouter_model_uses_single_language() {
        let b = body_str("microsoft/mai-transcribe-2", &["et"]);
        assert!(b.contains("name=\"language\"\r\n\r\net\r\n"));
        assert!(!b.contains("languages[]"));
        assert_eq!(provider_index("openrouter"), 1);
        assert_eq!(provider_index("unknown"), 0);
    }

    #[test]
    fn parses_success_and_errors() {
        assert_eq!(
            parse_response(200, r#"{"text":" Tere! ","languages":[]}"#),
            Ok("Tere!".into())
        );
        assert_eq!(
            parse_response(401, r#"{"error":{"message":"Incorrect API key"}}"#),
            Err("HTTP 401: Incorrect API key".into())
        );
        assert!(
            parse_response(502, "<html>Bad gateway</html>")
                .unwrap_err()
                .starts_with("HTTP 502")
        );
    }
}
