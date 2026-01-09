# WTF - Write The Formula 🚀

AI-powered natural language to shell command translator. Ask in plain English, get the command.

> **WTF** = *Write The Formula*

## Demo

```bash
$ wtf "show my ip address"
💡 curl -s ifconfig.me
$ curl -s ifconfig.me
203.0.113.42
```

## Installation

### 1. Install the CLI

```bash
# Build from source
cargo build --release

# Install globally
cargo install --path .
```

### 2. Get a Gemini API Key

1. Visit [Google AI Studio](https://aistudio.google.com/app/apikey)
2. Create a free API key
3. Add to your `~/.zshrc`:

```bash
export GEMINI_API_KEY='your-key-here'
```

### 3. Install the Zsh Plugin

```bash
# Symlink to Oh My Zsh custom plugins
ln -s ~/Documents/github/quickcmd ~/.oh-my-zsh/custom/plugins/quickcmd

# Add to plugins in ~/.zshrc
plugins=(... quickcmd)

# Reload
source ~/.zshrc
```

## Usage

```bash
wtf "find files larger than 100MB"
wtf "compress this folder"
wtf "kill process on port 3000"

# Or use the ?? alias
?? "show disk usage"
```

The command appears in your terminal buffer — press **Enter** to run or edit it first.

## License

MIT
