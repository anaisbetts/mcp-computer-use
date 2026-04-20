#!/usr/bin/env bash
set -euo pipefail

WORKFLOW_FILE="rust-ci.yml"
POLL_SECONDS=15
TIMEOUT_SECONDS=900
REPO="${GITHUB_REPOSITORY:-}"
TAG=""

usage() {
  cat <<'EOF'
Usage: publish-release-assets.sh --tag <tag> [--repo <owner/name>]

Finds the successful Rust CI tag build for the given release tag,
downloads every artifact from that run, and uploads them to the
corresponding GitHub release.
EOF
}

require_command() {
  local command_name="$1"

  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required command: $command_name" >&2
    exit 1
  fi
}

parse_args() {
  while (($# > 0)); do
    case "$1" in
      --tag)
        TAG="${2:-}"
        shift 2
        ;;
      --repo)
        REPO="${2:-}"
        shift 2
        ;;
      --help|-h)
        usage
        exit 0
        ;;
      *)
        echo "Unknown argument: $1" >&2
        usage >&2
        exit 1
        ;;
    esac
  done

  if [[ -z "$TAG" ]]; then
    echo "--tag is required" >&2
    usage >&2
    exit 1
  fi
}

resolve_repo() {
  if [[ -n "$REPO" ]]; then
    return
  fi

  REPO="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"

  if [[ -z "$REPO" ]]; then
    echo "Unable to determine repository name" >&2
    exit 1
  fi
}

wait_for_successful_run() {
  local started_at
  local now
  local run_json
  started_at="$(date +%s)"

  while true; do
    run_json="$(
      gh run list \
        --repo "$REPO" \
        --workflow "$WORKFLOW_FILE" \
        --branch "$TAG" \
        --event push \
        --json databaseId,status,conclusion,headBranch,createdAt \
        | jq --arg tag "$TAG" '
            map(select(.headBranch == $tag))
            | sort_by(.createdAt)
            | last // empty
          '
    )"

    if [[ -n "$run_json" && "$run_json" != "null" ]]; then
      local run_id
      local status
      local conclusion

      run_id="$(jq -r '.databaseId' <<<"$run_json")"
      status="$(jq -r '.status' <<<"$run_json")"
      conclusion="$(jq -r '.conclusion // ""' <<<"$run_json")"

      echo "Found Rust CI run $run_id for $TAG with status=$status conclusion=${conclusion:-pending}" >&2

      if [[ "$status" == "completed" && "$conclusion" == "success" ]]; then
        printf '%s\n' "$run_id"
        return
      fi

      if [[ "$status" == "completed" && "$conclusion" != "success" ]]; then
        echo "Rust CI run for $TAG completed with conclusion=$conclusion" >&2
        exit 1
      fi
    else
      echo "No Rust CI run found for $TAG yet" >&2
    fi

    now="$(date +%s)"
    if ((now - started_at >= TIMEOUT_SECONDS)); then
      echo "Timed out waiting for a successful Rust CI run for $TAG" >&2
      exit 1
    fi

    sleep "$POLL_SECONDS"
  done
}

wait_for_artifacts() {
  local run_id="$1"
  local started_at
  local now
  local artifacts_json
  local artifact_count
  started_at="$(date +%s)"

  while true; do
    artifacts_json="$(
      gh api "repos/$REPO/actions/runs/$run_id/artifacts?per_page=100"
    )"
    artifact_count="$(jq -r '.total_count' <<<"$artifacts_json")"

    if ((artifact_count > 0)); then
      echo "Found $artifact_count artifact(s) for run $run_id"
      return
    fi

    now="$(date +%s)"
    if ((now - started_at >= TIMEOUT_SECONDS)); then
      echo "Timed out waiting for artifacts from run $run_id" >&2
      exit 1
    fi

    echo "Run $run_id has no artifacts yet" >&2
    sleep "$POLL_SECONDS"
  done
}

upload_artifacts_to_release() {
  local run_id="$1"
  local temp_dir
  local download_dir
  local files=()

  temp_dir="$(mktemp -d)"
  trap 'rm -rf -- "${temp_dir:-}"' EXIT
  download_dir="$temp_dir/downloaded-artifacts"

  mkdir -p "$download_dir"
  gh release view "$TAG" --repo "$REPO" >/dev/null
  gh run download "$run_id" --repo "$REPO" --dir "$download_dir"

  while IFS= read -r -d '' file_path; do
    files+=("$file_path")
  done < <(find "$download_dir" -type f -print0 | sort -z)

  if ((${#files[@]} == 0)); then
    echo "Downloaded run $run_id but found no artifact files to upload" >&2
    exit 1
  fi

  echo "Uploading ${#files[@]} file(s) to release $TAG"
  gh release upload "$TAG" "${files[@]}" --repo "$REPO" --clobber
}

main() {
  require_command gh
  require_command jq
  parse_args "$@"
  resolve_repo

  echo "Publishing CI artifacts for release $TAG in $REPO" >&2
  local run_id
  run_id="$(wait_for_successful_run)"
  wait_for_artifacts "$run_id"
  upload_artifacts_to_release "$run_id"
}

main "$@"
