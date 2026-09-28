#!/usr/bin/env bash
#
# Every released CHANGELOG section must still match the tag that shipped it,
# and the Cargo version must never move backwards.
#
# Why this is a script and not a #[test]: it needs `git show <tag>`, and tests
# must not shell out to git.
#
# The hazard it catches: a branch that predates the last tag carries its
# entries under [Unreleased], which is exactly where the newest released
# section now sits, so git merges the two together *cleanly*. It has happened
# five times in this repo, in three different shapes — silently inserted into a
# published section, conflicted into one, and deleting one outright — and every
# time the merge looked successful.
#
# Two destinations. The changelog that shipped every tag up to HISTORY_LAST_TAG
# was moved, byte for byte, to HISTORY_FILE. That archive must stay identical
# to CHANGELOG.md at HISTORY_SOURCE, and those tags' sections are compared
# against it. Tags cut after the move are compared against the active
# CHANGELOG.md.
#
# HISTORY_LAST_TAG is the highest section in the archive, whether or not a tag
# exists for it yet: 1.23.0 was written there before any v1.23.0 tag. Such a tag
# may be cut after the move, when its CHANGELOG.md is already the active file,
# so an archived version reads its own section from the archive in the tag.
#
# The archive's identity (the snapshot) and the provenance of each published
# section are separate checks. A section amended after its tag, before the
# snapshot, legitimately differs from that tag; it passes only through a row in
# RECONCILIATIONS, whose digest is pinned here. Each row pins the tag object and
# commit, the amending commit, the digests of both sections, and the amendment
# as a pure insertion (reconciliations/<tag>.inserted after line insert_after).
# All of it is re-derived from git on every run, whatever tag window is chosen.
#
# Tags without a section to keep are listed in UNSHIPPED, also pinned, and are
# skipped only after the reason is re-derived from git.
#
# All tags are checked. CHANGELOG_TAGS_TO_CHECK=N limits the comparison to the
# N newest, for a quick local run only.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

HISTORY_FILE=docs/history/CHANGELOG.original.md
HISTORY_SOURCE=bcfe06b8065f37458dbc8beeb37fabf4eba63274
HISTORY_LAST_TAG=v1.23.0
ACTIVE_FILE=CHANGELOG.md
RECONCILIATIONS=docs/history/changelog-reconciliations.tsv
RECONCILIATIONS_SHA256=2c51bc84f478f8ae889337d79a9f77854e4e7fc0ddfac375dff7f8af92509e37
UNSHIPPED=docs/history/changelog-unshipped-tags.tsv
UNSHIPPED_SHA256=97c2b7aac4bfeb49b2574074ae205af7bc8bb0f19cde015c95706200960a448b

fail=0

# The archive is frozen: any byte that differs from the source commit is an
# edit to published history, whichever section it lands in.
if [ ! -f "$HISTORY_FILE" ]; then
  echo "error: $HISTORY_FILE is missing; it must hold CHANGELOG.md from $HISTORY_SOURCE byte for byte"
  fail=1
elif ! original=$(git show "$HISTORY_SOURCE:CHANGELOG.md" 2>/dev/null); then
  echo "error: cannot read CHANGELOG.md at $HISTORY_SOURCE; a full-history checkout is required"
  fail=1
elif ! git show "$HISTORY_SOURCE:CHANGELOG.md" | cmp -s - "$HISTORY_FILE"; then
  echo "error: $HISTORY_FILE no longer matches CHANGELOG.md at $HISTORY_SOURCE:"
  diff <(printf '%s\n' "$original") "$HISTORY_FILE" | sed 's/^/    /'
  fail=1
fi

# True when version $1 is at or below version $2.
version_le() {
  [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -1)" = "$1" ]
}

tags=$(git tag --list 'v*' --sort=-v:refname)
[ -z "$tags" ] && { echo "error: no v* tags found — nothing to compare"; exit 1; }
if [ -n "${CHANGELOG_TAGS_TO_CHECK+set}" ]; then
  case $CHANGELOG_TAGS_TO_CHECK in
    '' | 0* | *[!0-9]*)
      echo "error: CHANGELOG_TAGS_TO_CHECK must be a positive integer, got '$CHANGELOG_TAGS_TO_CHECK'"
      exit 1
      ;;
  esac
  tags=$(printf '%s\n' "$tags" | head -n "$CHANGELOG_TAGS_TO_CHECK")
  echo "note: comparing only the $CHANGELOG_TAGS_TO_CHECK newest tags (CHANGELOG_TAGS_TO_CHECK)"
