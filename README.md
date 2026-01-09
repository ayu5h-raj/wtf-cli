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
brew install wtf-cli
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

### 1. Configure API Key

**Gemini (Default, Free)**

```bash
# Get a free key at: https://aistudio.google.com/app/apikey
export GEMINI_API_KEY="your-gemini-key"
```

**OpenRouter / OpenAI / Other**

```bash
export WTF_API_KEY="your-api-key"
export WTF_BASE_URL="https://openrouter.ai/api/v1"
export WTF_MODEL="anthropic/claude-3-haiku"
```

### 2. Enable Shell Integration (Required)

Add this to your `~/.zshrc` (or `~/.bashrc`) to enable the buffer magic:

```bash
eval "$(command wtf --init zsh)"
```

Then reload: `source ~/.zshrc`

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



## How It Works

1. You type a natural language prompt
2. WTF sends it to your configured AI (Gemini, OpenRouter, OpenAI, etc.)
3. The AI returns a shell command
4. You review, edit, or run it

## License

MIT

---

Made with ❤️ by [Ayush Raj](https://github.com/ayu5h-raj)
