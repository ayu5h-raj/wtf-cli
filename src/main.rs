use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::process::Command;


/// WTF (Write The Formula) - Translate natural language to shell commands using AI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The natural language prompt describing what you want to do
    #[arg(required = false)]
    prompt: Vec<String>,

    /// Output only the command (no interactive prompt). Useful for scripting.
    #[arg(short, long)]
    raw: bool,

    /// Print shell integration script. Usage: eval "$(wtf --init zsh)"
    #[arg(long, value_name = "SHELL")]
    init: Option<String>,
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

    // Handle --init flag for shell integration
    if let Some(shell) = &args.init {
        print_init_script(shell);
        return Ok(());
    }

    // Check if prompt is provided
    if args.prompt.is_empty() {
        eprintln!("Usage: wtf <natural language prompt>");
        eprintln!("       eval \"$(command wtf --init zsh)\"");
        eprintln!("\nExample: wtf show my ip address");
        std::process::exit(1);
    }

    let prompt = args.prompt.join(" ");
    let config = Config::from_env()?;

    let command = get_command(&config, &prompt).await?;
    // Strip markdown code blocks if present
    let command = command
        .trim()
        .trim_start_matches("```bash")
        .trim_start_matches("```sh")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Raw mode: just output the command (for shell wrapper)
    if args.raw {
        println!("{}", command);
        return Ok(());
    }

    // Default mode: show command with emoji
    println!("💡 \x1b[36m{}\x1b[0m", command);

    Ok(())
}

fn print_init_script(shell: &str) {
    match shell {
        "zsh" => {
            print!(r#"# WTF (Write The Formula) - Shell integration
# Add to ~/.zshrc: eval "$(command wtf --init zsh)"

function wtf() {{
    if [[ -z "$1" ]]; then
        echo "Usage: wtf <natural language prompt>"
        return 1
    fi

    # Show loading state
    echo -n "⏳ Generating..." >&2

    local cmd
    # Use the binary to get the command (raw mode)
    cmd=$(command wtf --raw "$@" 2>&1)
    local exit_code=$?

    # Clear loading state (CR + Clear Line)
    echo -ne "\r\033[K" >&2

    if [[ $exit_code -ne 0 ]]; then
        echo "❌ $cmd"
        return 1
    fi

    # Show the command with formatting
    echo "💡 \033[36m$cmd\033[0m"
    echo ""
    
    # Put in buffer (print -z)
    print -z "$cmd"
}}

alias '??'='wtf'
"#);
        }
        "bash" => {
            print!(r#"# WTF (Write The Formula) - Shell integration
# Add to ~/.bashrc: eval "$(wtf --init bash)"

function wtf() {{
    if [[ -z "$1" ]]; then
        echo "Usage: wtf <natural language prompt>"
        return 1
    fi

    echo -n "⏳ Generating..." >&2

    local cmd
    cmd=$(command wtf --raw "$@" 2>&1)
    local exit_code=$?

    echo -ne "\r\033[K" >&2

    if [[ $exit_code -ne 0 ]]; then
        echo "❌ $cmd"
        return 1
    fi

    echo "💡 $cmd"
    echo "📋 Copied to clipboard"
    printf '%s' "$cmd" | pbcopy
}}

alias '??'='wtf'
"#);
        }
        _ => {
            eprintln!("Unsupported shell: {}. Supported: zsh, bash", shell);
            std::process::exit(1);
        }
    }
}

async fn get_command(config: &Config, prompt: &str) -> Result<String> {
    match config.provider {
        Provider::Gemini => get_command_gemini(config, prompt).await,
        Provider::OpenAI => get_command_openai(config, prompt).await,
    }
}

async fn get_command_gemini(config: &Config, prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let context = get_combined_context();
    let system_content = if context.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{}{}", SYSTEM_PROMPT, context)
    };

    let request_body = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![Part {
                text: prompt.to_string(),
            }],
        }],
        system_instruction: GeminiContent {
            parts: vec![Part {
                text: system_content,
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

    let context = get_combined_context();
    let system_content = if context.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{}{}", SYSTEM_PROMPT, context)
    };

    let request_body = OpenAIRequest {
        model: config.model.clone(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: system_content,
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

// ─────────────────────────────────────────────────────────────────────────────
// Context Awareness
// ─────────────────────────────────────────────────────────────────────────────

fn get_combined_context() -> String {
    // Check if context is disabled
    if env::var("WTF_NO_CONTEXT").is_ok() {
        return String::new();
    }

    let mut context_str = String::new();
    let dir_context = get_directory_context();
    let git_context = get_git_context();

    if dir_context.is_empty() && git_context.is_empty() {
        return String::new();
    }
    
    context_str.push_str("\nCurrent Directory Context:\n");

    if !dir_context.is_empty() {
        context_str.push_str("Files: [");
        context_str.push_str(&dir_context);
        context_str.push_str("]\n");
    }

    if !git_context.is_empty() {
        context_str.push_str("Git Status:\n");
        context_str.push_str(&git_context);
        context_str.push_str("\n");
    }

    context_str
}

fn get_directory_context() -> String {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(".") {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                // Skip hidden files and .git
                if !name.starts_with('.') {
                    files.push(name);
                }
            }
        }
    }

    // Sort to be deterministic
    files.sort();

    // Limit to 50 files to save tokens
    if files.len() > 50 {
        files.truncate(50);
        files.push("... (truncated)".to_string());
    }

    files.join(", ")
}

fn get_git_context() -> String {
    let output = Command::new("git")
        .args(["status", "--short"])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = stdout.lines().collect();
            
            if lines.is_empty() {
                return String::new();
            }

            // Limit to 20 lines
            if lines.len() > 20 {
                let mut limited = lines[..20].to_vec();
                limited.push("... (truncated)");
                return limited.join("\n");
            }
            
            stdout.to_string()
        }
        _ => String::new(), // Not a git repo or git not found
    }
}