fi

# The [$1] section of text on stdin, from its heading to the next one.
section() {
  sed -n "/^## \[$1\]/,/^## \[/p"
}

# The [$2] section that tag $1 shipped. An archived version tagged after the
# move finds it in the archive inside the tag.
published_section() {
  local s
  s=$(git show "$1:CHANGELOG.md" 2>/dev/null | section "$2")
  if [ -z "$s" ] && version_le "$2" "${HISTORY_LAST_TAG#v}"; then
    s=$(git show "$1:$HISTORY_FILE" 2>/dev/null | section "$2")
  fi
  printf '%s' "$s"
}

sha256() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256; else sha256sum; fi | cut -d' ' -f1
}

# Tags whose archived section differs from the tag only by a verified
# reconciliation row.
reconciled=" "

# $1 row file, $2 tag, $3 message: reports a pinned row that does not hold.
reject() {
  echo "error: $2 in $1: $3"
  fail=1
}

# $1 tag, $2 tag object, $3 tag commit, $4 amending commit, $5 published
# digest, $6 archived digest, $7 insertion offset. Succeeds only when every
# pin is re-derived from git and the archive.
verify_reconciliation() {
  local tag=$1 v=${1#v} a b before after lines
  [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { reject "$RECONCILIATIONS" "$tag" "not a release tag name"; return 1; }
  case " $reconciled " in *" $tag "*) reject "$RECONCILIATIONS" "$tag" "duplicate row"; return 1 ;; esac
  version_le "$v" "${HISTORY_LAST_TAG#v}" \
    || { reject "$RECONCILIATIONS" "$tag" "only sections in $HISTORY_FILE can be reconciled"; return 1; }
  [ "$(git rev-parse -q --verify "refs/tags/$tag" 2>/dev/null)" = "$2" ] \
    || { reject "$RECONCILIATIONS" "$tag" "the tag no longer points at object $2"; return 1; }
  [ "$(git rev-parse -q --verify "$tag^{commit}" 2>/dev/null)" = "$3" ] \
    || { reject "$RECONCILIATIONS" "$tag" "the tag no longer resolves to commit $3"; return 1; }
  git merge-base --is-ancestor "$3" "$4" 2>/dev/null && [ "$3" != "$4" ] \
    || { reject "$RECONCILIATIONS" "$tag" "amending commit $4 does not follow the tag"; return 1; }
  git merge-base --is-ancestor "$4" "$HISTORY_SOURCE" 2>/dev/null \
    || { reject "$RECONCILIATIONS" "$tag" "amending commit $4 is not in the history of $HISTORY_SOURCE"; return 1; }
  a=$(published_section "$tag" "$v")
  b=$(section "$v" < "$HISTORY_FILE")
  [ -n "$a" ] && [ -n "$b" ] || { reject "$RECONCILIATIONS" "$tag" "a section is missing"; return 1; }
  [ "$a" != "$b" ] || { reject "$RECONCILIATIONS" "$tag" "stale row: the archive already matches the tag"; return 1; }
  [ "$(printf '%s\n' "$a" | sha256)" = "$5" ] \
    || { reject "$RECONCILIATIONS" "$tag" "the tag's [$v] section no longer has digest $5"; return 1; }
  [ "$(printf '%s\n' "$b" | sha256)" = "$6" ] \
    || { reject "$RECONCILIATIONS" "$tag" "the archived [$v] section no longer has digest $6"; return 1; }
  before=$(git show "$4^:CHANGELOG.md" 2>/dev/null | section "$v")
  after=$(git show "$4:CHANGELOG.md" 2>/dev/null | section "$v")
  [ "$before" = "$a" ] && [ "$after" = "$b" ] \
    || { reject "$RECONCILIATIONS" "$tag" "commit $4 does not turn the tag's section into the archived one"; return 1; }
  lines=$(printf '%s\n' "$a" | wc -l | tr -d ' ')
  case $7 in '' | *[!0-9]*) reject "$RECONCILIATIONS" "$tag" "insert_after is not a line number"; return 1 ;; esac
  [ "$7" -le "$lines" ] || { reject "$RECONCILIATIONS" "$tag" "insert_after is past the section"; return 1; }
  [ -f "docs/history/reconciliations/$tag.inserted" ] \
    || { reject "$RECONCILIATIONS" "$tag" "docs/history/reconciliations/$tag.inserted is missing"; return 1; }
  [ "$( { printf '%s\n' "$a" | head -n "$7"
          cat "docs/history/reconciliations/$tag.inserted"
          printf '%s\n' "$a" | tail -n +"$(($7 + 1))"; } | sha256)" = "$6" ] \
    || { reject "$RECONCILIATIONS" "$tag" "inserting reconciliations/$tag.inserted after line $7 does not give the archived section"; return 1; }
  reconciled="$reconciled$tag "
}

