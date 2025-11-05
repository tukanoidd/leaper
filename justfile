run *args:
    cargo run -p leaper -- {{args}}

daemon:
    cargo run -p leaper-daemon

db:
    surreal start --unauthenticated

profile *args:
    cargo run -p leaper --features profile -- {{args}}

profile_ws *args:
    cargo run -p leaper --features db-websocket,profile -- {{args}}
