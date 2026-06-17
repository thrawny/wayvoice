use serde::Deserialize;
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use wayvoice::audio_guard::{analyze_audio, reject_before_transcribe, reject_transcript};
use wayvoice::config::{Config, Provider};
use wayvoice::transcription::transcribe_audio;

const CURRENT_PI_KEYWORDS: &[&str] = &[
    "wayvoice",
    "Pi",
    "prompt",
    "transcript",
    "eval suite",
    "Groq",
    "Whisper",
    "zmx",
    "just check",
    "dotfiles",
    "debug recordings",
];

const CLEAN_PHRASES: &[&str] = &[
    "test the Pi path",
    "look in the transcript logs",
    "check whether the prompt was added",
    "is the prompt causing the problem",
    "create a prompt eval suite",
];

const DOMAIN_FRAGMENTS: &[&str] = &[
    "Pi path",
    "transcript logs",
    "custom prompt",
    "prompt problem",
    "current transcript",
    "eval suite",
];

const SPOKEN_EXAMPLES: &[&str] = &[
    "Okay, then this is a test of the Pi path.",
    "I'm testing a prompt now.",
    "I'm going to look in the logs.",
    "I'm not sure if that's what we want.",
    "Are you sure that the prompt was the problem?",
    "Use the current transcript to create a nice eval suite.",
];

const REQUEST_DELAY: Duration = Duration::from_millis(3_200);
const RATE_LIMIT_RETRY_DELAY: Duration = Duration::from_secs(10);

const NOISY_PI_CONTEXT_PROMPT: &str = r#"User: Okay, good work, I'm testing a prompt now, I'm going to look in the logs to see if it's added something.

And it did:

{
  "extra_keywords": ["symlink", "cachix", "thrawny", "droop", "incus", "clamshell", "slug", "phase", "Codex", "zen browser", "solis", "pi path", "dotfiles", "Git", "git status", "server restart", "watch", "debug", "PID", "socket", "CLI", "zsh", "ps", "wayvoice", "pgrep", "watchexec", "file watch", "bwrap", "ECONNREFUSED", "sandbox", "pkill"],
  "prompt": "User: only prinut those things if they contain data Assistant: Done. Changes: Justfile clippy debug/d symlink cachix thrawny"
}

Assistant: Perfect — that confirms the Pi path is doing what we wanted.

User: I'm not sure if that's what we want, what do you think?

Assistant: Analyzing prompt semantics. Whisper prompts are bias context, not instructions."#;

#[derive(Debug, Deserialize)]
struct Manifest {
    cases: Vec<EvalCase>,
}

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
    audio: String,
    expected: String,
    #[serde(default)]
    key_terms: Vec<String>,
}

struct PromptVariant {
    name: &'static str,
    prompt: String,
}

fn manifest_dir() -> PathBuf {
    if let Ok(path) = std::env::var("WAYVOICE_PROMPT_EVAL_DIR") {
        return PathBuf::from(path);
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".cache/wayvoice/prompt-eval/current")
}

fn try_load_manifest() -> Option<Manifest> {
    let path = manifest_dir().join("manifest.json");
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!(
                "skipping prompt eval fixtures: could not read {}: {error}",
                path.display()
            );
            return None;
        }
    };
    Some(serde_json::from_str(&contents).unwrap())
}

fn prompt_variants() -> Vec<PromptVariant> {
    let keywords = CURRENT_PI_KEYWORDS.join(", ");
    let keyword_fragments = format!("{}. {}.", keywords, DOMAIN_FRAGMENTS.join(". "));
    let keywords_and_phrases = format!("{}. {}.", keywords, CLEAN_PHRASES.join(". "));
    let spoken_examples = SPOKEN_EXAMPLES.join(" ");
    let keywords_and_spoken_examples = format!("{}. {}", keywords, spoken_examples);

    vec![
        PromptVariant {
            name: "none",
            prompt: String::new(),
        },
        PromptVariant {
            name: "keywords_only",
            prompt: keywords,
        },
        PromptVariant {
            name: "keywords_plus_fragments",
            prompt: keyword_fragments,
        },
        PromptVariant {
            name: "keywords_plus_clean_phrases",
            prompt: keywords_and_phrases,
        },
        PromptVariant {
            name: "spoken_examples_only",
            prompt: spoken_examples,
        },
        PromptVariant {
            name: "keywords_plus_spoken_examples",
            prompt: keywords_and_spoken_examples,
        },
        PromptVariant {
            name: "noisy_pi_context_negative_control",
            prompt: NOISY_PI_CONTEXT_PROMPT.to_string(),
        },
    ]
}

fn groq_config(prompt: &str) -> Config {
    Config {
        provider: Provider::Groq,
        language: "en".to_string(),
        prompt: prompt.to_string(),
        keywords: Vec::new(),
        extra_keywords: Vec::new(),
        use_default_keywords: false,
        ..Config::default()
    }
}

