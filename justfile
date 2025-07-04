profile:
    cargo run -p leaper --features profile

run_ws:
    cargo run -p leaper --features db-websocket

docker_db:
    docker run --rm --pull always -p 8000:8000 surrealdb/surrealdb:latest start --unauthenticated
