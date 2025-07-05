profile:
    cargo run -p leaper --features profile

run_ws:
    cargo run -p leaper --features db-websocket

testbed which *release:
    cargo run -p leaper {{release}} --features testbed-{{which}}

testbed_log which log *release:
    cargo run -p leaper {{release}} --features testbed-{{which}} -- --{{log}}

docker_db:
    docker run --rm --pull always -p 8000:8000 surrealdb/surrealdb:latest start --unauthenticated
