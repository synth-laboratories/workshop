#!/usr/bin/env bash
# Sourced by the local entrypoints; never edits the user's shell profile.
workshop_brew="$(command -v brew || true)"
if [[ -z "$workshop_brew" && -x /opt/homebrew/bin/brew ]]; then
  workshop_brew=/opt/homebrew/bin/brew
fi
if [[ -n "$workshop_brew" ]]; then
  workshop_prefix="$("$workshop_brew" --prefix)"
  export PATH="$workshop_prefix/opt/node@20/bin:$workshop_prefix/opt/python@3.12/libexec/bin:$workshop_prefix/opt/rustup/bin:$workshop_prefix/bin:$PATH"
fi
unset workshop_brew workshop_prefix
