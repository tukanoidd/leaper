run_ws *args:
    cargo run -p leaper --features db-websocket -- {{args}}

db:
    surreal start --unauthenticated

profile *args:
    cargo run -p leaper --features profile -- {{args}}

profile_ws *args:
    cargo run -p leaper --features db-websocket,profile -- {{args}}

zipkin:
    docker run -d -p 9411:9411 openzipkin/zipkin