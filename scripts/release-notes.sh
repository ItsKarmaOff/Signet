#!/usr/bin/env bash
# Prints the "What's new" part of a Signet release's notes (Markdown), built from the commit messages since the previous
# release: <type>(<scope>): <description>, see docs/git/commit-policy.md. Used by the Build workflow when it publishes a
# release; scripts/release.sh --dry-run shows a preview.
#
# Usage: scripts/release-notes.sh <tag> [<commit>]
#   <tag>      the release tag, e.g. v0.2.0
#   <commit>   the commit being released, if the tag doesn't exist yet (default: <tag>)
#
# Previous release: the closest v… tag before <commit>. A stable release skips the prereleases, so 1.0.0 lists everything
# since 0.9.0, beta changes included. Without a previous release, the notes say it's the first one.
# Only the changes users see are listed (feat, fix, perf, security); docs, ci, tests, refactors… are left out.
set -euo pipefail

cd "$(dirname "$0")/.."

tag=${1:?Usage: scripts/release-notes.sh <tag> [<commit>]}
commit=${2:-$tag}

# Repository page, for the full changelog link: GH_REPO in the workflow, otherwise the origin remote.
if [[ -n ${GH_REPO:-} ]]; then
  repo_url="https://github.com/$GH_REPO"
else
  repo_url=$(git remote get-url origin | sed -E 's#^git@github\.com:#https://github.com/#; s#\.git$##')
fi

# bash 3.2 (macOS) with set -u: an empty array is expanded as ${exclude[@]+"${exclude[@]}"}.
exclude=()
[[ $tag == *-* ]] || exclude=(--exclude 'v*-*')
previous=$(git describe --tags --abbrev=0 --match 'v*' ${exclude[@]+"${exclude[@]}"} "$commit^" 2>/dev/null || true)

if [[ -z $previous ]]; then
  echo "First public release of Signet."
  exit 0
fi

# One list per section (bash 3.2 on macOS: no associative arrays).
security='' feat='' fix='' perf=''
pattern='^([a-z]+)(\(([a-z0-9._-]+)\))?(!?): (.+)$'
while IFS=' ' read -r hash subject; do
  [[ $subject =~ $pattern ]] || continue
  type=${BASH_REMATCH[1]} scope=${BASH_REMATCH[3]} breaking=${BASH_REMATCH[4]} description=${BASH_REMATCH[5]}
  line="- "
  [[ -n $breaking ]] && line+="**Breaking:** "
  [[ -n $scope ]] && line+="**$scope**: "
  # Capital first letter (commit messages start lowercase).
  line+="$(tr '[:lower:]' '[:upper:]' <<<"${description:0:1}")${description:1} ($hash)"$'\n'
  case $type in
    security) security+=$line ;;
    feat) feat+=$line ;;
    fix) fix+=$line ;;
    perf) perf+=$line ;;
  esac
done <<<"$(git log --no-merges --reverse --format='%h %s' "$previous..$commit")"

section() {
  [[ -n $2 ]] || return 0
  printf '### %s\n\n%s\n' "$1" "$2"
}

if [[ -z $security$feat$fix$perf ]]; then
  echo "No user-facing changes: maintenance only."
  echo
else
  section "Security" "$security"
  section "New" "$feat"
  section "Fixes" "$fix"
  section "Performance" "$perf"
fi
echo "**Full changelog**: $repo_url/compare/$previous...$tag"
