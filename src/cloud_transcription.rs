//! Opt-in hosted speech-to-text. Dictation audio leaves the machine only when
//! `cloud_transcription.enabled` is set; meetings and the local API stay local.
//!
//! Each provider receives one 16 kHz mono 16-bit WAV per request through a
//! multipart upload and returns JSON with a top-level `text` field.

use std::fmt;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};

const SAMPLE_RATE: u32 = 16_000;
/// Ten minutes of PCM16 is about 19 MB, below every provider's 25 MB upload cap.
const MAX_REQUEST_SAMPLES: usize = SAMPLE_RATE as usize * 600;
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_ERROR_CHARS: usize = 300;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudProvider {
    #[default]
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(alias = "grok")]
    Xai,
    Groq,
}

impl CloudProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Xai => "xAI",
            Self::Groq => "Groq",
        }
    }

    fn default_endpoint(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1/audio/transcriptions",
            Self::Xai => "https://api.x.ai/v1/stt",
            Self::Groq => "https://api.groq.com/openai/v1/audio/transcriptions",
        }
    }

    /// xAI selects its current transcription model when none is named.
    fn default_model(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("gpt-4o-mini-transcribe"),
            Self::Xai => None,
            Self::Groq => Some("whisper-large-v3-turbo"),
        }
    }

    pub fn api_key_variable(self) -> &'static str {
        match self {
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Xai => "XAI_API_KEY",
            Self::Groq => "GROQ_API_KEY",
        }
    }

    /// OpenAI-compatible endpoints accept `response_format`; xAI does not.
    fn sends_response_format(self) -> bool {
        !matches!(self, Self::Xai)
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct CloudTranscriptionSettings {
    /// Off by default: enabling it uploads every dictation to `provider`.
    pub enabled: bool,
    pub provider: CloudProvider,
    /// Provider model identifier; empty uses the provider default.
    pub model: Option<String>,
    /// Stored in plain text in the settings file. When absent, the provider's
    /// conventional environment variable (for example `OPENAI_API_KEY`) is read.
    pub api_key: Option<String>,
    /// ISO-639-1 language hint; empty lets the provider detect the language.
    pub language: Option<String>,
    /// Replaces the provider's transcription URL, for compatible gateways.
    pub endpoint: Option<String>,
    pub timeout_seconds: u64,
}

impl Default for CloudTranscriptionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: CloudProvider::default(),
            model: None,
            api_key: None,
            language: None,
            endpoint: None,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        }
    }
}

impl fmt::Debug for CloudTranscriptionSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CloudTranscriptionSettings")
            .field("enabled", &self.enabled)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("language", &self.language)
            .field("endpoint", &self.endpoint)
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}

impl CloudTranscriptionSettings {
    /// Checks the shape of the settings without requiring a key, which may only
    /// exist in the environment of the process that later transcribes.
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        endpoint_url(self)?;
        if !(1..=600).contains(&self.timeout_seconds) {
            bail!("Cloud transcription timeout must be between 1 and 600 seconds");
        }
        if let Some(language) = non_empty(&self.language)
            && !language
                .chars()
                .all(|ch| ch.is_ascii_alphabetic() || ch == '-')
        {
            bail!("Cloud transcription language must be a language code such as `en`");
        }
        Ok(())
    }

    pub fn model(&self) -> Option<&str> {
        non_empty(&self.model).or(self.provider.default_model())
    }

    fn api_key(&self) -> Result<String> {
        if let Some(key) = non_empty(&self.api_key) {
            return Ok(key.to_string());
        }
        let variable = self.provider.api_key_variable();
        std::env::var(variable)
            .ok()
            .filter(|key| !key.trim().is_empty())
            .map(|key| key.trim().to_string())
            .ok_or_else(|| {
                eyre!(
                    "{} transcription needs an API key: set cloud_transcription.api_key or {variable}",
                    self.provider.label()
                )
            })
    }

    pub fn describe(&self) -> String {
        match self.model() {
            Some(model) => format!("{} {model}", self.provider.label()),
            None => self.provider.label().to_string(),
        }
    }
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// HTTPS everywhere, except plain HTTP to a loopback gateway.
fn endpoint_url(settings: &CloudTranscriptionSettings) -> Result<url::Url> {
    let raw = non_empty(&settings.endpoint).unwrap_or(settings.provider.default_endpoint());
    let url = url::Url::parse(raw)
        .wrap_err_with(|| format!("invalid cloud transcription endpoint {raw}"))?;
    let loopback = match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => bail!("cloud transcription endpoint {raw} has no host"),
    };
    match url.scheme() {
        "https" => Ok(url),
        "http" if loopback => Ok(url),
        _ => bail!("cloud transcription endpoint must use HTTPS: {raw}"),
    }
}

