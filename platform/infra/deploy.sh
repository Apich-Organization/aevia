#!/bin/sh
# Aevia registry — deployment helper.
#
#   ./deploy.sh build            build the registry image for this host
#   ./deploy.sh up               start db + registry + caddy
#   ./deploy.sh down             stop everything (volumes are kept)
#   ./deploy.sh logs [service]   follow logs
#   ./deploy.sh ps               show container status
#   ./deploy.sh backup           dump the database and the archive volume
#   ./deploy.sh restore <file>   restore a .sql.gz or .tar.gz produced by backup
#   ./deploy.sh upgrade          rebuild the registry and recreate the stack
#
# POSIX sh on purpose: the target hosts are minimal Debian 13 ARM machines where
# bash is an extra package. No arrays, no [[ ]], no local.

set -eu

INFRA_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(CDPATH= cd -- "$INFRA_DIR/.." && pwd)
ENV_FILE="$INFRA_DIR/.env"
COMPOSE_FILE="$INFRA_DIR/compose.yaml"
BACKUP_DIR="$INFRA_DIR/backups"
LANG_CRATES=$(CDPATH= cd -- "$REPO_DIR/../crates" 2>/dev/null && pwd || true)

IMAGE_REGISTRY="localhost/aevia/registry:0.1.0"
RUST_VERSION="1.97"

COMPOSE_BIN=""
COMPOSE_SUB=""
RUNTIME=""
BUILDER=""

die() {
	printf 'deploy.sh: %s\n' "$*" >&2
	exit 1
}

# Echo the command, then run it. Everything that touches the outside world goes
# through here so an operator can copy the line out of the log and re-run it.
run() {
	printf '+ %s\n' "$*" >&2
	"$@"
}

compose() {
	if [ -n "$COMPOSE_SUB" ]; then
		"$COMPOSE_BIN" "$COMPOSE_SUB" --file "$COMPOSE_FILE" "$@"
	else
		"$COMPOSE_BIN" --file "$COMPOSE_FILE" "$@"
	fi
}

require_env() {
	if [ ! -f "$ENV_FILE" ]; then
		die "$ENV_FILE is missing. Run: cp .env.example .env, then fill it in."
	fi

	# The session secret signs every session cookie, so a placeholder value is a
	# forged login waiting to happen. Check the contract here rather than letting
	# the binary fail after the containers are already up.
	secret=$(sed -n 's/^AEVIA_SESSION_SECRET=//p' "$ENV_FILE" | tr -d '\r' | head -n 1)
	case $secret in
		*[!0-9a-f]*) die "AEVIA_SESSION_SECRET must be lowercase hex. Generate one with: openssl rand -hex 32" ;;
	esac
	if [ "${#secret}" -ne 64 ]; then
		die "AEVIA_SESSION_SECRET must be 64 hex characters (got ${#secret}). Generate one with: openssl rand -hex 32"
	fi
	case $secret in
		0000000000000000000000000000000000000000000000000000000000000000)
			die "AEVIA_SESSION_SECRET is still the .env.example placeholder. Generate one with: openssl rand -hex 32"
			;;
	esac
}

# First implementation that actually works wins: `podman compose` is a shim that
# exists even when no provider is installed, hence the `version` probe.
detect_compose() {
	if [ -n "$COMPOSE_BIN" ]; then
		return 0
	fi
	if command -v podman >/dev/null 2>&1 && podman compose version >/dev/null 2>&1; then
		COMPOSE_BIN=podman
		COMPOSE_SUB=compose
		RUNTIME=podman
	elif command -v podman-compose >/dev/null 2>&1; then
		COMPOSE_BIN=podman-compose
		COMPOSE_SUB=""
		RUNTIME=podman
	elif command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
		COMPOSE_BIN=docker
		COMPOSE_SUB=compose
		RUNTIME=docker
	else
		die "no compose implementation found (tried: podman compose, podman-compose, docker compose)"
	fi
	printf 'using: %s %s\n' "$COMPOSE_BIN" "$COMPOSE_SUB" >&2
}

detect_builder() {
	if [ -n "$BUILDER" ]; then
		return 0
	fi
	if command -v podman >/dev/null 2>&1; then
		BUILDER=podman
	elif command -v docker >/dev/null 2>&1; then
		BUILDER=docker
	else
		die "neither podman nor docker found"
	fi
}

# Name of the archive volume. Compose prefixes it with the project name
# (aevia_aevia-data), Quadlet uses the bare name, and both must work here.
archive_volume() {
	vols=$("$RUNTIME" volume ls --format '{{.Name}}')
	for v in $vols; do
		case $v in
			aevia-data | *_aevia-data) printf '%s\n' "$v" ;;
		esac
	done | head -n 1
}

