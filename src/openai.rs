//! OpenAI /v1/audio/transcriptions request building and response parsing.

/// Offered in the settings dropdown. The field is editable, so other model IDs work too.
/// All but gpt-transcribe shut down on 2027-02-26.
pub const MODELS: &[&str] = &[
    "gpt-transcribe",
    "gpt-4o-transcribe",
    "gpt-4o-mini-transcribe",
    "whisper-1",
];

const BOUNDARY: &str = "wsprBoundary7MA4YWxkTrZu0gW";

/// gpt-transcribe takes a `languages[]` list; older models take one `language`. Never send both.
pub fn multipart(model: &str, languages: &[String], prompt: &str, wav: &[u8]) -> (String, Vec<u8>) {
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
    if !prompt.trim().is_empty() {
        field("prompt", prompt.trim());
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
            .ok_or_else(|| "Vastuses puudub tekst".into())
    } else {
        Err(format!(
            "HTTP {status}: {}",
            json["error"]["message"].as_str().unwrap_or("tundmatu viga")
        ))
    }
}

#[cfg(windows)]
pub fn transcribe(
    api_key: &str,
    model: &str,
    languages: &[String],
    prompt: &str,
    wav: &[u8],
) -> Result<String, String> {
    use std::time::Duration;
    let (content_type, body) = multipart(model, languages, prompt, wav);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(180)))
        .build()
        .into();
    let mut resp = agent
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", &format!("Bearer {}", api_key.trim()))
        .header("Content-Type", &content_type)
        .send(&body[..])
        .map_err(|e| format!("Ühenduse viga: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Vastuse lugemise viga: {e}"))?;
    parse_response(status, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_str(model: &str, langs: &[&str], prompt: &str) -> String {
        let langs: Vec<String> = langs.iter().map(|s| s.to_string()).collect();
        String::from_utf8(multipart(model, &langs, prompt, b"WAVDATA").1).unwrap()
    }

    #[test]
    fn gpt_transcribe_uses_languages_list() {
        let b = body_str("gpt-transcribe", &["et", "en"], "");
        assert!(b.contains("name=\"languages[]\"\r\n\r\net\r\n"));
        assert!(b.contains("name=\"languages[]\"\r\n\r\nen\r\n"));
        assert!(!b.contains("name=\"language\""));
        assert!(!b.contains("name=\"prompt\""));
        assert!(b.ends_with("WAVDATA\r\n--wsprBoundary7MA4YWxkTrZu0gW--\r\n"));
    }

    #[test]
    fn older_models_use_single_language_and_prompt() {
        let b = body_str("whisper-1", &["et", "en"], " Tõnis ");
        assert!(b.contains("name=\"language\"\r\n\r\net\r\n"));
        assert!(!b.contains("languages[]"));
        assert!(b.contains("name=\"prompt\"\r\n\r\nTõnis\r\n"));
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
