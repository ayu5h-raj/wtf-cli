# WTF - Write The Formula 🚀

AI-powered natural language to shell command translator. Describe what you want, get the command.

> **WTF** = *Write The Formula*

## Demo

```bash
$ wtf "show my ip address"
💡 curl -s ifconfig.me
$ curl -s ifconfig.me   # ← command appears in your buffer, ready to run!
203.0.113.42
```

The command is placed directly in your terminal — just press **Enter** to run, or edit it first.

## Installation

### Homebrew (macOS) — Recommended

```bash
brew tap ayu5h-raj/tap
brew install wtf
```

### Cargo

```bash
cargo install wtf
```

### Manual Download

Download from [Releases](https://github.com/ayu5h-raj/wtf-cli/releases), extract, and move to your PATH:

```bash
tar -xzf wtf-macos-arm64-v0.1.0.tar.gz
sudo mv wtf /usr/local/bin/
```

## Setup

### Option 1: Gemini (Default, Free)

```bash
# Get a free key at: https://aistudio.google.com/app/apikey
export GEMINI_API_KEY="your-gemini-key"
```

### Option 2: OpenRouter / OpenAI / Azure / Ollama

```bash
export WTF_API_KEY="your-api-key"
export WTF_BASE_URL="https://openrouter.ai/api/v1"  # or your provider's URL
export WTF_MODEL="anthropic/claude-3-haiku"          # or any model
```

| Variable | Default | Description |
|----------|---------|-------------|
| `WTF_API_KEY` | - | API key (or use `GEMINI_API_KEY`) |
| `WTF_BASE_URL` | Gemini URL | Custom base URL (enables OpenAI-compatible mode) |
| `WTF_MODEL` | `gemini-2.0-flash` | Model to use |

Add to your `~/.zshrc` to persist.

## Usage

```bash
wtf "your question here"
```

The command appears in your buffer — press **Enter** to run or edit it first.

### Examples

```bash
wtf "find files larger than 100MB"
wtf "compress this folder"
wtf "kill process on port 3000"
wtf "show disk usage"
wtf "what is 2+2"
```

## Oh My Zsh Plugin

If you're using Oh My Zsh, the plugin is already giving you the best experience:

```bash
# The ?? alias also works
?? "show my ip"
```

> **Note:** If you installed via Homebrew AND have the plugin, the plugin takes over for the smoother buffer experience. To use the raw binary with y/n/e prompts, call it directly: `/opt/homebrew/bin/wtf "prompt"`

## Manual Install (Without Plugin)

If you're NOT using Oh My Zsh, the raw binary shows an interactive prompt:

```bash
$ /opt/homebrew/bin/wtf "show my ip"

💡 Suggested command:

   curl -s ifconfig.me

Execute? [yes/No/edit]: y
▶️  Running...
```

| Key | Action |
|-----|--------|
| `y` | Run the command |
| `e` | Copy to clipboard for editing |
| `n` | Cancel (copies to clipboard) |

## How It Works

1. You type a natural language prompt
2. WTF sends it to your configured AI (Gemini, OpenRouter, OpenAI, etc.)
3. The AI returns a shell command
4. You review, edit, or run it

## License

MIT

---

Made with ❤️ by [Ayush Raj](https://github.com/ayu5h-raj)
