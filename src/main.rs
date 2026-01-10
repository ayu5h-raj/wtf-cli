use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use chrono::Utc;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;


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

    /// Clear command history
    #[arg(long)]
    clear_history: bool,

    /// Explain the generated command
    #[arg(short, long)]
    explain: bool,

    /// Start interactive mode (REPL)
    #[arg(short, long)]
    interactive: bool,
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

    // Handle interactive mode
    if args.interactive {
        let config = Config::from_env()?;
        return run_interactive_mode(&config, args.explain).await;
    }

    // Check if prompt is provided
    if args.prompt.is_empty() {
        if args.clear_history {
            clear_history()?;
            return Ok(());
        }
        if args.history {
            show_history()?;
            return Ok(());
        }

        eprintln!("Usage: wtf <natural language prompt>");
        eprintln!("       wtf --interactive  # Start interactive mode");
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

fn strip_ansi_codes(text: &str) -> String {
    // Remove ANSI escape sequences (e.g., \x1b[36m, \x1b[0m)
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '\x1b' || ch == '\u{001b}' {
            // Skip ANSI escape sequence
            if let Some('[') = chars.peek() {
                chars.next(); // consume '['
                // Skip until we find a letter (end of escape sequence)
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_ascii_alphabetic() || next_ch == 'm' {
                        chars.next();
                        break;
                    }
                    chars.next();
                }
            }
        } else {
            result.push(ch);
        }
    }
    
    result
}

fn append_to_history(prompt: &str, command: &str) -> Result<()> {
    let path = get_history_path()?;
    
    // Strip any ANSI codes that might have accidentally gotten in
    let clean_command = strip_ansi_codes(command);
    let clean_prompt = strip_ansi_codes(prompt);
    
    let entry = HistoryEntry {
        timestamp: Utc::now().timestamp(),
        prompt: clean_prompt.trim().to_string(),
        command: clean_command.trim().to_string(),
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
    
    // Parse all entries
    let mut entries: Vec<HistoryEntry> = Vec::new();
    for line in &lines {
        if let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) {
            entries.push(entry);
        }
    }
    
    if entries.is_empty() {
        println!("No history found.");
        return Ok(());
    }
    
    // Show last 20
    let start = if entries.len() > 20 { entries.len() - 20 } else { 0 };
    let recent_entries = &entries[start..];
    
    println!("\x1b[1;36m╔═══════════════════════════════════════════════════════════════════════════╗\x1b[0m");
    println!("\x1b[1;36m║  Command History (Last {} entries)                                      ║\x1b[0m", recent_entries.len());
    println!("\x1b[1;36m╚═══════════════════════════════════════════════════════════════════════════╝\x1b[0m");
    println!();
    
    for (idx, entry) in recent_entries.iter().enumerate() {
        let num = start + idx + 1;
        
        // Format timestamp
        let timestamp = chrono::DateTime::from_timestamp(entry.timestamp, 0)
            .unwrap_or_else(|| chrono::Utc::now());
        let time_str = timestamp.format("%Y-%m-%d %H:%M").to_string();
        
        // Truncate long commands for display
        let command_display = if entry.command.len() > 80 {
            format!("{}...", &entry.command[..77])
        } else {
            entry.command.clone()
        };
        
        // Print entry
        println!("\x1b[90m[{:3}] {}\x1b[0m", num, time_str);
        println!("     \x1b[1mPrompt:\x1b[0m   {}", entry.prompt);
        println!("     \x1b[1mCommand:\x1b[0m  \x1b[36m{}\x1b[0m", command_display);
        
        // Show full command if truncated
        if entry.command.len() > 80 {
            println!("     \x1b[90m(Full: {})\x1b[0m", entry.command);
        }
        
        println!();
    }
    
    println!("\x1b[90mTotal entries: {}\x1b[0m", entries.len());
    
    Ok(())
}

