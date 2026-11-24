#!/usr/bin/env bash
# Apply .github/labels.yml to a GitHub repository: create each label, or update its colour and
# description when it exists (`gh label create --force`). Safe to run again. Labels that are not in
# the file are left alone.
#
#   scripts/labels.sh OWNER/REPO            # needs `gh auth login` with write access to the repository
#   scripts/labels.sh --dry-run OWNER/REPO  # print the gh commands, change nothing
set -euo pipefail

usage() { echo "usage: $0 [--dry-run] OWNER/REPO" >&2; exit 2; }

dry_run=0
if [[ "${1:-}" == "--dry-run" ]]; then dry_run=1; shift; fi
[[ $# -eq 1 ]] || usage
repo=$1
[[ "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo "not OWNER/REPO: $repo" >&2; usage; }

file="$(cd "$(dirname "$0")/.." && pwd)/.github/labels.yml"
[[ -f "$file" ]] || { echo "missing $file" >&2; exit 1; }

# One "name<TAB>color<TAB>description" line per label. The file keeps each key on one line with a
# double-quoted value (see its header), so a line-based read is enough.
labels=$(awk '
  function val(s) { sub(/^[^:]*:[ ]*"/, "", s); sub(/"[ ]*$/, "", s); return s }
  /^- name: "/        { if (name != "") print name "\t" color "\t" desc; name = val($0); color = ""; desc = "" ; next }
  /^  color: "/       { color = val($0); next }
  /^  description: "/ { desc = val($0); next }
  END                 { if (name != "") print name "\t" color "\t" desc }
' "$file")

count=0
while IFS=$'\t' read -r name color desc; do
  if [[ -z "$name" || ! "$color" =~ ^[0-9a-fA-F]{6}$ ]]; then
    echo "bad entry in $file: name='$name' color='$color'" >&2
    exit 1
  fi
  cmd=(gh label create "$name" --repo "$repo" --color "$color" --description "$desc" --force)
  if [[ $dry_run -eq 1 ]]; then
    printf '%q ' "${cmd[@]}"; echo
  else
    "${cmd[@]}"
  fi
  count=$((count + 1))
done <<< "$labels"

[[ $count -gt 0 ]] || { echo "no labels found in $file" >&2; exit 1; }
echo "$count labels $([[ $dry_run -eq 1 ]] && echo "to apply to" || echo "applied to") $repo" >&2