cmd_build() {
	require_env
	detect_builder

	if [ -z "$LANG_CRATES" ]; then
		die "the language repository is not checked out next to this one: expected $REPO_DIR/../crates (needed by aevia-registry-docs)"
	fi

	# The image resolves the language crates' inherited fields from a checked-in
	# mirror of their workspace root. Fail before a ten-minute build if the
	# mirror has drifted.
	if [ -f "$INFRA_DIR/check-lang-workspace.sh" ]; then
		if ! sh "$INFRA_DIR/check-lang-workspace.sh" "$REPO_DIR/.."; then
			die "infra/lang-workspace.toml is out of step with the language repository"
		fi
	fi

	# The image is built for the host's own architecture. On the ARM deployment
	# host that is a native aarch64 build; --platform makes it explicit so a
	# stale builder default cannot silently produce an amd64 image.
	set -- build \
		--file "$INFRA_DIR/Containerfile.server" \
		--build-context "lang=$LANG_CRATES" \
		--tag "$IMAGE_REGISTRY" \
		--build-arg "RUST_VERSION=$RUST_VERSION"

	if [ "$(uname -m)" = "aarch64" ]; then
		set -- "$@" --platform linux/arm64
	fi

	set -- "$@" "$REPO_DIR"
	run "$BUILDER" "$@"
}

cmd_up() {
	require_env
	detect_compose
	run compose up --detach --remove-orphans
	run compose ps
}

cmd_down() {
	detect_compose
	# No --volumes: the database and the package archives are the service.
	run compose down --remove-orphans
}

cmd_logs() {
	detect_compose
	run compose logs --follow --tail=200 "$@"
}

cmd_ps() {
	detect_compose
	run compose ps
}

cmd_backup() {
	require_env
	detect_compose

	mkdir -p "$BACKUP_DIR"
	stamp=$(date -u +%Y%m%dT%H%M%SZ)
	sql="$BACKUP_DIR/aevia-db-$stamp.sql"
	tarball="$BACKUP_DIR/aevia-data-$stamp.tar.gz"

	# Dump to a file first, not into a pipe: POSIX sh has no pipefail, so a
	# failing pg_dump inside `pg_dump | gzip` would still leave a valid, empty
	# .sql.gz behind. The -s check below catches that.
	printf '+ pg_dump (db) > %s\n' "$sql" >&2
	compose exec -T db sh -c \
		'pg_dump --no-owner --clean --if-exists -U "$POSTGRES_USER" -d "$POSTGRES_DB"' >"$sql"

	if [ ! -s "$sql" ]; then
		rm -f "$sql"
		die "pg_dump produced no output; is the db container running?"
	fi
	run gzip -9 "$sql"

	vol=$(archive_volume)
	if [ -z "$vol" ]; then
		die "archive volume not found; has 'up' been run?"
	fi

	# A throwaway container is the only way to read a volume that no host path
	# points at. --user 0:0 because the archives are owned by uid 10001.
	printf '+ tar volume %s > %s\n' "$vol" "$tarball" >&2
	"$RUNTIME" run --rm --user 0:0 --volume "$vol:/data:ro" \
		"$IMAGE_REGISTRY" tar -czf - -C /data . >"$tarball"

	printf 'backup written:\n  %s\n  %s\n' "$sql.gz" "$tarball" >&2
}

cmd_restore() {
	require_env
	detect_compose

	[ "$#" -eq 1 ] || die "usage: deploy.sh restore <backup.sql.gz|backup.tar.gz>"
	file=$1
	[ -f "$file" ] || die "$file does not exist"

	if [ "${AEVIA_RESTORE_YES:-}" != "1" ]; then
		printf 'This overwrites the live database and/or archives. Type "yes" to continue: ' >&2
		read -r answer || die "no answer on stdin; set AEVIA_RESTORE_YES=1 for unattended restores"
		[ "$answer" = "yes" ] || die "aborted"
	fi

	case $file in
		*.sql.gz)
			printf '+ psql < %s\n' "$file" >&2
			gunzip -c "$file" | compose exec -T db sh -c \
				'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB"'
			;;
		*.tar.gz)
			vol=$(archive_volume)
			[ -n "$vol" ] || die "archive volume not found; has 'up' been run?"
			printf '+ untar %s > volume %s\n' "$file" "$vol" >&2
			"$RUNTIME" run --rm -i --user 0:0 --volume "$vol:/data" \
				"$IMAGE_REGISTRY" tar -xzf - -C /data <"$file"
			;;
		*)
			die "unknown backup format: $file (expected .sql.gz or .tar.gz)"
			;;
	esac
}

cmd_upgrade() {
	require_env
	detect_compose
	detect_builder

	# Caddy and the base images float on their major version, so pull them.
	# PostgreSQL is *not* upgraded in place: a major-version bump needs a
	# pg_dump/pg_restore (see docs/DEPLOY.md).
	run compose pull caddy
	cmd_build
	run compose up --detach --remove-orphans
	run compose ps
}

usage() {
	sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'
}

case ${1:-} in
	build) shift; cmd_build "$@" ;;
	up) shift; cmd_up "$@" ;;
	down) shift; cmd_down "$@" ;;
	logs) shift; cmd_logs "$@" ;;
	ps) shift; cmd_ps "$@" ;;
	backup) shift; cmd_backup "$@" ;;
	restore) shift; cmd_restore "$@" ;;
	upgrade) shift; cmd_upgrade "$@" ;;
	-h | --help | help) usage ;;
	*) usage >&2; exit 2 ;;
esac