fn clear_history() -> Result<()> {
    let path = get_history_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
        println!("✅ History cleared.");
    } else {
        println!("No history found to clear.");
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Command Execution
// ─────────────────────────────────────────────────────────────────────────────

fn execute_command(command: &str) -> Result<()> {
    println!("\x1b[90m🚀 Executing...\x1b[0m");
    println!("\x1b[90m─────────────────────────────────────────────────────────\x1b[0m");
    
    // Use shell to execute the command (supports pipes, redirects, etc.)
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", command])
            .output()
            .context("Failed to execute command")?
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .context("Failed to execute command")?
    };
    
    // Print stdout
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    
    // Print stderr
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    
    // Show exit status
    println!("\x1b[90m─────────────────────────────────────────────────────────\x1b[0m");
    if output.status.success() {
        println!("\x1b[32m✅ Command completed successfully\x1b[0m");
    } else {
        println!("\x1b[31m❌ Command failed with exit code: {}\x1b[0m", 
                 output.status.code().unwrap_or(-1));
    }
    
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Interactive Mode
// ─────────────────────────────────────────────────────────────────────────────

async fn run_interactive_mode(config: &Config, explain: bool) -> Result<()> {
    println!("\x1b[1;36m╔═══════════════════════════════════════════════════════════╗\x1b[0m");
    println!("\x1b[1;36m║  WTF Interactive Mode - Write The Formula 🚀            ║\x1b[0m");
    println!("\x1b[1;36m╚═══════════════════════════════════════════════════════════╝\x1b[0m");
    println!("\x1b[90mType your natural language prompts. Commands:\x1b[0m");
    println!("\x1b[90m  • exit, quit, or Ctrl+D to exit\x1b[0m");
    println!("\x1b[90m  • clear to clear screen\x1b[0m");
    println!("\x1b[90m  • help to show this message\x1b[0m");
    println!("\x1b[90m  • After generating a command, use 'y' to run, 'n' to skip, 'e' to edit\x1b[0m");
    println!();

    let mut rl = DefaultEditor::new().context("Failed to initialize readline")?;
    
    // Load history if available
    let history_path = get_history_path().ok().and_then(|p| {
        p.parent().map(|parent| parent.join(".wtf_interactive_history"))
    });
    
    if let Some(ref path) = history_path {
        if path.exists() {
            let _ = rl.load_history(path);
        }
    }

    // Conversation context for better AI responses
    let mut conversation_context: Vec<String> = Vec::new();

    loop {
        match rl.readline("\x1b[1;36mwtf>\x1b[0m ") {
            Ok(line) => {
                let input = line.trim();
                
                // Handle empty input
                if input.is_empty() {
                    continue;
                }

                // Handle special commands
                match input.to_lowercase().as_str() {
                    "exit" | "quit" => {
                        println!("\x1b[90m👋 Goodbye!\x1b[0m");
                        break;
                    }
                    "clear" => {
                        print!("\x1b[2J\x1b[1;1H");
                        continue;
                    }
                    "help" => {
                        println!("\x1b[90mCommands:\x1b[0m");
                        println!("\x1b[90m  exit, quit, Ctrl+D  - Exit interactive mode\x1b[0m");
                        println!("\x1b[90m  clear                - Clear the screen\x1b[0m");
                        println!("\x1b[90m  help                 - Show this help message\x1b[0m");
                        println!("\x1b[90m  <your prompt>        - Generate a shell command\x1b[0m");
                        println!();
                        println!("\x1b[90mAfter generating a command:\x1b[0m");
                        println!("\x1b[90m  y / yes              - Run the command immediately\x1b[0m");
                        println!("\x1b[90m  n / no               - Skip (don't run)\x1b[0m");
                        println!("\x1b[90m  e / edit             - Edit the command (natural language or full command)\x1b[0m");
                        println!();
                        println!("\x1b[90mEdit examples:\x1b[0m");
                        println!("\x1b[90m  • 'only show top 10'  - Natural language modification\x1b[0m");
                        println!("\x1b[90m  • 'change size to 1GB' - Natural language modification\x1b[0m");
                        println!("\x1b[90m  • 'find . -size +1G'  - Direct command replacement\x1b[0m");
                        println!();
                        continue;
                    }
                    _ => {
                        // Add to readline history
                        let _ = rl.add_history_entry(input);
                        
                        // Show loading indicator
                        print!("\x1b[90m⏳ Generating...\x1b[0m\r");
                        io::stdout().flush().ok();
                        
                        // Build prompt with context if available
                        let prompt_with_context = if conversation_context.is_empty() {
                            input.to_string()
                        } else {
                            let context = conversation_context.join("\n");
                            format!("Previous conversation:\n{}\n\nNew request: {}", context, input)
                        };
                        
                        // Get command from AI
                        match get_command(config, &prompt_with_context, explain).await {
                            Ok(result) => {
                                // Clear loading indicator
                                print!("\r\x1b[K");
                                
                                // Strip markdown code blocks if present
                                let command = result.command
                                    .trim()
                                    .trim_start_matches("```bash")
                                    .trim_start_matches("```sh")
                                    .trim_start_matches("```")
                                    .trim_end_matches("```")
                                    .trim()
                                    .to_string();
                                
                                // Save to history
                                if let Err(e) = append_to_history(input, &command) {
                                    eprintln!("\x1b[33mWarning: Failed to save history: {}\x1b[0m", e);
                                }
                                
                                // Display result
                                println!("💡 \x1b[36m{}\x1b[0m", command);
                                
                                if let Some(explanation) = result.explanation {
                                    println!("\x1b[90m📝 {}\x1b[0m", explanation.trim());
                                }
                                
                                // Ask if user wants to run the command
                                let mut final_command = command;
                                loop {
                                    print!("\x1b[90mRun this command? (y/n/e to edit): \x1b[0m");
                                    io::stdout().flush().ok();
                                    
                                    let stdin = io::stdin();
                                    let mut line = String::new();
                                    
                                    match stdin.lock().read_line(&mut line) {
                                        Ok(_) => {
                                            let choice = line.trim().to_lowercase();
                                            match choice.as_str() {
                                                "y" | "yes" => {
                                                    // Execute the command
                                                    execute_command(&final_command)?;
                                                    break;
                                                }
                                                "n" | "no" | "" => {
                                                    println!("\x1b[90mSkipped.\x1b[0m");
                                                    break;
                                                }
                                                "e" | "edit" => {
                                                    // Allow editing the command (supports natural language)
                                                    println!("\x1b[90m💡 Tip: You can use natural language (e.g., 'only show top 10') or type the full command\x1b[0m");
                                                    match rl.readline(&format!("\x1b[90mEdit (current: {}): \x1b[0m", final_command)) {
                                                        Ok(edit_request) => {
                                                            let edit_request = edit_request.trim();
                                                            if edit_request.is_empty() {
                                                                println!("\x1b[90mNo changes made.\x1b[0m");
                                                                continue;
                                                            }
                                                            
                                                            // Check if it looks like a direct command (starts with common commands, has pipes, etc.)
                                                            let looks_like_command = edit_request.contains('|') 
                                                                || edit_request.contains("&&")
                                                                || edit_request.contains(';')
                                                                || edit_request.starts_with("find")
                                                                || edit_request.starts_with("grep")
                                                                || edit_request.starts_with("ls")
                                                                || edit_request.starts_with("cat")
                                                                || edit_request.starts_with("curl")
                                                                || edit_request.starts_with("git")
                                                                || edit_request.starts_with("docker")
                                                                || edit_request.starts_with("kubectl");
                                                            
                                                            if looks_like_command {
                                                                // User provided a direct command, use it as-is
                                                                final_command = edit_request.to_string();
                                                                println!("💡 \x1b[36m{}\x1b[0m", final_command);
                                                            } else {
                                                                // Natural language edit - use AI to modify the command
                                                                print!("\x1b[90m⏳ Applying edit...\x1b[0m\r");
                                                                io::stdout().flush().ok();
                                                                
                                                                let edit_prompt = format!(
                                                                    "Current command: {}\n\nUser wants to modify it: {}\n\nGenerate the modified command. Output ONLY the new command, nothing else.",
                                                                    final_command, edit_request
                                                                );
                                                                
                                                                match get_command(config, &edit_prompt, false).await {
                                                                    Ok(edited_result) => {
                                                                        // Clear loading indicator
                                                                        print!("\r\x1b[K");
                                                                        
                                                                        let new_command = edited_result.command
                                                                            .trim()
                                                                            .trim_start_matches("```bash")
                                                                            .trim_start_matches("```sh")
                                                                            .trim_start_matches("```")
                                                                            .trim_end_matches("```")
                                                                            .trim()
                                                                            .to_string();
                                                                        
                                                                        if !new_command.is_empty() {
                                                                            final_command = new_command;
                                                                            println!("💡 \x1b[36m{}\x1b[0m", final_command);
                                                                        } else {
                                                                            println!("\x1b[33m⚠️  Could not generate modified command. Using your input as-is.\x1b[0m");
                                                                            final_command = edit_request.to_string();
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        // Clear loading indicator
                                                                        print!("\r\x1b[K");
                                                                        eprintln!("\x1b[33m⚠️  Failed to process edit with AI: {}\x1b[0m", e);
                                                                        println!("\x1b[90mUsing your input as direct command.\x1b[0m");
                                                                        final_command = edit_request.to_string();
                                                                    }
                                                                }
                                                            }
                                                            
                                                            // Loop back to ask again
                                                            continue;
                                                        }
                                                        Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                                                            println!("\x1b[90mEdit cancelled.\x1b[0m");
                                                            continue;
                                                        }
                                                        Err(e) => {
                                                            eprintln!("\x1b[31mError: {}\x1b[0m", e);
                                                            break;
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    println!("\x1b[33mInvalid choice. Use 'y' to run, 'n' to skip, or 'e' to edit.\x1b[0m");
                                                    continue;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("\x1b[31mError reading input: {}\x1b[0m", e);
                                            break;
                                        }
                                    }
                                }
                                
                                // Add to conversation context (keep last 3 interactions)
                                conversation_context.push(format!("User: {}\nAssistant: {}", input, final_command));
                                if conversation_context.len() > 3 {
                                    conversation_context.remove(0);
                                }
                                
                                println!();
                            }
                            Err(e) => {
                                // Clear loading indicator
                                print!("\r\x1b[K");
                                eprintln!("\x1b[31m❌ Error: {}\x1b[0m", e);
                                println!();
                            }
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("\x1b[90m\n👋 Interrupted. Use 'exit' or 'quit' to exit.\x1b[0m");
                println!();
            }
            Err(ReadlineError::Eof) => {
                println!("\x1b[90m\n👋 Goodbye!\x1b[0m");
                break;
            }
            Err(err) => {
                eprintln!("\x1b[31m❌ Error: {}\x1b[0m", err);
                break;
            }
        }
    }

    // Save history
    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }
    
    Ok(())
}