# The rows are only read once their file matches the pinned digest, and every
# row is verified whether or not its tag falls in the window compared below.
if [ ! -f "$RECONCILIATIONS" ]; then
  echo "error: $RECONCILIATIONS is missing"
  fail=1
elif [ "$(sha256 < "$RECONCILIATIONS")" != "$RECONCILIATIONS_SHA256" ]; then
  echo "error: $RECONCILIATIONS does not match its pinned digest $RECONCILIATIONS_SHA256"
  fail=1
elif [ -f "$HISTORY_FILE" ]; then
  # shellcheck disable=SC2094 # reject only names the file in its message
  while IFS=$'\t' read -r tag obj commit amend published archived insert_after extra; do
    case $tag in '' | '#'*) continue ;; esac
    if [ -n "$extra" ] || [ -z "$insert_after" ]; then
      reject "$RECONCILIATIONS" "$tag" "expected 7 tab-separated fields"
      continue
    fi
    verify_reconciliation "$tag" "$obj" "$commit" "$amend" "$published" "$archived" "$insert_after"
  done < "$RECONCILIATIONS"
fi

# Tags cut but abandoned before their release ever shipped are `superseded`:
# v1.20.0 was caught by verify-version with a stale Scoop manifest and never
# published, and the release that replaced it (1.20.1) folded its section away
# in the same stroke — no linear changelog can satisfy both that tag and
# v1.20.1, because each section's trailing next-heading is part of the
# comparison. v0.17.0 was likewise replaced by v0.17.1. Both tags survive
# (repository rules forbid deleting them). Each row pins the tag object, the
# section it carried and its successor, whose commit must be the tag's direct
# child and is itself compared like any other tag. Tags cut before CHANGELOG.md
# existed are `no-changelog`.
unshipped=" "

# $1 tag, $2 tag object, $3 kind, $4 successor, $5 digest of its section.
verify_unshipped() {
  local tag=$1 v=${1#v}
  [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { reject "$UNSHIPPED" "$tag" "not a release tag name"; return 1; }
  case " $unshipped " in *" $tag "*) reject "$UNSHIPPED" "$tag" "duplicate row"; return 1 ;; esac
  [ "$(git rev-parse -q --verify "refs/tags/$tag" 2>/dev/null)" = "$2" ] \
    || { reject "$UNSHIPPED" "$tag" "the tag no longer points at object $2"; return 1; }
  case $3 in
    no-changelog)
      [ "$4" = - ] && [ "$5" = - ] || { reject "$UNSHIPPED" "$tag" "no-changelog rows take no successor or digest"; return 1; }
      if git cat-file -e "$tag:CHANGELOG.md" 2>/dev/null || git cat-file -e "$tag:$HISTORY_FILE" 2>/dev/null; then
        reject "$UNSHIPPED" "$tag" "the tag has a changelog after all"
        return 1
      fi
      ;;
    superseded)
      if ! [[ $4 =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || [ "$4" = "$tag" ] || ! version_le "$v" "${4#v}"; then
        reject "$UNSHIPPED" "$tag" "successor $4 is not a later release tag"
        return 1
      fi
      [ "$(git rev-parse -q --verify "$4^{commit}^1" 2>/dev/null)" = "$(git rev-parse -q --verify "$tag^{commit}" 2>/dev/null)" ] \
        || { reject "$UNSHIPPED" "$tag" "successor $4 is not the tag's direct child"; return 1; }
      [ "$(printf '%s\n' "$(published_section "$tag" "$v")" | sha256)" = "$5" ] \
        || { reject "$UNSHIPPED" "$tag" "the tag's [$v] section no longer has digest $5"; return 1; }
      if { [ -f "$HISTORY_FILE" ] && [ -n "$(section "$v" < "$HISTORY_FILE")" ]; } \
        || { [ -f "$ACTIVE_FILE" ] && [ -n "$(section "$v" < "$ACTIVE_FILE")" ]; }; then
        reject "$UNSHIPPED" "$tag" "a [$v] section is published after all"
        return 1
      fi
      ;;
    *)
      reject "$UNSHIPPED" "$tag" "unknown kind '$3'"
      return 1
      ;;
  esac
  unshipped="$unshipped$tag "
}