pub struct CloudTranscriber {
    settings: CloudTranscriptionSettings,
    endpoint: url::Url,
    api_key: String,
    agent: ureq::Agent,
}

impl CloudTranscriber {
    pub fn new(settings: &CloudTranscriptionSettings) -> Result<Self> {
        settings.validate()?;
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(settings.timeout_seconds)))
            .user_agent(concat!("hex/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Ok(Self {
            endpoint: endpoint_url(settings)?,
            api_key: settings.api_key()?,
            settings: settings.clone(),
            agent,
        })
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn matches(&self, settings: &CloudTranscriptionSettings) -> bool {
        self.settings == *settings
    }

    pub fn describe(&self) -> String {
        self.settings.describe()
    }

    /// Transcribes 16 kHz mono samples, splitting very long captures into
    /// sequential uploads that stay under provider size limits.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let mut output = Vec::new();
        for chunk in samples.chunks(MAX_REQUEST_SAMPLES) {
            let text = self.transcribe_chunk(chunk)?;
            let text = text.trim();
            if !text.is_empty() {
                output.push(text.to_string());
            }
        }
        Ok(output.join(" "))
    }

    fn transcribe_chunk(&self, samples: &[f32]) -> Result<String> {
        let boundary = boundary()?;
        let body = multipart_body(&self.settings, &boundary, &encode_wav(samples));
        let provider = self.settings.provider.label();
        let mut response = self
            .agent
            .post(self.endpoint.as_str())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send(&body[..])
            .map_err(|error| eyre!("{provider} transcription request failed: {error}"))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_to_vec()
            .map_err(|error| eyre!("{provider} transcription response failed: {error}"))?;
        parse_response(provider, status, &body)
    }
}

