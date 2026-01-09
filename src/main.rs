use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::env;

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

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(rename = "systemInstruction")]
    system_instruction: Content,
}

#[derive(Serialize, Deserialize)]
struct Content {
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
    content: Content,
}

#[derive(Deserialize)]
struct GeminiError {
    message: String,
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

use std::io::{self, Write};
use std::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let prompt = args.prompt.join(" ");

    let api_key = env::var("GEMINI_API_KEY")
        .context("GEMINI_API_KEY environment variable not set.\n\nTo get an API key:\n1. Visit https://aistudio.google.com/app/apikey\n2. Create a free API key\n3. Run: export GEMINI_API_KEY='your-key-here'")?;

    let command = get_command_from_gemini(&api_key, &prompt).await?;
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
            // Copy to clipboard and inform user
            let _ = Command::new("sh")
                .arg("-c")
                .arg(format!("printf '%s' '{}' | pbcopy", command.replace("'", "'\\''")))
                .status();
            println!("📋 Command copied to clipboard. Paste and edit it!");
        }
        _ => {
            // Copy to clipboard on cancel too
            let _ = Command::new("sh")
                .arg("-c")
                .arg(format!("printf '%s' '{}' | pbcopy", command.replace("'", "'\\''")))
                .status();
            println!("📋 Cancelled. Command copied to clipboard.");
        }
    }

    Ok(())
}


async fn get_command_from_gemini(api_key: &str, prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![Part {
                text: prompt.to_string(),
            }],
        }],
        system_instruction: Content {
            parts: vec![Part {
                text: SYSTEM_PROMPT.to_string(),
            }],
        },
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key={}",
        api_key
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