if [ ! -f "$UNSHIPPED" ]; then
  echo "error: $UNSHIPPED is missing"
  fail=1
elif [ "$(sha256 < "$UNSHIPPED")" != "$UNSHIPPED_SHA256" ]; then
  echo "error: $UNSHIPPED does not match its pinned digest $UNSHIPPED_SHA256"
  fail=1
else
  # shellcheck disable=SC2094 # reject only names the file in its message
  while IFS=$'\t' read -r tag obj kind successor published extra; do
    case $tag in '' | '#'*) continue ;; esac
    if [ -n "$extra" ] || [ -z "$published" ]; then
      reject "$UNSHIPPED" "$tag" "expected 5 tab-separated fields"
      continue
    fi
    verify_unshipped "$tag" "$obj" "$kind" "$successor" "$published"
  done < "$UNSHIPPED"
fi

for tag in $tags; do
  if [[ $unshipped == *" $tag "* ]]; then
    echo "skip: $tag has no published section, verified against $UNSHIPPED"
    continue
  fi
  v=${tag#v}
  if version_le "$v" "${HISTORY_LAST_TAG#v}"; then
    file=$HISTORY_FILE
  else
    file=$ACTIVE_FILE
  fi
  a=$(published_section "$tag" "$v")
  if [ -z "$a" ]; then
    echo "error: $tag has no [$v] section of its own"
    fail=1
    continue
  fi
  if [ ! -f "$file" ]; then
    echo "error: $file is missing, so the [$v] section of $tag cannot be checked"
    fail=1
    continue
  fi
  b=$(section "$v" < "$file")
  if [ -z "$b" ]; then
    # Removing the `[X.Y.Z]:` link along with the heading does not make a
    # published release retired; only the allowlist above does.
    echo "error: the [$v] section is MISSING from $file but exists in $tag"
    fail=1
    continue
  fi
  if [ "$a" != "$b" ] && [ "$file" = "$HISTORY_FILE" ] && [[ $reconciled == *" $tag "* ]]; then
    echo "reconciled: the archived [$v] section differs from $tag only by the pinned insertion from reconciliations/$tag.inserted"
  elif [ "$a" != "$b" ]; then
    echo "error: the published [$v] section in $file no longer matches $tag:"
    diff <(printf '%s\n' "$a") <(printf '%s\n' "$b") | sed 's/^/    /'
    fail=1
  fi
done

# The floor is the newest tag or the highest archived section, whichever is
# higher: an archived version stays recorded even before its tag exists.
newest=$(git tag --list 'v*' --sort=-v:refname | head -1)
want=${newest#v}
version_le "${HISTORY_LAST_TAG#v}" "$want" || want=${HISTORY_LAST_TAG#v}
# First matching line only; `0,/re/` is GNU-only and BSD sed prints nothing.
got=$(sed -n '/^version = "\(.*\)"/{s//\1/p;q;}' Cargo.toml)
if [ -z "$got" ]; then
  echo "error: could not read a version from Cargo.toml"
  fail=1
elif ! version_le "$want" "$got"; then
  # A release branch bumps past the floor and must pass; going backwards must not.
  echo "error: Cargo.toml says $got, older than $want (newest tag or archived section)"
  fail=1
fi

[ "$fail" = 0 ] && echo "released changelog sections and the Cargo version are intact"
exit $fail
