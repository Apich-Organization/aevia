#!/bin/sh
# A local stack for developing the registry.
#
#   ./scripts/dev.sh up      start PostgreSQL on 127.0.0.1:55432, print the exports
#   ./scripts/dev.sh down    stop and remove the container
#   ./scripts/dev.sh reset   drop and recreate the schema (the server migrates it again on start)
#   ./scripts/dev.sh run     up, then cargo run -p aevia-registry-server
#
# The container is named `aevia-dev-db` and is reused: `up` after `up` prints
# the exports again instead of failing, and `down` on a machine with no
# container is a no-op. Nothing outside this container and `target/` is touched.
#
# The port is 55432, not 5432, so a real PostgreSQL on the host keeps working.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

container=aevia-dev-db
image=postgres:18-trixie
port=55432

if command -v podman >/dev/null 2>&1; then
    engine=podman
elif command -v docker >/dev/null 2>&1; then
    engine=docker
else
    echo "dev.sh: podman or docker is required" >&2
    exit 1
fi

# Every command is printed before it runs, so a failure is reproducible by
# copying the last line out of the terminal.
say() {
    printf '+ %s\n' "$*"
}

run_cmd() {
    say "$@"
    "$@"
}

container_exists() {
    "$engine" container inspect "$container" >/dev/null 2>&1
}

container_running() {
    state=$("$engine" inspect --format '{{.State.Running}}' "$container" 2>/dev/null) || return 1
    [ "$state" = "true" ]
}

# The container is "started" well before the database accepts connections:
# initdb runs on first boot and the entrypoint restarts the server once it is
# done, so a client that connects immediately gets a refused socket.
wait_for_db() {
    tries=0
    while [ "$tries" -lt 60 ]; do
        if "$engine" exec "$container" pg_isready -h 127.0.0.1 -U aevia -d aevia >/dev/null 2>&1; then
            echo "dev.sh: database ready on 127.0.0.1:$port"
            return 0
        fi
        tries=$((tries + 1))
        sleep 1
    done
    echo "dev.sh: database did not become ready in 60s" >&2
    return 1
}

# A fresh secret per invocation, unless one is already exported. Sessions do not
# survive a restart of the dev server, which is the right trade here; export
# AEVIA_SESSION_SECRET yourself to keep them.
configure() {
    DATABASE_URL="postgres://aevia:aevia@127.0.0.1:$port/aevia"
    AEVIA_SESSION_SECRET=${AEVIA_SESSION_SECRET:-$(openssl rand -hex 32)}
    AEVIA_STORAGE_DIR=$root/target/packages
    AEVIA_SPEC_DIR=$(CDPATH= cd -- "$root/.." && pwd)/spec
    AEVIA_BASE_URL=http://127.0.0.1:8080
    export DATABASE_URL AEVIA_SESSION_SECRET AEVIA_STORAGE_DIR AEVIA_SPEC_DIR AEVIA_BASE_URL
}

print_exports() {
    echo "dev.sh: exports for this database:"
    printf "export DATABASE_URL='%s'\n" "$DATABASE_URL"
    printf "export AEVIA_SESSION_SECRET='%s'\n" "$AEVIA_SESSION_SECRET"
    printf "export AEVIA_STORAGE_DIR='%s'\n" "$AEVIA_STORAGE_DIR"
    printf "export AEVIA_SPEC_DIR='%s'\n" "$AEVIA_SPEC_DIR"
    printf "export AEVIA_BASE_URL='%s'\n" "$AEVIA_BASE_URL"
}

up() {
    configure
    if container_exists; then
        if container_running; then
            echo "dev.sh: $container is already running"
        else
            run_cmd "$engine" start "$container"
        fi
    else
        run_cmd "$engine" run --detach --name "$container" \
            --env POSTGRES_USER=aevia \
            --env POSTGRES_PASSWORD=aevia \
            --env POSTGRES_DB=aevia \
            --publish "127.0.0.1:$port:5432" \
            "$image"
    fi
    wait_for_db
    print_exports
}

down() {
    if ! container_exists; then
        echo "dev.sh: no $container to remove"
        return 0
    fi
    if container_running; then
        run_cmd "$engine" stop "$container"
    fi
    run_cmd "$engine" rm --force "$container"
}

reset_schema() {
    if ! container_running; then
        echo "dev.sh: $container is not running; run '$0 up' first" >&2
        exit 1
    fi
    run_cmd "$engine" exec "$container" \
        psql --username aevia --dbname aevia --set ON_ERROR_STOP=1 \
        --command 'DROP SCHEMA IF EXISTS public CASCADE' \
        --command 'CREATE SCHEMA public'
    echo "dev.sh: schema dropped; the server applies the migrations on start"
}

run_server() {
    up
    RUST_LOG=aevia_registry=debug,tower_http=warn
    export RUST_LOG
    run_cmd cargo run --package aevia-registry-server
}

case "${1:-}" in
    up)
        up
        ;;
    down)
        down
        ;;
    reset)
        reset_schema
        ;;
    run)
        run_server
        ;;
    *)
        echo "usage: $0 {up|down|reset|run}" >&2
        exit 2
        ;;
esac
