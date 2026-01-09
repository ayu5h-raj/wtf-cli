use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, Write};
use std::process::Command;

/// WTF (Write The Formula) - Translate natural language to shell commands using AI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The natural language prompt describing what you want to do
    #[arg(required = true)]
    prompt: Vec<String>,

    /// Output only the command (no interactive prompt). Useful for scripting.
    #[arg(short, long)]
    raw: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenAI-compatible API structures (works with OpenRouter, Azure, Ollama, etc.)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Option<Vec<Choice>>,
    error: Option<OpenAIError>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

#[derive(Deserialize)]
struct OpenAIError {
    message: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Gemini API structures (for backwards compatibility)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction")]
    system_instruction: GeminiContent,
}

#[derive(Serialize, Deserialize)]
struct GeminiContent {
    parts: Vec<Part>,
}

#[derive(Serialize, Deserialize)]
struct Part {
    text: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
struct Candidate {
    content: GeminiContent,
}

#[derive(Deserialize)]
struct GeminiError {
    message: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Configuration
// ─────────────────────────────────────────────────────────────────────────────

struct Config {
    api_key: String,
    base_url: String,
    model: String,
    provider: Provider,
}

#[derive(PartialEq)]
enum Provider {
    Gemini,
    OpenAI, // OpenAI-compatible (OpenRouter, Azure, Ollama, etc.)
}

impl Config {
    fn from_env() -> Result<Self> {
        // Try WTF_API_KEY first, then fall back to GEMINI_API_KEY
        let api_key = env::var("WTF_API_KEY")
            .or_else(|_| env::var("GEMINI_API_KEY"))
            .context(
                "API key not set.\n\n\
                Set one of these environment variables:\n\
                  export WTF_API_KEY='your-key'      # For any provider\n\
                  export GEMINI_API_KEY='your-key'   # For Gemini (legacy)\n\n\
                Get a free Gemini key at: https://aistudio.google.com/app/apikey"
            )?;

        let base_url = env::var("WTF_BASE_URL").unwrap_or_default();
        let model = env::var("WTF_MODEL").unwrap_or_default();

        // Determine provider based on base_url
        let (provider, base_url, model) = if base_url.is_empty() {
            // Default to Gemini
            (
                Provider::Gemini,
                "https://generativelanguage.googleapis.com/v1beta".to_string(),
                if model.is_empty() { "gemini-2.0-flash".to_string() } else { model },
            )
        } else {
            // Custom base URL = OpenAI-compatible
            (
                Provider::OpenAI,
                base_url,
                if model.is_empty() { "gpt-4o-mini".to_string() } else { model },
            )
        };

        Ok(Config {
            api_key,
            base_url,
            model,
            provider,
        })
    }
}

const SYSTEM_PROMPT: &str = r#"You are a shell command expert. Your task is to translate the user's natural language request into a valid shell command.

Rules:
1. Output ONLY the shell command, nothing else. No explanations, no markdown, no code blocks.
2. Use standard POSIX commands when possible for portability.
3. For macOS-specific tasks, use the appropriate macOS commands.
4. If the request is dangerous (like rm -rf /), still provide the command but add a comment warning.
5. If the request is ambiguous, provide the most common interpretation.
6. Use single quotes for strings unless double quotes are necessary for variable expansion.
7. For multi-step operations, chain commands with && or use a single-line script.

Examples:
User: show my ip address
Output: curl -s ifconfig.me

User: find large files over 100mb
Output: find . -type f -size +100M

User: kill process on port 3000
Output: lsof -ti:3000 | xargs kill -9

User: compress this folder
Output: tar -czvf archive.tar.gz .
"#;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let prompt = args.prompt.join(" ");
    let config = Config::from_env()?;

    let command = get_command(&config, &prompt).await?;
    let command = command.trim();

    // Raw mode: just output the command and exit
    if args.raw {
        println!("{}", command);
        return Ok(());
    }

    // Display the suggested command
    println!("\n💡 \x1b[1mSuggested command:\x1b[0m\n");
    println!("   \x1b[36m{}\x1b[0m\n", command);

    // Interactive prompt
    print!("Execute? [\x1b[32my\x1b[0mes/\x1b[31mN\x1b[0mo/\x1b[33me\x1b[0mdit]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    match input.as_str() {
        "y" | "yes" => {
            println!("\n▶️  \x1b[1mRunning...\x1b[0m\n");
            let status = Command::new("sh")
                .arg("-c")
                .arg(command)
                .status()
                .context("Failed to execute command")?;

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        "e" | "edit" => {
            let _ = Command::new("sh")
                .arg("-c")
                .arg(format!("printf '%s' '{}' | pbcopy", command.replace("'", "'\\''")))
                .status();
            println!("📋 Command copied to clipboard. Paste and edit it!");
        }
        _ => {
            let _ = Command::new("sh")
                .arg("-c")
                .arg(format!("printf '%s' '{}' | pbcopy", command.replace("'", "'\\''")))
                .status();
            println!("📋 Cancelled. Command copied to clipboard.");
        }
    }

    Ok(())
}

async fn get_command(config: &Config, prompt: &str) -> Result<String> {
    match config.provider {
        Provider::Gemini => get_command_gemini(config, prompt).await,
        Provider::OpenAI => get_command_openai(config, prompt).await,
    }
}

async fn get_command_gemini(config: &Config, prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let request_body = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![Part {
                text: prompt.to_string(),
            }],
        }],
        system_instruction: GeminiContent {
            parts: vec![Part {
                text: SYSTEM_PROMPT.to_string(),
            }],
        },
    };

    let url = format!(
        "{}/models/{}:generateContent?key={}",
        config.base_url, config.model, config.api_key
    );

    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .context("Failed to send request to Gemini API")?;

    let status = response.status();
    let response_text = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("Gemini API error ({}): {}", status, response_text);
    }

    let gemini_response: GeminiResponse =
        serde_json::from_str(&response_text).context("Failed to parse Gemini response")?;

    if let Some(error) = gemini_response.error {
        anyhow::bail!("Gemini API error: {}", error.message);
    }

    let command = gemini_response
        .candidates
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text)
        .context("No command generated from Gemini")?;

    Ok(command)
}

async fn get_command_openai(config: &Config, prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let request_body = OpenAIRequest {
        model: config.model.clone(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: SYSTEM_PROMPT.to_string(),
            },
            Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            },
        ],
        max_tokens: 500,
    };

    let url = format!("{}/chat/completions", config.base_url);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("Failed to send request to API")?;

    let status = response.status();
    let response_text = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("API error ({}): {}", status, response_text);
    }

    let openai_response: OpenAIResponse =
        serde_json::from_str(&response_text).context("Failed to parse API response")?;

    if let Some(error) = openai_response.error {
        anyhow::bail!("API error: {}", error.message);
    }

    let command = openai_response
        .choices
        .and_then(|c| c.into_iter().next())
        .map(|c| c.message.content)
        .context("No command generated from API")?;

    Ok(command)
}
