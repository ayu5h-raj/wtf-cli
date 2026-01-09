# WTF (Write The Formula) - AI-powered natural language to shell command translator
# https://github.com/ayu5h-raj/wtf-cli

# Find the binary path
_wtf_bin=""
if [[ -x "$HOME/.cargo/bin/wtf" ]]; then
    _wtf_bin="$HOME/.cargo/bin/wtf"
elif [[ -x "$HOME/Documents/github/quickcmd/target/release/wtf" ]]; then
    _wtf_bin="$HOME/Documents/github/quickcmd/target/release/wtf"
elif [[ -x "/usr/local/bin/wtf" ]]; then
    _wtf_bin="/usr/local/bin/wtf"
fi

if [[ -z "$_wtf_bin" ]]; then
    echo "⚠️  wtf binary not found. Install with: cargo install --path ~/Documents/github/quickcmd"
fi

# Main function: translates natural language to shell command
function _wtf_run() {
    if [[ -z "$1" ]]; then
        echo "Usage: wtf <natural language prompt>"
        echo "Example: wtf show my ip address"
        return 1
    fi

    # Check for API key (WTF_API_KEY or GEMINI_API_KEY)
    if [[ -z "$WTF_API_KEY" && -z "$GEMINI_API_KEY" ]]; then
        echo "❌ API key not set."
        echo ""
        echo "Set one of these:"
        echo "  export WTF_API_KEY='your-key'       # For any provider"
        echo "  export GEMINI_API_KEY='your-key'    # For Gemini"
        echo ""
        echo "Get a free Gemini key: https://aistudio.google.com/app/apikey"
        return 1
    fi

    # Get the command from AI
    local cmd
    cmd=$("$_wtf_bin" --raw "$@" 2>&1)
    local exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        echo "❌ Error: $cmd"
        return 1
    fi

    # Show the command and put it directly in the buffer
    echo "💡 \033[36m$cmd\033[0m"
    print -z "$cmd"
}

# Aliases
alias 'wtf'='_wtf_run'
alias '??'='_wtf_run'
