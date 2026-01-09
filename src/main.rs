use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use chrono::Utc;


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

    /// Show command history
    #[arg(long)]
    history: bool,

    /// Explain the generated command
    #[arg(short, long)]
    explain: bool,
}

#[derive(Serialize, Deserialize)]
struct HistoryEntry {
    timestamp: i64,
    prompt: String,
    command: String,
}

struct CommandResult {
    command: String,
    explanation: Option<String>,
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

const SYSTEM_PROMPT_EXPLAIN: &str = r#"You are a shell command expert.
1. Output the shell command.
2. Output a separator: " ### "
3. Output a concise explanation of what the command does.

Example:
User: list files
Output: ls -la ### Lists all files including hidden ones in long format.
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
        if args.history {
            show_history()?;
            return Ok(());
        }

        eprintln!("Usage: wtf <natural language prompt>");
        eprintln!("       eval \"$(command wtf --init zsh)\"");
        eprintln!("\nExample: wtf show my ip address");
        std::process::exit(1);
    }

    let prompt = args.prompt.join(" ");
    let config = Config::from_env()?;

    let result = get_command(&config, &prompt, args.explain).await?;
    
    // Strip markdown code blocks if present in command
    let command = result.command
        .trim()
        .trim_start_matches("```bash")
        .trim_start_matches("```sh")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
        
    // Save to history
    if let Err(e) = append_to_history(&prompt, command) {
        eprintln!("Warning: Failed to save history: {}", e);
    }

    // Raw mode: just output the command (for shell wrapper)
    if args.raw {
        println!("{}", command);
        return Ok(());
    }

    // Default mode: show command with emoji
    println!("💡 \x1b[36m{}\x1b[0m", command);
    
    if let Some(explanation) = result.explanation {
        println!("\x1b[90m📝 {}\x1b[0m", explanation.trim());
    }

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

async fn get_command(config: &Config, prompt: &str, explain: bool) -> Result<CommandResult> {
    match config.provider {
        Provider::Gemini => get_command_gemini(config, prompt, explain).await,
        Provider::OpenAI => get_command_openai(config, prompt, explain).await,
    }
}

async fn get_command_gemini(config: &Config, prompt: &str, explain: bool) -> Result<CommandResult> {
    let client = reqwest::Client::new();
    
    let system_prompt = if explain { SYSTEM_PROMPT_EXPLAIN } else { SYSTEM_PROMPT };

    let request_body = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![Part {
                text: prompt.to_string(),
            }],
        }],
        system_instruction: GeminiContent {
            parts: vec![Part {
                text: system_prompt.to_string(),
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

    let text = gemini_response
        .candidates
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text)
        .context("No command generated from Gemini")?;
        
    Ok(parse_output(&text))
}

async fn get_command_openai(config: &Config, prompt: &str, explain: bool) -> Result<CommandResult> {
    let client = reqwest::Client::new();

    let system_prompt = if explain { SYSTEM_PROMPT_EXPLAIN } else { SYSTEM_PROMPT };

    let request_body = OpenAIRequest {
        model: config.model.clone(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: system_prompt.to_string(),
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

    let text = openai_response
        .choices
        .and_then(|c| c.into_iter().next())
        .map(|c| c.message.content)
        .context("No command generated from API")?;

    Ok(parse_output(&text))
}

fn parse_output(text: &str) -> CommandResult {
    if let Some((cmd, expl)) = text.split_once("###") {
        CommandResult {
            command: cmd.trim().to_string(),
            explanation: Some(expl.trim().to_string()),
        }
    } else {
        CommandResult {
            command: text.trim().to_string(),
            explanation: None,
        }
    }
}



// ─────────────────────────────────────────────────────────────────────────────
// History
// ─────────────────────────────────────────────────────────────────────────────

fn get_history_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("Could not find HOME directory")?;
    Ok(Path::new(&home).join(".wtf_history"))
}

fn append_to_history(prompt: &str, command: &str) -> Result<()> {
    let path = get_history_path()?;
    
    let entry = HistoryEntry {
        timestamp: Utc::now().timestamp(),
        prompt: prompt.to_string(),
        command: command.to_string(),
    };
    
    let json = serde_json::to_string(&entry)?;
    
    // Read existing
    let mut lines: Vec<String> = Vec::new();
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        lines = content.lines().map(|s| s.to_string()).collect();
    }
    
    // Append new
    lines.push(json);
    
    // Truncate if > 1000
    if lines.len() > 1000 {
        let remove_count = lines.len() - 1000;
        lines.drain(0..remove_count);
    }
    
    // Write back
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
        
    for line in lines {
        writeln!(file, "{}", line)?;
    }
    
    Ok(())
}

fn show_history() -> Result<()> {
    let path = get_history_path()?;
    if !path.exists() {
        println!("No history found.");
        return Ok(());
    }
    
    let content = fs::read_to_string(&path)?;
    let lines: Vec<&str> = content.lines().collect();
    
    // Show last 20
    let start = if lines.len() > 20 { lines.len() - 20 } else { 0 };
    
    println!("Example History (Last 20):");
    for line in &lines[start..] {
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) {
            println!("• {} -> \x1b[36m{}\x1b[0m", entry.prompt, entry.command);
        }
    }
    
    Ok(())
}