fn boundary() -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| eyre!("could not generate boundary: {error}"))?;
    Ok(format!(
        "hex-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn multipart_body(settings: &CloudTranscriptionSettings, boundary: &str, wav: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav.len() + 1024);
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    };
    if let Some(model) = settings.model() {
        field("model", model);
    }
    if let Some(language) = non_empty(&settings.language) {
        field("language", language);
    }
    if settings.provider.sends_response_format() {
        field("response_format", "json");
    }
    // xAI requires the file after every other field.
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"dictation.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn encode_wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

fn parse_response(provider: &str, status: u16, body: &[u8]) -> Result<String> {
    let json = serde_json::from_slice::<serde_json::Value>(body).ok();
    if !(200..300).contains(&status) {
        let message = json
            .as_ref()
            .and_then(|json| {
                json.pointer("/error/message")
                    .or_else(|| json.get("error"))
                    .or_else(|| json.get("message"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string());
        let message: String = message.chars().take(MAX_ERROR_CHARS).collect();
        bail!("{provider} transcription failed with HTTP {status}: {message}");
    }
    json.as_ref()
        .and_then(|json| json.get("text"))
        .and_then(|text| text.as_str())
        .map(str::to_string)
        .ok_or_else(|| eyre!("{provider} transcription response had no text"))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    fn enabled(provider: CloudProvider) -> CloudTranscriptionSettings {
        CloudTranscriptionSettings {
            enabled: true,
            provider,
            api_key: Some("test-key".into()),
            ..Default::default()
        }
    }

    #[test]
    fn settings_default_off_and_accept_grok_alias() {
        let settings: CloudTranscriptionSettings =
            serde_json::from_str(r#"{"provider":"grok"}"#).unwrap();
        assert!(!settings.enabled);
        assert_eq!(settings.provider, CloudProvider::Xai);
        assert_eq!(settings.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
        let settings: CloudTranscriptionSettings =
            serde_json::from_str(r#"{"provider":"openai"}"#).unwrap();
        assert_eq!(settings.provider, CloudProvider::OpenAi);
        assert_eq!(settings.model(), Some("gpt-4o-mini-transcribe"));
    }

    #[test]
    fn debug_output_redacts_the_key() {
        let debug = format!("{:?}", enabled(CloudProvider::OpenAi));
        assert!(!debug.contains("test-key"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn endpoints_require_https_except_loopback() {
        let mut settings = enabled(CloudProvider::OpenAi);
        settings.endpoint = Some("http://example.com/v1/audio/transcriptions".into());
        assert!(settings.validate().is_err());
        settings.endpoint = Some("http://127.0.0.1:9000/v1/audio/transcriptions".into());
        assert!(settings.validate().is_ok());
        settings.endpoint = Some("https://gateway.example.com/stt".into());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn disabled_settings_are_not_validated() {
        let settings = CloudTranscriptionSettings {
            endpoint: Some("ftp://nowhere".into()),
            ..Default::default()
        };
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn wav_header_describes_sixteen_khz_mono_pcm16() {
        let wav = encode_wav(&[0.0, 1.0, -1.0]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([wav[48], wav[49]]), -i16::MAX);
    }

    #[test]
    fn multipart_places_the_file_last() {
        let mut settings = enabled(CloudProvider::Xai);
        settings.language = Some("en".into());
        let body = String::from_utf8_lossy(&multipart_body(&settings, "b", b"WAV")).into_owned();
        let language = body.find("name=\"language\"").unwrap();
        let file = body.find("name=\"file\"").unwrap();
        assert!(language < file);
        assert!(!body.contains("response_format"));
        assert!(!body.contains("name=\"model\""));
        assert!(body.ends_with("WAV\r\n--b--\r\n"));

        let body =
            String::from_utf8_lossy(&multipart_body(&enabled(CloudProvider::Groq), "b", b"WAV"))
                .into_owned();
        assert!(body.contains("whisper-large-v3-turbo"));
        assert!(body.contains("name=\"response_format\"\r\n\r\njson"));
    }

    #[test]
    fn error_responses_surface_the_provider_message() {
        let error = parse_response(
            "OpenAI",
            401,
            br#"{"error":{"message":"Incorrect API key provided"}}"#,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "OpenAI transcription failed with HTTP 401: Incorrect API key provided"
        );
        assert_eq!(
            parse_response("xAI", 200, br#"{"text":"hello","words":[]}"#).unwrap(),
            "hello"
        );
        assert!(parse_response("xAI", 200, b"{}").is_err());
    }

    #[test]
    fn transcribes_against_a_loopback_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length: usize = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body = br#"{"text":" hello world "}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        let mut settings = enabled(CloudProvider::OpenAi);
        settings.endpoint = Some(format!("http://127.0.0.1:{port}/v1/audio/transcriptions"));
        let transcriber = CloudTranscriber::new(&settings).unwrap();
        assert_eq!(
            transcriber.transcribe(&vec![0.1; 16_000]).unwrap(),
            "hello world"
        );
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/audio/transcriptions HTTP/1.1"));
        assert!(
            request.contains("authorization: Bearer test-key")
                || request.contains("Authorization: Bearer test-key")
        );
        assert!(request.contains("gpt-4o-mini-transcribe"));
    }
}
