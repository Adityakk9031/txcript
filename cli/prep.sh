#!/usr/bin/env bash
# Prep txcript for interactive shells.
#
#   cli/prep.sh            # install the binary, then wire up ~/.zshrc and ~/.bashrc
#
# What the shell blocks provide:
#   ctrl+shift+r  open a txcript session picker scoped to the current folder
#
# In legacy terminal encoding ctrl+shift+r and ctrl+r send the same byte, so
# the blocks enable the kitty keyboard protocol (progressive enhancement
# flag 1, "disambiguate escape codes") only while the line editor is reading,
# using the stateless `CSI = u` set form (the same mechanism fish 4 uses).
# Ghostty, kitty, and WezTerm speak the protocol; terminals that don't ignore
# the sequences, and ctrl+shift+r degrades to plain ctrl+r (history search).
#
# Re-running is safe: each rc file carries one marker-delimited block that is
# replaced in place.
set -euo pipefail

cli_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

begin='# >>> txcript prep >>>'
end='# <<< txcript prep <<<'

# Replace the marker-delimited block in $1 with $2 (appends if absent).
install_block() {
  local file=$1 content=$2 tmp
  touch "$file"
  tmp=$(mktemp)
  awk -v b="$begin" -v e="$end" '
    $0 == b { skip = 1 }
    skip { if ($0 == e) skip = 0; next }
    { print }
  ' "$file" >"$tmp"
  printf '%s\n%s\n%s\n' "$begin" "$content" "$end" >>"$tmp"
  mv "$tmp" "$file"
}

zsh_block=$(cat <<'ZSH'
# Managed by txcript's cli/prep.sh — edits here are overwritten on re-run.
# ctrl+shift+r: pick a txcript session recorded in the current folder.
if command -v txcript >/dev/null; then
  txcript-cwd-picker() {
    printf '\e[=0;1u'                       # legacy keys for the picker's raw reader
    txcript query --cwd "$PWD" < /dev/tty
    printf '\e[=1;1u'
    zle reset-prompt
  }
  zle -N txcript-cwd-picker
  if [[ $TERM == (*kitty*|*ghostty*|*wezterm*) ]]; then
    # Disambiguate ctrl+shift+r from ctrl+r while zle reads a line.
    txcript--keys-on()  { printf '\e[=1;1u' }
    txcript--keys-off() { printf '\e[=0;1u' }
    autoload -Uz add-zle-hook-widget
    add-zle-hook-widget line-init txcript--keys-on
    add-zle-hook-widget line-finish txcript--keys-off
    # Under flag 1, ctrl/alt+letter arrive as CSI-u sequences; feed the
    # legacy bytes back so every existing binding still fires.
    () {
      local i
      for i in {97..122}; do
        bindkey -s "\e[${i};5u" "${(#):-$((i - 96))}"     # ctrl+letter
        bindkey -s "\e[${i};3u" "\e${(#):-$i}"            # alt+letter
        bindkey -s "\e[${i};7u" "\e${(#):-$((i - 96))}"   # ctrl+alt+letter
      done
    }
    bindkey '\e[99;5u' send-break                         # ctrl+c aborts the line
    bindkey -s '\e[27u' '\e'                              # bare ESC
    bindkey '\e[114;6u' txcript-cwd-picker                # ctrl+shift+r
  fi
fi
ZSH
)

bash_block=$(cat <<'BASH'
# Managed by txcript's cli/prep.sh — edits here are overwritten on re-run.
# ctrl+shift+r: pick a txcript session recorded in the current folder.
if command -v txcript >/dev/null && [[ $- == *i* ]]; then
  eval "$(txcript completion bash)"
  txcript_cwd_picker() {
    printf '\e[=0;1u'                       # legacy keys for the picker's raw reader
    txcript query --cwd "$PWD" < /dev/tty
    printf '\e[=1;1u'
  }
  case $TERM in (*kitty*|*ghostty*|*wezterm*)
    # Disambiguate ctrl+shift+r from ctrl+r while readline reads a line:
    # on when the prompt is shown, off just before the command runs.
    PROMPT_COMMAND="printf '\\e[=1;1u'${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
    PS0="\$(printf '\\e[=0;1u')${PS0-}"
    # Under flag 1, ctrl/alt+letter arrive as CSI-u sequences; feed the
    # legacy bytes back so every existing binding still fires.
    _txcript_letters=abcdefghijklmnopqrstuvwxyz
    for ((_txcript_i = 0; _txcript_i < 26; _txcript_i++)); do
      _txcript_l=${_txcript_letters:_txcript_i:1}
      _txcript_k=$((97 + _txcript_i))
      bind "\"\e[${_txcript_k};5u\": \"\C-${_txcript_l}\""     # ctrl+letter
      bind "\"\e[${_txcript_k};3u\": \"\e${_txcript_l}\""      # alt+letter
      bind "\"\e[${_txcript_k};7u\": \"\e\C-${_txcript_l}\""   # ctrl+alt+letter
    done
    unset _txcript_letters _txcript_i _txcript_l _txcript_k
    bind '"\e[99;5u": abort'                                   # ctrl+c
    bind '"\e[27u": "\e"'                                      # bare ESC
    bind -x '"\e[114;6u": txcript_cwd_picker'                  # ctrl+shift+r
  esac
fi
BASH
)

echo "installing txcript from $cli_dir..."
cargo install --quiet --path "$cli_dir"

install_block "$HOME/.zshrc" "$zsh_block"
echo "wired ~/.zshrc"
install_block "$HOME/.bashrc" "$bash_block"
echo "wired ~/.bashrc"

echo "done — restart your shell (or source your rc) and press ctrl+shift+r in a project folder"
