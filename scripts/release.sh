#!/usr/bin/env bash
# Creates a Signet release: checks everything, then creates and pushes the `vX.Y.Z` tag.
# The tag starts the Build workflow, which builds the packages (macOS, Linux, Windows, Android)
# and publishes them in a GitHub release.
#
# Usage: scripts/release.sh <version> [--dry-run] [--yes]
#        npm run release -- <version> [--dry-run] [--yes]
#   <version>   X.Y.Z or X.Y.Z-pre.N (a leading v is accepted), e.g. 0.2.0 or 1.0.0-beta.1
#   --dry-run   run the checks only, create nothing
#   --yes       do not ask for confirmation
#
# Checks, in order:
#   - the version is valid and matches every package.json and app/mobile/app.json (scripts/check-version.mjs)
#   - we are on main, the working tree is clean and in sync with origin/main
#   - the tag does not exist yet, and the version is higher than the latest v… tag (local or on origin)
#
# Bump the version first (all package.json files + app/mobile/app.json), commit and push, then run this script.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -t 1 ]]; then
  red=$'\033[31m' green=$'\033[32m' bold=$'\033[1m' dim=$'\033[2m' reset=$'\033[0m'
else
  red='' green='' bold='' dim='' reset=''
fi

die() {
  echo "${red}✗ $*${reset}" >&2
  exit 1
}
ok() { echo "${green}✓${reset} $*"; }

usage() {
  sed -n '6,11p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

# --- Arguments ---------------------------------------------------------------

version='' dry_run=false assume_yes=false
for arg in "$@"; do
  case "$arg" in
    -h | --help) usage 0 ;;
    --dry-run) dry_run=true ;;
    -y | --yes) assume_yes=true ;;
    -*) die "Unknown option: $arg" ;;
    *)
      [[ -z $version ]] || die "Only one version expected (got $version and $arg)."
      version=${arg#v}
      ;;
  esac
done
[[ -n $version ]] || usage 1
tag="v$version"

# --- Version comparison (semver precedence) -----------------------------------

# Prints 1, 0 or -1 depending on whether prerelease $1 is higher, equal or lower than $2 (both non-empty).
compare_prerelease() {
  local IFS=.
  local -a a=($1) b=($2)
  local i=0 x y
  while :; do
    if ((i >= ${#a[@]} && i >= ${#b[@]})); then echo 0 && return; fi
    if ((i >= ${#a[@]})); then echo -1 && return; fi # fewer identifiers = lower (beta < beta.1)
    if ((i >= ${#b[@]})); then echo 1 && return; fi
    x=${a[i]} y=${b[i]}
    if [[ $x =~ ^[0-9]+$ && $y =~ ^[0-9]+$ ]]; then
      if ((10#$x > 10#$y)); then echo 1 && return; fi
      if ((10#$x < 10#$y)); then echo -1 && return; fi
    elif [[ $x =~ ^[0-9]+$ ]]; then
      echo -1 && return # numbers come before words (1 < beta)
    elif [[ $y =~ ^[0-9]+$ ]]; then
      echo 1 && return
    elif [[ $x != "$y" ]]; then
      if [[ $x > $y ]]; then echo 1; else echo -1; fi
      return
    fi
    i=$((i + 1))
  done
}

# Prints 1, 0 or -1 depending on whether version $1 is higher, equal or lower than $2.
compare_versions() {
  local a_pre='' b_pre=''
  [[ $1 == *-* ]] && a_pre=${1#*-}
  [[ $2 == *-* ]] && b_pre=${2#*-}
  local IFS=.
  local -a a=(${1%%-*}) b=(${2%%-*})
  local i
  for i in 0 1 2; do
    if ((10#${a[i]} > 10#${b[i]})); then echo 1 && return; fi
    if ((10#${a[i]} < 10#${b[i]})); then echo -1 && return; fi
  done
  # Same X.Y.Z: the release is higher than its prereleases (1.0.0-beta < 1.0.0).
  if [[ -z $a_pre && -z $b_pre ]]; then echo 0
  elif [[ -z $a_pre ]]; then echo 1
  elif [[ -z $b_pre ]]; then echo -1
  else compare_prerelease "$a_pre" "$b_pre"
  fi
}

semver_re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'

# --- Checks --------------------------------------------------------------------

echo "${bold}Signet release $tag${reset}"

node scripts/check-version.mjs "$version" >/dev/null || die "Version check failed (see above)."
ok "Version $version matches every package.json and app/mobile/app.json"

branch=$(git rev-parse --abbrev-ref HEAD)
[[ $branch == main ]] || die "Releases are made from main (current branch: $branch)."
[[ -z $(git status --porcelain) ]] || die "The working tree has uncommitted changes. Commit or stash them first."

git fetch --quiet --tags origin main || die "Could not fetch origin."
head=$(git rev-parse HEAD)
remote_head=$(git rev-parse origin/main)
if [[ $head != "$remote_head" ]]; then
  if git merge-base --is-ancestor "$head" "$remote_head"; then
    die "main is behind origin/main. Pull first."
  else
    die "main has commits not pushed to origin/main. Push first (the release must be built from pushed code)."
  fi
fi
ok "On main, clean, in sync with origin ($(git rev-parse --short HEAD))"

# Every v… tag, local or on origin (the remote list catches a tag pushed from elsewhere and not fetched).
all_tags=$({
  git tag -l 'v*'
  git ls-remote --tags --refs origin 'v*' | sed 's#.*refs/tags/##'
} | sort -u)

if grep -qx "$tag" <<<"$all_tags"; then
  die "Tag $tag already exists. Bump the version."
fi

latest=''
while IFS= read -r t; do
  v=${t#v}
  [[ $v =~ $semver_re ]] || continue
  if [[ -z $latest || $(compare_versions "$v" "$latest") == 1 ]]; then latest=$v; fi
done <<<"$all_tags"

if [[ -n $latest ]]; then
  [[ $(compare_versions "$version" "$latest") == 1 ]] || die "Version $version is not higher than the latest release (v$latest)."
  ok "Higher than the latest release (v$latest)"
else
  ok "First release (no v… tag yet)"
fi

# --- Tag and push --------------------------------------------------------------

if $dry_run; then
  echo
  echo "${bold}Release notes (what's new)${reset}"
  bash scripts/release-notes.sh "$tag" HEAD | sed 's/^/  /'
  echo
  echo "${dim}Dry run: nothing created. Without --dry-run, $tag would be tagged on $(git log -1 --format='%h %s').${reset}"
  exit 0
fi

echo
echo "  Tag     ${bold}$tag${reset}$([[ $version == *-* ]] && echo ' (prerelease)')"
echo "  Commit  $(git log -1 --format='%h %s')"
[[ -n $latest ]] && echo "  Since   v$latest: $(git rev-list --count "v$latest..HEAD" 2>/dev/null || echo '?') commits"
echo

if ! $assume_yes; then
  read -r -p "Create and push $tag? This starts the build and publishes the release. [y/N] " answer
  [[ $answer == [yY] || $answer == [yY][eE][sS] ]] || die "Cancelled."
fi

git tag -a "$tag" -m "Signet $version"
if ! git push --quiet origin "$tag"; then
  git tag -d "$tag" >/dev/null
  die "Push failed: local tag $tag removed."
fi
ok "Tag $tag pushed"

repo_url=$(git remote get-url origin | sed -E 's#^git@github\.com:#https://github.com/#; s#\.git$##')
echo
echo "Build in progress: $repo_url/actions/workflows/build.yml"
echo "Release (once the builds pass): $repo_url/releases/tag/$tag"