fn normalize_for_distance(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn normalize_chars(text: &str) -> Vec<char> {
    text.to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn edit_distance<T: Eq>(left: &[T], right: &[T]) -> usize {
    let mut prev: Vec<usize> = (0..=right.len()).collect();
    let mut curr = vec![0; right.len() + 1];

    for (i, left_item) in left.iter().enumerate() {
        curr[0] = i + 1;
        for (j, right_item) in right.iter().enumerate() {
            let substitution = prev[j] + usize::from(left_item != right_item);
            let insertion = curr[j] + 1;
            let deletion = prev[j + 1] + 1;
            curr[j + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[right.len()]
}

fn word_error_rate(expected: &str, actual: &str) -> f64 {
    let expected = normalize_for_distance(expected);
    let actual = normalize_for_distance(actual);
    edit_distance(&expected, &actual) as f64 / expected.len().max(1) as f64
}

fn char_error_rate(expected: &str, actual: &str) -> f64 {
    let expected = normalize_chars(expected);
    let actual = normalize_chars(actual);
    edit_distance(&expected, &actual) as f64 / expected.len().max(1) as f64
}

fn matched_key_terms<'a>(transcript: &str, terms: &'a [String]) -> Vec<&'a str> {
    let lower = transcript.to_lowercase();
    terms
        .iter()
        .filter(|term| lower.contains(&term.to_lowercase()))
        .map(String::as_str)
        .collect()
}

fn report_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/prompt-eval/current.jsonl")
}

fn append_report(path: &Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut line = serde_json::to_string(&value).unwrap();
    line.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(line.as_bytes())
        .unwrap();
}

async fn transcribe_with_rate_limit_retry(audio: Vec<u8>, config: &Config) -> String {
    let mut last_error = None;
    for attempt in 1..=3 {
        match transcribe_audio(audio.clone(), config).await {
            Ok(transcript) => return transcript,
            Err(error) if error.to_string().contains("429") => {
                last_error = Some(error.to_string());
                eprintln!(
                    "rate limited on attempt {attempt}; waiting {:.1}s",
                    RATE_LIMIT_RETRY_DELAY.as_secs_f64()
                );
                tokio::time::sleep(RATE_LIMIT_RETRY_DELAY).await;
            }
            Err(error) => panic!("transcription failed: {error}"),
        }
    }
    panic!(
        "transcription failed after rate-limit retries: {}",
        last_error.unwrap_or_else(|| "unknown error".to_string())
    );
}

#[test]
fn prompt_eval_manifest_is_well_formed() {
    let Some(manifest) = try_load_manifest() else {
        return;
    };
    assert!(!manifest.cases.is_empty());

    for case in manifest.cases {
        assert!(!case.id.trim().is_empty(), "case id is required");
        assert!(
            !case.expected.trim().is_empty(),
            "{} needs expected text",
            case.id
        );
        let audio_path = manifest_dir().join(&case.audio);
        assert!(
            audio_path.exists(),
            "missing fixture for {}: {audio_path:?}",
            case.id
        );
        assert!(
            audio_path.extension().is_some_and(|ext| ext == "wav"),
            "fixture should be a WAV file: {audio_path:?}"
        );
    }
}

#[tokio::test]
#[ignore]
async fn prompt_variants_eval_current_transcript_fixtures() {
    if std::env::var("GROQ_API_KEY").is_err() {
        eprintln!("skipping: GROQ_API_KEY not set");
        return;
    }

    let report = report_path();
    let _ = std::fs::remove_file(&report);
    let Some(manifest) = try_load_manifest() else {
        return;
    };
    let variants = prompt_variants();

    eprintln!("writing prompt eval report to {}", report.display());

    for case in manifest.cases {
        let audio_path = manifest_dir().join(&case.audio);
        let audio = std::fs::read(&audio_path).unwrap();
        let metrics = analyze_audio(&audio);
        let pre_guard = reject_before_transcribe(Provider::Groq, metrics);

        for variant in &variants {
            let config = groq_config(&variant.prompt);
            let start = Instant::now();
            let transcript = transcribe_with_rate_limit_retry(audio.clone(), &config).await;
            let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
            let post_guard = reject_transcript(&config, &transcript, metrics);
            let wer = word_error_rate(&case.expected, &transcript);
            let cer = char_error_rate(&case.expected, &transcript);
            let matched = matched_key_terms(&transcript, &case.key_terms);

            let row = json!({
                "case": case.id,
                "audio": case.audio,
                "variant": variant.name,
                "prompt_chars": variant.prompt.chars().count(),
                "expected": case.expected,
                "transcript": transcript,
                "wer": (wer * 1000.0).round() / 1000.0,
                "cer": (cer * 1000.0).round() / 1000.0,
                "key_terms": case.key_terms,
                "matched_key_terms": matched,
                "pre_guard": pre_guard,
                "post_guard": post_guard,
                "latency_ms": (latency_ms * 10.0).round() / 10.0,
            });
            append_report(&report, row);

            eprintln!(
                "{} / {}: WER={:.3} CER={:.3} key_terms={}/{} latency={:.1}ms transcript={:?}",
                case.id,
                variant.name,
                wer,
                cer,
                matched.len(),
                case.key_terms.len(),
                latency_ms,
                transcript
            );

            tokio::time::sleep(REQUEST_DELAY).await;
        }
    }
}
